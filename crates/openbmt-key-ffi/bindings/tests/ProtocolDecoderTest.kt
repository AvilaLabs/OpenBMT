package org.openbmt.key

import java.io.ByteArrayOutputStream

/** Plain JVM regression harness for the production OBM9 decoder; run its main. */
object ProtocolDecoderTest {
    @JvmStatic
    fun main(args: Array<String>) {
        val success = decodeResponse(byteArrayOf('O'.code.toByte(), 'B'.code.toByte(), 'M'.code.toByte(), '9'.code.toByte(), 1, 0))
        success.end()

        val error = decodeError(errorFrame())
        check(error.rule == "sample-rule")
        check(error.text == "sample text")
        check(error.status == 1)
        check(error.words?.heading == "Heading")
        check(error.words?.who == 5)

        rejects("status") { decodeResponse(errorFrame(status = 5)) }
        rejects("tone") { decodeResponse(errorFrame(tone = 4)) }
        rejects("person") { decodeResponse(errorFrame(who = 7)) }
        rejects("UTF-8") { decodeResponse(errorFrame(heading = byteArrayOf(0xff.toByte()))) }
        rejects("truncation") { decodeResponse(errorFrame().copyOf(errorFrame().size - 1)) }
        rejects("trailing bytes") { decodeResponse(errorFrame(trailingByte = true)) }
    }

    private fun decodeError(frame: ByteArray): KeyException = try {
        decodeResponse(frame)
        error("expected an OBM9 error")
    } catch (e: KeyException) {
        e
    }

    private inline fun rejects(label: String, block: () -> Unit) {
        val e = try {
            block()
            error("accepted invalid $label")
        } catch (e: KeyException) {
            e
        }
        check(e.rule == "response-malformed") { "$label: ${e.rule}" }
    }

    private fun errorFrame(
        status: Int = 1,
        tone: Int = 0,
        who: Int = 5,
        heading: ByteArray = "Heading".toByteArray(Charsets.UTF_8),
        trailingByte: Boolean = false,
    ): ByteArray {
        val out = ByteArrayOutputStream()
        out.write(byteArrayOf('O'.code.toByte(), 'B'.code.toByte(), 'M'.code.toByte(), '9'.code.toByte()))
        out.write(1)
        out.write(status)
        string16(out, "sample-rule".toByteArray(Charsets.UTF_8))
        string16(out, "sample text".toByteArray(Charsets.UTF_8))
        out.write(tone)
        out.write(who)
        string16(out, heading)
        string16(out, "What happened.".toByteArray(Charsets.UTF_8))
        string16(out, "Try again.".toByteArray(Charsets.UTF_8))
        out.write(0) // code absent
        out.write(0) // at absent
        out.write(0) // quoted absent
        out.write(0) // detail absent
        if (trailingByte) out.write(0x55)
        return out.toByteArray()
    }

    private fun string16(out: ByteArrayOutputStream, bytes: ByteArray) {
        out.write(bytes.size and 0xff)
        out.write((bytes.size ushr 8) and 0xff)
        out.write(bytes)
    }
}
