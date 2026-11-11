package org.openbmt.key.app

import java.io.File
import java.io.IOException
import java.nio.file.Files
import java.security.MessageDigest
import org.openbmt.app.Format
import org.openbmt.app.SealedFile
import org.openbmt.app.Sealer
import org.openbmt.app.Sealed
import org.openbmt.app.SayCheck
import org.openbmt.app.StoreFormat
import org.openbmt.app.StoreFormatException
import org.openbmt.app.Who
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

    /** SC-12 as far as the app can check it; null when the words pass. */
    private fun wordsProblem(w: Words): String? = SayCheck.problem(w.say())

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
        wordsAreFine(keyMd)
        guardTests()
        screenTests()
        formatOfFileTests()
        fileTests()
        lineTests()
        println("AppModelTest: $count checks passed")
    }
}
