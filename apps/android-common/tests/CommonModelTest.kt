package org.openbmt.app

import java.io.File
import java.io.IOException
import java.nio.ByteBuffer
import java.nio.file.Files

/**
 * The tests of the code both apps share: text helpers, the words model, the
 * checks of the app's own words and the durable write. They run on the
 * desktop JVM, where no Android class exists.
 */
object CommonModelTest {
    private var count = 0

    private fun ok(cond: Boolean, what: String) {
        count++
        if (!cond) throw AssertionError("FAILED: $what")
    }

    private fun words(heading: String, what: String, next: String, tone: Int = 0, who: Int = 5, detail: String? = null) =
        SayWords(tone, who, heading, what, next, null, null, null, detail)

    private fun checkerTests() {
        ok(SayCheck.problem(words("Fine", "This is fine.", "Do nothing.")) == null, "plain words pass")
        ok(SayCheck.problem(words("Bad", "no full stop", "Fine.")) != null, "the checker finds a missing full stop")
        ok(SayCheck.problem(words("Bad", "A hash is here.", "Fine.")) != null, "the checker finds jargon")
        ok(SayCheck.problem(words("Bad", "Rule SR-35 broke.", "Fine.")) != null, "the checker finds a rule number")
        ok(SayCheck.problem(words("Bad", "See 0xabc.", "Fine.")) != null, "the checker finds notation")
        ok(SayCheck.problem(words("Bad", "Check 123456789 now.", "Fine.")) != null, "the checker finds a raw number")
        ok(SayCheck.problem(words("Bad", "Look at deadbeef0123.", "Fine.")) != null, "the checker finds long hex")
        ok(SayCheck.problem(words("Bad", "It broke.", "It broke.")) != null, "the checker finds two equal sentences")
        ok(SayCheck.problem(words("A heading that is far too long to fit", "It broke.", "Fix it.")) != null, "a long heading")
        ok(SayCheck.problem(words("Bad", "It broke.", "Fix it.", tone = 4)) != null, "a tone outside 0 to 3")
        ok(SayCheck.problem(words("Bad", "It broke.", "Fix it.", detail = "x".repeat(201))) != null, "a long detail")
        ok(SayCheck.problem(words("Fine", "A sale of 12,345 sats.", "Wait.")) == null, "separated numbers pass")
        val dirty = SayWords(0, 5, "H", "W.", "N.", null, null, SayQuoted("From", "a\u0007b"), null)
        ok(SayCheck.problem(dirty) != null, "quoted text that is not clean")
        val same = listOf("a" to words("H", "Same.", "One."), "b" to words("H", "Same.", "Two."))
        ok(SayCheck.distinctProblem(same) != null, "two that share a heading and a first sentence")
        ok(SayCheck.distinctProblem(listOf("a" to words("H", "One.", "x."), "b" to words("H", "Two.", "x."))) == null, "different")
    }

    private fun wordsModelTests() {
        val w = SayWords(
            2, 5, "Over the limit", "This is over.", "Pay less.", "SR-35", 1_700_000_000L,
            SayQuoted("The cashier wrote", "hello‮\u0007  world\nagain"), "a detail",
        )
        val m = WordsModel.of(w) { "T$it" }
        ok(m.colour == Colour.Red, "stop is red")
        ok(m.who == "The key holder does this step.", "who as a sentence")
        ok(m.details == listOf("Rule: SR-35", "Time: T1700000000", "Detail: a detail"), "the fold: ${m.details}")
        ok(m.quotedText == "“hello world again”", "quoted text cleaned and in quotation marks: ${m.quotedText}")
        ok(m.quotedLabel!!.contains("elsewhere") && m.quotedLabel!!.contains("The cashier wrote"), "quoted text is marked")
        ok(!m.heading.contains("hello") && !m.what.contains("hello") && !m.next.contains("hello"), "quoted text is not in the sentences")
        val plain = WordsModel.of(SayWords(1, 0, "Done", "It is done.", "Nothing to do.", null, null, null, null))
        ok(plain.colour == Colour.Green && plain.details.isEmpty() && plain.quotedText == null, "no fold, no quote")
        ok(WordsModel.colourOf(0) == Colour.Blue && WordsModel.colourOf(3) == Colour.Neutral, "tone colours")
        ok(WordsModel.buttonColour(Tones.STOP) == Colour.Blue && WordsModel.buttonColour(Tones.GOOD) == Colour.Green && WordsModel.buttonColour(Tones.INFO) == Colour.Blue, "a stop screen's main button is blue, the others take the tone")
        ok(WordsModel.whoText(Who.MANAGER) == "A manager does this step.", "a manager")
        ok(WordsModel.whoText(Who.CASHIER) == "The cashier does this step.", "a cashier")
        val long = SayWords(0, 0, "H", "W.", "N.", null, null, SayQuoted("From", "x".repeat(300)), null)
        ok(WordsModel.of(long).quotedText!!.length == 64 + 2, "quoted text cut at 64 characters")
    }

    private fun formatTests() {
        ok(Format.sats(1) == "1 sat" && Format.sats(12345) == "12,345 sats" && Format.sats(0) == "0 sats", "sats")
        ok(Format.sats(1_000_000) == "1,000,000 sats", "sats with two separators")
        ok(Format.fromHex("00ff10")!!.contentEquals(byteArrayOf(0, -1, 16)), "hex")
        ok(Format.fromHex("0") == null && Format.fromHex("zz") == null, "bad hex")
        ok(Format.toAmount("1,000") == 1000L && Format.toAmount("") == null && Format.toAmount("-1") == null, "amounts")
        ok(Format.toAmount("1".repeat(19)) == null, "amounts have at most 18 digits")
        ok(Format.toFlips("HT h,t") == listOf(true, false, true, false) && Format.toFlips("HX") == null, "flips")
        ok(Format.toRolls("1 2 7 6x") == "126", "rolls")
        ok(Format.cleanForeign("  a\tb ​ c  ") == "a b c", "foreign text is cleaned")
    }

    private fun durableTests() {
        val dir = Files.createTempDirectory("durable-test").toFile()
        val file = File(dir, "x.bin")
        DurableFile.write(dir, file, byteArrayOf(1, 2, 3))
        ok(file.readBytes().contentEquals(byteArrayOf(1, 2, 3)), "a write lands")
        ok(dir.list()!!.sorted() == listOf("x.bin"), "no temporary file is left: ${dir.list()!!.toList()}")
        File(dir, "x.bin.tmp").writeBytes(byteArrayOf(9))
        DurableFile.write(dir, file, byteArrayOf(4))
        ok(file.readBytes().contentEquals(byteArrayOf(4)), "a write replaces a leftover temporary file")
        ok(dir.list()!!.sorted() == listOf("x.bin"), "the leftover is gone")
        // A directory that is not there: the write fails and leaves nothing.
        val missing = File(dir, "nope")
        var failed = false
        try {
            DurableFile.write(missing, File(missing, "y.bin"), byteArrayOf(1))
        } catch (_: IOException) {
            failed = true
        }
        ok(failed, "a write into a missing folder fails")
        dir.listFiles()?.forEach { it.delete() }
        dir.delete()
    }

    private fun frameSizeTests() {
        val offered = listOf(1920 to 1080, 1280 to 720, 640 to 480, 4000 to 3000, 1600 to 1200)
        ok(QrFrames.chooseSize(offered) == 1280 to 720, "1280 by 720 is chosen when offered")
        ok(QrFrames.chooseSize(listOf(1920 to 1080, 800 to 600, 4000 to 3000)) == 800 to 600, "the nearest in area: 800 by 600 over 1920 by 1080")
        ok(QrFrames.chooseSize(listOf(960 to 960, 1152 to 800)) == 1152 to 800, "the nearer shape wins a tie in area")
        ok(QrFrames.chooseSize(listOf(320 to 240, 176 to 144)) == 320 to 240, "only small sizes: the largest")
        ok(QrFrames.chooseSize(listOf(320 to 240, 640 to 480)) == 640 to 480, "a size with a short side of 480 beats a small one")
        ok(QrFrames.chooseSize(emptyList()) == null, "nothing offered")
        ok(QrFrames.chooseSize(listOf(0 to 0, -1 to 5)) == null, "sizes that are not positive")
        ok(QrFrames.chooseSize(listOf(720 to 1280, 1280 to 720)) == 1280 to 720, "a tie in everything goes to the wider")
    }

    /** A plane of [h] rows of [rs] bytes where the byte at (x, y) is (y * 16 + x) mod 251. */
    private fun plane(w: Int, h: Int, rs: Int, ps: Int): ByteBuffer {
        val size = (h - 1) * rs + (w - 1) * ps + 1
        val b = ByteArray(size) { 99 }
        for (y in 0 until h) for (x in 0 until w) b[y * rs + x * ps] = ((y * 16 + x) % 251).toByte()
        return ByteBuffer.wrap(b)
    }

    private fun lumaTests() {
        val w = 20
        val h = 6
        val strided = QrFrames.copyLuma(plane(w, h, 32, 1), w, h, 32, 1, false)!!
        ok(strided.stride == 32 && strided.bytes.size == 5 * 32 + 20, "a strided copy keeps the row stride and cuts the last row")
        ok((0 until h).all { y -> (0 until w).all { x -> strided.bytes[y * 32 + x] == ((y * 16 + x) % 251).toByte() } }, "strided pixels are where the stride says")
        val tight = QrFrames.copyLuma(plane(w, h, 32, 1), w, h, 32, 1, true)!!
        ok(tight.stride == w && tight.bytes.size == w * h, "a tight copy has rows of the width")
        ok((0 until h).all { y -> (0 until w).all { x -> tight.bytes[y * w + x] == ((y * 16 + x) % 251).toByte() } }, "tight pixels are in order")
        val spread = QrFrames.copyLuma(plane(w, h, 48, 2), w, h, 48, 2, false)!!
        ok(spread.stride == w && spread.bytes.size == w * h, "a pixel stride of 2 is gathered into tight rows")
        ok((0 until h).all { y -> (0 until w).all { x -> spread.bytes[y * w + x] == ((y * 16 + x) % 251).toByte() } }, "gathered pixels are in order")
        val exact = QrFrames.copyLuma(plane(w, h, w, 1), w, h, w, 1, false)!!
        ok(exact.stride == w && exact.bytes.size == w * h, "no padding: the plane as it is")
        val short = ByteBuffer.wrap(ByteArray(5 * 32 + 19))
        ok(QrFrames.copyLuma(short, w, h, 32, 1, false) == null, "a plane one byte short is refused")
        ok(QrFrames.copyLuma(plane(w, h, 32, 1), w, h, 10, 1, false) == null, "a row stride under the width is refused")
        ok(QrFrames.copyLuma(plane(w, h, 32, 1), 0, h, 32, 1, false) == null, "a width of 0 is refused")
        ok(QrFrames.copyLuma(plane(w, h, 32, 1), w, h, 32, 0, false) == null, "a pixel stride of 0 is refused")
        val src = plane(w, h, 32, 1)
        src.position(7)
        src.limit(100)
        QrFrames.copyLuma(src, w, h, 32, 1, false)
        ok(src.position() == 7 && src.limit() == 100, "the plane's position and limit are not changed")
        val big = QrFrames.copyLuma(ByteBuffer.wrap(ByteArray(1280 * 720)), 1280, 720, 1280, 1, false)!!
        ok(big.bytes.size == 1280 * 720, "a 1280 by 720 plane")
    }

    private fun throttleTests() {
        val t = DecodeThrottle(125)
        ok(t.admit(1000), "the first frame is read")
        ok(!t.admit(1050) && !t.admit(1124), "frames within 125 ms are dropped")
        ok(t.admit(1125), "the next frame after 125 ms is read")
        ok(!t.admit(1200), "and the gap counts from the last frame read")
        val steady = DecodeThrottle(125)
        val taken = (0 until 300).count { steady.admit(it * 33L) }
        ok(taken in 70..80, "30 frames a second give about 8 reads a second: $taken in 9.9 seconds")
        val back = DecodeThrottle(125)
        back.admit(5000)
        ok(back.admit(100), "a clock that went back is not stuck")
    }

    private fun replyTests() {
        val d = QrReply.parse(byteArrayOf(0, 'h'.code.toByte(), 'i'.code.toByte()))
        ok(d is QrReply.Decoded && String(d.bytes, Charsets.UTF_8) == "hi", "a decoded reply")
        ok((QrReply.parse(byteArrayOf(0)) as QrReply.Decoded).bytes.isEmpty(), "an empty code")
        val r = QrReply.parse(byteArrayOf(1) + "QR-12".toByteArray())
        ok(r is QrReply.Refused && r.code == "QR-12", "a refusal carries its rule code")
        ok(QrReply.parse(null) is QrReply.Fault, "no answer")
        ok(QrReply.parse(ByteArray(0)) is QrReply.Fault, "an empty answer")
        ok(QrReply.parse(byteArrayOf(2, 1)) is QrReply.Fault, "an unknown first byte")
        ok(QrReply.parse(byteArrayOf(1, 7)) is QrReply.Fault, "a rule code that is not text")
    }

    private fun urTests() {
        ok(UrText.isMultiPart("ur:crypto-psbt/1-5/lpadaxcfax"), "a part of a sequence")
        ok(UrText.isMultiPart("UR:CRYPTO-PSBT/12-30/ABC"), "upper case")
        ok(UrText.isMultiPart("Ur:bytes/2-3/xyz"), "mixed case")
        ok(!UrText.isMultiPart("ur:crypto-psbt/hdcxlkdadkc"), "a single-part UR has no sequence")
        ok(!UrText.isMultiPart("ur:bytes"), "a type only")
        ok(!UrText.isMultiPart("openbmt-counter-pairing 1\nur:x/1-2/y"), "not at the start")
        ok(!UrText.isMultiPart("BITCOIN:BC1Q"), "not a UR at all")
        ok(!UrText.isMultiPart("ur:t/a-b/c"), "the part is not numbers")
        ok(UrText.isMultiPart("ur:crypto-psbt/1-5/lpad".toByteArray()), "bytes of a part")
        ok(!UrText.isMultiPart(ByteArray(0)), "no bytes")
    }

    private fun cameraWordsTests() {
        for (who in listOf(Who.KEY_HOLDER, Who.MANAGER)) {
            val off = CameraWords.off(who)
            ok(SayCheck.problem(off) == null, "the camera-off words pass the checks: ${SayCheck.problem(off)}")
            ok(off.what.contains("camera") && off.next.contains("settings") && off.next.contains("paste") && off.next.contains("file"), "they say what to do")
            val no = CameraWords.notStarted(who, "a problem")
            ok(SayCheck.problem(no) == null, "the camera-not-started words pass: ${SayCheck.problem(no)}")
            ok(SayCheck.problem(CameraWords.notStarted(who, null)) == null, "and without a detail")
            ok(SayCheck.distinctProblem(listOf("a" to off, "b" to no)) == null, "the two read differently")
        }
        ok(CameraWords.notStarted(Who.KEY_HOLDER, "x".repeat(500)).detail!!.length <= 200, "a long detail is cut")
    }

    // ---- the UR call: framing, frames, reading, words ----------------------------------------------------------

    private fun reply(status: Int, body: ByteArray, magic: ByteArray = UrCall.REPLY_MAGIC, version: Int = UrCall.VERSION) =
        magic + byteArrayOf(version.toByte(), status.toByte()) + body

    private fun okReply(vararg body: Int) = reply(0, ByteArray(body.size) { body[it].toByte() })

    private fun refusedReply(code: String) = reply(1, code.toByteArray())

    private fun u32(n: Long) = ByteArray(4) { (n shr (24 - 8 * it)).toByte() }

    private fun urCallTests() {
        // The request is exactly the framing.
        var seen: ByteArray? = null
        val call = UrCall { request -> seen = request.copyOf(); okReply(0, 0, 0, 7) }
        ok(call.newDecoder().let { it is UrOutcome.Ok && it.value == 7L }, "a handle")
        ok(seen!!.contentEquals(byteArrayOf(0x4f, 0x42, 0x4d, 0x6f, 1, 3)), "a decoder_new request is the magic, the version and the operation")
        val rec = UrCall { request -> seen = request.copyOf(); okReply(1, 0, 0, 0, 3, 0, 0, 0, 3, 0, 15, 66, 64) }
        val p = (rec.receive(0x01020304L, "ur:x") as UrOutcome.Ok).value
        ok(p.complete && p.received == 3 && p.expected == 3 && p.fraction == 1.0, "a progress reply")
        ok(
            seen!!.contentEquals(byteArrayOf(0x4f, 0x42, 0x4d, 0x6f, 1, 4, 1, 2, 3, 4, 0, 0, 0, 4, 'u'.code.toByte(), 'r'.code.toByte(), ':'.code.toByte(), 'x'.code.toByte())),
            "a receive request",
        )
        var big: ByteArray? = null
        UrCall { r -> big = r.copyOf(); null }.urEncode("bytes", ByteArray(300), 230)
        ok(big!![6].toInt() == 5 && String(big!!, 7, 5) == "bytes" && big!![12].toInt() == 0 && (big!![13].toInt() and 0xff) == 230, "a ur_encode request")
        ok(big!!.size == 6 + 1 + 5 + 2 + 4 + 300, "and its length")
        ok(UrCall { null }.newDecoder() is UrOutcome.Fault, "no answer is a fault")
        ok(UrCall { ByteArray(0) }.newDecoder() is UrOutcome.Fault, "an empty answer is a fault")
        ok(UrCall { okReply(0, 0, 0, 7) }.qrEncode(ByteArray(1), 4).let { it is UrOutcome.Refused && it.code == "UR-21" }, "a level outside 0 to 3 is not sent")
        ok(UrCall { throw AssertionError("sent") }.urEncode("x".repeat(256), ByteArray(1), 100) is UrOutcome.Refused, "a type over 255 bytes is not sent")
        ok(UrCall { throw AssertionError("sent") }.urEncode("bytes", ByteArray(1), 70000) is UrOutcome.Refused, "a fragment over 65535 is not sent")

        // Replies are read strictly: every cut, an extra byte and a changed header byte are faults.
        val good = okReply(0, 0, 0, 7)
        for (cut in good.indices) ok(UrCall { good.copyOfRange(0, cut) }.newDecoder() is UrOutcome.Fault, "a reply cut at $cut")
        ok(UrCall { good + byteArrayOf(0) }.newDecoder() is UrOutcome.Fault, "an extra byte after the body")
        for (at in 0 until 6) for (bit in 0 until 8) {
            val flipped = good.copyOf()
            flipped[at] = (flipped[at].toInt() xor (1 shl bit)).toByte()
            ok(UrCall { flipped }.newDecoder() is UrOutcome.Fault, "a reply with byte $at bit $bit changed")
        }
        ok(UrCall { refusedReply("UR-12") }.newDecoder().let { it is UrOutcome.Refused && it.code == "UR-12" }, "a refusal carries its rule")
        ok(UrCall { refusedReply("QR-01") }.qrEncode(ByteArray(1), 0).let { it is UrOutcome.Refused && it.code == "QR-01" }, "a QR refusal")
        for (bad in listOf("UR-1", "UR-123", "ur-12", "XX-12", "UR-1x", "")) {
            ok(UrCall { refusedReply(bad) }.newDecoder() is UrOutcome.Fault, "a refusal '$bad' is no rule code")
        }
        ok(UrCall { reply(0, ByteArray(4), magic = UrCall.REQUEST_MAGIC) }.newDecoder() is UrOutcome.Fault, "a request's magic is not a reply's")
        ok(UrCall { reply(2, ByteArray(4)) }.newDecoder() is UrOutcome.Fault, "a status that is neither")

        // Each operation's body must be exactly its shape.
        ok(UrCall { okReply(2, 0, 0, 0, 3, 0, 0, 0, 3, 0, 0, 0, 0) }.receive(1, "x") is UrOutcome.Fault, "a complete flag that is not 0 or 1")
        ok(UrCall { okReply(0, 0, 0, 0, 3, 0, 0, 0, 3, 0, 15, 66, 65) }.receive(1, "x") is UrOutcome.Fault, "a fraction over 1")
        ok(UrCall { okReply(0, 0, 0, 0, 3, 0, 0, 0, 3, 0, 15, 66) }.receive(1, "x") is UrOutcome.Fault, "a progress that is a byte short")
        ok(UrCall { okReply(0, 0, 0, 7, 0) }.newDecoder() is UrOutcome.Fault, "a handle with a byte over")
        ok(UrCall { okReply(0) }.drop(1) is UrOutcome.Fault, "a drop reply with a body")
        ok(UrCall { okReply() }.drop(1) is UrOutcome.Ok, "a drop reply with none")
        val msg = byteArrayOf(5) + "bytes".toByteArray() + u32(3) + byteArrayOf(1, 2, 3)
        val m = (UrCall { reply(0, msg) }.result(1) as UrOutcome.Ok).value
        ok(m.type == "bytes" && m.payload.contentEquals(byteArrayOf(1, 2, 3)), "a result")
        ok(UrCall { reply(0, msg + byteArrayOf(9)) }.result(1) is UrOutcome.Fault, "a result with a byte over")
        ok(UrCall { reply(0, msg.copyOfRange(0, msg.size - 1)) }.result(1) is UrOutcome.Fault, "a result with a byte short")
        keyTextTests()
        val side = 21
        val packed = ByteArray((side * side + 7) / 8)
        val qr = reply(0, byteArrayOf(0, side.toByte()) + packed)
        ok(UrCall { qr }.qrEncode(ByteArray(1), 0) is UrOutcome.Ok, "a drawn code")
        ok(UrCall { reply(0, byteArrayOf(0, side.toByte()) + packed + byteArrayOf(0)) }.qrEncode(ByteArray(1), 0) is UrOutcome.Fault, "a code with a byte over")
        ok(UrCall { reply(0, byteArrayOf(0, 20) + ByteArray(50)) }.qrEncode(ByteArray(1), 0) is UrOutcome.Fault, "a side under 21")
        val one = "UR:BYTES/AB".toByteArray()
        val parts = reply(0, u32(1) + u32(one.size.toLong()) + one)
        ok((UrCall { parts }.urEncode("bytes", ByteArray(1), 100) as UrOutcome.Ok).value == listOf("UR:BYTES/AB"), "one part")
        ok(UrCall { reply(0, u32(2) + u32(one.size.toLong()) + one) }.urEncode("bytes", ByteArray(1), 100) is UrOutcome.Fault, "a count that claims more parts than there are")
        ok(UrCall { reply(0, u32(0)) }.urEncode("bytes", ByteArray(1), 100) is UrOutcome.Fault, "no parts")
        ok(UrCall { reply(0, u32(1) + u32(900) + one) }.urEncode("bytes", ByteArray(1), 100) is UrOutcome.Fault, "a part longer than the reply")
        val matrix = QrMatrix(21, ByteArray(56).also { it[0] = 0x80.toByte(); it[55] = 0x80.toByte() })
        ok(matrix.get(0, 0) && !matrix.get(1, 0) && matrix.get(20, 20) && matrix.version == 1, "the packed modules, most significant bit first")
    }

    /** The words part of a reply with status 2: the rule code, then three texts of a 16-bit length each. */
    private fun wordsBody(code: String, vararg texts: String): ByteArray {
        var out = code.toByteArray()
        for (t in texts) {
            val b = t.toByteArray()
            out += byteArrayOf((b.size shr 8).toByte(), b.size.toByte()) + b
        }
        return out
    }

    private fun keyTextTests() {
        val text = "[56c4fac3/48h/0h/0h/2h]xpub6Ewx2N9hNSArJyF35CUGhaZLuZxQPNmJzWVwmpoV9U7Xu5wqka93nd3zEzokew9MzkNV4u6TCVDkHHR6QHQuYEFaasKzWkrkncXHMXGNdZP"
        // The request: the magic, the version, operation 7, the type with its length, the message with its length.
        var seen: ByteArray? = null
        val call = UrCall { request -> seen = request.copyOf(); reply(0, text.toByteArray()) }
        val k = call.keyText("crypto-account", byteArrayOf(1, 2, 3))
        ok(k is KeyTextOutcome.Key && k.text == text, "the key text")
        val want = byteArrayOf(0x4f, 0x42, 0x4d, 0x6f, 1, 7, 14) + "crypto-account".toByteArray() + u32(3) + byteArrayOf(1, 2, 3)
        ok(seen!!.contentEquals(want), "a key_text request is the magic, version, operation, type and message")
        // For bytes the byte string around the content is put back, in its shortest head.
        for ((n, head) in listOf(0 to byteArrayOf(0x40), 23 to byteArrayOf(0x57), 24 to byteArrayOf(0x58, 24), 255 to byteArrayOf(0x58, -1), 256 to byteArrayOf(0x59, 1, 0), 65536 to byteArrayOf(0x5a, 0, 1, 0, 0))) {
            UrCall { request -> seen = request.copyOf(); null }.keyText("bytes", ByteArray(n))
            val at = 6 + 1 + 5 + 4
            ok(seen!!.copyOfRange(at, at + head.size).contentEquals(head) && seen!!.size == at + head.size + n, "a byte string of $n bytes has the head of its shortest form")
        }
        ok(UrCall { throw AssertionError("sent") }.keyText("x".repeat(256), ByteArray(1)).let { it is KeyTextOutcome.Refused && it.code == UrCall.BAD_FRAMING }, "a type over 255 bytes is not sent")
        ok(UrCall { throw AssertionError("sent") }.keyText("", ByteArray(1)) is KeyTextOutcome.Refused, "an empty type is not sent")

        // A refusal with words.
        val words = wordsBody("UR-24", "Not a key code", "This code carries something other than one key, so nothing was taken from it.", "Scan the code that shows the key, or show the key as text and scan that.")
        val d = UrCall { reply(2, words) }.keyText("crypto-output", ByteArray(1))
        ok(d is KeyTextOutcome.Declined && d.code == "UR-24" && d.heading == "Not a key code" && d.next.startsWith("Scan"), "a refusal with its words")
        val say = UrWords.keyDeclined(Who.KEY_HOLDER, d as KeyTextOutcome.Declined)
        ok(say.tone == Tones.STOP && say.who == Who.KEY_HOLDER && say.code == "UR-24" && SayCheck.problem(say) == null, "the words pass the checks: ${SayCheck.problem(say)}")
        ok(UrCall { refusedReply("UR-21") }.keyText("bytes", ByteArray(1)).let { it is KeyTextOutcome.Refused && it.code == "UR-21" }, "a framing refusal")

        // Everything else is a fault: every cut and every byte over, a bad code, an empty text.
        for (cut in words.indices) ok(UrCall { reply(2, words.copyOfRange(0, cut)) }.keyText("bytes", ByteArray(1)) is KeyTextOutcome.Fault, "words cut at $cut")
        ok(UrCall { reply(2, words + byteArrayOf(0)) }.keyText("bytes", ByteArray(1)) is KeyTextOutcome.Fault, "words with a byte over")
        ok(UrCall { reply(2, wordsBody("ur-24", "a", "b", "c")) }.keyText("bytes", ByteArray(1)) is KeyTextOutcome.Fault, "words with no rule code")
        ok(UrCall { reply(2, wordsBody("UR-24", "", "b", "c")) }.keyText("bytes", ByteArray(1)) is KeyTextOutcome.Fault, "words with an empty text")
        ok(UrCall { reply(3, words) }.keyText("bytes", ByteArray(1)) is KeyTextOutcome.Fault, "a status that is none of the three")
        ok(UrCall { reply(0, ByteArray(0)) }.keyText("bytes", ByteArray(1)) is KeyTextOutcome.Fault, "an empty key text")
        ok(UrCall { reply(0, "key\n".toByteArray()) }.keyText("bytes", ByteArray(1)) is KeyTextOutcome.Fault, "a key text with a control character")
        ok(UrCall { reply(0, ByteArray(161) { 'a'.code.toByte() }) }.keyText("bytes", ByteArray(1)) is KeyTextOutcome.Fault, "a key text that is too long")
        ok(UrCall { null }.keyText("bytes", ByteArray(1)) is KeyTextOutcome.Fault, "no answer")
        ok(UrCall { refusedReply("UR-1") }.keyText("bytes", ByteArray(1)) is KeyTextOutcome.Fault, "a refusal that is no rule code")
    }

    private fun animatedQrTests() {
        ok(AnimatedQr.FPS == 5 && AnimatedQr.QUIET == 4 && AnimatedQr.MAX_FRAGMENT <= 250 && AnimatedQr.MAX_VERSION == 12, "the numbers of the animated code")
        ok(AnimatedQr.indexAt(0, 8) == 0 && AnimatedQr.indexAt(199, 8) == 0 && AnimatedQr.indexAt(200, 8) == 1, "five frames a second")
        ok(AnimatedQr.indexAt(1600, 8) == 0 && AnimatedQr.indexAt(1800, 8) == 1, "and round again")
        ok(AnimatedQr.indexAt(5_000_000, 1) == 0 && AnimatedQr.indexAt(-5, 8) == 0, "a single part, and a clock that went back")
        ok((0 until 40).map { AnimatedQr.indexAt(it * 200L, 8) } == (0 until 40).map { it % 8 }, "each part in turn")
        val b = AnimatedQr.box(21, 290, 400)
        ok(b.cell == 10 && b.left == 0 && b.top == 55 && b.x(0) == 40 && b.y(0) == 95 && b.fits, "a box: whole pixels and the quiet zone of 4")
        ok(!AnimatedQr.box(21, 28, 28).fits && AnimatedQr.box(21, 29, 29).fits, "a box too small for a cell")
        ok(AnimatedQr.box(65, 1080, 1920).cell == 1080 / 73, "version 12 on a phone screen")
        var failed = false
        try {
            AnimatedQr.box(0, 10, 10)
        } catch (_: IllegalArgumentException) {
            failed = true
        }
        ok(failed, "no modules")
        // The frames call the library once for each part and keep the code.
        var calls = 0
        val side = 21
        val packed = ByteArray((side * side + 7) / 8)
        val frames = AnimatedFrames(UrCall { calls++; reply(0, byteArrayOf(0, side.toByte()) + packed) }, listOf("UR:BYTES/A", "UR:BYTES/B"))
        ok(frames.animated && frames.matrix(0) != null && frames.matrix(0) != null && frames.matrix(1) != null && calls == 2, "each part is drawn once: $calls")
        ok(frames.matrix(2) == null && frames.matrix(-1) == null, "no such part")
        ok(frames.drawAll() == null && calls == 2, "all drawn")
        ok(AnimatedFrames(UrCall { null }, listOf("UR:BYTES/A")).let { !it.animated && it.matrix(0) == null && it.drawAll() == 0 }, "a code the library cannot draw")
    }

    private class Script(private val replies: List<ByteArray?>) {
        var next = 0
        val call = UrCall { replies.getOrNull(next++) }
    }

    private fun progress(complete: Boolean, received: Int, expected: Int) =
        okReply(if (complete) 1 else 0, 0, 0, 0, received, 0, 0, 0, expected, 0, 0, 0, if (complete) 0 else 1)

    private fun readingTests() {
        val handle = okReply(0, 0, 0, 1)
        // A part: the decoder is opened with the first, and progress is shown in words.
        val s = Script(listOf(handle, progress(false, 2, 9), progress(false, 3, 9)))
        val r = UrReading(s.call, setOf("bytes"), Who.KEY_HOLDER)
        ok(!r.started, "nothing open before the first part")
        val a = r.feed("ur:bytes/2-9/x")
        ok(a is UrStep.Show && a.words.what == "Reading the animated code: 2 of about 9 parts.", "the progress words")
        ok(r.started && s.next == 2, "one decoder, opened once")
        ok(r.feed("ur:bytes/3-9/x") is UrStep.Show && s.next == 3, "the next part reuses it")
        // Refusals: a restart is told, misreads are skipped, the call's faults stop the scan.
        val t = Script(listOf(handle, refusedReply("UR-12"), refusedReply("UR-06"), refusedReply("UR-07"), refusedReply("UR-11"), refusedReply("UR-22")))
        val r2 = UrReading(t.call, setOf("bytes"), Who.MANAGER)
        val restart = r2.feed("x")
        ok(restart is UrStep.Show && restart.words.heading == "Code did not add up" && restart.words.tone == Tones.STOP, "UR-12 starts again with words")
        ok(r2.feed("x") === UrStep.Ignore && r2.feed("x") === UrStep.Ignore && r2.feed("x") === UrStep.Ignore, "misreads are skipped")
        ok(r2.feed("x").let { it is UrStep.Fail && it.words.heading == "Code reader failed" }, "a fault of the call stops the scan")
        val strangers = Script(listOf(handle) + List(UrReading.STRANGER_RUN * 2) { refusedReply("UR-10") })
        val r3 = UrReading(strangers.call, setOf("bytes"), Who.MANAGER)
        val steps = (1..UrReading.STRANGER_RUN * 2).map { r3.feed("x") }
        ok(steps.withIndex().all { (i, st) -> ((i + 1) % UrReading.STRANGER_RUN == 0) == (st is UrStep.Show) }, "another stream is told after each run of ${UrReading.STRANGER_RUN}")
        // A decoder that cannot be opened, a call with no answer.
        ok(UrReading(Script(listOf(refusedReply("UR-22"))).call, setOf("bytes"), 0).feed("x") is UrStep.Fail, "no decoder")
        ok(UrReading(Script(listOf(handle, null)).call, setOf("bytes"), 0).feed("x") is UrStep.Fail, "no answer to a part")
        // Complete: the type decides.
        fun result(type: String, payload: ByteArray) = reply(0, byteArrayOf(type.length.toByte()) + type.toByteArray() + u32(payload.size.toLong()) + payload)
        val done = UrReading(Script(listOf(handle, progress(true, 4, 4), result("bytes", byteArrayOf(1, 2)))).call, setOf("bytes", "crypto-psbt"), 0).feed("x")
        ok(done is UrStep.Done && done.type == "bytes" && done.payload.contentEquals(byteArrayOf(1, 2)), "a finished message")
        val wrong = UrReading(Script(listOf(handle, progress(true, 4, 4), result("psbt", byteArrayOf(1)))).call, setOf("bytes"), 0).feed("x")
        ok(wrong is UrStep.Fail && wrong.words.heading == "Not the code expected", "a type that is not taken")
        val refused = UrReading(Script(listOf(handle, progress(true, 4, 4), refusedReply("UR-20"))).call, setOf("bytes"), 0).feed("x")
        ok(refused is UrStep.Fail && refused.words.heading == "Not the code expected", "UR-20 is the wrong kind")
        val bad = UrReading(Script(listOf(handle, progress(true, 4, 4), refusedReply("UR-13"))).call, setOf("bytes"), 0).feed("x")
        ok(bad is UrStep.Fail && bad.words.heading == "Code content not usable", "content that was refused")
        // Dropping closes the decoder once.
        val d = Script(listOf(handle, progress(false, 1, 3), okReply()))
        val r4 = UrReading(d.call, setOf("bytes"), 0)
        r4.feed("x")
        r4.drop()
        r4.drop()
        ok(!r4.started && d.next == 3, "dropped once: ${d.next}")
        UrReading(Script(emptyList()).call, setOf("bytes"), 0).drop()
    }

    private fun urWordsTests() {
        for (who in listOf(Who.KEY_HOLDER, Who.MANAGER, Who.OWNER)) {
            val all = UrWords.all(who)
            for ((name, w) in all) ok(SayCheck.problem(w) == null, "$name passes the checks: ${SayCheck.problem(w)}")
            ok(SayCheck.distinctProblem(all) == null, "the words read differently")
            ok(all.all { it.second.who == who }, "who does the next step")
        }
        ok(UrWords.reading(Who.KEY_HOLDER, 3, 9).what == "Reading the animated code: 3 of about 9 parts.", "the progress sentence")
        ok(UrWords.cannotShow(0).next.contains("file") && UrWords.wrongKind(0).next.contains("file"), "a file is the way round")
        ok(UrText.startsWithUr("ur:bytes/x".toByteArray()) && UrText.startsWithUr("UR:BYTES".toByteArray()) && UrText.startsWithUr("Ur:".toByteArray()), "a UR starts with ur: in any case")
        ok(!UrText.startsWithUr("u".toByteArray()) && !UrText.startsWithUr("BITCOIN:".toByteArray()) && !UrText.startsWithUr(" ur:".toByteArray()), "others do not")
    }

    /** SR-77: the time in a picker or a permission dialog counts toward the 60 seconds. */
    private fun externalWaits() {
        val t0 = 5_000_000L
        ok(ExternalWait.decide(t0, t0 + 59_000) == WaitOutcome.Continue, "a result after 59 s continues")
        ok(ExternalWait.decide(t0, t0 + 60_000) == WaitOutcome.Continue, "60 s exactly is not more than 60 s")
        ok(ExternalWait.decide(t0, t0 + 61_000) == WaitOutcome.Lock, "a result after 61 s locks")
        // A resume at 30 s lets the wait go on, and the result that comes after it at 61 s locks.
        ok(ExternalWait.decide(t0, t0 + 30_000) == WaitOutcome.Continue, "a resume at 30 s waits for the result")
        ok(ExternalWait.decide(t0, t0 + 61_000) == WaitOutcome.Lock, "the result that follows that resume at 61 s locks")
        // A result at 10 s is taken, and the resume that follows has no wait left to decide on.
        ok(ExternalWait.decide(t0, t0 + 10_000) == WaitOutcome.Continue, "a result at 10 s is taken")
        // A cancelled picker is a result like any other: past 60 s it locks too.
        ok(ExternalWait.decide(t0, t0 + 90_000) == WaitOutcome.Lock, "a cancelled picker after 90 s locks")
        ok(ExternalWait.decide(null, t0) == WaitOutcome.Lock, "no wait noted fails closed")
        ok(ExternalWait.decide(t0, t0 - 1) == WaitOutcome.Lock, "a clock that went back locks")
    }

    /** The lock model both apps use (LockModel.kt): the tries table and file, the wipe order over a list of files, the relock rule. */
    private fun lockModelTests() {
        val waits = listOf(0L, 0, 0, 0, 0, 30, 60, 300, 900, 3600)
        for ((n, w) in waits.withIndex()) ok(Tries.waitSeconds(n) == w, "$n wrong tries wait $w s")
        ok(Tries.WIPE_AT == 10 && Tries.WARN_FROM == 5, "the wipe and the warning counts")
        val t0 = 1_000_000L
        for (n in 1..9) {
            val c = Tries.check(TriesRecord(n, t0), t0)
            ok(c.waitLeft == waits[n] && !c.wipe, "right after $n wrong: wait ${waits[n]}")
        }
        ok(Tries.check(TriesRecord(10, t0), t0).wipe, "10 wrong tries wipe")
        ok(Tries.check(TriesRecord(6, t0), t0 - 500).record == TriesRecord(6, t0 - 500), "a clock behind restarts the wait from now")
        ok(Tries.before(TriesRecord(3, 5), 99) == TriesRecord(4, 99) && Tries.afterRight() == TriesRecord(0, 0), "one more before, none after")
        ok(Relock.needed(null, 5) && !Relock.needed(1000, 61_000) && Relock.needed(1000, 61_001) && Relock.needed(1000, 500), "the 60-second rule")
        externalWaits()
        val dir = Files.createTempDirectory("lock-model").toFile()
        try {
            val tries = TriesFile(dir)
            ok(tries.read(5) == TriesRecord(0, 0) && !tries.exists(), "no file: no wrong tries")
            tries.write(TriesRecord(7, 1234))
            ok(tries.read(5) == TriesRecord(7, 1234) && File(dir, TriesFile.NAME).length() == 17L, "the record is read back in 17 bytes")
            for (bad in listOf(ByteArray(16), ByteArray(18), ByteArray(17))) {
                File(dir, TriesFile.NAME).writeBytes(bad)
                ok(tries.read(77) == TriesRecord(9, 77), "a file that is not exact counts as 9 wrong tries just now")
            }
            // The wipe order over two files: the files first, in the order given, then the key, then the tries.
            fun setUp() {
                File(dir, "one.obks").writeBytes(byteArrayOf(1))
                File(dir, "two.obks").writeBytes(byteArrayOf(2))
                File(dir, "one.obks.tmp").writeBytes(byteArrayOf(3))
                tries.write(TriesRecord(10, 5))
            }
            setUp()
            val seen = ArrayList<String>()
            WipeOrder.run(dir, listOf("one.obks", "two.obks"), tries) {
                seen.add("key")
                ok(!File(dir, "one.obks").exists() && !File(dir, "two.obks").exists() && !File(dir, "one.obks.tmp").exists() && tries.exists(), "the files go before the key, the tries file after")
            }
            ok(seen == listOf("key") && !tries.exists(), "all of it is gone")
            // Nothing there is no trouble, and a key that stays leaves the count at 10.
            WipeOrder.run(dir, listOf("one.obks"), tries) { }
            setUp()
            var threw = false
            try {
                WipeOrder.run(dir, listOf("one.obks", "two.obks"), tries) { throw IOException("still there") }
            } catch (_: IOException) {
                threw = true
            }
            ok(threw && tries.read(0).wrong == 10, "a key that stays throws and the count stays at 10")
        } finally {
            dir.deleteRecursively()
        }
    }

    @JvmStatic
    fun main(args: Array<String>) {
        lockModelTests()
        checkerTests()
        wordsModelTests()
        formatTests()
        durableTests()
        frameSizeTests()
        lumaTests()
        throttleTests()
        replyTests()
        urTests()
        cameraWordsTests()
        urCallTests()
        animatedQrTests()
        readingTests()
        urWordsTests()
        println("CommonModelTest: $count checks passed")
    }
}
