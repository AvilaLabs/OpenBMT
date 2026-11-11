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

    @JvmStatic
    fun main(args: Array<String>) {
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
        println("CommonModelTest: $count checks passed")
    }
}
