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

/**
 * What the library answered to a request to read a cosigner key out of a finished code (UR-31): the
 * key as text, a code that was read but is not a key this app takes (with the library's own words), or
 * a request or answer that is not in the framing.
 */
sealed class KeyTextOutcome {
    /** The key in the form the key ceremony takes: `[<fingerprint>/48h/<coin>h/0h/2h]<key>`. */
    class Key(val text: String) : KeyTextOutcome()

    /** The code is not taken as a key. [code] is the rule; the three texts are for the person. */
    class Declined(val code: String, val heading: String, val what: String, val next: String) : KeyTextOutcome()

    /** The request was refused before the code was looked at (a bad type, a bad frame). */
    class Refused(val code: String) : KeyTextOutcome()

    /** No answer, or one that does not follow the framing. */
    object Fault : KeyTextOutcome()
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

/** How far a reader of a BBQr set has come: parts [have] of [total], in any order. */
class BbqrProgress(val complete: Boolean, val have: Int, val total: Int)

/** A finished BBQr set: its [kind] (the letter P for a PSBT, T for a transaction) and its [payload]. */
class BbqrMessage(val kind: Char, val payload: ByteArray)

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
     * The parts of [payload] as UR text of [type] (`bytes`, `crypto-psbt`, or `crypto-output`, whose payload
     * is the descriptor's CBOR), in upper case: one part
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

    /**
     * The cosigner key in a finished code of [type] (`crypto-account`, `crypto-hdkey` or `bytes`): [payload]
     * is what [result] gave, which for `bytes` is the content (its CBOR byte string is put back here).
     * The newer types and other types come back as [KeyTextOutcome.Declined] with words.
     */
    fun keyText(type: String, payload: ByteArray): KeyTextOutcome {
        val name = type.toByteArray(Charsets.US_ASCII)
        if (name.isEmpty() || name.size > 255) return KeyTextOutcome.Refused(BAD_FRAMING)
        val message = if (type == "bytes") byteString(payload) else payload
        val out = head(OP_KEY_TEXT)
        out.write(name.size)
        out.write(name)
        sized(out, message)
        val bytes = out.toByteArray()
        val reply = try {
            transport(bytes)
        } finally {
            bytes.fill(0)
        }
        if (reply == null || reply.size < 6) return KeyTextOutcome.Fault
        for (i in 0 until 4) if (reply[i] != REPLY_MAGIC[i]) return KeyTextOutcome.Fault
        if (reply[4].toInt() != VERSION) return KeyTextOutcome.Fault
        val body = reply.copyOfRange(6, reply.size)
        return when (reply[5].toInt()) {
            STATUS_OK -> {
                if (body.isEmpty() || body.size > MAX_KEY_TEXT || body.any { it < 0x20 || it > 0x7e }) {
                    KeyTextOutcome.Fault
                } else {
                    KeyTextOutcome.Key(String(body, Charsets.US_ASCII))
                }
            }
            STATUS_REFUSED -> {
                val code = String(body, Charsets.US_ASCII)
                if (RULE_CODE.matches(code)) KeyTextOutcome.Refused(code) else KeyTextOutcome.Fault
            }
            STATUS_REFUSED_WITH_WORDS -> declined(body)
            else -> KeyTextOutcome.Fault
        }
    }

    /** The body of a refusal with words: the rule code, then three texts of a 16-bit length each, exactly. */
    private fun declined(body: ByteArray): KeyTextOutcome {
        if (body.size < 5) return KeyTextOutcome.Fault
        val code = String(body, 0, 5, Charsets.US_ASCII)
        if (!RULE_CODE.matches(code)) return KeyTextOutcome.Fault
        var at = 5
        val texts = ArrayList<String>(3)
        for (i in 0 until 3) {
            if (at + 2 > body.size) return KeyTextOutcome.Fault
            val len = u16(body, at)
            at += 2
            if (len == 0 || len > body.size - at) return KeyTextOutcome.Fault
            texts.add(String(body, at, len, Charsets.UTF_8))
            at += len
        }
        if (at != body.size) return KeyTextOutcome.Fault
        return KeyTextOutcome.Declined(code, texts[0], texts[1], texts[2])
    }

    /** [content] as a CBOR byte string: the shortest head, then the bytes. */
    private fun byteString(content: ByteArray): ByteArray {
        val out = ByteArrayOutputStream()
        val n = content.size
        when {
            n < 24 -> out.write(0x40 or n)
            n <= 0xff -> {
                out.write(0x58)
                out.write(n)
            }
            n <= 0xffff -> {
                out.write(0x59)
                out.write(n shr 8)
                out.write(n and 0xff)
            }
            else -> {
                out.write(0x5a)
                u32(out, n.toLong())
            }
        }
        out.write(content)
        return out.toByteArray()
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

    /**
     * The parts of [payload] as a BBQr set of [kind] (P, T, U, J, C, B or X), each at most [maxChars]
     * characters (16 to 4296). A set is shown round and round; the order does not matter to a reader.
     */
    fun bbqrEncode(kind: Char, payload: ByteArray, maxChars: Int): UrOutcome<List<String>> {
        if (kind.code > 0x7f || maxChars !in 0..0xffff) return UrOutcome.Refused(BAD_FRAMING)
        val out = head(OP_BBQR_ENCODE)
        out.write(kind.code)
        out.write(maxChars shr 8)
        out.write(maxChars and 0xff)
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

    /** Opens a reader of a BBQr set of a PSBT or a transaction; the handle is never given twice. */
    fun newJoiner(): UrOutcome<Long> = run(head(OP_BBQR_NEW)) { body -> if (body.size == 4) u32(body, 0) else null }

    /** Gives the reader the text of one scanned part. */
    fun joinerAdd(handle: Long, text: String): UrOutcome<BbqrProgress> {
        val out = head(OP_BBQR_ADD)
        u32(out, handle)
        sized(out, text.toByteArray(Charsets.UTF_8))
        return run(out) { body ->
            if (body.size != 9 || (body[0].toInt() != 0 && body[0].toInt() != 1)) return@run null
            BbqrProgress(body[0].toInt() == 1, u32(body, 1).toInt(), u32(body, 5).toInt())
        }
    }

    /** The message of a complete set. */
    fun joinerResult(handle: Long): UrOutcome<BbqrMessage> {
        val out = head(OP_BBQR_RESULT)
        u32(out, handle)
        return run(out) { body ->
            if (body.size < 5) return@run null
            val len = u32(body, 1)
            if (len != (body.size - 5).toLong()) return@run null
            BbqrMessage((body[0].toInt() and 0x7f).toChar(), body.copyOfRange(5, body.size))
        }
    }

    /** Closes a reader of a BBQr set. */
    fun joinerDrop(handle: Long): UrOutcome<Unit> {
        val out = head(OP_BBQR_DROP)
        u32(out, handle)
        return run(out) { body -> if (body.isEmpty()) Unit else null }
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
        const val STATUS_REFUSED_WITH_WORDS = 2
        const val OP_QR_ENCODE = 1
        const val OP_UR_ENCODE = 2
        const val OP_DECODER_NEW = 3
        const val OP_DECODER_RECEIVE = 4
        const val OP_DECODER_RESULT = 5
        const val OP_DECODER_DROP = 6
        const val OP_KEY_TEXT = 7
        const val OP_BBQR_ENCODE = 8
        const val OP_BBQR_NEW = 9
        const val OP_BBQR_ADD = 10
        const val OP_BBQR_RESULT = 11
        const val OP_BBQR_DROP = 12

        /** The rule codes of the call itself. */
        const val TYPE_NOT_USED = "UR-20"
        const val BAD_FRAMING = "UR-21"
        const val BAD_STATE = "UR-22"

        /** The most parts one reply can hold: two turns of the longest sequence. */
        private const val MAX_PARTS = 20_000L

        /** The longest key text: the origin and an extended key of at most 112 characters. */
        private const val MAX_KEY_TEXT = 160
        private val RULE_CODE = Regex("^(UR|QR|BQ)-[0-9]{2}$")

        /** The call through the library; [org.openbmt.common.QrNative.load] must have loaded it. */
        fun native(): UrCall = UrCall { UrNative.call(it) }
    }
}
