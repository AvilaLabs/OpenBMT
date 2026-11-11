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

        ceremonyChecks()
        println("ProtocolDecoderTest: passed")
    }

    // ---- ops 80 to 83: key 3 and the wallet setup ------------------------------------------

    private fun u32(out: ByteArrayOutputStream, v: Int) {
        for (i in 0 until 4) out.write((v ushr (8 * i)) and 0xff)
    }

    private fun string32(out: ByteArrayOutputStream, v: String) {
        val b = v.toByteArray(Charsets.UTF_8)
        u32(out, b.size)
        out.write(b)
    }

    private fun strings32(out: ByteArrayOutputStream, v: List<String>) {
        u32(out, v.size)
        for (x in v) string32(out, x)
    }

    private fun okFrame(fill: (ByteArrayOutputStream) -> Unit): ByteArray {
        val out = ByteArrayOutputStream()
        out.write("OBM9".toByteArray(Charsets.US_ASCII))
        out.write(1)
        out.write(0)
        fill(out)
        return out.toByteArray()
    }

    private fun key3Body(out: ByteArrayOutputStream) {
        out.write(1) // mainnet
        string32(out, "a1b2c3d4")
        string32(out, "48h/0h/0h/2h")
        string32(out, "xpub-text")
    }

    private fun ceremonyChecks() {
        // Op 80: the key 3 view.
        val r = decodeResponse(okFrame { key3Body(it) })
        val v = r.key3View()
        r.end()
        check(v == Key3View(true, "a1b2c3d4", "48h/0h/0h/2h", "xpub-text")) { "key 3 view: $v" }
        rejects("a key 3 view cut short") { decodeResponse(okFrame { key3Body(it) }.let { it.copyOf(it.size - 1) }).let { it.key3View(); it.end() } }
        rejects("a key 3 view with trailing bytes") { decodeResponse(okFrame { key3Body(it); it.write(0) }).let { it.key3View(); it.end() } }
        rejects("a key 3 view with a bool that is 2") { decodeResponse(okFrame { it.write(2); string32(it, "a"); string32(it, "b"); string32(it, "c") }).key3View() }

        // Op 81: an option of it.
        val none = decodeResponse(okFrame { it.write(0) })
        check(none.option { none.key3View() } == null) { "key 3 absent" }
        none.end()
        val some = decodeResponse(okFrame { it.write(1); key3Body(it) })
        check(some.option { some.key3View() }?.masterFingerprint == "a1b2c3d4") { "key 3 present" }
        some.end()
        rejects("an option flag that is 2") { decodeResponse(okFrame { it.write(2) }).let { x -> x.option { x.key3View() } } }

        // Op 82: bytes.
        val answer = ByteArray(7) { it.toByte() }
        val a = decodeResponse(okFrame { u32(it, answer.size); it.write(answer) })
        check(a.bytes().contentEquals(answer)) { "answer bytes" }
        a.end()

        // Op 83: the wallet view.
        val wallet = okFrame {
            strings32(it, listOf("alpha", "bravo", "charlie", "delta"))
            u32(it, 32)
            it.write(ByteArray(32) { 9 })
            u32(it, 3)
            for ((i, fp) in listOf("11223344", "aabbccdd", "01020304").withIndex()) {
                u32(it, i)
                string32(it, fp)
                string32(it, "xpub$i")
            }
            string32(it, "bc1qexample")
            strings32(it, listOf("one", "two", "three", "four"))
            strings32(it, listOf("five", "six", "seven", "eight"))
        }
        val w = decodeResponse(wallet)
        val view = w.walletView()
        w.end()
        check(view.checksumWords == listOf("alpha", "bravo", "charlie", "delta")) { "checksum words" }
        check(view.slots.map { it.fingerprint } == listOf("11223344", "aabbccdd", "01020304") && view.slots[2].index == 2L) { "slots" }
        check(view.firstReceiveAddress == "bc1qexample" && view.walletId.size == 32) { "address and ID" }
        check(view.ownRootWords.size == 4 && view.ownerRootWords[3] == "eight") { "identity words" }
        rejects("a wallet view cut short") { decodeResponse(wallet.copyOf(wallet.size - 1)).let { it.walletView(); it.end() } }
        rejects("a wallet view with trailing bytes") { decodeResponse(wallet + byteArrayOf(0)).let { it.walletView(); it.end() } }

        // The requests: magic, version 1, op, handle, then the fields.
        val take = takeKey3Request(7, "abc")
        check(take.copyOf(5).contentEquals("OBM8".toByteArray(Charsets.US_ASCII) + byteArrayOf(1))) { "request magic and version" }
        check(take[5].toInt() == 80 && take[6].toInt() == 0) { "op 80" }
        check(take.copyOfRange(7, 15).contentEquals(byteArrayOf(7, 0, 0, 0, 0, 0, 0, 0))) { "the handle" }
        check(take.copyOfRange(15, take.size).contentEquals(byteArrayOf(3, 0, 0, 0, 'a'.code.toByte(), 'b'.code.toByte(), 'c'.code.toByte()))) { "the text" }
        val accept = acceptWalletSetupRequest(1, byteArrayOf(5, 6))
        check(accept[5].toInt() == 83 && accept[6].toInt() == 0) { "op 83" }
        check(accept.copyOfRange(15, accept.size).contentEquals(byteArrayOf(2, 0, 0, 0, 5, 6))) { "the setup bytes" }
        check(requestFrame(81, 3).size == 15 && requestFrame(82, 3)[5].toInt() == 82) { "ops 81 and 82 carry no fields" }
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
