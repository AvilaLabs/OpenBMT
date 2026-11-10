package org.openbmt.key.app

import java.io.File
import java.io.IOException
import java.nio.file.Files
import java.security.MessageDigest
import org.openbmt.key.HealthDetail
import org.openbmt.key.LineDetail
import org.openbmt.key.LineKind
import org.openbmt.key.LineView
import org.openbmt.key.PairingDetail
import org.openbmt.key.Quoted
import org.openbmt.key.SetupState
import org.openbmt.key.Words

/**
 * The tests of the pure parts of the Key app: the screen model, the words
 * model, the words themselves, the line text and the saved-state file. They
 * run on the desktop JVM, where no Android class exists. Run the main with
 * the path of docs/screens/key.md as the one argument.
 */
object AppModelTest {
    private var count = 0

    private fun ok(cond: Boolean, what: String) {
        count++
        if (!cond) throw AssertionError("FAILED: $what")
    }

    private inline fun throwsIo(what: String, block: () -> Unit) {
        count++
        try {
            block()
        } catch (_: IOException) {
            return
        }
        throw AssertionError("FAILED: no error for $what")
    }

    // ---- the checks of openbmt_core::say::check, as far as text can be checked ------

    private val jargon = listOf(
        "change mark", "receive mark", "dust", "slice", "exposure", "sighash", "p2wsh", "p2wpkh", "p2tr",
        "p2pkh", "p2sh", "witness", "outpoint", "psbt", "xpub", "descriptor", "mempool", "utxo", "nonce",
        "hash", "unix", "internal", "panic", "decode",
    )

    private fun plainProblem(text: String): String? {
        if (text.any { Character.isISOControl(it) }) return "control character"
        if (Regex("[A-Z]{2}-[0-9]").containsMatchIn(text)) return "rule number"
        if (Regex("[0-9a-fA-F]{12,}").findAll(text).any { m -> m.value.any { it.isDigit() } && m.value.any { it.isLetter() } }) {
            return "long hex"
        }
        if (Regex("[0-9]{9,}").containsMatchIn(text)) return "raw number"
        if (Regex("[A-Za-z0-9]\\(").containsMatchIn(text)) return "call notation"
        if (listOf("_", "::", "{", "}", "0x").any { text.contains(it) }) return "notation"
        val lower = text.lowercase()
        jargon.firstOrNull { lower.contains(it) }?.let { return "jargon $it" }
        return null
    }

    private fun sentenceProblem(text: String): String? {
        if (text.isEmpty() || text.length > 140) return "length"
        if (!text.endsWith(".") && !text.endsWith("?")) return "no full stop"
        if (text[0].isLowerCase()) return "small first letter"
        return plainProblem(text)
    }

    /** SC-12 as far as the app can check it; null when the words pass. */
    fun wordsProblem(w: Words): String? {
        if (w.heading.isEmpty() || w.heading.length > 28 || w.heading.endsWith(".")) return "heading length"
        plainProblem(w.heading)?.let { return "heading: $it" }
        sentenceProblem(w.what)?.let { return "what: $it" }
        sentenceProblem(w.next)?.let { return "next: $it" }
        if (w.what == w.next) return "same sentences"
        if (w.who !in 0..6 || w.tone !in 0..3) return "tone or who"
        w.detail?.let { if (it.length > 200 || it.any { c -> Character.isISOControl(c) }) return "detail" }
        w.quoted?.let { if (it.text != Format.cleanForeign(it.text)) return "quoted not clean" }
        return null
    }

    // ---- a fake cipher: not secret, only shaped like AES-GCM (12-byte IV, 16-byte tag) ------

    private class FakeSealer : Sealer {
        var counter = 0L
        var failNext = false

        private fun stream(iv: ByteArray, n: Int): ByteArray {
            val out = ByteArray(n)
            var block = 0
            var i = 0
            while (i < n) {
                val d = MessageDigest.getInstance("SHA-256").digest(iv + byteArrayOf(block.toByte(), (block shr 8).toByte()))
                for (b in d) if (i < n) out[i++] = b
                block++
            }
            return out
        }

        private fun tag(iv: ByteArray, aad: ByteArray, plain: ByteArray): ByteArray =
            MessageDigest.getInstance("SHA-256").digest(iv + aad + plain).copyOf(16)

        override fun seal(plain: ByteArray, aad: ByteArray): Sealed {
            if (failNext) {
                failNext = false
                throw IOException("the cipher failed")
            }
            counter++
            val iv = ByteArray(12) { i -> (counter shr (8 * (i % 8))).toByte() }
            val ks = stream(iv, plain.size)
            val ct = ByteArray(plain.size) { (plain[it].toInt() xor ks[it].toInt()).toByte() }
            return Sealed(iv, ct + tag(iv, aad, plain))
        }

        override fun open(sealed: Sealed, aad: ByteArray): ByteArray {
            val n = sealed.ciphertext.size - 16
            val ks = stream(sealed.iv, n)
            val plain = ByteArray(n) { (sealed.ciphertext[it].toInt() xor ks[it].toInt()).toByte() }
            if (!tag(sealed.iv, aad, plain).contentEquals(sealed.ciphertext.copyOfRange(n, n + 16))) {
                throw IOException("the saved state does not decrypt")
            }
            return plain
        }
    }

    private fun tempDir(): File = Files.createTempDirectory("keystore-test").toFile()

    // ---- tests --------------------------------------------------------------------------

    private fun wordsAreFine(keyMd: File?) {
        val all = AppWords.all()
        for ((name, w) in all) ok(wordsProblem(w) == null, "words of $name: ${wordsProblem(w)}")
        for ((i, a) in all.withIndex()) {
            for (b in all.drop(i + 1)) {
                ok(!(a.second.heading == b.second.heading && a.second.what == b.second.what), "${a.first} and ${b.first} read the same")
            }
        }
        // The setup, check and kit words equal the rows of docs/screens/key.md.
        if (keyMd != null) {
            val rows = keyMd.readLines().filter { it.startsWith("| ") }
            fun who(n: Int) = if (n == Who.NOBODY) "nobody" else "key holder"
            for ((name, w) in all.filter { it.first !in notInTheCatalogue }) {
                val row = "| ${w.heading} | ${w.what} | ${w.next} | ${who(w.who)} |"
                ok(rows.any { it.contains(row) }, "docs/screens/key.md has the words of $name")
            }
        }
    }

    // Words the app wrote itself: the library has no row for them.
    private val notInTheCatalogue = setOf(
        "debuggingOn", "wirelessDebuggingOn", "accessibilityOn", "keyInSoftware", "libraryMissing", "fault",
        "keystoreUnavailable", "fileNotRead", "fileNotSaved", "notANumber", "identityNotReadable", "flipsNotReadable", "answersReady",
    )

    private fun guardTests() {
        ok(Guard.refusals(0, 0, 0, 0).isEmpty(), "nothing on: the app may run")
        ok(Guard.refusals(1, 0, 0, 0) == listOf(AppWords.debuggingOn), "adb on")
        ok(Guard.refusals(0, 1, 0, 0) == listOf(AppWords.wirelessDebuggingOn), "wireless adb on")
        ok(Guard.refusals(0, 0, 1, 0) == listOf(AppWords.accessibilityOn), "accessibility flag on")
        ok(Guard.refusals(0, 0, 0, 2) == listOf(AppWords.accessibilityOn), "a service on")
        ok(Guard.refusals(1, 1, 1, 1).size == 3, "all three on give three words")
        ok(Guard.refusals(2, 0, 0, 0).isEmpty(), "only the value 1 counts as on")
    }

    private fun screenTests() {
        val bad = Words(0, 5, "x", "x.", "y.", null, null, null, null)
        ok(ScreenModel.pick(listOf(AppWords.debuggingOn), bad, SetupState.Empty) is Screen.Refused, "a refusal comes first")
        ok(ScreenModel.pick(emptyList(), bad, SetupState.Empty) is Screen.Fault, "a fault comes before the state")
        ok(ScreenModel.pick(emptyList(), null, null) is Screen.Fault, "no state and no fault is a fault")
        ok(ScreenModel.pick(emptyList(), null, SetupState.Empty) is Screen.Setup, "empty: setup")
        ok(ScreenModel.pick(emptyList(), null, SetupState.HasIdentity(true, true)) is Screen.ShowWords, "words waiting: show them")
        val accept = ScreenModel.pick(emptyList(), null, SetupState.HasIdentity(false, false))
        ok(accept is Screen.AcceptWallet && !accept.holdsKey2, "identity only: accept the wallet")
        val home = ScreenModel.pick(emptyList(), null, SetupState.Ready(true))
        ok(home is Screen.Home && home.holdsKey2, "ready: home")
    }

    private fun wordsModelTests() {
        val w = Words(
            2, 5, "Over the limit", "This is over.", "Pay less.", "SR-35", 1_700_000_000L,
            Quoted("The cashier wrote", "hello‮\u0007  world\nagain"), "a detail",
        )
        val m = WordsModel.of(w) { "T$it" }
        ok(m.colour == Colour.Red, "stop is red")
        ok(m.who == "The key holder does this step.", "who as a sentence")
        ok(m.details == listOf("Rule: SR-35", "Time: T1700000000", "Detail: a detail"), "the fold: ${m.details}")
        ok(m.quotedText == "“hello world again”", "quoted text cleaned and in quotation marks: ${m.quotedText}")
        ok(m.quotedLabel!!.contains("elsewhere") && m.quotedLabel!!.contains("The cashier wrote"), "quoted text is marked")
        ok(!m.heading.contains("hello") && !m.what.contains("hello") && !m.next.contains("hello"), "quoted text is not in the sentences")
        val plain = WordsModel.of(Words(1, 0, "Done", "It is done.", "Nothing to do.", null, null, null, null))
        ok(plain.colour == Colour.Green && plain.details.isEmpty() && plain.quotedText == null, "no fold, no quote")
        ok(WordsModel.colourOf(0) == Colour.Blue && WordsModel.colourOf(3) == Colour.Neutral, "tone colours")
        val long = Words(0, 0, "H", "W.", "N.", null, null, Quoted("From", "x".repeat(300)), null)
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
    }

    private fun formatOfFileTests() {
        val s = Sealed(ByteArray(12) { it.toByte() }, ByteArray(40) { 7 })
        val bytes = StoreFormat.encode(s)
        ok(bytes.size == 5 + 1 + 12 + 4 + 40, "file length")
        ok(String(bytes, 0, 4, Charsets.US_ASCII) == "OBKS" && bytes[4].toInt() == 1 && bytes[5].toInt() == 12, "header")
        val back = StoreFormat.decode(bytes)
        ok(back.iv.contentEquals(s.iv) && back.ciphertext.contentEquals(s.ciphertext), "re-decode equals")
        ok(StoreFormat.encode(back).contentEquals(bytes), "re-encode equality")
        for (cut in listOf(0, 3, 5, 17, 21, bytes.size - 1)) {
            throwsIo("cut at $cut") { StoreFormat.decode(bytes.copyOf(cut)) }
        }
        throwsIo("trailing byte") { StoreFormat.decode(bytes + byteArrayOf(0)) }
        for (at in listOf(0, 3, 4, 5)) {
            val flipped = bytes.copyOf().also { it[at] = (it[at].toInt() xor 1).toByte() }
            throwsIo("header byte $at changed") { StoreFormat.decode(flipped) }
        }
        throwsIo("a 13-byte IV") { StoreFormat.encode(Sealed(ByteArray(13), ByteArray(40))) }
        throwsIo("a ciphertext without a tag") { StoreFormat.encode(Sealed(ByteArray(12), ByteArray(15))) }
        val huge = bytes.copyOf().also { it[18] = 0x7f }
        throwsIo("an oversized length") { StoreFormat.decode(huge) }
    }

    private fun fileTests() {
        val dir = tempDir()
        val sealer = FakeSealer()
        val file = SealedFile(dir, sealer)
        ok(file.load() == null, "a first start loads null")
        val one = ByteArray(300) { it.toByte() }
        file.save(one)
        ok(file.load()!!.contentEquals(one), "save then load")
        ok(dir.list()!!.sorted() == listOf("state.obks"), "no temporary file is left: ${dir.list()!!.toList()}")
        val first = File(dir, "state.obks").readBytes()
        file.save(one)
        val second = File(dir, "state.obks").readBytes()
        ok(!first.copyOfRange(6, 18).contentEquals(second.copyOfRange(6, 18)), "a fresh IV for every write")
        ok(!String(first, Charsets.ISO_8859_1).contains(String(one.copyOf(40), Charsets.ISO_8859_1)), "the plain bytes are not in the file")

        // A failing cipher leaves the old state and no temporary file.
        sealer.failNext = true
        throwsIo("a failing cipher") { file.save(byteArrayOf(9, 9, 9)) }
        ok(file.load()!!.contentEquals(one), "the old state stands after a failed save")
        ok(dir.list()!!.sorted() == listOf("state.obks"), "no temporary file after a failed save")

        // A leftover temporary file is not read.
        File(dir, "state.obks.tmp").writeBytes(byteArrayOf(1, 2, 3))
        ok(file.load()!!.contentEquals(one), "a leftover temporary file is ignored")
        file.save(byteArrayOf(5))
        ok(file.load()!!.contentEquals(byteArrayOf(5)), "a save replaces a leftover temporary file")

        // A file that does not decrypt throws and is not replaced by load.
        val stored = File(dir, "state.obks")
        val damaged = stored.readBytes().also { it[it.size - 1] = (it[it.size - 1].toInt() xor 1).toByte() }
        stored.writeBytes(damaged)
        throwsIo("a damaged file") { file.load() }
        ok(stored.readBytes().contentEquals(damaged), "load does not touch a damaged file")

        // A different key (another sealer instance with other state) cannot read it either.
        val other = SealedFile(dir, object : Sealer {
            override fun seal(plain: ByteArray, aad: ByteArray) = throw IOException("not used")
            override fun open(sealed: Sealed, aad: ByteArray): ByteArray = throw IOException("the key is gone")
        })
        file.save(one)
        throwsIo("a lost key") { other.load() }

        // The store around it.
        var now = 1_700_000_000L
        val store = PersistentStore(file, java.security.SecureRandom()) { now }
        ok(store.load()!!.contentEquals(one), "the store loads")
        store.save(byteArrayOf(1, 2))
        ok(store.load()!!.contentEquals(byteArrayOf(1, 2)), "the store saves")
        ok(store.random(32).size == 32 && store.random(0).isEmpty(), "random has the length asked for")
        ok(!store.random(32).contentEquals(store.random(32)), "random differs between calls")
        ok(store.now() == 1_700_000_000L, "now is in seconds")
        now += 5
        ok(store.now() == 1_700_000_005L, "now follows the clock")
        dir.listFiles()?.forEach { it.delete() }
        dir.delete()
    }

    private fun lineTests() {
        val hostile = "Shop‮\u0000 1\n" + "A".repeat(100)
        val line = LineView(
            3, LineKind.Pairing, "", true, null, null,
            LineDetail.Pairing(PairingDetail(hostile, ByteArray(32), listOf("one", "two", "three", "four"), 4, 5)),
        )
        val t = LineModel.describe(line)
        ok(t.title == "Pairing a counter", "a title when the library gives none")
        ok(t.quoted.size == 1 && t.quoted[0].text.length <= 64 && !t.quoted[0].text.contains('‮'), "the counter's name is quoted and clean")
        ok(t.rows.none { it.contains("Shop") }, "the name is in no row")
        ok(t.rows.any { it.contains("one two three four") }, "the session words are shown")
        val health = LineModel.describe(
            LineView(1, LineKind.HealthCheck, "Health", false, "SR-xx failed", null, LineDetail.Health(HealthDetail(ByteArray(8)))),
        )
        ok(!health.canApprove && health.failure == "SR-xx failed" && health.title == "Health", "a failing line says why")
    }

    @JvmStatic
    fun main(args: Array<String>) {
        val keyMd = args.firstOrNull()?.let { File(it) }
        if (keyMd != null) ok(keyMd.isFile, "docs/screens/key.md exists at ${keyMd.path}")
        ok(wordsProblem(Words(0, 5, "Bad", "no full stop", "Fine.", null, null, null, null)) != null, "the checker finds a missing full stop")
        ok(wordsProblem(Words(0, 5, "Bad", "A hash is here.", "Fine.", null, null, null, null)) != null, "the checker finds jargon")
        ok(wordsProblem(Words(0, 5, "Bad", "Rule SR-35 broke.", "Fine.", null, null, null, null)) != null, "the checker finds a rule number")
        wordsAreFine(keyMd)
        guardTests()
        screenTests()
        wordsModelTests()
        formatTests()
        formatOfFileTests()
        fileTests()
        lineTests()
        println("AppModelTest: $count checks passed")
    }
}
