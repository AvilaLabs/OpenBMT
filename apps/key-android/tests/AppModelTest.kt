package org.openbmt.key.app

import java.io.File
import java.io.IOException
import java.nio.file.Files
import java.security.MessageDigest
import org.openbmt.app.Format
import org.openbmt.app.KeyTextOutcome
import org.openbmt.app.Relock
import org.openbmt.app.SealedFile
import org.openbmt.app.Sealer
import org.openbmt.app.Sealed
import org.openbmt.app.SayCheck
import org.openbmt.app.StoreFormat
import org.openbmt.app.StoreFormatException
import org.openbmt.app.Tones
import org.openbmt.app.UrWords
import org.openbmt.app.Tries
import org.openbmt.app.TriesFile
import org.openbmt.app.TriesRecord
import org.openbmt.app.Who
import org.openbmt.key.BoxClaimDetail
import org.openbmt.key.CheckStatus
import org.openbmt.key.CheckView
import org.openbmt.key.Direction
import org.openbmt.key.HealthDetail
import org.openbmt.key.InvitationView
import org.openbmt.key.Key2TakenView
import org.openbmt.key.Key3View
import org.openbmt.key.KeyException
import org.openbmt.key.LineDetail
import org.openbmt.key.LineKind
import org.openbmt.key.LineView
import org.openbmt.key.LockStatus
import org.openbmt.key.PairingDetail
import org.openbmt.key.Quoted
import org.openbmt.key.SetupState
import org.openbmt.key.SettingsDetail
import org.openbmt.key.SettingChangeView
import org.openbmt.key.SlotView
import org.openbmt.key.WalletView
import org.openbmt.key.Words
import org.openbmt.key.AllowlistView

/**
 * The tests of the pure parts of the Key app: the screen model, the words
 * model, the words themselves, the line text and the saved-state file. They
 * run on the desktop JVM, where no Android class exists. Run the main with
 * the path of docs/screens/key.md as the first argument and the path of
 * docs/screens/key-app.md as the second (OPENBMT_WRITE_SCREENS=1 writes it).
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

    private fun wordsAreFine(keyMd: File?, appMd: File?) {
        val all = AppWords.all()
        for ((name, w) in all) ok(wordsProblem(w) == null, "words of $name: ${wordsProblem(w)}")
        for ((i, a) in all.withIndex()) {
            for (b in all.drop(i + 1)) {
                ok(!(a.second.heading == b.second.heading && a.second.what == b.second.what), "${a.first} and ${b.first} read the same")
            }
        }
        ok(AppWords.cameraOff.who == Who.KEY_HOLDER && AppWords.cameraOff.next.contains("settings") && AppWords.cameraOff.next.contains("file"), "the camera-off words say what to do")
        ok(AppWords.codeNotForField.next.contains("paste") && AppWords.codeNotForField.who == Who.KEY_HOLDER, "the words for a UR in a text field say what to do")
        ok(AppWords.urWords().all { it.second.who == Who.KEY_HOLDER } && AppWords.urWords().size == 9, "the animated-code words are the key holder's")
        val text = AppWords.screensMarkdown()
        ok(text.lines().filter { it.startsWith("| ") }.size == all.size + 1, "one row for each words and the header")
        ok(text.lines().filter { !it.startsWith("|") }.all { it.length <= 80 }, "lines are at most 80 characters")
        if (appMd != null) {
            if (System.getenv("OPENBMT_WRITE_SCREENS") != null) appMd.writeText(text)
            ok(appMd.isFile && appMd.readText() == text, "docs/screens/key-app.md is what screensMarkdown makes (run with OPENBMT_WRITE_SCREENS=1)")
        }
        // The setup, check and kit words equal the rows of docs/screens/key.md.
        if (keyMd != null) {
            val rows = keyMd.readLines().filter { it.startsWith("| ") }
            fun who(n: Int) = when (n) {
                Who.NOBODY -> "nobody"
                Who.OWNER -> "owner"
                else -> "key holder"
            }
            for ((name, w) in all.filter { it.first !in notInTheCatalogue }) {
                val row = "| ${w.heading} | ${w.what} | ${w.next} | ${who(w.who)} |"
                ok(rows.any { it.contains(row) }, "docs/screens/key.md has the words of $name")
            }
        }
    }

    // Words the app wrote itself: the library has no row for them.
    private val notInTheCatalogue = AppWords.urWords().map { it.first }.toSet() + setOf(
        "debuggingOn", "wirelessDebuggingOn", "accessibilityOn", "keyInSoftware", "libraryMissing", "fault",
        "keystoreUnavailable", "fileNotRead", "fileNotSaved", "cameraOff", "cameraNotStarted", "codeNotForField", "notANumber", "flipsNotReadable", "answersReady", "pairOffer", "pairBox", "invitationRunOut", "boxFound",
        "key3Needed", "key3UrNotRead", "key3NotText", "key3Mismatch", "key2Needed", "key2Taken", "key2Mismatch", "scanOwnerWallet", "checkWallet", "walletMismatch",
        "walletLacksKey3", "choosePin", "upgradePin", "pinsDiffer", "pinChanged", "working", "waitForTry",
        "triesNotSaved", "wiped", "wipeFailed", "cashOut", "scanDeposit", "notADepositCode", "nameAddress",
        "checkAddress", "addressAdded", "removeAddress", "addressRemoved",
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
        val ask = ScreenModel.pick(emptyList(), null, SetupState.HasIdentity(false, false))
        ok(ask is Screen.Key3 && !ask.holdsKey2, "identity only, no key 3: ask for key 3")
        val k3 = Key3View(true, "a1b2c3d4", "48h/0h/0h/2h", "xpub-text")
        val give = ScreenModel.pick(emptyList(), null, SetupState.HasIdentity(true, false), k3)
        ok(give is Screen.GiveAnswer && give.holdsKey2 && give.key3 == k3, "key 3 held: give the answer")
        // No key 2 here: key 2 is scanned from the signer after key 3 and before the answer (SR-84).
        val k2 = Key2TakenView(true, "00ff00ff", "48h/0h/0h/2h", "xpub-two")
        val scan2 = ScreenModel.pick(emptyList(), null, SetupState.HasIdentity(false, false), k3)
        ok(scan2 is Screen.Key2 && scan2.key3 == k3, "no key 2 held and none taken: scan key 2 from the signer")
        val answer2 = ScreenModel.pick(emptyList(), null, SetupState.HasIdentity(false, false), k3, null, k2)
        ok(answer2 is Screen.GiveAnswer && !answer2.holdsKey2, "key 2 taken: give the answer")
        ok(ScreenModel.pick(emptyList(), null, SetupState.HasIdentity(false, false), null, null, k2) is Screen.Key3, "key 3 comes before key 2")
        ok(ScreenModel.pick(emptyList(), null, SetupState.HasIdentity(true, false), k3, null, null) is Screen.GiveAnswer, "key 2 held here: no key 2 step")
        ok(ScreenModel.pick(emptyList(), null, SetupState.HasIdentity(true, true), k3) is Screen.ShowWords, "words waiting come before key 3")
        ok(ScreenModel.pick(emptyList(), null, SetupState.Ready(true), k3) is Screen.Home, "a wallet accepted: home, key 3 or not")
        val pending = ScreenModel.pick(emptyList(), null, SetupState.WalletPending(true), k3)
        ok(pending is Screen.ComparePending && pending.holdsKey2, "a wallet pending: the comparison again, after a restart too")
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
        ok(t.title.startsWith("Pair counter \u201c") && t.title.endsWith("\u201d"), "the title is Pair counter and the name in quotation marks: ${t.title}")
        ok(!t.title.contains('‮') && !t.title.contains('\u0000') && t.title.length <= 100, "the name in the title is clean and short")
        ok(t.quoted.size == 1 && t.quoted[0].text.length <= 64 && !t.quoted[0].text.contains('‮'), "the counter's name is quoted and clean")
        ok(t.rows.none { it.contains("Shop") }, "the name is in no row")
        ok(t.prominent == "one   two   three   four", "the four words are the prominent line: ${t.prominent}")
        ok(t.rows.none { it.contains("one") }, "the words are not repeated in a row")
        ok(t.instruction!!.contains("four words") && t.instruction!!.contains("counter's screen"), "the instruction says to compare with the counter")
        val noWords = LineModel.describe(
            LineView(3, LineKind.Pairing, "Pairing", false, "AS-28 failed", null, LineDetail.Pairing(PairingDetail("Front", ByteArray(32), null, 4, 5))),
        )
        ok(noWords.prominent == null && noWords.instruction == null && !noWords.canApprove, "no words, no comparison, no approval")
        ok(noWords.rows.any { it.contains("cannot be compared") }, "the words that could not be made are said")
        // Settings: an identity is shown in full, apart from the rows, to be set in monospace (AS-39).
        val idOne = "0102 0304 0506 0708 090a 0b0c 0d0e 0f10 1112 1314 1516 1718 191a 1b1c 1d1e 1f20 2122 2324 2526 2728 2930 3132 3334 3536 3738 3940 4142 4344 4546 4748 4950 5152"
        val settings = LineModel.describe(
            LineView(
                2, LineKind.Settings, "Settings", true, null, null,
                LineDetail.Settings(
                    SettingsDetail(
                        3, 2,
                        listOf(
                            SettingChangeView("tor_on", "off", "on", Direction.Loosening),
                            SettingChangeView("devices", "none", idOne, Direction.Loosening),
                            SettingChangeView("boxes", null, idOne, Direction.Loosening),
                        ),
                        Direction.Loosening, emptyList(),
                    ),
                ),
            ),
        )
        ok(settings.identities.size == 2 && settings.identities.all { it.contains(idOne) }, "devices and Boxes are shown in full, apart")
        ok(settings.rows.any { it.startsWith("tor_on:") } && settings.rows.none { it.contains(idOne) }, "other fields stay in the rows, identities do not")
        val other = LineModel.describe(
            LineView(1, LineKind.HealthCheck, "Health", false, null, null, LineDetail.Health(HealthDetail(ByteArray(8)))),
        )
        ok(other.prominent == null && other.instruction == null, "only a pairing has a prominent line")
        val health = LineModel.describe(
            LineView(1, LineKind.HealthCheck, "Health", false, "SR-xx failed", null, LineDetail.Health(HealthDetail(ByteArray(8)))),
        )
        ok(!health.canApprove && health.failure == "SR-xx failed" && health.title == "Health", "a failing line says why")
    }

    private fun pairOfferTests() {
        val good = ByteArray(PairOfferModel.LENGTH)
        "OBMO".forEachIndexed { i, c -> good[i] = c.code.toByte() }
        good[4] = 1
        ok(PairOfferModel.looksLikeOffer(good), "an offer has the frame and the length")
        ok(PairOfferModel.LENGTH == 102, "an offer is 102 bytes")
        ok(!PairOfferModel.looksLikeOffer(good.copyOf(101)) && !PairOfferModel.looksLikeOffer(good + 0), "another length is not an offer")
        ok(!PairOfferModel.looksLikeOffer(good.copyOf().also { it[0] = 'X'.code.toByte() }), "another magic is not an offer")
        ok(!PairOfferModel.looksLikeOffer(good.copyOf().also { it[4] = 2 }), "another version is not an offer")
        ok(!PairOfferModel.looksLikeOffer(ByteArray(0)), "nothing is not an offer")
        ok(AppWords.pairOffer.next.contains("Scan this with the counter") && AppWords.pairOffer.who == Who.KEY_HOLDER, "the screen says to scan it with the counter")
    }


    private fun answerWith(magic: Char, version: Int, extra: Int = 10): ByteArray =
        ByteArray(5 + extra).also {
            "OBM$magic".forEachIndexed { i, c -> it[i] = c.code.toByte() }
            it[4] = version.toByte()
        }

    private fun key3Tests() {
        val text = "[a1b2c3d4/48h/0h/0h/2h]xpub6Dummy"
        val plain = CeremonyModel.readKey3Code(text.toByteArray())
        ok(plain is Key3Input.Text && plain.text == text, "a plain text code goes to the library as it is")
        val withSpace = CeremonyModel.readKey3Text("  \n$text\n")
        ok(withSpace is Key3Input.Text && withSpace.text == "  \n$text\n", "the library trims, the app does not change the text")
        for (ur in listOf("ur:crypto-account/abc", "UR:CRYPTO-ACCOUNT/ABC", "  ur:bytes/1-3/xyz")) {
            val r = CeremonyModel.readKey3Code(ur.toByteArray())
            ok(r is Key3Input.Refused && r.words == AppWords.key3UrNotRead, "a ur: code that reached the plain path is refused with its words: $ur")
            val t = CeremonyModel.readKey3Text(ur)
            ok(t is Key3Input.Refused && t.words == AppWords.key3UrNotRead, "pasted ur: text is refused the same way: $ur")
        }
        ok(AppWords.key3UrNotRead.what.contains("only when it is scanned") && AppWords.key3UrNotRead.next.contains("as text"), "the ur: words say a key code is scanned and a paste is text")
        val binary = CeremonyModel.readKey3Code(byteArrayOf(0x00, 0xff.toByte(), 0xfe.toByte(), 0x80.toByte()))
        ok(binary is Key3Input.Refused && binary.words == AppWords.key3NotText, "bytes that are not text are refused")
        val empty = CeremonyModel.readKey3Text("")
        ok(empty is Key3Input.Text && empty.text == "", "an empty text is left to the library to refuse")
        ok(CeremonyModel.groupFingerprint("a1b2c3d4") == "A1B2 C3D4", "the fingerprint is in two groups of four, in capitals")
        ok(CeremonyModel.groupFingerprint("abc") == "abc", "a fingerprint of another length is shown as it is")
        val c = CeremonyModel.key3Check(Key3View(false, "00ff00ff", "48h/1h/0h/2h", "tpub-text"))
        ok(c.fingerprint == "00FF 00FF" && c.network == "A test network", "key 3 on a test network")
        ok(c.advanced == listOf("Origin path: 48h/1h/0h/2h", "Account key: tpub-text"), "the origin and the account key are for the Advanced fold")
        ok(CeremonyModel.key3Check(Key3View(true, "00ff00ff", "p", "x")).network == "Main bitcoin network", "key 3 on the main network")
        // The plain words never name the parts of the key; only the Advanced text does.
        val plainWords = listOf(AppWords.key3Needed, AppWords.key3UrNotRead, AppWords.key3NotText, AppWords.key3Taken, AppWords.key3Mismatch)
        for (w in plainWords) {
            val all = "${w.heading} ${w.what} ${w.next}".lowercase()
            ok(listOf("xpub", "descriptor", "multisig").none { all.contains(it) }, "no jargon in ${w.heading}")
        }
        ok(AppWords.Text.ADVANCED_KEY3.contains("xpub") && AppWords.Text.ADVANCED_KEY3.contains("multisig"), "the Advanced text may name them")
        ok(AppWords.Text.ADVANCED == "Advanced" && AppWords.Text.SCAN_KEY3 == "Scan key 3", "the button words")
        ok(AppWords.key3Needed.next.contains("alone") && AppWords.key3Needed.what.contains("another publisher"), "key 3 is made alone, elsewhere")
        ok(AppWords.key3Mismatch.tone == Tones.STOP && AppWords.key3Taken.tone == Tones.GOOD, "mismatch is a stop, taken is done")
        // Key 2, from the signer (SR-84).
        val c2 = CeremonyModel.key2Check(Key2TakenView(false, "00ff00ff", "48h/1h/0h/2h", "tpub-two"))
        ok(c2.fingerprint == "00FF 00FF" && c2.network == "A test network", "key 2 on a test network")
        ok(c2.advanced == listOf("Origin path: 48h/1h/0h/2h", "Account key: tpub-two"), "key 2's origin and account key are for the Advanced fold")
        ok(CeremonyModel.key2Check(Key2TakenView(true, "00ff00ff", "p", "x")).network == "Main bitcoin network", "key 2 on the main network")
        for (w in listOf(AppWords.key2Needed, AppWords.key2Taken, AppWords.key2Mismatch)) {
            val all = "${w.heading} ${w.what} ${w.next}".lowercase()
            ok(listOf("xpub", "descriptor", "multisig").none { all.contains(it) }, "no jargon in ${w.heading}")
            ok(w.who == Who.KEY_HOLDER, "${w.heading} is for the key holder")
        }
        ok(AppWords.key2Needed.what.contains("not from the owner's phone"), "key 2 is taken from the signer, not the owner phone")
        ok(AppWords.key2Mismatch.tone == Tones.STOP && AppWords.key2Taken.tone == Tones.GOOD, "key 2: mismatch is a stop, taken is done")
    }

    private fun sr_64_key_3_by_a_key_code() {
        ok(CeremonyModel.KEY3_TYPES == setOf("crypto-account", "crypto-hdkey", "bytes"), "the key 3 scan takes these three types")
        val text = "[a1b2c3d4/48h/0h/0h/2h]xpub6Dummy"
        val key = CeremonyModel.readKey3Message(KeyTextOutcome.Key(text))
        ok(key is Key3Input.Text && key.text == text, "a key goes to the library as scanned text does")
        val declined = KeyTextOutcome.Declined("UR-23", "Not a key this app takes", "The code holds something else.", "Choose the multisig export.")
        val d = CeremonyModel.readKey3Message(declined)
        ok(d is Key3Input.Declined && d.words.heading == "Not a key this app takes" && d.words.code == "UR-23" && d.words.tone == Tones.STOP, "a declined code shows the library's words and stays on the scan")
        ok(d is Key3Input.Declined && d.words.who == Who.KEY_HOLDER, "the words are for the one holding the Key app")
        val refused = CeremonyModel.readKey3Message(KeyTextOutcome.Refused("UR-21"))
        ok(refused is Key3Input.Refused && refused.words.heading == UrWords.readerFailed(Who.KEY_HOLDER).heading, "a refused request shows the reader's words and goes back to key 3")
        val fault = CeremonyModel.readKey3Message(KeyTextOutcome.Fault)
        ok(fault is Key3Input.Refused && fault.words.heading == UrWords.readerFailed(Who.KEY_HOLDER).heading, "no answer shows the same words")
        ok(CeremonyModel.readKey3Text("ur:crypto-hdkey/abc") is Key3Input.Refused, "paste stays text only")
        ok(AppWords.key3Needed.next.contains("a key code from the other wallet app, or its key as text"), "the scan step says which codes work")
    }

    private fun answerTests() {
        ok(CeremonyModel.looksLikeAnswer(answerWith('v', 1)), "an answer has its magic and version")
        ok(!CeremonyModel.looksLikeAnswer(answerWith('w', 1)) && !CeremonyModel.looksLikeAnswer(answerWith('v', 2)), "another magic or version is not an answer")
        ok(!CeremonyModel.looksLikeAnswer(answerWith('v', 1, 0)) && !CeremonyModel.looksLikeAnswer(ByteArray(0)), "an empty frame is not an answer")
        ok(CeremonyModel.looksLikeSetup(answerWith('w', 1)) && !CeremonyModel.looksLikeSetup(answerWith('v', 1)), "a setup has its own magic")
        ok(AppWords.publicAnswerReady.who == Who.OWNER && AppWords.publicAnswerReady.next.contains("Scan the answer"), "the answer words send the person to the owner phone")
        ok(AppWords.Text.ON_OWNER_PHONE == "On the owner phone, choose Scan the Key app.", "the line for the owner phone")
    }

    private fun walletTests() {
        val view = WalletView(
            checksumWords = listOf("alpha", "bravo", "charlie", "delta"),
            walletId = ByteArray(32),
            slots = listOf(SlotView(0, "11223344", "xpub1"), SlotView(1, "aabbccdd", "xpub2"), SlotView(2, "01020304", "xpub3")),
            firstReceiveAddress = "bc1qexample",
            ownRootWords = listOf("one", "two", "three", "four"),
            ownerRootWords = listOf("five", "six", "seven", "eight"),
        )
        val c = CeremonyModel.walletCheck(view)
        ok(c.checkWords == "alpha   bravo   charlie   delta", "the four words on one line")
        ok(c.fingerprints == listOf("Key 1: 1122 3344", "Key 2: AABB CCDD", "Key 3: 0102 0304"), "three fingerprints: ${c.fingerprints}")
        ok(c.identities == listOf("This app's identity: one two three four", "The owner phone's identity: five six seven eight"), "both identities as words")
        ok(c.firstAddress == "bc1qexample", "the first address")
        ok(AppWords.checkWallet.next == "The owner phone and the counter must show these same words and fingerprints.", "the comparison sentence")
        ok(AppWords.walletMismatch.next.contains("Nothing was registered") && AppWords.walletMismatch.next.contains("scan its wallet code again"), "they differ: nothing registered, scan again")
        ok(AppWords.walletMismatch.tone == Tones.STOP, "they differ is a stop")
    }

    private fun refusalTests() {
        val lib = Words(Tones.STOP, Who.KEY_HOLDER, "Key 3 not in the wallet", "None of the wallet's keys is the key 3 this app was given.", "Do not accept the wallet, and tell the owner in person.", "SR-67", null, null, null)
        val notInWallet = KeyException(CeremonyModel.RULE_KEY3_NOT_IN_WALLET, "SR-67: no key of the wallet is the key 3 taken", 1, lib)
        val r = CeremonyModel.setupRefusal(notInWallet)
        ok(r == listOf(AppWords.walletLacksKey3, lib), "a swapped key 3: the plain words, then the library's")
        val differs = KeyException(CeremonyModel.RULE_KEY3_DIFFERS, "SR-67: the key 3 given is not the key 3 taken", 1, lib)
        ok(CeremonyModel.setupRefusal(differs) == listOf(AppWords.walletLacksKey3, lib), "the other SR-67 refusal reads the same")
        ok(CeremonyModel.setupRefusal(KeyException(CeremonyModel.RULE_KEY3_NOT_IN_WALLET, "x", 1)) == listOf(AppWords.walletLacksKey3), "no library words: the app's alone")
        ok(CeremonyModel.setupRefusal(KeyException("SR-67", "SR-67: no key of the wallet is the key 3 taken", 1, lib)) == listOf(lib), "the kind is the rule string, not the text")
        val noKey3 = KeyException(CeremonyModel.RULE_KEY3_MISSING, "SR-67: no key 3 has been taken yet", 1, lib)
        ok(CeremonyModel.setupRefusal(noKey3) == listOf(lib), "no key 3 taken: only the library's words")
        val frame = KeyException("SR-68", "SR-68: the setup is cut short", 1, lib)
        ok(CeremonyModel.setupRefusal(frame) == listOf(lib), "a frame refusal: only the library's words")
        val bare = CeremonyModel.setupRefusal(KeyException("response-null", "no response", 4))
        ok(bare.size == 1 && bare[0].heading == AppWords.fault(KeyException("a", "b", 4)).heading, "an error without words shows the app's fault words")
        ok(AppWords.walletLacksKey3.what.contains("does not hold the key 3 scanned here") && AppWords.walletLacksKey3.what.contains("nothing was accepted"), "the swapped key 3 is said plainly")
    }

    // ---- the PIN lock and the cash-out screens (docs/SIGNER-RULES.md SR-74 to SR-79) ----

    /** SR-74: the key files are in the no-backup folder, and the manifest and both rule files exclude all. */
    private fun sr_74_key_files_stay_out_of_backups(appDir: File?) {
        ok(Wipe.STATE_FILE == "state.obks" && TriesFile.NAME == "tries.obkt", "the two key files have their names")
        if (appDir == null) return
        val manifest = File(appDir, "AndroidManifest.xml").readText()
        ok(manifest.contains("android:allowBackup=\"false\""), "cloud backup is off in the manifest")
        ok(manifest.contains("@xml/backup_rules") && manifest.contains("@xml/data_extraction_rules"), "both rule files are named")
        val domains = listOf("root", "file", "database", "sharedpref", "external")
        for (name in listOf("backup_rules", "data_extraction_rules")) {
            val xml = File(appDir, "res/xml/$name.xml").readText()
            val withoutComments = xml.replace(Regex("<!--.*?-->", RegexOption.DOT_MATCHES_ALL), "")
            val parts = if (name == "backup_rules") 1 else 2
            for (d in domains) {
                val n = Regex("<exclude domain=\"$d\" />").findAll(withoutComments).count()
                ok(n == parts, "$name excludes $d $parts time(s)")
            }
            ok(!withoutComments.contains("<include"), "$name includes nothing")
        }
        val controller = File(appDir, "src/android/Controller.kt").readText()
        ok(!Regex("[^a-zA-Z]filesDir").containsMatchIn(controller), "the plain files folder is not used")
        ok(controller.contains("TriesFile(act.noBackupFilesDir)"), "the tries file is in the no-backup folder")
    }

    private fun sr_75_waits_and_tries_left() {
        val waits = listOf(0L, 0, 0, 0, 0, 30, 60, 300, 900, 3600)
        for ((n, w) in waits.withIndex()) ok(Tries.waitSeconds(n) == w, "$n wrong tries wait $w s")
        val t0 = 1_000_000L
        for (n in 1..9) {
            val rec = TriesRecord(n, t0)
            val c = Tries.check(rec, t0)
            ok(c.waitLeft == waits[n] && !c.wipe, "right after $n wrong: wait ${waits[n]}")
            ok(Tries.check(rec, t0 + waits[n]).waitLeft == 0L, "after the wait the try goes on ($n)")
            ok(Tries.check(rec, t0 + 10).waitLeft == (waits[n] - 10).coerceAtLeast(0), "the wait counts down ($n)")
        }
        ok(!Tries.showsTriesLeft(4) && Tries.showsTriesLeft(5), "tries left are said from the 5th wrong try")
        ok(Tries.triesLeftText(5) == "5 tries left before this key is wiped.", "5 left")
        ok(Tries.triesLeftText(9) == "1 try left before this key is wiped.", "1 left")
        ok(Tries.before(TriesRecord(3, 5), 99) == TriesRecord(4, 99), "one more is counted before the try")
        ok(Tries.afterRight() == TriesRecord(0, 0), "a right PIN clears the count")
        ok(Tries.waitText(30) == "30 s" && Tries.waitText(260) == "4 min 20 s" && Tries.waitText(3600) == "1 h 0 min", "wait text")
        // A clock set back restarts the wait.
        val back = Tries.check(TriesRecord(6, t0), t0 - 500)
        ok(back.waitLeft == 60L && back.record == TriesRecord(6, t0 - 500), "a clock behind restarts the wait and stores it")
        ok(Tries.check(TriesRecord(10, t0), t0).wipe, "10 wrong tries wipe")
        // The file.
        val dir = tempDir()
        try {
            val f = TriesFile(dir)
            ok(f.read(5) == TriesRecord(0, 0), "no file: no wrong tries")
            f.write(TriesRecord(7, 1234))
            ok(f.read(5) == TriesRecord(7, 1234), "the record is read back")
            val file = File(dir, TriesFile.NAME)
            val good = file.readBytes()
            ok(good.size == 17, "17 bytes")
            for (bad in listOf(good.copyOf(16), good + byteArrayOf(0), good.copyOf().also { it[4] = 2 }, good.copyOf().also { it[0] = 0 })) {
                file.writeBytes(bad)
                ok(f.read(77) == TriesRecord(9, 77), "a file that is not exact counts as 9 wrong tries just now")
            }
            file.writeBytes(good.copyOf().also { it[8] = 0x80.toByte() })
            ok(f.read(77) == TriesRecord(9, 77), "a negative count is not read")
            f.delete()
            ok(!f.exists(), "deleted")
        } finally {
            dir.deleteRecursively()
        }
    }

    private fun sr_76_wipe_order_and_failure() {
        val dir = tempDir()
        try {
            val state = File(dir, Wipe.STATE_FILE)
            val tries = TriesFile(dir)
            fun setUp() {
                state.writeBytes(byteArrayOf(1, 2, 3))
                tries.write(TriesRecord(10, 5))
            }
            setUp()
            val seen = ArrayList<String>()
            Wipe.run(dir, tries) {
                seen.add("key")
                ok(!state.exists() && tries.exists(), "the state file goes before the key, the tries file after")
            }
            ok(seen == listOf("key") && !state.exists() && !tries.exists(), "all three are gone")
            // A failing key deletion leaves the tries file, so the next start wipes again.
            setUp()
            throwsIo("a key that stays throws") { Wipe.run(dir, tries) { throw IOException("still there") } }
            ok(!state.exists() && tries.exists() && tries.read(0).wrong == 10, "a wipe that stops leaves the count at 10")
            ok(Tries.check(tries.read(0), 0).wipe, "and the next start wipes")
            ok(AppWords.wiped.what.contains("the other two keys still exist") && AppWords.wiped.what.contains("not lost"), "the wiped words")
            ok(AppWords.wiped.next.contains("24 words"), "the wiped words say what is next")
        } finally {
            dir.deleteRecursively()
        }
    }

    private fun sr_77_picker_and_permission_waits_count(appDir: File?) {
        if (appDir == null) return
        val controller = File(appDir, "src/android/Controller.kt").readText()
        val resumed = controller.substringAfter("fun resumed() {").substringBefore("fun paused()")
        ok(resumed.contains("waitedTooLong()") && resumed.contains("return start()"), "a resume after more than a minute in a picker locks")
        val picked = controller.substringAfter("fun picked(").substringBefore("private fun openRequest(")
        ok(picked.indexOf("ExternalWait.decide(") in 0 until picked.indexOf("data?.data"), "a picker's result is judged before it is read")
        val permission = controller.substringAfter("fun permission(").substringBefore("// ---- the approval session")
        ok(permission.indexOf("waitedTooLong()") in 0 until permission.indexOf("scan.permissionResult"), "a permission answer is judged before it is taken")
    }

    private fun sr_77_relock_after_a_minute() {
        ok(Relock.needed(null, 5), "never paused: locked")
        ok(!Relock.needed(1000, 1000 + 60_000), "60 s exactly is not more than 60 s")
        ok(Relock.needed(1000, 1000 + 60_001), "more than 60 s locks")
        ok(!Relock.needed(1000, 1500), "a short stay does not lock")
        ok(Relock.needed(1000, 500), "a clock that went back locks")
        val st = SetupState.Locked
        ok(ScreenModel.pick(emptyList(), null, st, null, LockStatus.Locked) is Screen.Unlock, "locked: the unlock screen")
        ok(ScreenModel.pick(emptyList(), null, st, null, LockStatus.Legacy).let { it is Screen.SetPin && it.upgrade }, "legacy: set a PIN")
        ok(ScreenModel.pick(emptyList(), null, SetupState.Empty, null, LockStatus.NoPin).let { it is Screen.SetPin && !it.upgrade }, "no PIN yet: choose one first")
        ok(ScreenModel.pick(emptyList(), null, SetupState.Empty, null, LockStatus.Open) is Screen.Setup, "a PIN set: the setup")
        ok(ScreenModel.pick(listOf(AppWords.debuggingOn), null, st, null, LockStatus.Locked) is Screen.Refused, "a refusal still comes first")
        ok(AppWords.working.heading == "Working", "the working page")
        ok(wordsProblem(AppWords.working) == null && wordsProblem(AppWords.wiped) == null, "the words of the lock pass the words check")
    }

    private fun sr_83_a_redraw_is_drawn_from_state_and_never_skips_the_pin() {
        // A change between light and dark draws the screen again with `refresh`, which picks it from the state: a locked
        // app can only come back as the unlock screen, whatever was on show, and a wallet that waits is not skipped.
        ok(ScreenModel.pick(emptyList(), null, SetupState.Locked, null, LockStatus.Locked) is Screen.Unlock, "SR-83: a locked app is redrawn as the unlock screen")
        ok(ScreenModel.pick(listOf(AppWords.debuggingOn), null, SetupState.Locked, null, LockStatus.Locked) is Screen.Refused, "SR-83: a refusal is redrawn as the refusal")
    }

    /** The cards of the new look take the app's own lines apart and lose nothing. */
    private fun screenPartsTests() {
        val four = listOf("alpha", "bravo", "charlie", "delta")
        ok(ScreenParts.words(LineModel.wordsLine(four)) == four, "the four words of a pairing line become four cards, in order")
        ok(ScreenParts.words("alpha   bravo \n charlie delta ") == four, "any spacing between the words splits them")
        ok(ScreenParts.words("   ").isEmpty(), "an empty line has no words")
        ok(ScreenParts.keyValue("Key 1: ABCD 1234") == ("Key 1" to "ABCD 1234"), "a fingerprint line is a key and its value")
        ok(ScreenParts.keyValue("no colon here") == ("no colon here" to ""), "a line without a colon is the key alone")
        val twentyFour = (1..24).map { "w$it" }
        val list = ScreenParts.numbered(twentyFour)
        ok(list.lines().size == 24 && list.lines().first() == "1. w1" && list.lines().last() == "24. w24", "recovery words stay a numbered list")
        ok(ScreenParts.asCards(4) && !ScreenParts.asCards(12) && !ScreenParts.asCards(24) && !ScreenParts.asCards(0), "only groups of four go through cards")
    }

    private fun sr_78_deposit_code_from_the_camera() {
        ok((CashOut.readCode("bc1qexample".toByteArray()) as DepositInput.Text).text == "bc1qexample", "a bare address goes on")
        ok((CashOut.readCode(" bitcoin:bc1qexample?amount=1\n".toByteArray()) as DepositInput.Text).text == "bitcoin:bc1qexample?amount=1", "a URI goes on, trimmed")
        ok(CashOut.readCode("ur:crypto-psbt/abc".toByteArray()) is DepositInput.Refused, "a ur code is refused")
        ok(CashOut.readCode("UR:bytes/abc".toByteArray()) is DepositInput.Refused, "a ur code in capitals is refused")
        ok(CashOut.readCode(byteArrayOf(0xff.toByte(), 0xfe.toByte())) is DepositInput.Refused, "bytes that are not text are refused")
        ok(CashOut.groupsOfFour("bc1qabcdefghij") == "bc1q abcd efgh ij", "groups of four")
        ok(wordsProblem(AppWords.checkAddress) == null && AppWords.checkAddress.next.contains("24 hours"), "the check words name the 24 hours")
        ok(AppWords.scanDeposit.what.contains("cannot be typed or pasted"), "no typing")
    }

    private fun sr_79_list_rows() {
        val now = 100_000L
        val soon = AllowlistView("bc1qabcdefghij", "0014aa", "Exchange", 90_000, now + 3 * 3600 + 5 * 60)
        val row = CashOut.row(soon, now)
        ok(row.status == "active in 3h 5m" && row.address == "bc1q abcd efgh ij" && row.removable == "bc1qabcdefghij", "a waiting row")
        ok(CashOut.row(AllowlistView("bc1qabcdefghij", "0014aa", "E", 1, now), now).status == "active", "an active row")
        ok(CashOut.row(AllowlistView("bc1qabcdefghij", "0014aa", "E", 1, now - 5), now).status == "active", "an older active row")
        ok(CashOut.status(now + 1, now) == "active in 0h 1m", "a second left rounds up to a minute")
        ok(CashOut.status(now + 3599 * 1L + 1, now) == "active in 1h 0m", "an hour exactly")
        val noAddress = CashOut.row(AllowlistView(null, "6a0102", "Odd", 1, null), now)
        ok(noAddress.removable == null && noAddress.address == "6a0102" && noAddress.status == "not active", "a script without an address")
    }

    // ---- pairing a Box (docs/SIGNER-RULES.md SR-80 to SR-82) ----

    private fun sr_80_the_invitation_is_one_of_six_groups_of_five() {
        val text = "ABCDE-FGHJK-MNPQR-STVWX-YZ012-34567"
        ok(BoxModel.groups(text) == listOf("ABCDE", "FGHJK", "MNPQR", "STVWX", "YZ012", "34567"), "six groups of five")
        ok(BoxModel.spaced(text) == "ABCDE   FGHJK   MNPQR   STVWX   YZ012   34567", "the groups stand apart")
        ok(BoxModel.groups("ABCDE-FGHJK-MNPQR-STVWX-YZ012") == null, "five groups are not an invitation")
        ok(BoxModel.groups("ABCDE-FGHJK-MNPQR-STVWX-YZ012-3456") == null, "a short group is not an invitation")
        ok(BoxModel.groups("ABCDE-FGHJK-MNPQR-STVWX-YZ012-3456I") == null, "a letter outside the alphabet is not an invitation")
        ok(BoxModel.groups("abcde-fghjk-mnpqr-stvwx-yz012-34567") == null, "lower case is not drawn")
        ok(BoxModel.spaced("junk") == null, "junk is not drawn")
    }

    private fun sr_81_the_countdown_and_the_run_out() {
        val inv = InvitationView("ABCDE-FGHJK-MNPQR-STVWX-YZ012-34567", 2_800, 1_800)
        ok(BoxModel.secsLeft(inv, 1_000) == 1_800L && BoxModel.timeLeft(1_800) == "30:00", "30 minutes at the start")
        ok(BoxModel.timeLeft(61) == "1:01" && BoxModel.timeLeft(9) == "0:09" && BoxModel.timeLeft(-3) == "0:00", "minutes and seconds")
        ok(!BoxModel.runOut(inv, 2_799) && BoxModel.runOut(inv, 2_800) && BoxModel.runOut(inv, 9_999), "it runs out at its end and not before")
        ok(BoxModel.secsLeft(inv, 9_999) == 0L, "no negative time")
        ok(AppWords.pairBox.what.contains("30 minutes") && AppWords.pairBox.what.contains("once"), "the screen says it works once and for 30 minutes")
        ok(AppWords.pairBox.next.contains("30 characters") && AppWords.pairBox.next.contains("shared computer"), "the screen warns about a shared computer")
        ok(AppWords.pairBox.next.contains("Box's own setup"), "the screen says where to type it")
        ok(AppWords.invitationRunOut.next.contains("new invitation"), "the run-out words say what to do")
    }

    private fun sr_82_the_box_found_line() {
        val checks = listOf(CheckView("an invitation of this app is open", CheckStatus.Passed), CheckView("the settings are approved", CheckStatus.Failed))
        val key = ByteArray(32) { (it + 1).toByte() }
        val line = LineView(
            4, LineKind.BoxClaim, "Box", false, "AS-81: the settings that add this Box are not approved: approve that line first", null,
            LineDetail.BoxClaim(BoxClaimDetail(key, listOf("focus", "hold", "cousin", "dose"), checks)),
        )
        val t = LineModel.describe(line)
        ok(t.title == "Box" || t.title == "Box found", "the title says a Box was found: ${t.title}")
        ok(t.prominent == "focus   hold   cousin   dose", "the four words are the prominent line: ${t.prominent}")
        ok(t.instruction!!.contains("four words") && t.instruction!!.contains("same order"), "the instruction says to compare")
        ok(t.rows.any { it.contains("01020304 05060708") && it.contains("1d1e1f20") }, "the key is shown in full")
        ok(t.rows.any { it.startsWith("Check not passed:") } && t.rows.any { it.startsWith("Check passed:") }, "each check has its row")
        ok(!t.canApprove && t.failure!!.startsWith("AS-81"), "a failing claim says why and is not approvable")
        val noWords = LineModel.describe(
            LineView(4, LineKind.BoxClaim, "", true, null, null, LineDetail.BoxClaim(BoxClaimDetail(key, null, emptyList()))),
        )
        ok(noWords.title == "Box found" && noWords.prominent == null && noWords.rows.any { it.contains("cannot be compared") }, "no words, no comparison")
        ok(BoxModel.hexGroups(ByteArray(4)) == "00000000", "a short key is one group")
        ok(AppWords.boxFound.heading == "Box found" && AppWords.boxFound.who == Who.KEY_HOLDER, "the words say Box found")
    }

    private fun lockTests(appDir: File?) {
        sr_74_key_files_stay_out_of_backups(appDir)
        sr_75_waits_and_tries_left()
        sr_76_wipe_order_and_failure()
        sr_77_relock_after_a_minute()
        sr_77_picker_and_permission_waits_count(appDir)
        sr_83_a_redraw_is_drawn_from_state_and_never_skips_the_pin()
        sr_78_deposit_code_from_the_camera()
        sr_79_list_rows()
        sr_80_the_invitation_is_one_of_six_groups_of_five()
        sr_81_the_countdown_and_the_run_out()
        sr_82_the_box_found_line()
    }

    @JvmStatic
    fun main(args: Array<String>) {
        val keyMd = args.getOrNull(0)?.let { File(it) }
        val appMd = args.getOrNull(1)?.let { File(it) }
        if (keyMd != null) ok(keyMd.isFile, "docs/screens/key.md exists at ${keyMd.path}")
        wordsAreFine(keyMd, appMd)
        guardTests()
        screenTests()
        formatOfFileTests()
        fileTests()
        lineTests()
        pairOfferTests()
        key3Tests()
        sr_64_key_3_by_a_key_code()
        answerTests()
        walletTests()
        refusalTests()
        screenPartsTests()
        lockTests(args.getOrNull(2)?.let { File(it) })
        println("AppModelTest: $count checks passed")
    }
}
