package org.openbmt.app

import java.io.File
import java.io.IOException
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

    @JvmStatic
    fun main(args: Array<String>) {
        checkerTests()
        wordsModelTests()
        formatTests()
        durableTests()
        println("CommonModelTest: $count checks passed")
    }
}
