package org.openbmt.app

import java.io.ByteArrayOutputStream
import org.openbmt.common.UrNative

/** What the library answered to one request. */
sealed class UrOutcome<out T> {
    class Ok<out T>(val value: T) : UrOutcome<T>()

    /** The library refused; [code] is the rule code, such as UR-12. */
    class Refused(val code: String) : UrOutcome<Nothing>()

    /** No answer, or one that does not follow the framing. */
    object Fault : UrOutcome<Nothing>()
}

/** A QR code as the library draws it: [side] by [side] modules, without the quiet zone. */
class QrMatrix(val side: Int, private val packed: ByteArray) {
    /** Whether the module in column [x] and row [y] is dark. */
    fun get(x: Int, y: Int): Boolean {
        if (x < 0 || y < 0 || x >= side || y >= side) return false
        val i = y * side + x
        return ((packed[i / 8].toInt() shr (7 - i % 8)) and 1) == 1
    }

    /** The QR version of this side (1 to 40). */
    val version: Int get() = (side - 17) / 4
}

/** How far a decoder has come: [fraction] is an estimate from 0.0 to 1.0, and 1.0 only when complete. */
class UrProgress(val complete: Boolean, val received: Int, val expected: Int, val fraction: Double)

/** A finished message: the [type] of the UR and its [payload]. */
class UrMessage(val type: String, val payload: ByteArray)

/**
 * The framing of the library's second call, in pure Kotlin (the Rust side is `crates/openbmt-qr-jni/src/
 * call.rs`). Requests are the bytes `OBMo`, the version 1, an operation and its fields, big-endian, with
 * nothing after them. Replies are `OBMp`, the version, a status and the body, which is read strictly:
 * a reply with a length that is not exactly what its operation gives is a [UrOutcome.Fault], never
 * partly read. [transport] carries one request to the library and brings the reply back.
 */
class UrCall(private val transport: (ByteArray) -> ByteArray?) {
    // ---- the operations ---------------------------------------------------------------------

    /** Draws [text] at error-correction [level] (0 to 3 for L, M, Q, H). */
    fun qrEncode(text: ByteArray, level: Int): UrOutcome<QrMatrix> {
        if (level !in 0..3) return UrOutcome.Refused(BAD_FRAMING)
        val out = head(OP_QR_ENCODE)
        out.write(level)
        sized(out, text)
        return run(out) { body ->
            if (body.size < 2) return@run null
            val side = u16(body, 0)
            if (side < 21 || side > 177 || body.size != 2 + (side * side + 7) / 8) return@run null
            QrMatrix(side, body.copyOfRange(2, body.size))
        }
    }

    /**
     * The parts of [payload] as UR text of [type] (`bytes` or `crypto-psbt`), in upper case: one part
     * when it fits [maxFragment] bytes, otherwise two turns of the sequence (twice its length).
     */
    fun urEncode(type: String, payload: ByteArray, maxFragment: Int): UrOutcome<List<String>> {
        val name = type.toByteArray(Charsets.US_ASCII)
        if (name.size > 255 || maxFragment !in 0..0xffff) return UrOutcome.Refused(BAD_FRAMING)
        val out = head(OP_UR_ENCODE)
        out.write(name.size)
        out.write(name)
        out.write(maxFragment shr 8)
        out.write(maxFragment and 0xff)
        sized(out, payload)
        return run(out) { body ->
            if (body.size < 4) return@run null
            val count = u32(body, 0)
            if (count < 1 || count > MAX_PARTS) return@run null
            val parts = ArrayList<String>(count.toInt())
            var at = 4
            for (i in 0 until count.toInt()) {
                if (at + 4 > body.size) return@run null
                val len = u32(body, at)
                at += 4
                if (len > body.size - at) return@run null
                parts.add(String(body, at, len.toInt(), Charsets.US_ASCII))
                at += len.toInt()
            }
            if (at != body.size) null else parts
        }
    }

    /** Opens a decoder; the handle is a number that is never given twice. */
    fun newDecoder(): UrOutcome<Long> = run(head(OP_DECODER_NEW)) { body -> if (body.size == 4) u32(body, 0) else null }

    /** Gives the decoder the text of one scanned part. */
    fun receive(handle: Long, text: String): UrOutcome<UrProgress> {
        val out = head(OP_DECODER_RECEIVE)
        u32(out, handle)
        sized(out, text.toByteArray(Charsets.UTF_8))
        return run(out) { body ->
            if (body.size != 13 || (body[0].toInt() != 0 && body[0].toInt() != 1)) return@run null
            val millionths = u32(body, 9)
            if (millionths > 1_000_000) return@run null
            UrProgress(body[0].toInt() == 1, u32(body, 1).toInt(), u32(body, 5).toInt(), millionths / 1_000_000.0)
        }
    }

    /** The message of a complete decoder. */
    fun result(handle: Long): UrOutcome<UrMessage> {
        val out = head(OP_DECODER_RESULT)
        u32(out, handle)
        return run(out) { body ->
            if (body.isEmpty()) return@run null
            val n = body[0].toInt() and 0xff
            if (body.size < 1 + n + 4) return@run null
            val type = String(body, 1, n, Charsets.US_ASCII)
            val len = u32(body, 1 + n)
            if (len != (body.size - 5 - n).toLong()) return@run null
            UrMessage(type, body.copyOfRange(5 + n, body.size))
        }
    }

    /** Closes a decoder; whatever it held is gone. */
    fun drop(handle: Long): UrOutcome<Unit> {
        val out = head(OP_DECODER_DROP)
        u32(out, handle)
        return run(out) { body -> if (body.isEmpty()) Unit else null }
    }

    // ---- framing ----------------------------------------------------------------------------------

    private fun <T : Any> run(request: ByteArrayOutputStream, parse: (ByteArray) -> T?): UrOutcome<T> {
        val bytes = request.toByteArray()
        val reply = try {
            transport(bytes)
        } finally {
            bytes.fill(0)
        }
        if (reply == null || reply.size < 6) return UrOutcome.Fault
        for (i in 0 until 4) if (reply[i] != REPLY_MAGIC[i]) return UrOutcome.Fault
        if (reply[4].toInt() != VERSION) return UrOutcome.Fault
        val body = reply.copyOfRange(6, reply.size)
        return when (reply[5].toInt()) {
            STATUS_OK -> {
                val value = parse(body)
                if (value == null) UrOutcome.Fault else UrOutcome.Ok(value)
            }
            STATUS_REFUSED -> {
                val code = String(body, Charsets.US_ASCII)
                if (RULE_CODE.matches(code)) UrOutcome.Refused(code) else UrOutcome.Fault
            }
            else -> UrOutcome.Fault
        }
    }

    private fun head(op: Int): ByteArrayOutputStream {
        val out = ByteArrayOutputStream()
        out.write(REQUEST_MAGIC)
        out.write(VERSION)
        out.write(op)
        return out
    }

    private fun sized(out: ByteArrayOutputStream, bytes: ByteArray) {
        u32(out, bytes.size.toLong())
        out.write(bytes)
    }

    private fun u32(out: ByteArrayOutputStream, n: Long) {
        for (shift in intArrayOf(24, 16, 8, 0)) out.write(((n shr shift) and 0xff).toInt())
    }

    private fun u16(b: ByteArray, at: Int): Int = ((b[at].toInt() and 0xff) shl 8) or (b[at + 1].toInt() and 0xff)

    private fun u32(b: ByteArray, at: Int): Long {
        var n = 0L
        for (i in 0 until 4) n = (n shl 8) or (b[at + i].toLong() and 0xff)
        return n
    }

    companion object {
        val REQUEST_MAGIC = byteArrayOf('O'.code.toByte(), 'B'.code.toByte(), 'M'.code.toByte(), 'o'.code.toByte())
        val REPLY_MAGIC = byteArrayOf('O'.code.toByte(), 'B'.code.toByte(), 'M'.code.toByte(), 'p'.code.toByte())
        const val VERSION = 1
        const val STATUS_OK = 0
        const val STATUS_REFUSED = 1
        const val OP_QR_ENCODE = 1
        const val OP_UR_ENCODE = 2
        const val OP_DECODER_NEW = 3
        const val OP_DECODER_RECEIVE = 4
        const val OP_DECODER_RESULT = 5
        const val OP_DECODER_DROP = 6

        /** The rule codes of the call itself. */
        const val TYPE_NOT_USED = "UR-20"
        const val BAD_FRAMING = "UR-21"
        const val BAD_STATE = "UR-22"

        /** The most parts one reply can hold: two turns of the longest sequence. */
        private const val MAX_PARTS = 20_000L
        private val RULE_CODE = Regex("^(UR|QR)-[0-9]{2}$")

        /** The call through the library; [org.openbmt.common.QrNative.load] must have loaded it. */
        fun native(): UrCall = UrCall { UrNative.call(it) }
    }
}
