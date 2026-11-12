package org.openbmt.counter.app

import java.io.File
import java.io.IOException
import java.net.InetAddress
import java.net.ServerSocket
import java.nio.file.Files
import java.security.SecureRandom
import org.openbmt.app.Colour
import org.openbmt.app.ControlReader
import org.openbmt.app.SayCheck
import org.openbmt.app.SayWords
import org.openbmt.app.Tones
import org.openbmt.app.TorControl
import org.openbmt.app.Who
import org.openbmt.counter.Actor
import org.openbmt.counter.ActorRole
import org.openbmt.counter.CounterException
import org.openbmt.counter.DayTotals
import org.openbmt.counter.CounterNetwork
import org.openbmt.counter.LnKind
import org.openbmt.counter.LnMethod
import org.openbmt.counter.LnRequest
import org.openbmt.counter.LnStep
import org.openbmt.counter.LnTransport
import org.openbmt.counter.Payment
import org.openbmt.counter.PersonLine
import org.openbmt.counter.TipTally
import org.openbmt.counter.Quoted
import org.openbmt.counter.Rate
import org.openbmt.counter.RateSource
import org.openbmt.counter.SaleState
import org.openbmt.counter.Sample
import org.openbmt.counter.Tally
import org.openbmt.counter.Words

/**
 * The tests of the pure parts of the counter app: its words, the PIN store
 * with a fake clock, the pairing steps, the config, the amount keypad, the QR
 * layout, the price schedule and book with a fake clock and a fake fetcher
 * (no network, no library), the Tor gate, the screen choice and the sale
 * buttons. They run on the desktop JVM, where no Android class exists. Run the
 * main with the path of docs/screens/counter-app.md as the one argument; the
 * environment variable OPENBMT_WRITE_SCREENS=1 writes that file first.
 */
object AppModelTest {
    private var count = 0

    private fun ok(cond: Boolean, what: String) {
        count++
        if (!cond) throw AssertionError("FAILED: $what")
    }

    private inline fun throws(what: String, block: () -> Unit) {
        count++
        try {
            block()
        } catch (_: IOException) {
            return
        } catch (_: IllegalArgumentException) {
            return
        }
        throw AssertionError("FAILED: no error for $what")
    }

    private fun tempDir(): File = Files.createTempDirectory("counter-test").toFile()

    private fun clean(dir: File) {
        dir.listFiles()?.forEach { it.delete() }
        dir.delete()
    }

    // ---- the words ------------------------------------------------------------------------

    private fun wordsTests(screensMd: File?) {
        val all = AppWords.all()
        for ((name, w) in all) ok(SayCheck.problem(w) == null, "words of $name: ${SayCheck.problem(w)}")
        ok(SayCheck.distinctProblem(all) == null, "no two words read the same: ${SayCheck.distinctProblem(all)}")
        ok(AppWords.orbotNotRunning.what == "Tor is on for this shop, but Orbot is not running.", "the Orbot words, as written down")
        ok(AppWords.orbotNotRunning.next == "Open Orbot and turn it on. If it listens at another address, the manager can change it under Network.", "the Orbot next step")
        ok(AppWords.orbotNotRunning.who == Who.MANAGER, "the manager does it")
        ok(AppWords.lightningRefundCash.who == Who.CASHIER && AppWords.lightningRefundCash.next == "Give the refund in cash from the till.", "a Lightning-paid sale is refunded in cash by the cashier")
        ok(AppWords.pinWrong(4).tone == Tones.STOP && AppWords.pinWrong(4).next.contains("4 seconds"), "a wrong PIN says how long")
        ok(AppWords.waitText(1) == "1 second" && AppWords.waitText(59) == "59 seconds", "short waits")
        ok(AppWords.waitText(60) == "1 minute" && AppWords.waitText(300) == "5 minutes" && AppWords.waitText(121) == "3 minutes", "long waits")
        ok(AppWords.priceReady("1.00 USD", 1).tone == Tones.GOOD, "a ready price is green")
        ok(AppWords.cameraOff.who == Who.MANAGER && AppWords.cameraOff.tone == Tones.STOP, "the camera-off words: the manager, stop")
        ok(AppWords.cameraOff.next.contains("settings") && !AppWords.cameraOff.next.contains("paste") && !AppWords.cameraOff.what.contains("file"), "they say to allow it, and that pairing needs the camera (CR-90)")
        ok(AppWords.cameraOff.what.contains("pairing needs it"), "the camera-off words say why")
        ok(AppWords.cameraNotStarted("bad\u0007 text").detail == "bad text", "the camera's own text is cleaned in the details")
        ok(all.map { it.first }.toSet().size == all.size, "every name is used once")
        val dirty = AppWords.fileNotRead("bad\u0007 text‮")
        ok(dirty.detail == "bad text", "a system text is cleaned in the details: ${dirty.detail}")
        ok(SayCheck.problem(dirty) == null, "words with a cleaned detail pass")
        val text = AppWords.screensMarkdown()
        ok(text.lines().filter { it.startsWith("| ") }.size == all.size + 1, "one row for each words and the header")
        ok(text.lines().filter { !it.startsWith("|") }.all { it.length <= 80 }, "lines are at most 80 characters")
        if (screensMd != null) {
            if (System.getenv("OPENBMT_WRITE_SCREENS") != null) screensMd.writeText(text)
            ok(screensMd.isFile && screensMd.readText() == text, "docs/screens/counter-app.md is what screensMarkdown makes (run with OPENBMT_WRITE_SCREENS=1)")
        }
        // The library's words go through the same checks.
        val lib = Words(2, 3, "Heading", "What happened.", "Do this.", "CR-12", 1_700_000_000L, Quoted("The POS", "a\u0007b"), "d")
        val say = lib.say()
        ok(say.heading == "Heading" && say.code == "CR-12" && say.at == 1_700_000_000L && say.quoted!!.from == "The POS", "words convert")
        ok(CounterException("r", "t", 1, lib).sayWords() == say, "a refusal with words shows them")
        ok(CounterException("r", "t", 4).sayWords().heading == "Answer not readable", "a refusal with none shows the app's own")
    }

    // ---- the PIN ----------------------------------------------------------------------------

    private class FakeHasher : PinHasher {
        var calls = 0

        override fun hash(pin: CharArray, salt: ByteArray, iterations: Int): ByteArray {
            calls++
            return java.security.MessageDigest.getInstance("SHA-256").digest(salt + String(pin).toByteArray() + iterations.toString().toByteArray())
        }
    }

    private fun pinTests() {
        // PBKDF2-HMAC-SHA256, the published test vectors (RFC 7914, section 11).
        fun hex(b: ByteArray) = b.joinToString("") { "%02x".format(it) }
        ok(hex(Pbkdf2Hasher.hash("password".toCharArray(), "salt".toByteArray(), 1)) == "120fb6cffcf8b32c43e7225256c4f837a86548c92ccc35480805987cb70be17b", "PBKDF2 vector, 1 iteration")
        ok(hex(Pbkdf2Hasher.hash("password".toCharArray(), "salt".toByteArray(), 2)) == "ae4d0c95af6b46d32d0adff928f06dd02a303f8ef3c251dfd6e2d85a95474c43", "PBKDF2 vector, 2 iterations")

        ok(PinStore.validPin("123456") && PinStore.validPin("123456789012"), "6 and 12 digits")
        ok(!PinStore.validPin("12345") && !PinStore.validPin("1234567890123") && !PinStore.validPin("12345a") && !PinStore.validPin("12 456") && !PinStore.validPin(""), "bad PINs")
        ok((1L..12L).map { PinStore.delayFor(it) } == listOf(1L, 2, 4, 8, 16, 32, 64, 128, 256, 300, 300, 300), "delays 1, 2, 4 up to 300")
        ok(PinStore.delayFor(0) == 0L && PinStore.delayFor(10_000) == 300L, "delay edges")

        val dir = tempDir()
        var now = 1_000L
        val hasher = FakeHasher()
        val store = PinStore(dir, { now }, SecureRandom(), hasher, 1000)
        ok(!store.isSet(), "no PIN at first")
        throws("a check with no PIN") { store.check("123456") }
        throws("a PIN of 5 digits") { store.set("12345") }
        store.set("123456")
        ok(store.isSet() && File(dir, "pin.obcp").length() == PinStore.SIZE.toLong(), "the file has the length of the layout")
        ok(dir.list()!!.sorted() == listOf("pin.obcp"), "no temporary file is left")
        ok(!String(File(dir, "pin.obcp").readBytes(), Charsets.ISO_8859_1).contains("123456"), "the PIN is not in the file")
        ok(store.check("123456") == PinResult.Ok, "the right PIN")
        // Wrong tries hold the PIN for 1, 2, 4, 8 seconds.
        for (expected in listOf(1L, 2L, 4L, 8L)) {
            val r = store.check("000000")
            ok(r is PinResult.Wrong && r.waitSecs == expected, "wrong try holds for $expected s: $r")
            val locked = store.check("123456")
            ok(locked is PinResult.Locked && locked.waitSecs == expected, "locked for $expected s, even with the right PIN: $locked")
            now += expected - 1
            ok(store.check("123456") is PinResult.Locked, "still locked one second early")
            now += 1
        }
        ok(store.check("123456") == PinResult.Ok, "after the wait the right PIN is taken and the count is cleared")
        val again = store.check("999999")
        ok(again is PinResult.Wrong && again.waitSecs == 1L, "the count started over")
        // A locked try does not compare and does not count.
        val callsBefore = hasher.calls
        ok(store.check("999999") is PinResult.Locked, "locked")
        ok(hasher.calls == callsBefore, "a locked try is not compared")
        // The count and the hold survive a restart of the app.
        val reopened = PinStore(dir, { now }, SecureRandom(), hasher, 1000)
        ok(reopened.check("123456") is PinResult.Locked, "the hold is kept in the file")
        ok(reopened.heldFor() == 1L, "held for the rest of the second: ${reopened.heldFor()}")
        now += 1
        ok(reopened.heldFor() == 0L, "no longer held")
        // The cap.
        for (i in 1..14) {
            reopened.check("000000")
            now += 400
        }
        val capped = reopened.check("000000")
        ok(capped is PinResult.Wrong && capped.waitSecs == 300L, "the delay is capped at 5 minutes: $capped")
        now += 300
        ok(reopened.check("123456") == PinResult.Ok, "the right PIN after the cap")
        // A new PIN replaces the old one and the count.
        reopened.set("654321")
        ok(reopened.check("123456") is PinResult.Wrong, "the old PIN no longer works")
        now += 2
        ok(reopened.check("654321") == PinResult.Ok, "the new PIN works")
        // Two PINs get two salts and two hashes.
        val a = File(dir, "pin.obcp").readBytes()
        reopened.set("654321")
        val b = File(dir, "pin.obcp").readBytes()
        ok(!a.copyOfRange(9, 25).contentEquals(b.copyOfRange(9, 25)), "a fresh salt for every PIN")
        ok(!a.copyOfRange(25, 57).contentEquals(b.copyOfRange(25, 57)), "so a fresh hash for the same PIN")

        // The file is strict.
        val good = File(dir, "pin.obcp").readBytes()
        for (cut in listOf(0, 4, 5, 20, good.size - 1)) {
            File(dir, "pin.obcp").writeBytes(good.copyOf(cut))
            throws("a PIN file cut to $cut") { PinStore(dir, { now }, SecureRandom(), hasher, 1000).check("654321") }
        }
        File(dir, "pin.obcp").writeBytes(good + byteArrayOf(0))
        throws("a trailing byte") { PinStore(dir, { now }, SecureRandom(), hasher, 1000).check("654321") }
        for (at in listOf(0, 3, 4)) {
            File(dir, "pin.obcp").writeBytes(good.copyOf().also { it[at] = (it[at].toInt() xor 1).toByte() })
            throws("a changed header byte $at") { PinStore(dir, { now }, SecureRandom(), hasher, 1000).check("654321") }
        }
        File(dir, "pin.obcp").writeBytes(good.copyOf().also { it[5] = 0; it[6] = 0; it[7] = 0; it[8] = 0 })
        throws("an iteration count of zero") { PinStore(dir, { now }, SecureRandom(), hasher, 1000).check("654321") }
        File(dir, "pin.obcp").writeBytes(good)
        ok(PinStore(dir, { now }, SecureRandom(), hasher, 1000).check("654321") == PinResult.Ok, "the good file reads")

        // A store that cannot write: nothing is compared.
        val gone = tempDir()
        val lone = PinStore(gone, { now }, SecureRandom(), hasher, 1000)
        lone.set("123456")
        val before = hasher.calls
        File(gone, "pin.obcp").setReadOnly()
        gone.setWritable(false)
        val canWrite = try { File(gone, "probe").createNewFile() } catch (_: IOException) { false }
        if (!canWrite) {
            throws("a store that cannot be written") { lone.check("123456") }
            ok(hasher.calls == before, "no comparison when the try could not be stored")
        } else {
            File(gone, "probe").delete()
        }
        gone.setWritable(true)
        clean(gone)

        // The real parameters: 600,000 iterations, a real salt, the platform's PBKDF2.
        val real = PinStore(dir, { now })
        real.set("246810")
        val bytes = File(dir, "pin.obcp").readBytes()
        ok(java.nio.ByteBuffer.wrap(bytes, 5, 4).order(java.nio.ByteOrder.LITTLE_ENDIAN).int == 600_000, "600,000 iterations are written")
        ok(real.check("246810") == PinResult.Ok, "the real hasher takes the right PIN")
        ok(real.check("135790") is PinResult.Wrong, "and refuses another")
        clean(dir)
    }

    // ---- the pairing steps ------------------------------------------------------------------

    private fun pairingTests() {
        // The order of the screens (CR-98): the camera for the bundle opens only after "They match".
        val p = PairingProgress()
        ok(p.phase == PairPhase.Idle, "nothing under way at first")
        ok(!p.offerRead() && !p.matched() && !p.differed() && !p.finished(), "no step is taken from idle")
        ok(p.start() && p.phase == PairPhase.ScanOffer, "the camera opens for the offer")
        ok(!p.matched() && !p.finished() && !p.start(), "the offer comes first, and a second start is refused")
        ok(p.offerRead() && p.phase == PairPhase.Compare, "the words are on the screen")
        ok(!p.finished() && !p.offerRead(), "no bundle is taken before They match")
        ok(p.matched() && p.phase == PairPhase.ScanBundle, "They match opens the camera for the bundle")
        ok(!p.matched() && !p.differed(), "They match cannot be said twice, and They differ is too late")
        ok(p.finished() && p.phase == PairPhase.Idle, "the pairing is over")
        // They differ ends it.
        ok(p.start() && p.offerRead() && p.differed() && p.phase == PairPhase.Idle, "They differ ends the ceremony")
        ok(!p.matched() && !p.finished(), "after They differ there is nothing to match or finish")
        // A cancel ends any phase and says whether one was open.
        ok(!p.cancelled(), "nothing to cancel when idle")
        p.start()
        p.offerRead()
        p.matched()
        ok(p.cancelled() && p.phase == PairPhase.Idle, "a cancel ends the camera for the bundle")
        // Leaving the app with a ceremony open forgets it and says so once; leaving with none says nothing.
        ok(!p.left() && !p.takeForgotten(), "leaving with nothing open says nothing")
        p.start()
        p.offerRead()
        ok(p.left() && p.phase == PairPhase.Idle, "leaving ends the ceremony, and the library is told")
        ok(p.takeForgotten() && !p.takeForgotten(), "the message comes once")
        ok(AppWords.pairingForgotten.next.contains("Key app") && AppWords.pairingForgotten.what.contains("forgotten"), "the words say what happened and to start again from the Key app")

        // What is checked before the camera opens.
        fun problem(counter: String = "Front", manager: String = "Mia", pin1: String = "246810", pin2: String = "246810") =
            FirstStartCheck.problem(counter, manager, pin1, pin2)
        ok(problem() == null, "good entries")
        ok(problem(counter = "") === AppWords.counterNameNotAccepted, "no counter name")
        ok(problem(counter = "x".repeat(33)) === AppWords.counterNameNotAccepted, "a counter name of 33 characters")
        ok(problem(counter = "x".repeat(32)) == null, "a counter name of 32 characters")
        ok(problem(counter = "\u00e9".repeat(32)) == null, "a name of 32 two-byte characters")
        ok(problem(counter = "a\nb") === AppWords.counterNameNotAccepted, "a control character")
        ok(problem(manager = "") === AppWords.nameNotAccepted, "no manager name")
        ok(problem(pin1 = "12345", pin2 = "12345") === AppWords.pinNotAccepted, "a short PIN")
        ok(problem(pin2 = "246811") === AppWords.pinNotAccepted, "two different PINs")
        ok(NetworkCheck.problem("orbot") === AppWords.torAddressNotAccepted, "an address that is not host:port")
        ok(NetworkCheck.problem("127.0.0.1:9050") == null && NetworkCheck.problem("192.168.1.5:9150") == null, "an address that is")
        ok(AppConfig(AppConfig.DEFAULT_PROXY, "Mia", "Sam", "node.onion").withProxy("10.0.0.2:9150").encode().toString(Charsets.UTF_8) == "proxy: 10.0.0.2:9150\nmanager: Mia\ncashier: Sam\nownnode: node.onion\ntor: builtin\n", "the Network screen changes the proxy and nothing else")
        ok(AppWords.orbotNotRunning.next.contains("Network") && AppWords.networkAbout.who == Who.MANAGER, "the Orbot words point to the Network screen")
        ok(!AppWords.notSetUp.next.contains("Orbot") && !AppWords.torIsOn.next.contains("below"), "the first start no longer asks for Orbot's address")
        ok(FirstStartCheck.wordsLine(listOf("river", "tulip", "anchor", "maple")) == "river   tulip   anchor   maple", "the four words on one line, spaced")

        ok(HostPort.valid("127.0.0.1:9050") && HostPort.valid("localhost:1") && HostPort.valid("[::1]:9050") && HostPort.valid("a.b-c.onion:65535"), "host:port")
        ok(!HostPort.valid("127.0.0.1") && !HostPort.valid(":9050") && !HostPort.valid("h:0") && !HostPort.valid("h:65536") && !HostPort.valid("h:") && !HostPort.valid("h p:1") && !HostPort.valid("[]:1"), "not host:port")
    }

    // ---- the config -----------------------------------------------------------------------------

    private fun configTests() {
        val c = AppConfig("127.0.0.1:9050", "Mia", "Ann")
        val back = AppConfig.decode(c.encode())
        ok(back.proxy == "127.0.0.1:9050" && back.manager == "Mia" && back.cashier == "Ann" && back.ownNode == "", "round trip")
        val own = c.withOwnNode("a4dqobyha4dqobyha4dqobyha4dqobyha4dqobyha4dqobyha4dwc6ad.onion:18444")
        ok(AppConfig.decode(own.encode()).ownNode == own.ownNode && own.withCashier("Bo").ownNode == own.ownNode, "an own node, kept (CR-100)")
        ok(AppConfig.decode(own.withOwnNode("").encode()).ownNode == "", "an own node, removed")
        ok(AppConfig.decode("proxy: h:1\nmanager: M\ncashier: \n".toByteArray(Charsets.UTF_8)).ownNode == "", "a three-line file has no own node")
        ok(String(c.encode(), Charsets.UTF_8).endsWith("cashier: Ann\nownnode: \ntor: builtin\n"), "the own node line and the choice of Tor are always written")
        ok(!AppConfig.validOwnNode("a b") && !AppConfig.validOwnNode("x".repeat(129)) && AppConfig.validOwnNode("") && AppConfig.validOwnNode("x.onion:1"), "own node text")
        ok(AppConfig.decode(c.withCashier("").encode()).cashier == "", "an empty cashier name")
        ok(AppConfig.decode(AppConfig("h:1", "Méa", "").encode()).manager == "Méa", "a name with a letter outside ASCII")
        ok(AppConfig.validName("Ann Lee") && !AppConfig.validName("") && !AppConfig.validName(" Ann") && !AppConfig.validName("A\nB") && !AppConfig.validName("x".repeat(65)), "names")
        ok(AppConfig.validName("x".repeat(64)) && !AppConfig.validName("é".repeat(64).plus("é".repeat(64))), "name lengths")
        val text = String(c.encode(), Charsets.UTF_8)
        for (bad in listOf("", text.dropLast(1), text + "x: y\n", text.replace("ownnode:", "own:"), text.replace("ownnode: ", "ownnode: a b"), text + "ownnode: \n", text.replace("proxy:", "prox:"), text.replace("127.0.0.1:9050", "nope"), text.replace("manager: Mia", "manager: "), "proxy: h:1\nmanager: M\ncashier: \u0007\n")) {
            var failed = false
            try {
                AppConfig.decode(bad.toByteArray(Charsets.UTF_8))
            } catch (_: ConfigFormatException) {
                failed = true
            }
            ok(failed, "a config that is refused: ${bad.take(20).replace("\n", "|")}")
        }
        var failed = false
        try {
            AppConfig.decode(byteArrayOf(0xff.toByte()))
        } catch (_: ConfigFormatException) {
            failed = true
        }
        ok(failed, "not UTF-8")
        val dir = tempDir()
        val file = ConfigFile(dir)
        ok(file.load() == null, "a first start")
        file.save(c)
        ok(file.load()!!.manager == "Mia" && dir.list()!!.sorted() == listOf("config.txt"), "saved and read")
        // The sounds choice: on by default, saved next to the cashier's name, read back, and kept by the other changes.
        ok(c.sounds && back.sounds && AppConfig.decode("proxy: h:1\nmanager: M\ncashier: \n".toByteArray(Charsets.UTF_8)).sounds, "the sounds are on by default")
        val quietOn = c.withSounds(false)
        ok(String(quietOn.encode(), Charsets.UTF_8) == "proxy: 127.0.0.1:9050\nmanager: Mia\ncashier: Ann\nownnode: \nsounds: off\ntor: builtin\n", "the sounds line is written when they are off")
        ok(String(c.withSounds(true).encode(), Charsets.UTF_8) == String(c.encode(), Charsets.UTF_8), "and not when they are on")
        file.save(quietOn)
        ok(!file.load()!!.sounds && file.load()!!.cashier == "Ann", "the choice is saved and read back")
        ok(!quietOn.withCashier("Bo").sounds && !quietOn.withProxy("h:2").sounds && !quietOn.withOwnNode("x.onion:1").sounds, "a change of another setting keeps the choice")
        ok(quietOn.withSounds(true).sounds && file.also { it.save(quietOn.withSounds(true)) }.load()!!.sounds, "switched on again")
        var refused = 0
        for (bad in listOf("sounds: on", "sound: off", "sounds: ", "sounds: maybe")) {
            try {
                AppConfig.decode("proxy: h:1\nmanager: M\ncashier: \nownnode: \n$bad\n".toByteArray(Charsets.UTF_8))
            } catch (_: ConfigFormatException) {
                refused++
            }
        }
        ok(refused == 4, "a sounds line that is not \"sounds: off\" is refused")
        clean(dir)
    }

    // ---- the keypad ---------------------------------------------------------------------------------

    private fun amountTests() {
        val usd = Money.of("USD")!!
        ok(usd.show(0) == "0.00 USD" && usd.show(5) == "0.05 USD" && usd.show(1234) == "12.34 USD", "small amounts")
        ok(usd.show(123456789) == "1,234,567.89 USD" && usd.plain(100) == "1.00", "separators")
        ok(Money.of("JPY")!!.show(1500) == "1,500 JPY" && Money.of("XXX") == null, "currencies")
        // CR-193, CR-196: the nine currencies and Bitcoin Standard, with the places after the point of ISO 4217.
        for (code in listOf("USD", "EUR", "GBP", "AUD", "INR", "IDR", "SGD")) ok(Money.of(code)!!.exponent == 2 && !Money.of(code)!!.isSats, "$code has two places")
        for (code in listOf("JPY", "KRW")) ok(Money.of(code)!!.exponent == 0 && Money.of(code)!!.show(12_345) == "12,345 $code", "$code has none")
        ok(Money.of("CAD") == null && Money.of("CHF") == null, "a currency that is not a choice of the shop")
        val sats = Money.of("SAT")!!
        ok(sats.isSats && sats.exponent == 0 && sats.show(7_500) == "7,500 sats" && sats.show(1) == "1 sat" && sats.parse("7500") == 7_500L && sats.parse("7.5") == null, "Bitcoin Standard: whole sats, no point")
        val yen = AmountEntry(Money.of("JPY")!!)
        for (d in "1500") yen.press(d)
        ok(yen.minor == 1500L && yen.show() == "1,500 JPY", "the keypad of a currency with no minor digits takes whole units")
        val sat = AmountEntry(sats)
        ok(sat.show() == "0 sats", "the keypad of Bitcoin Standard starts at no sats")
        for (d in "7500") sat.press(d)
        ok(sat.minor == 7_500L && sat.show() == "7,500 sats", "and takes whole sats")
        ok(usd.parse("12.34") == 1234L && usd.parse("12") == 1200L && usd.parse("12.3") == 1230L && usd.parse("1,000.50") == 100050L && usd.parse(".5") == 50L, "typed amounts")
        ok(usd.parse("") == null && usd.parse(".") == null && usd.parse("1.234") == null && usd.parse("1.2.3") == null && usd.parse("-1") == null && usd.parse("a") == null && usd.parse("1".repeat(11)) == null, "amounts that are refused")
        ok(Money.of("JPY")!!.parse("1500") == 1500L && Money.of("JPY")!!.parse("1.5") == null, "an amount in a currency with no decimals")
        val e = AmountEntry(usd)
        ok(e.minor == 0L && e.show() == "0.00 USD", "empty")
        ok(!e.press('0') && e.minor == 0L, "a leading zero is ignored")
        for (d in "1234") e.press(d)
        ok(e.minor == 1234L && e.show() == "12.34 USD", "digits push into the minor units")
        e.press('0')
        ok(e.minor == 12340L, "a zero after digits")
        e.backspace()
        ok(e.minor == 1234L, "backspace")
        ok(!e.press('x') && !e.press('-'), "only digits")
        e.clear()
        ok(e.minor == 0L, "clear")
        e.backspace()
        ok(e.minor == 0L, "backspace on nothing")
        for (i in 1..AmountEntry.MAX_DIGITS) ok(e.press('9'), "digit $i")
        ok(!e.press('9') && e.minor == 999_999_999L, "no more than 9 digits")
    }

    // ---- the code on screen ----------------------------------------------------------------------------

    private fun qrTests() {
        // Version 1 is 21 modules; 21 + 8 = 29 cells across.
        val b = QrLayout.box(21, 290, 400)
        ok(b.cell == 10 && b.side == 290 && b.left == 0 && b.top == 55, "as large as fits in a narrow box: $b")
        ok(b.x(0) == 40 && b.y(0) == 95, "the first module is after the quiet zone of 4 cells")
        ok(b.x(20) + b.cell == 290 - 40, "the last module ends before the quiet zone")
        val c = QrLayout.box(25, 1000, 700)
        ok(c.cell == 700 / 33 && c.side == c.cell * 33 && c.left == (1000 - c.side) / 2 && c.top == (700 - c.side) / 2, "centred in a wide box")
        ok(QrLayout.box(21, 28, 28).cell == 0 && !QrLayout.box(21, 28, 28).fits, "a box too small for a cell")
        ok(QrLayout.box(21, 29, 29).fits, "29 pixels is 1 pixel a cell")
        ok(QrLayout.box(177, 1080, 1920).cell == 1080 / 185, "version 40")
        throws("no modules") { QrLayout.box(0, 100, 100) }
        // The module mapping of the library's payment, as the view reads it.
        val bits = byteArrayOf(0b1010_0001.toByte(), 0)
        val p = Payment("a", "A", "a", 3L, bits)
        val dark = ArrayList<Pair<Int, Int>>()
        for (y in 0 until 3) for (x in 0 until 3) if (p.module(x, y)) dark.add(x to y)
        ok(dark == listOf(0 to 0, 2 to 0, 1 to 2), "rows first, most significant bit first: $dark")
        val small = QrLayout.box(3, 110, 110)
        ok(small.cell == 10, "a 3 by 3 code has 11 cells across")
        ok(dark.map { (x, y) -> small.x(x) to small.y(y) } == listOf(40 to 40, 60 to 40, 50 to 60), "dark modules at their places")
    }

    // ---- the price ----------------------------------------------------------------------------------------

    private class Clock(var ms: Long) {
        val secs: Long get() = ms / 1000
    }

    /** Prices are the digits of a reply; two fresh sources make the median. */
    private class FakeRates(val clock: Clock) : RatesPort {
        val parsed = ArrayList<String>()

        override fun parse(source: String, body: ByteArray, fetchedAt: Long): Sample {
            parsed.add(source)
            val cents = String(body).toLongOrNull()
                ?: throw CounterException("RT-13", "RT-13: not a price", 1, Words(2, 0, "Price not read", "A reply was not a price.", "The app asks again.", null, null, null, null))
            return Sample(source, fetchedAt, cents)
        }

        override fun combine(samples: List<Sample>, now: Long): Rate {
            val fresh = samples.filter { now - it.fetchedAt <= 600 }
            if (fresh.size < 2) {
                throw CounterException("RT-31", "RT-31: too few", 1, Words(2, 2, "Price not available", "The price is not available right now.", "Take another kind of payment.", "RT-31", null, null, null))
            }
            val sorted = fresh.map { it.minorPerBtc }.sorted()
            return Rate(sorted[(sorted.size - 1) / 2], "med." + fresh.joinToString("") { it.source.take(1) }, fresh.minOf { it.fetchedAt })
        }
    }

    private class FakeFetcher(val clock: Clock) : Fetcher {
        val asked = ArrayList<Pair<String, Long>>()
        val replies = HashMap<String, FetchResult>()
        var spendMs = 0L

        override fun fetch(source: RateSource): FetchResult {
            asked.add(source.id to clock.ms)
            clock.ms += spendMs
            return replies[source.id] ?: FetchResult.NotReached("no reply")
        }
    }

    private fun src(id: String) = RateSource(id, "https://$id.example/", "USD", id.take(1))

    private fun priceTests() {
        ok(PriceLimits.FETCH_INTERVAL_SECS == 60L && PriceLimits.FETCH_TIMEOUT_SECS == 20L && PriceLimits.MAX_RESPONSE_BYTES == 65536, "the numbers of the rate rules")
        // The schedule.
        val s = PriceSchedule(listOf("a", "b"), 5_000)
        ok(!s.due("a", 4_999) && s.due("a", 5_000) && s.waitMs("a", 4_000) == 1_000L, "the first turn is at the start")
        s.taken("a", 5_200)
        ok(!s.due("a", 64_999) && s.due("a", 65_000), "the next turn is one interval after the last turn, not after the fetch")
        ok(s.due("b", 5_000), "another source keeps its own turn")
        s.taken("a", 190_000)
        ok(s.waitMs("a", 190_000) == 55_000L && !s.due("a", 244_999) && s.due("a", 245_000), "missed turns are skipped, and the grid stays")
        s.taken("a", 245_000)
        ok(s.waitMs("a", 245_000) == 60_000L, "a turn taken exactly on time moves a whole interval")
        ok(!s.due("nobody", 1_000_000), "an unknown source is never due")
        // Two fetches of a source are never closer than the interval, whatever the late turns.
        var last = -1_000_000L
        var spaced = true
        val t = PriceSchedule(listOf("a"), 0)
        for (now in listOf(0L, 100L, 60_000L, 61_000L, 130_000L, 130_100L, 400_000L, 400_001L)) {
            if (t.due("a", now)) {
                if (now - last < 60_000 && last >= 0) spaced = false
                last = now
                t.taken("a", now)
            }
        }
        ok(spaced, "no two fetches of a source are less than 60 seconds apart")

        // The book and the runner, with a fake clock and a fake fetcher.
        val clock = Clock(1_800_000_000_000L)
        val rates = FakeRates(clock)
        val fetcher = FakeFetcher(clock)
        val book = PriceBook(rates, { clock.secs })
        val runner = PriceRunner(book, fetcher)
        ok(book.status() is PriceStatus.Refused, "no price before the first turn")
        ok((book.status() as PriceStatus.Refused).words.heading == "Price not available", "the library's words are shown")
        fetcher.spendMs = 3_000
        fetcher.replies["kraken"] = FetchResult.Body("7355450".toByteArray())
        fetcher.replies["coinbase"] = FetchResult.Body("7355218".toByteArray())
        runner.turn(src("kraken"))
        ok(book.status() is PriceStatus.Refused, "one source is not enough")
        runner.turn(src("coinbase"))
        val ready = book.status()
        ok(ready is PriceStatus.Ready && ready.rate.rate == 7_355_218L && ready.sources == 2, "two sources make a price: ${ready::class.simpleName}")
        val n = book.notes()
        // RT-45: the sample is stamped when the reply has been read: after the fetch took its time.
        ok(n.size == 2 && n[0].atSecs == 1_800_000_003L && n[0].price == 7_355_450L && n[0].words == null, "a sample is stamped when the reply is in: ${n.map { it.atSecs }}")
        // RT-44: a failed turn is a missing sample and a note; the older sample stays until it is too old.
        fetcher.replies["kraken"] = FetchResult.Status(503)
        runner.turn(src("kraken"))
        ok(book.notes()[0].words!!.heading == "Price source said no" && book.notes()[0].words!!.detail == "Status 503.", "a status that is not 200 is a note")
        ok(book.status() is PriceStatus.Ready, "the sample of a failed turn is not thrown away early")
        fetcher.replies["kraken"] = FetchResult.NotReached("timeout")
        runner.turn(src("kraken"))
        ok(book.notes()[0].words!!.heading == "Price source not reached", "a timeout is a note")
        fetcher.replies["kraken"] = FetchResult.TooLong
        runner.turn(src("kraken"))
        ok(book.notes()[0].words!!.heading == "Price reply too long", "a reply over the limit is a note")
        fetcher.replies["kraken"] = FetchResult.Body("not a price".toByteArray())
        runner.turn(src("kraken"))
        ok(book.notes()[0].words!!.heading == "Price not read" && book.notes()[0].price == null, "a reply the library refuses is a note with the library's words")
        // The samples go stale: the library refuses and the book says so; nothing is invented.
        clock.ms += 700_000
        ok(book.status() is PriceStatus.Refused, "stale samples give the library's refusal")
        ok(book.rate().isFailure && book.rate().exceptionOrNull() is CounterException, "no rate for a sale")
        // Tor on with Orbot missing: nothing is parsed, and the status says why.
        val parsedBefore = rates.parsed.size
        fetcher.replies["kraken"] = FetchResult.TorMissing
        runner.turn(src("kraken"))
        ok(book.status() is PriceStatus.TorMissing && rates.parsed.size == parsedBefore, "a missing Orbot is its own status and parses nothing")
        ok(book.notes()[0].words == AppWords.orbotNotRunning, "and a note with the Orbot words")
        // CR-204: with the built-in Tor the missing proxy is Tor's, and Orbot is not named.
        ok(TorChoice.BuiltIn.missingWords === AppWords.torFailed && TorChoice.Orbot.missingWords === AppWords.orbotNotRunning, "CR-204: each choice has its words for a proxy that does not answer")
        val builtInBook = PriceBook(rates, { clock.secs }, TorChoice.BuiltIn.missingWords)
        PriceRunner(builtInBook, fetcher).turn(src("kraken"))
        ok(builtInBook.status() is PriceStatus.TorMissing && builtInBook.torMissingWords === AppWords.torFailed && builtInBook.notes()[0].words === AppWords.torFailed, "CR-204: a built-in Tor that does not answer is a note and a status in Tor's words")
        fetcher.replies["kraken"] = FetchResult.Body("7355450".toByteArray())
        fetcher.replies["coinbase"] = FetchResult.Body("7355218".toByteArray())
        runner.turn(src("kraken"))
        runner.turn(src("coinbase"))
        ok(book.status() is PriceStatus.Ready, "the price comes back when Orbot does")
        // RT-41: the rate for a sale is made from the samples already there; asking for it fetches nothing.
        val askedBefore = fetcher.asked.size
        book.rate()
        book.status()
        ok(fetcher.asked.size == askedBefore, "a quote never fetches")
    }

    // ---- the link to the owner (CR-134, CR-135) -----------------------------------------------------------------

    private fun linkTests() {
        var asked = 0
        var answer = true
        val probe = SocksProbe { _, _ -> asked++; answer }
        ok(LinkProxy.EVERY_MS == 5000L, "the link's turn is every 5 seconds")
        ok(LinkProxy.pick(false, false, "127.0.0.1:9050", probe) == null && asked == 0, "an address on the local network needs no proxy and none is asked")
        ok(LinkProxy.pick(true, true, "127.0.0.1:9050", probe) == null && asked == 0, "Tor on: the node's proxy is the library's, nothing more is given")
        ok(LinkProxy.pick(true, false, "127.0.0.1:9050", probe) == "127.0.0.1:9050" && asked == 1, "an onion relay with Tor off gets Orbot's address when Orbot answers")
        answer = false
        ok(LinkProxy.pick(true, false, "127.0.0.1:9050", probe) == null, "Orbot does not answer: no proxy, and the library says Orbot is needed")
        ok(LinkProxy.pick(true, false, "nonsense", probe) == null, "an address that is not an address gives none")
    }

    // ---- Tor ------------------------------------------------------------------------------------------------------

    private fun torTests() {
        var asked: Pair<String, Int>? = null
        var answer = true
        val probe = SocksProbe { h, p -> asked = h to p; answer }
        val off = TorGate(false, "127.0.0.1:9050", probe)
        ok(off.proxy == null && off.available() && asked == null, "Tor off: connections are direct and the proxy is not asked")
        val on = TorGate(true, "127.0.0.1:9050", probe)
        ok(on.proxy == "127.0.0.1:9050", "Tor on: the node's proxy is the address")
        ok(on.available() && asked == "127.0.0.1" to 9050, "Orbot answers")
        answer = false
        ok(!on.available(), "Orbot does not answer: not available")
        ok(!TorGate(true, "nonsense", probe).available(), "an address that is not an address is not available")
        // The real probe, against a SOCKS5-speaking stand-in on the loopback: no network beyond this machine.
        fun server(reply: ByteArray?): Pair<ServerSocket, Thread> {
            val ss = ServerSocket(0, 1, InetAddress.getByName("127.0.0.1"))
            val t = Thread {
                try {
                    ss.accept().use { c ->
                        val got = ByteArray(3)
                        var n = 0
                        while (n < 3) n += c.getInputStream().read(got, n, 3 - n).also { if (it < 0) return@Thread }
                        if (got.contentEquals(byteArrayOf(5, 1, 0)) && reply != null) {
                            c.getOutputStream().write(reply)
                            c.getOutputStream().flush()
                        }
                    }
                } catch (_: IOException) {
                }
            }
            t.isDaemon = true
            t.start()
            return ss to t
        }
        val real = Socks5Probe(1500)
        val (ok1, t1) = server(byteArrayOf(5, 0))
        ok(real.answers("127.0.0.1", ok1.localPort), "a SOCKS5 server that accepts no authentication")
        t1.join(2000); ok1.close()
        val (bad, t2) = server(byteArrayOf(72, 84))
        ok(!real.answers("127.0.0.1", bad.localPort), "a port that speaks something else is not Orbot")
        t2.join(2000); bad.close()
        val (none, t3) = server(byteArrayOf(5, (0xff).toByte()))
        ok(!real.answers("127.0.0.1", none.localPort), "a SOCKS5 server that wants authentication is not usable")
        t3.join(2000); none.close()
        val (silent, t4) = server(null)
        ok(!Socks5Probe(300).answers("127.0.0.1", silent.localPort), "a server that does not answer")
        t4.join(2000); silent.close()
        val free = ServerSocket(0, 1, InetAddress.getByName("127.0.0.1")).also { it.close() }
        ok(!real.answers("127.0.0.1", free.localPort), "nothing listens")
    }

    // ---- the screens ------------------------------------------------------------------------------------------------

    private fun screenTests() {
        val none = SetupStatus(pairing = false, pin = false, config = false)
        val all = SetupStatus(pairing = true, pin = true, config = true)
        val bad = AppWords.fault(CounterException("r", "t", 4))
        ok(ScreenModel.pick(bad, all, AppWords.orbotNotRunning, true) is Screen.Fault, "a fault comes first")
        ok(ScreenModel.pick(null, none, null, false) is Screen.FirstStart, "nothing saved: first start")
        ok(ScreenModel.pick(null, SetupStatus(true, true, false), null, false) is Screen.FirstStart, "a setup that was not finished: first start")
        ok(ScreenModel.pick(null, SetupStatus(true, false, true), null, false) is Screen.FirstStart, "no PIN: first start")
        ok(!none.complete && all.complete, "complete")
        val blocked = ScreenModel.pick(null, all, AppWords.orbotNotRunning, false)
        ok(blocked is Screen.TorBlocked && blocked.words == AppWords.orbotNotRunning, "Tor without Orbot")
        ok(ScreenModel.pick(null, all, null, false) is Screen.Starting, "set up, not running yet")
        ok(ScreenModel.pick(null, all, null, true) is Screen.Home, "running: home")
        // Sale buttons.
        fun b(s: SaleState) = SaleModel.buttons(s)
        ok(b(SaleState.NotPaidYet).let { it.requote && it.cancel && !it.override }, "not paid yet: requote and cancel")
        ok(b(SaleState.PartPaid).let { it.requote && !it.cancel && it.override }, "part paid: requote and override")
        ok(b(SaleState.Waiting).let { !it.requote && !it.cancel && it.override }, "waiting: override")
        ok(b(SaleState.DoNotHandOver).let { !it.requote && !it.cancel && it.override }, "do not hand over: override")
        for (s in listOf(SaleState.PaidInstant, SaleState.Confirmed, SaleState.Final, SaleState.Overridden, SaleState.Expired, SaleState.Cancelled, SaleState.NeedsAPerson, SaleState.PaidLightning)) {
            ok(b(s).let { !it.requote && !it.cancel && !it.override }, "no button for $s")
        }
        val usd = Money.of("USD")!!
        ok(SaleModel.amountLine(usd, 1234, 25_000) == "12.34 USD, 25,000 sats to send", "the amount line gives both (SC-09)")
        ok(SaleModel.amountLine(usd, null, 1) == "1 sat to send", "an amount in sats alone")
        ok(SaleModel.amountBig(usd, 1234, 25_000) == "12.34 USD" && SaleModel.amountSmall(usd, 1234, 25_000) == "25,000 sats to send", "the band's amount and its line")
        ok(SaleModel.amountBig(usd, 1234, 25_000) + ", " + SaleModel.amountSmall(usd, 1234, 25_000) == SaleModel.amountLine(usd, 1234, 25_000), "the band says what the amount line says")
        ok(SaleModel.amountBig(usd, null, 1) + " " + SaleModel.amountSmall(usd, null, 1) == SaleModel.amountLine(usd, null, 1), "and in sats alone")
        // CR-197: with Bitcoin Standard the amount is the sats, and they are said once.
        val bitcoin = Money.of("SAT")!!
        ok(SaleModel.amountLine(bitcoin, 7_500, 7_500) == "7,500 sats to send" && SaleModel.outcomeLine(bitcoin, 7_500, 7_500) == "7,500 sats", "Bitcoin Standard: the sats once")
        ok(SaleModel.amountBig(bitcoin, 7_500, 7_500) + " " + SaleModel.amountSmall(bitcoin, 7_500, 7_500) == SaleModel.amountLine(bitcoin, 7_500, 7_500), "and the band says it too")
        ok(SaleModel.isPaid(SaleState.PaidInstant) && SaleModel.isPaid(SaleState.Final) && SaleModel.isPaid(SaleState.PaidLightning) && SaleModel.isPaid(SaleState.Overridden), "paid states")
        ok(!SaleModel.isPaid(SaleState.NotPaidYet) && !SaleModel.isPaid(SaleState.DoNotHandOver) && !SaleModel.isPaid(SaleState.Waiting) && !SaleModel.isPaid(SaleState.Expired), "unpaid states")
        ok(SaleState.values().none { SaleModel.isPaid(it) && SaleModel.buttons(it).requote }, "Done and Requote are never both the main action")
        ok(SaleModel.receivedLine(5000, 6000) == "Received 5,000 sats of 6,000 sats", "the received line")
        ok(SaleModel.isEnded(SaleState.Expired) && !SaleModel.isEnded(SaleState.Waiting), "ended sales")
        ok(SaleModel.isEnded(SaleState.PaidLightning), "a sale paid by Lightning is over (CR-109)")
        ok(SaleModel.outcomeLine(usd, 2450, 33_309) == "24.50 USD \u00B7 33,309 sats" && SaleModel.outcomeLine(usd, null, 1) == "1 sat", "the amount line of an outcome screen")
        ok(SaleModel.showsCode(SaleState.NotPaidYet) && SaleModel.showsCode(SaleState.PartPaid) && SaleState.values().filter { SaleModel.showsCode(it) }.size == 2, "only not paid yet and part paid show the code")
        ok(SaleModel.outcomeHeadingSp("Paid") == 56f && SaleModel.outcomeHeadingSp("Do not hand over") == 44f && SaleModel.outcomeHeadingSp("Accepted by a person") == 40f, "the heading's size")
        // The outcome sound: once per sale for green, once for red, never for blue or neutral.
        val plays = OutcomePlays()
        ok(plays.first(1, Colour.Green) && !plays.first(1, Colour.Green) && !plays.first(1, Colour.Green), "green plays once for a sale")
        ok(plays.first(1, Colour.Red) && !plays.first(1, Colour.Red), "red plays once for the same sale")
        ok(plays.first(2, Colour.Green) && plays.first(2, Colour.Red), "another sale plays its own")
        ok((0L..3L).none { plays.first(it, Colour.Blue) || plays.first(it, Colour.Neutral) }, "blue and neutral never play")
        ok(OutcomePlays().let { !it.first(7, Colour.Blue) && it.first(7, Colour.Green) && !it.first(7, Colour.Green) }, "a sale seen waiting first still plays green once")
        val table = listOf(src("a"), RateSource("b", "u", "EUR", "b"), RateSource("c", "u", "EUR", "c"), RateSource("d", "u", "JPY", "d"))
        ok(ShopPrices.sources(Money.of("EUR")!!, table).map { it.id } == listOf("b", "c"), "RT-40: only the sources of the shop's currency are asked")
        ok(ShopPrices.sources(Money.of("SAT")!!, table).isEmpty() && ShopPrices.sources(Money.of("GBP")!!, table).isEmpty(), "Bitcoin Standard asks no source (CR-197), and a currency with none asks none")
    }

    // ---- Lightning: the inputs and the steps of the test (CR-115) --------------------------------------

    private fun lightningTests() {
        ok(LightningSetup.lnd("  ") == null && LightningSetup.lnd("") == null, "no LND code")
        ok(LightningSetup.lnd(" lndconnect://h:1?cert=a&macaroon=b \n")!!.let { it.kind == LnKind.Lnd && it.address == "lndconnect://h:1?cert=a&macaroon=b" && it.secret.isEmpty() && it.cert.isEmpty() }, "an LND code, trimmed")
        ok(LightningSetup.cln("", "r", "c") == null && LightningSetup.cln("h:1", " ", "c") == null && LightningSetup.cln("h:1", "r", "") == null, "a Core Lightning field left empty")
        ok(LightningSetup.cln(" h:1 ", " r ", "-----BEGIN CERTIFICATE-----\nAA\n-----END CERTIFICATE-----\n")!!.let { it.kind == LnKind.Cln && it.address == "h:1" && it.secret == "r" && it.cert.startsWith("-----BEGIN") && it.cert.endsWith("-----") }, "Core Lightning fields, trimmed, the PEM kept whole")

        fun req(id: Long) = LnRequest(id, "h:1", LnMethod.Get, "/p", "n", "v", null)
        val sent = ArrayList<Long>()
        val replied = ArrayList<Triple<Long, Long, Int>>()
        val record = byteArrayOf(9, 8)
        // Two requests, then the record.
        val got = LightningSetup.drive(
            req(1),
            { r -> sent.add(r.id); LnOutcome.Answer(200, ByteArray(3)) },
            { id, status, body -> replied.add(Triple(id, status, body.size)); if (id == 1L) LnStep(req(2), null) else LnStep(null, record) },
            { _, _ -> error("no failure here") },
        )
        ok(got.contentEquals(record) && sent == listOf(1L, 2L) && replied == listOf(Triple(1L, 200L, 3), Triple(2L, 200L, 3)), "a test of two requests ends with the record")
        // A request with no answer goes to failed, with the reason, and nothing is saved.
        var why: LnTransport? = null
        val none = LightningSetup.drive(req(1), { LnOutcome.Failed(LnTransport.PinRefused) }, { _, _, _ -> error("no answer") }, { id, w -> ok(id == 1L, "the id that failed"); why = w })
        ok(none == null && why == LnTransport.PinRefused, "a refused pin stops the test, with its reason")
        // The library may throw from `failed` (the words of why the test stopped): it goes up.
        var thrown = false
        try {
            LightningSetup.drive(req(1), { LnOutcome.Failed(LnTransport.TimedOut) }, { _, _, _ -> error("no answer") }, { _, _ -> throw CounterException("ln", "the words", 4) })
        } catch (_: CounterException) {
            thrown = true
        }
        ok(thrown, "the words of a stopped test are thrown on")
        // A reply with neither a record nor a request ends it; a test that never ends is given up.
        ok(LightningSetup.drive(req(1), { LnOutcome.Answer(200, ByteArray(0)) }, { _, _, _ -> LnStep(null, null) }, { _, _ -> }) == null, "an empty step ends the test")
        var rounds = 0
        ok(LightningSetup.drive(req(1), { rounds++; LnOutcome.Answer(200, ByteArray(0)) }, { id, _, _ -> LnStep(req(id + 1), null) }, { _, _ -> }) == null && rounds == LightningSetup.MAX_STEPS, "a test that never ends is given up")
    }

    // ---- the cashiers (CR-140, CR-141) -----------------------------------------------------------

    private fun cr141CashierRulesTests() {
        ok(StaffStore.validName("Ann") && StaffStore.validName("x".repeat(24)) && StaffStore.validName("é".repeat(24)) && StaffStore.validName("Ann Lee"), "CR-141: names of 1 to 24 characters")
        ok(!StaffStore.validName("") && !StaffStore.validName("x".repeat(25)) && !StaffStore.validName(" Ann") && !StaffStore.validName("Ann ") && !StaffStore.validName("A\nB") && !StaffStore.validName("A\u0007"), "CR-141: names that are not names")
        ok(StaffStore.validPin("1234") && StaffStore.validPin("12345678"), "CR-141: PINs of 4 and 8 digits")
        ok(!StaffStore.validPin("123") && !StaffStore.validPin("123456789") && !StaffStore.validPin("12a4") && !StaffStore.validPin("12 4") && !StaffStore.validPin(""), "CR-141: PINs that are not PINs")
        ok(StaffStore.taken("ANN", listOf("Ann"), "Max") && StaffStore.taken("ann", listOf("Bob", "ANN"), "Max"), "CR-141: unique ignoring case")
        ok(StaffStore.taken("MAX", emptyList(), "Max") && !StaffStore.taken("Maxine", listOf("Ann"), "Max"), "CR-141: not the manager's name")
        ok(StaffStore.MAX_CASHIERS == 30 && StaffStore.MAX_NAME == 24, "CR-140: at most 30 cashiers, names of at most 24")
        ok(StaffCheck.nameProblem("", emptyList(), "Max") === AppWords.cashierNameNotAccepted, "a name that is not a name has its words")
        ok(StaffCheck.nameProblem("max", emptyList(), "Max") === AppWords.cashierNameUsed && StaffCheck.nameProblem("Ann", listOf("ann"), "Max") === AppWords.cashierNameUsed, "a used name has its words")
        ok(StaffCheck.nameProblem("Zed", List(30) { "c$it" }, "Max") === AppWords.cashiersFull, "a full list has its words")
        ok(StaffCheck.nameProblem("Ann", listOf("Bob"), "Max") == null, "a good name")
        ok(StaffCheck.pinProblem("1234", "1234") == null && StaffCheck.pinProblem("123", "123") === AppWords.cashierPinNotAccepted && StaffCheck.pinProblem("1234", "1235") === AppWords.cashierPinNotAccepted, "the PIN, typed twice")
    }

    private fun cr140CashierFileTests() {
        val dir = tempDir()
        var now = 1_000L
        val hasher = FakeHasher()
        fun store() = StaffStore(dir, { now }, { "Max" }, SecureRandom(), hasher, 1000)
        val file = File(dir, "cashiers.txt")
        val s = store()
        ok(s.names().isEmpty() && !file.exists(), "no file is an empty list")
        s.add("Ann", "1234")
        s.add("Bob", "87654321")
        ok(s.names() == listOf("Ann", "Bob"), "CR-140: the cashiers, in the order added")
        ok(dir.list()!!.sorted() == listOf("cashiers.txt"), "CR-140: the file is written whole and no temporary file is left")
        val text = file.readText()
        ok(text.startsWith("openbmt-cashiers 1\n") && text.lines().size == 4 && text.endsWith("\n"), "CR-140: a version line, then one line for each cashier")
        ok(!text.contains("1234") && !text.contains("87654321"), "CR-141: no PIN is in the file")
        ok(text.lines()[1].endsWith(" Ann") && text.lines()[2].endsWith(" Bob"), "the name is the rest of the line")
        ok(text.lines()[1].split(' ').first() == "1000", "the iteration count is written with the entry")

        // Adding is refused for a name that is used, the manager's, a bad name, a bad PIN and a 31st cashier.
        for ((n, p) in listOf("ann" to "1234", "MAX" to "1234", "" to "1234", "Cy" to "123", "Cy" to "1234567890")) {
            throws("adding $n with $p") { store().add(n, p) }
        }
        ok(store().names() == listOf("Ann", "Bob"), "a refused add changes nothing")
        for (i in 3..30) store().add("c$i", "1111")
        ok(store().names().size == 30, "CR-140: 30 cashiers")
        throws("a 31st cashier") { store().add("one more", "1111") }
        for (i in 3..30) store().remove("c$i")
        ok(store().names() == listOf("Ann", "Bob"), "removing keeps the others in order")

        // CR-141: a count and a hold of each cashier's own.
        ok(store().check("Ann", "1234") == PinResult.Ok, "the right PIN signs in")
        val wrong = store().check("Ann", "0000")
        ok(wrong is PinResult.Wrong && wrong.waitSecs == 1L, "a wrong PIN holds that cashier for 1 second")
        ok(store().check("Ann", "1234") is PinResult.Locked, "CR-150: a held PIN is not looked at, even the right one")
        ok(store().check("Bob", "87654321") == PinResult.Ok, "CR-141: one cashier's wrong tries never hold another's PIN")
        ok(store().heldFor("Ann") == 1L && store().heldFor("Bob") == 0L, "the hold is Ann's alone, and it is in the file")
        now += 1
        val second = store().check("Ann", "0000")
        ok(second is PinResult.Wrong && second.waitSecs == 2L, "the next wrong try holds for 2 seconds")
        now += 2
        ok(store().check("Ann", "1234") == PinResult.Ok && store().heldFor("Ann") == 0L, "the right PIN clears the count")
        val before = hasher.calls
        store().check("Ann", "0000")
        ok(store().check("Ann", "0000") is PinResult.Locked && hasher.calls == before + 1, "a held try is not compared")
        now += 1
        // CR-142: a new PIN clears the count and the hold.
        store().setPin("Ann", "5555")
        ok(store().heldFor("Ann") == 0L && store().check("Ann", "1234") is PinResult.Wrong, "a new PIN replaces the old one")
        now += 1
        ok(store().check("Ann", "5555") == PinResult.Ok, "and works at once")
        throws("a new PIN of 3 digits") { store().setPin("Ann", "123") }
        throws("a check of a cashier who is not listed") { store().check("Zed", "1234") }
        throws("a removal of a cashier who is not listed") { store().remove("Zed") }
        throws("a new PIN for a cashier who is not listed") { store().setPin("Zed", "1234") }
        // The last one removed leaves an empty list, which is the lock turned off (CR-146).
        store().remove("Ann")
        store().remove("Bob")
        ok(store().names().isEmpty() && file.readText() == "openbmt-cashiers 1\n", "CR-146: removing the last leaves a file with the version line only")
        store().add("Ann", "1234")

        // CR-140: the file is read whole and refused whole when any line is not exact.
        val good = file.readText()
        store().add("Bob", "4321")
        val two = file.readText()
        val line = good.lines()[1]
        val fields = line.split(' ', limit = 6)
        fun refused(content: ByteArray, what: String) {
            file.writeBytes(content)
            var failed = false
            try {
                store().names()
            } catch (_: StaffFormatException) {
                failed = true
            }
            ok(failed, "CR-140: refused: $what")
        }
        fun refused(content: String, what: String) = refused(content.toByteArray(Charsets.UTF_8), what)
        file.writeText(two)
        ok(store().names() == listOf("Ann", "Bob"), "the good file reads")
        refused("", "an empty file")
        refused("openbmt-cashiers 1", "a version line with no line end")
        refused("openbmt-cashiers 2\n", "another version")
        refused("openbmt-cashiers 1\n$line", "a last line with no line end")
        refused(good + "\n", "an empty line")
        refused(good.replace("openbmt-cashiers 1", "openbmt-cashiers  1"), "a version line spaced another way")
        refused("openbmt-cashiers 1\n" + fields.take(5).joinToString(" ") + "\n", "five fields")
        refused("openbmt-cashiers 1\n" + fields.toMutableList().also { it[0] = "01000" }.joinToString(" ") + "\n", "a number with a leading zero")
        refused("openbmt-cashiers 1\n" + fields.toMutableList().also { it[0] = "0" }.joinToString(" ") + "\n", "an iteration count of zero")
        refused("openbmt-cashiers 1\n" + fields.toMutableList().also { it[0] = "+1000" }.joinToString(" ") + "\n", "a number with a sign")
        refused("openbmt-cashiers 1\n" + fields.toMutableList().also { it[1] = it[1].uppercase() }.joinToString(" ") + "\n", "a salt in capital letters")
        refused("openbmt-cashiers 1\n" + fields.toMutableList().also { it[1] = it[1].drop(2) }.joinToString(" ") + "\n", "a short salt")
        refused("openbmt-cashiers 1\n" + fields.toMutableList().also { it[2] = it[2] + "00" }.joinToString(" ") + "\n", "a long hash")
        refused("openbmt-cashiers 1\n" + fields.toMutableList().also { it[3] = "4294967296" }.joinToString(" ") + "\n", "a count past 32 bits")
        refused("openbmt-cashiers 1\n" + fields.toMutableList().also { it[4] = "-1" }.joinToString(" ") + "\n", "a negative time")
        refused("openbmt-cashiers 1\n" + fields.toMutableList().also { it[4] = "99999999999999999999" }.joinToString(" ") + "\n", "a time past 64 bits")
        refused("openbmt-cashiers 1\n" + fields.toMutableList().also { it[5] = "Ann " }.joinToString(" ") + "\n", "a name with a space at the end")
        refused("openbmt-cashiers 1\n" + fields.toMutableList().also { it[5] = "A\u0007" }.joinToString(" ") + "\n", "a name with a control character")
        refused("openbmt-cashiers 1\n" + fields.toMutableList().also { it[5] = "x".repeat(25) }.joinToString(" ") + "\n", "a name of 25 characters")
        refused(two + two.lines()[2] .replace("Bob", "BOB") + "\n", "two names that are the same ignoring case")
        refused(good.replace("Ann", "max"), "the manager's name")
        refused("openbmt-cashiers 1\n" + (1..31).joinToString("") { fields.toMutableList().also { f -> f[5] = "c$it" }.joinToString(" ") + "\n" }, "31 cashiers")
        refused(good.toByteArray(Charsets.UTF_8) + byteArrayOf(0xff.toByte(), 0x0a), "bytes that are not UTF-8")
        refused("openbmt-cashiers 1\n" + "x".repeat(20_000) + "\n", "a file far too long")
        // The first start of the app reads it the same way: a file that does not read is a fault, never an empty list.
        file.writeText(two)
        ok(store().names() == listOf("Ann", "Bob"), "the good file reads again")
        clean(dir)
    }

    // ---- the app does not back the list up (CR-140) -----------------------------------------------

    private fun cr140NoBackupTests(appDir: File?) {
        if (appDir == null) return
        val rules = File(appDir, "res/xml/data_extraction_rules.xml").readText()
        fun section(name: String): String {
            val start = rules.indexOf("<$name>")
            val end = rules.indexOf("</$name>")
            ok(start >= 0 && end > start, "the extraction rules have a $name section")
            return rules.substring(start, end)
        }
        for (name in listOf("cloud-backup", "device-transfer")) {
            val body = section(name)
            ok(!body.contains("<include"), "CR-140: $name includes nothing")
            // The list is a file under noBackupFilesDir, which is below the root domain; both domains are excluded.
            for (domain in listOf("root", "file")) ok(body.contains("<exclude domain=\"$domain\" />"), "CR-140: $name excludes the $domain domain")
        }
        val old = File(appDir, "res/xml/backup_rules.xml").readText()
        ok(!old.contains("<include") && old.contains("<exclude domain=\"root\" />") && old.contains("<exclude domain=\"file\" />"), "CR-140: the rules for older phones include nothing")
        val manifest = File(appDir, "AndroidManifest.xml").readText()
        ok(manifest.contains("android:allowBackup=\"false\"") && manifest.contains("android:dataExtractionRules=\"@xml/data_extraction_rules\""), "CR-140: backup is off and the rules are named")
        // The list is kept in the folder the system leaves out of a backup, as the PIN and the pairing are.
        val controller = File(appDir, "src/android/Controller.kt").readText()
        ok(controller.contains("private val dir: File = act.noBackupFilesDir") && controller.contains("StaffStore(dir,"), "CR-140: the app writes the list into noBackupFilesDir")
        ok(File(appDir, "src/model/TillLock.kt").readText().let { !it.contains("File") && !it.contains("DurableFile") }, "CR-149: the till lock never writes a file")
    }

    // ---- the till lock (CR-147 to CR-150) -----------------------------------------------------------

    private fun cr149TillLockTests() {
        var t = 100L
        val lock = TillLock { t }
        val ann = Actor(ActorRole.Cashier, "Ann")
        ok(lock.who == null && TillLock { 0 }.who == null, "CR-149: a stopped app starts with nobody signed in")
        lock.signIn(ann)
        ok(lock.who === ann, "a person signs in")
        lock.backInView()
        ok(lock.who === ann, "back in view with no time out changes nothing")
        lock.leftView()
        t += 300
        lock.backInView()
        ok(lock.who === ann, "CR-149: exactly 5 minutes out is not more than 5 minutes")
        lock.leftView()
        t += 301
        lock.backInView()
        ok(lock.who == null && !lock.waiting, "CR-149: more than 5 minutes out locks")
        lock.signIn(ann)
        lock.leftView()
        t += 200
        lock.leftView()
        t += 150
        lock.backInView()
        ok(lock.who == null, "a second word of leaving does not start the time again")
        lock.signIn(ann)
        lock.lock()
        ok(lock.who == null, "CR-149: Lock locks")
        // The lock waits for a sale's screen to close, then happens.
        lock.signIn(ann)
        lock.saleScreen(7)
        lock.leftView()
        t += 1000
        lock.backInView()
        ok(lock.who === ann && lock.waiting && lock.reopen() == 7L, "CR-149: it never locks while a sale's screen is open")
        lock.saleScreen(8)
        ok(lock.who === ann && lock.waiting && lock.reopen() == 8L, "another sale's screen is still a sale's screen")
        lock.saleScreen(null)
        ok(lock.who == null && !lock.waiting && lock.reopen() == null, "CR-149: when the sale's screen closes the lock happens")
        // A sale's screen open with no lock due keeps the person, who comes back to it.
        lock.signIn(ann)
        lock.saleScreen(3)
        lock.leftView()
        t += 10
        lock.backInView()
        ok(lock.who === ann && !lock.waiting && lock.reopen() == 3L, "a short time out leaves the person and the sale")
        lock.saleScreen(null)
        ok(lock.who === ann, "closing the sale's screen with no lock due changes nothing")
        // A sign-in ends a wait; forgetting ends everything (CR-146).
        lock.saleScreen(4)
        lock.leftView()
        t += 1000
        lock.backInView()
        lock.signIn(Actor(ActorRole.Manager, "Max"))
        ok(!lock.waiting && lock.who!!.name == "Max", "a new sign-in ends the wait")
        lock.forget()
        ok(lock.who == null && lock.reopen() == null && !lock.waiting, "CR-146: forgetting leaves nobody and nothing waiting")
        lock.saleScreen(9)
        ok(lock.reopen() == null, "a locked till reopens no sale")
        // The screen choice (CR-147).
        val all = SetupStatus(pairing = true, pin = true, config = true)
        ok(ScreenModel.pick(null, all, null, true, locked = true) is Screen.WhoAtTill, "CR-147: a locked till shows who is at the till")
        ok(ScreenModel.pick(null, all, null, true, locked = false) is Screen.Home, "an unlocked till shows home")
        ok(ScreenModel.pick(null, all, null, false, locked = true) is Screen.Starting, "a node that is not running is first")
        ok(ScreenModel.pick(AppWords.stopped, all, null, true, locked = true) is Screen.Fault, "a fault comes before the lock")
        ok(ScreenModel.pick(null, all, AppWords.orbotNotRunning, true, locked = true) is Screen.TorBlocked, "Tor without Orbot comes before the lock")
        ok(ScreenModel.pick(null, all, null, true) is Screen.Home, "no cashier, no lock: as before (CR-146)")
        ok(AppWords.signInWrong(4).next.contains("4 seconds") && AppWords.signInHeld(300).next.contains("5 minutes"), "CR-150: a held PIN shows the wait")
        ok(AppWords.whoAtTill.heading == "Who is at the till?", "CR-147: the first screen")
    }

    // ---- the screen kept across leaving the app (CR-177) ---------------------------------------------------------

    private fun cr177ShowingTests() {
        // Which sales are open: ended ones (expired, cancelled, Lightning, final) are not.
        val ended = listOf(SaleState.Expired, SaleState.Cancelled, SaleState.PaidLightning, SaleState.Final)
        ok(ended.none { Resume.open(it) }, "CR-177: an ended sale is not open")
        ok(SaleState.values().filter { it !in ended }.all { Resume.open(it) }, "CR-177: every other state is open")
        // A new process asks the library: one open sale opens on it, several on the list, none on home.
        ok(Resume.atStart(emptyList()) == Showing.Home, "CR-177: no open sale: home")
        ok(Resume.atStart(listOf(4L)) == Showing.Sale(4), "CR-177: one open sale: its screen")
        ok(Resume.atStart(listOf(4L, 7L)) == Showing.Sales, "CR-177: several open sales: the list")
        // The same run remembers its screen across pause and resume, and does not ask the library.
        var asked = 0
        val none = { asked++; emptyList<Long>() }
        ok(Resume.choose(Showing.Sale(2), none) == Showing.Sale(2) && asked == 0, "CR-177: a sale comes back as that sale")
        ok(Resume.choose(Showing.Sales, none) == Showing.Sales, "CR-177: the list comes back as the list")
        ok(Resume.choose(Showing.Home, none) == Showing.Home, "CR-177: home comes back as home")
        ok(Resume.choose(Showing.Other, none) == Showing.Home && asked == 0, "CR-177: another screen gives way to home")
        ok(Resume.choose(null, { listOf(9L) }) == Showing.Sale(9), "CR-177: a new process takes the open sale")
        ok(Resume.choose(null, none) == Showing.Home && asked == 1, "CR-177: a new process with no open sale: home")
        // A locked till still comes first (CR-147): the choice of the screen does not unlock it.
        val all = SetupStatus(pairing = true, pin = true, config = true)
        ok(ScreenModel.pick(null, all, null, true, locked = true) is Screen.WhoAtTill, "CR-177: a locked till asks who is at the till before the sale")
    }

    // ---- today (CR-152 to CR-155) -----------------------------------------------------------------

    private fun tally(count: Long, minor: Long = 0, sats: Long = 0) = Tally(count, minor, sats)

    private fun cr152DayWindowTests() {
        val zone = java.time.ZoneId.of("America/New_York")
        fun at(y: Int, mo: Int, d: Int, h: Int, mi: Int, s: Int) = java.time.ZonedDateTime.of(y, mo, d, h, mi, s, 0, zone).toEpochSecond()
        val midnight = at(2026, 11, 11, 0, 0, 0)
        ok(DayWindow.start(at(2026, 11, 11, 15, 30, 5), zone) == midnight, "CR-152: local midnight before now")
        ok(DayWindow.start(midnight, zone) == midnight, "midnight itself starts the day")
        ok(DayWindow.start(midnight - 1, zone) == at(2026, 11, 10, 0, 0, 0), "the second before midnight is the day before")
        ok(DayWindow.start(at(2026, 11, 11, 23, 59, 59), zone) == midnight, "the last second of the day")
        // A day with the clocks going back is 25 hours long, and the window still starts at its midnight.
        val change = at(2026, 11, 1, 0, 0, 0)
        ok(at(2026, 11, 2, 0, 0, 0) - change == 25 * 3600L, "a day with a change of clocks")
        ok(DayWindow.start(at(2026, 11, 1, 23, 0, 0), zone) == change && DayWindow.start(at(2026, 11, 2, 0, 0, 1), zone) == at(2026, 11, 2, 0, 0, 0), "CR-152: the window follows the phone's time zone across midnight")
        ok(DayWindow.start(1_700_000_000L, java.time.ZoneOffset.UTC) == 1_699_920_000L, "in UTC")
        ok(DayWindow.end(500) == 501L, "the end is not counted, so it is one second after now")
    }

    private fun cr153TodayLinesTests() {
        val usd = Money.of("USD")!!
        val t = DayTotals(
            1_700_000_000L, 1_700_050_000L, "USD", tally(3, 4500, 54_000), tally(2, 3000, 36_000), tally(1, 1500, 18_000),
            tally(2, 2000, 24_000), 1L, 4L, tally(1, 0, 12_000), tally(2, 0, 5_000), 1L, tally(1, 500, 6_000), TipTally(2, 300), 2L,
            listOf(PersonLine(ActorRole.Cashier, "Ann", 4, 1, 0, 1, 150), PersonLine(ActorRole.Manager, "Ma\u0007x‮", 1, 0, 2, 0)), 3L,
        )
        val lines = TodayModel.lines(t, usd)
        ok(lines[0] == "Sales paid: 3 sales, 45.00 USD, 54,000 sats", "CR-153: paid, with the amount shown at the time and the bitcoin: ${lines[0]}")
        ok(lines[1] == "On the chain: 2 sales, 30.00 USD, 36,000 sats" && lines[2] == "Over Lightning: 1 sale, 15.00 USD, 18,000 sats", "split on the chain and over Lightning: ${lines[1]} / ${lines[2]}")
        ok(lines[3] == "Sales still waiting: 2 sales, 20.00 USD, 24,000 sats asked for", "waiting: ${lines[3]}")
        ok(lines[4] == "Not to be handed over: 1 sale" && lines[5] == "Sales cancelled or expired: 4 sales", "stopped and ended: ${lines[4]} / ${lines[5]}")
        ok(lines[6] == "Overrides: 1, 12,000 sats at risk", "overrides: ${lines[6]}")
        ok(lines[7] == "Refunds asked for: 2, 5,000 sats (1 withdrawn)", "refunds: ${lines[7]}")
        ok(lines[8] == "Cash refunds: 1, 5.00 USD, 6,000 sats", "cash refunds: ${lines[8]}")
        ok(lines[9] == "Tips: 2 sales, 3.00 USD" && lines.size == 10, "CR-191: the tips of the paid sales, in the shop's currency: ${lines.drop(9)}")
        ok(lines.none { it.contains("balance") }, "CR-155: no wallet balance is shown")
        val quiet = DayTotals(0, 1, null, tally(0), tally(0), tally(0), tally(0), 0, 0, tally(0), tally(0), 0, tally(0), TipTally(0, 0), 0, emptyList(), 0)
        val none = TodayModel.lines(quiet, usd)
        ok(none.size == 8 && none.none { it.startsWith("Not to be handed over") } && none[0] == "Sales paid: 0 sales, 0.00 USD, 0 sats" && none[7] == "Cash refunds: 0, 0.00 USD, 0 sats", "a day with nothing in it: $none")
        ok(TodayModel.lines(quiet, null)[0] == "Sales paid: 0 sales, 0 sats", "with no currency known")
        val people = TodayModel.people(t)
        ok(TodayModel.people(t)[0] == "Ann (cashier): 4 sales, 1 override, 0 refunds asked for, 1 cash refund", "CR-154: a person's line: ${people[0]}")
        val tipped = TodayModel.people(t, usd)
        ok(tipped[0] == "Ann (cashier): 4 sales, 1 override, 0 refunds asked for, 1 cash refund\nTips taken by Ann: 1.50 USD" && tipped[1] == people[1], "CR-191: the tips a person took, as the library says: ${tipped[0]}")
        ok(TodayModel.personParts(t.people[0], usd).tips == "Tips taken by Ann: 1.50 USD" && TodayModel.personParts(t.people[1], usd).tips == null, "a person who took no tip has no tips line")
        ok(people[1] == "Ma x (manager): 1 sale, 0 overrides, 2 refunds asked for, 0 cash refunds", "a name from a record is cleaned: ${people[1]}")
        ok(people[2] == "Sales with no name in the record: 2" && people[3] == "People left out of this list: 3", "unnamed sales and people left out are counted apart: ${people.drop(2)}")
        ok(TodayModel.people(quiet).isEmpty(), "no one, no lines")
        // The lines cut at their colon and commas for the screen keep every word.
        val allLines = lines + none + TodayModel.lines(quiet, null)
        for (l in allLines) {
            val sp = TodayModel.split(l)
            val parts = sp.parts
            ok(sp.label + ": " + parts.joinToString(", ") == l, "the cut line joins back to the line: $l")
            val shown = sp.label + " " + sp.amount + " " + sp.detail.replace(" \u00B7 ", " ")
            ok(parts.all { p -> shown.contains(p) } || parts.size == 1, "no part is dropped from the hero's three pieces: $l")
        }
        val paid = TodayModel.split(lines[0])
        ok(paid.label == "Sales paid" && paid.amount == "45.00 USD" && paid.detail == "3 sales \u00B7 54,000 sats" && paid.count == "3 sales", "the hero's three pieces: ${paid.label} / ${paid.amount} / ${paid.detail}")
        val waiting = TodayModel.split(lines[3])
        ok(waiting.count == "2 sales" && waiting.amount == "20.00 USD" && waiting.parts.last() == "24,000 sats asked for", "the waiting row keeps its sats words: ${waiting.parts}")
        ok(TodayModel.split(lines[7]).rest == "5,000 sats (1 withdrawn)" && TodayModel.split(lines[5]).value == "4 sales", "refunds and the quiet lines")
        ok(TodayModel.split(TodayModel.lines(quiet, null)[0]).let { it.amount == "0 sats" && it.detail == "0 sales" }, "a line with no money has the bitcoin as its amount")
        ok(TodayModel.split("no colon").let { it.label == "no colon" && it.parts.isEmpty() }, "a line without a colon")
        val person = TodayModel.personParts(t.people[1])
        ok(person.name == "Ma x" && person.role == "manager" && "${person.name} (${person.role}): ${person.line}" == people[1], "a person's line cut in three says what the line says")
        for (w in listOf(AppWords.todayAbout, AppWords.todayWaiting, AppWords.todayStopped, AppWords.todayPeopleNeedPin)) ok(SayCheck.problem(w) == null, "today's words pass the checks: ${SayCheck.problem(w)}")
        ok(AppWords.todayAbout.what.contains("No wallet balance") && AppWords.todayAbout.next.contains("full books") && AppWords.todayAbout.next.contains("export"), "CR-155: the owner's phone has the full books and the export")
        ok(AppWords.todayWaiting.what.contains("accepted") && AppWords.todayWaiting.next.contains("sale ends"), "CR-155: why a sale waits and what happens next")
    }

    // ---- the PIN steps run only while the app is shown (CR-200) -----------------------------------------

    private fun cr200PinStepsTests(appDir: File?) {
        ok(PinSteps.applies(true) && !PinSteps.applies(false), "CR-200: an answer is acted on only while the app is shown")
        if (appDir == null) return
        val controller = File(appDir, "src/android/Controller.kt").readText()
        ok(controller.contains("private var visible = false"), "the app keeps a flag for being shown")
        ok(controller.substringAfter("fun visible() {").substringBefore("}").contains("visible = true") && controller.substringAfter("fun hidden() {").substringBefore("}").contains("visible = false"), "the activity's show sets it and its hide clears it")
        val check = controller.substringAfter("private fun checkPin(").substringBefore("private fun showOverride(")
        ok(check.indexOf("if (!PinSteps.applies(visible)) return@async") in 0 until check.indexOf("PinResult.Ok -> ok()"), "CR-200: the answer of a check is dropped before ok() can run, hidden")
        ok(check.indexOf("if (!PinSteps.applies(visible)) return@async") < check.indexOf("failed("), "and before a wrong try or a hold is shown")
        val signIn = controller.substringAfter("private fun signIn(").substringBefore("// ---- staff")
        ok(signIn.indexOf("if (!PinSteps.applies(visible)) return@async") in 0 until signIn.indexOf("locks.signIn(person)"), "CR-200: a sign-in that finishes hidden signs nobody in")
        val rules = File(appDir, "../../docs/COUNTER-RULES.md").readText()
        ok(rules.contains("- **CR-200.**") && rules.contains("The answer is acted on only while the app is shown (CR-200)."), "CR-200 is in the rules and amends CR-85")
    }

    // ---- the shop's currency and the tips (CR-188, CR-191, CR-192, CR-196, CR-197) ------------------------

    private fun cr188TipTests(appDir: File?) {
        val m = Money.of("USD")!!
        val choose = WordsOfTips.choose(m.show(1250))
        ok(choose.heading == "Add a tip?" && choose.what == "The sale is 12.50 USD. A tip goes to the shop with it." && choose.next == "Choose a tip, or No tip.", "CR-188: the words of the tip step")
        ok(choose.who == Who.CUSTOMER && choose.code == "CR-188" && SayCheck.problem(choose) == null, "they are for the customer and pass the checks: ${SayCheck.problem(choose)}")
        ok(WordsOfTips.button(15, m.show(188)) == "15% \u00B7 1.88 USD" && WordsOfTips.OTHER == "Other amount" && WordsOfTips.NONE == "No tip", "CR-188: the buttons")
        ok(WordsOfTips.LINE == "Tips" && WordsOfTips.personLabel("Ann") == "Tips taken by Ann", "CR-191: the lines of Today")
        ok(WordsOfTips.refundLine(m.show(50)) == "Of which tip: 0.50 USD", "CR-192: the line of a refund")
        ok(AppWords.all().any { it.first == "tipChoose" }, "the tip step is in the list of words")
        // In a currency with no minor digits and in Bitcoin Standard.
        ok(WordsOfTips.button(10, Money.of("JPY")!!.show(150)) == "10% \u00B7 150 JPY" && WordsOfTips.button(20, Money.of("SAT")!!.show(1_500)) == "20% \u00B7 1,500 sats", "tips in yen and in sats")
        // Today with Bitcoin Standard: the sats are said once, and the tips are sats too.
        val sats = Money.of("SAT")!!
        val t = DayTotals(
            1, 2, "SAT", tally(2, 9_000, 9_000), tally(2, 9_000, 9_000), tally(0), tally(0), 0L, 0L, tally(0), tally(0), 0L, tally(1, 500, 500), TipTally(1, 900), 0L,
            listOf(PersonLine(ActorRole.Cashier, "Ann", 2, 0, 0, 1, 900)), 0L,
        )
        val lines = TodayModel.lines(t, sats)
        ok(lines[0] == "Sales paid: 2 sales, 9,000 sats" && lines[7] == "Cash refunds: 1, 500 sats" && lines[8] == "Tips: 1 sale, 900 sats", "Bitcoin Standard: no amount twice: ${lines[0]} / ${lines[7]} / ${lines.getOrNull(8)}")
        ok(TodayModel.personParts(t.people[0], sats).tips == "Tips taken by Ann: 900 sats", "and the person's tips in sats")
        ok(TodayModel.lines(t.let { DayTotals(1, 2, "JPY", it.paid, it.onChain, it.lightning, it.waiting, 0, 0, it.overrides, it.refunds, 0, it.cashRefunds, it.tips, 0, emptyList(), 0) }, null)[0] == "Sales paid: 2 sales, 9,000 JPY, 9,000 sats", "a currency with no minor digits shows both")
        if (appDir == null) return
        // The screens: a sale goes through the tip step when the library offers tips, and the tip is part of the sale's total.
        val controller = File(appDir, "src/android/Controller.kt").readText()
        ok(controller.contains("else chooseTip(entry.minor)"), "CR-188: the keypad goes to the tip step, not straight to the sale")
        val chooseTip = controller.substringAfter("private fun chooseTip(").substringBefore("private fun showTip(")
        ok(chooseTip.contains("n.tipChoices(amountMinor)") && chooseTip.contains("if (choices.isEmpty()) return openSale(amountMinor, 0, refused)"), "CR-187, CR-188: with tips off the sale opens at once, as it did")
        val showTip = controller.substringAfter("private fun showTip(").substringBefore("private fun showTipKeypad(")
        ok(showTip.contains("WordsOfTips.button(c.percent, m.show(c.tipMinor))") && showTip.contains("WordsOfTips.OTHER") && showTip.contains("WordsOfTips.NONE"), "CR-188: a button for each percent, Other amount and No tip")
        ok(showTip.indexOf("WordsOfTips.NONE") > showTip.indexOf("WordsOfTips.OTHER") && showTip.contains("ui.secondary(WordsOfTips.NONE) { openSale(amountMinor, 0, failed) }"), "No tip opens the sale with no tip")
        ok(!showTip.contains("back =") && !showTip.contains("gear ="), "the customer's screen has nothing else on it: no way back or settings")
        val keypad = controller.substringAfter("private fun showTipKeypad(").substringBefore("// ---- the sale")
        ok(keypad.contains("openSale(amountMinor, entry.minor) { msg.show(note(it)) }"), "CR-190: a refusal of the tip shows on the keypad, which stays")
        val openSale = controller.substringAfter("private fun openSale(").substringBefore("// ---- the sale")
        ok(openSale.contains("n.openWithTip(") && openSale.contains("amounts[o.index] = amountMinor + tipMinor"), "CR-189: the code is for the sale and the tip together")
        val adopt = controller.substringAfter("private fun adopt(").substringBefore("val runner")
        ok(adopt.contains("Money.of(settings.currency)") && adopt.contains("ShopPrices.sources(m, allSources)") && adopt.contains("if (m.isSats) null else b"), "CR-193, RT-40, CR-197: the shop's currency from the settings, its sources only, none for Bitcoin Standard")
        ok(controller.contains("if (money?.isSats == true) return org.openbmt.counter.Rate(0, \"\", 0) to null"), "CR-197: Bitcoin Standard asks no price")
        ok(controller.contains("r.tipMinor > 0") && controller.contains("WordsOfTips.refundLine(m.show(r.tipMinor))"), "CR-192: a refund's details say how much is the tip")
        ok(controller.contains("if (restartNeeded(n) || currencyChanged())"), "CR-195: a change of currency starts the node again")
        val tipWords = File(appDir, "src/model/TipWords.kt").readText()
        ok(tipWords.contains("Add a tip?") && tipWords.contains("Choose a tip, or No tip.") && !tipWords.contains("sats offered"), "the words file holds the library's words")
    }


    // ---- the built-in Tor (CR-201 to CR-204) --------------------------------------------------------------

    private fun torReply(text: String) = ControlReader(java.io.ByteArrayInputStream(text.toByteArray(Charsets.ISO_8859_1))).next()!!

    private fun cr201BuiltInTorTests(appDir: File?) {
        // The node, the price fetches and the Lightning transport are given the port Tor reported, where Orbot's address was.
        var asked: Pair<String, Int>? = null
        val gate = TorGate(true, "127.0.0.1:41233", SocksProbe { h, p -> asked = h to p; true })
        ok(gate.proxy == "127.0.0.1:41233" && gate.address == "127.0.0.1:41233" && gate.available() && asked == "127.0.0.1" to 41233, "CR-201: with Tor on the proxy is the built-in Tor's port, and it answers")
        ok(TorGate(false, "127.0.0.1:9050").proxy == null, "CR-201: with Tor off there is no proxy and nothing is started")
        // The strict reader is the owner app's: the port of the one SOCKS listener, and a reply that is not exact is refused.
        ok(TorControl.socksPort(torReply("250-net/listeners/socks=\"127.0.0.1:41233\"\r\n250 OK\r\n")) == 41233, "CR-201: the port is read from the control socket")
        var refused = false
        try {
            torReply("250 OK\n")
        } catch (_: java.io.IOException) {
            refused = true
        }
        ok(refused, "CR-201: a line end that is not a carriage return and a line feed is refused")
        if (appDir == null) return
        val controller = File(appDir, "src/android/Controller.kt").readText()
        val start = controller.substringAfter("private fun startNode()")
        ok(start.contains("val builtIn = torOn && cfg.tor == TorChoice.BuiltIn"), "CR-201: the built-in Tor is wanted only with Tor on in the settings and the choice of it")
        ok(start.indexOf("ensureTor().awaitPort(TOR_WAIT_MS)") in 1 until start.indexOf("CounterNode.start("), "CR-201: the node starts only after the built-in Tor has a port")
        ok(start.contains("} else {\n                    releaseTor()\n                    address = cfg.proxy"), "CR-201: with Tor off or Orbot chosen no Tor of the app runs")
        ok(start.contains("TorGate(torOn, address)") && start.contains("gate.proxy") && controller.contains("HttpsFetcher(gate)") && controller.contains("LightningTransport(gate.torOn, gate.address)"), "CR-201: the node, the price fetches and the Lightning transport use that address")
        val hidden = controller.substringAfter("fun hidden() {").substringBefore("/** CR-91")
        ok(hidden.indexOf("stopNode()") in 0 until hidden.indexOf("releaseTor()"), "CR-201: Tor is let go of when the app is hidden, after the node")
        ok(controller.contains("if (!visible) throw TorStartException(\"the app is hidden\")"), "CR-201: a start that comes after the app was hidden does not take Tor")
        ok(controller.contains("TorRunner.acquire(dir)") && controller.contains("torLease?.takeIf { !it.dead }"), "CR-201: one lease, taken again when its run has ended")
        // The code is the shared Tor part, which the owner and counter builds take and the Key app's does not.
        val common = File(appDir, "../android-common/tor")
        ok(File(common, "src/android/TorRunner.kt").isFile && File(common, "src/android/TorService.kt").isFile && File(common, "src/model/TorControl.kt").isFile, "CR-201: the runner, the service and the control client are in the shared Tor part")
        ok(!File(appDir, "src/android/TorRunner.kt").exists() && !File(appDir, "src/android/TorService.kt").exists() && !File(appDir, "src/model/TorControl.kt").exists(), "CR-201: the counter has no copy of them")
        ok(File(appDir, "build.sh").readText().contains("\"\$tor_part\"/src/model/*.kt \"\$tor_part\"/src/android/*.kt") && File(appDir, "../owner-android/build.sh").readText().contains("\"\$tor_part\"/src/model/*.kt \"\$tor_part\"/src/android/*.kt"), "CR-201: the owner build and the counter build compile the Tor part")
        val keyBuild = File(appDir, "../key-android/build.sh").readText()
        ok(!keyBuild.contains("tor_part") && !keyBuild.contains("android-common/tor") && !keyBuild.contains("libtor") && !keyBuild.contains("tor-library"), "CR-201: the Key app's build takes none of the Tor part and no libtor.so")
        val manifest = File(appDir, "AndroidManifest.xml").readText()
        ok(!manifest.contains("TorService") && manifest.contains("android:extractNativeLibs=\"false\""), "CR-201: Tor is not an Android component, and the library is mapped from the APK")
        val rules = File(appDir, "../../docs/COUNTER-RULES.md").readText()
        ok(rules.contains("- **CR-201.**") && rules.substringAfter("- **CR-87.**").substringBefore("- **CR-88.**").let { it.contains("CR-201") && it.contains("CR-203") && it.contains("CR-204") }, "CR-201 is in the rules and CR-87 points to it")
    }

    private fun cr202TorFilesTests(appDir: File?) {
        if (appDir == null) return
        val common = File(appDir, "../android-common/tor")
        val runner = File(common, "src/android/TorRunner.kt").readText()
        val svc = File(common, "src/android/TorService.kt").readText()
        ok(runner.contains("const val TOR_DIR = \"tor\"") && runner.contains("File(dir, TorRunner.TOR_DIR)") && runner.contains("File(torDir, \"data\")") && runner.contains("File(torDir, \"cache\")"), "CR-202: the folders are in a folder tor of the app's folder")
        val controller = File(appDir, "src/android/Controller.kt").readText()
        ok(controller.contains("private val dir: File = act.noBackupFilesDir") && controller.contains("TorRunner.acquire(dir)"), "CR-202: and that folder is noBackupFilesDir")
        ok(runner.contains("setReadable(true, true)") && runner.contains("setWritable(true, true)") && runner.contains("setExecutable(true, true)"), "CR-202: the folders are the app's alone")
        val options = Regex("\"(--[A-Za-z-]+)\"").findAll(svc).map { it.groupValues[1] }.toList()
        ok(options == listOf("--verify-config", "--RunAsDaemon", "--defaults-torrc", "--ignore-missing-torrc", "--CacheDirectory", "--DataDirectory", "--ControlSocket", "--CookieAuthentication", "--SocksPort"), "CR-202: the options of the command line: $options")
        ok(svc.contains("\"--SocksPort\", \"127.0.0.1:auto\"") && svc.contains("socket.delete()") && !svc.contains("writeText") && !svc.contains("FileWriter"), "CR-202: one SOCKS listener on 127.0.0.1, no configuration file written, an old control socket removed")
        val manifest = File(appDir, "AndroidManifest.xml").readText()
        ok(manifest.contains("android:allowBackup=\"false\""), "CR-202: nothing of it is backed up")
        val script = File(common, "tor-library.sh").readText()
        val build = File(appDir, "build.sh").readText()
        ok(script.contains("aar_sha256=\"28ff2bb6d4d42eabe57183dedd7a97fce77e8ce4b6f5af6fa03993b3ca054edd\"") && script.contains("is not the pinned tor-android 0.4.9.14 AAR"), "CR-202: the shared script checks the library's hash and stops on a mismatch")
        ok(build.contains("tor-library.sh\" check") && build.contains("tor-library.sh\" unpack") && !build.contains("unzip -q"), "CR-202: the counter build takes libtor.so only through that script")
        ok(script.contains("jni/\$abi/libtor.so") && Regex("unzip [^\\n]*\"\\\$tor_aar\"").findAll(script).count() == 1, "CR-202: only libtor.so is taken from the AAR, by one unzip")
        ok(File(appDir, "../../docs/COUNTER-RULES.md").readText().contains("- **CR-202.**"), "CR-202 is in the rules")
    }

    private fun cr203TorChoiceTests(appDir: File?) {
        fun config(text: String) = AppConfig.decode(text.toByteArray(Charsets.UTF_8))
        ok(AppConfig(AppConfig.DEFAULT_PROXY, "Mia", "").tor == TorChoice.BuiltIn, "CR-203: a new counter uses the built-in Tor")
        val old3 = config("proxy: 10.0.0.2:9150\nmanager: M\ncashier: \n")
        val old4 = config("proxy: 10.0.0.2:9150\nmanager: M\ncashier: A\nownnode: x.onion:1\n")
        val old5 = config("proxy: 10.0.0.2:9150\nmanager: M\ncashier: A\nownnode: \nsounds: off\n")
        ok(listOf(old3, old4, old5).all { it.tor == TorChoice.Orbot && it.proxy == "10.0.0.2:9150" }, "CR-203: a counter from before the choice keeps Use Orbot, at its address")
        ok(!old5.sounds && old4.ownNode == "x.onion:1", "and keeps the rest of what its file said")
        for (choice in TorChoice.values()) for (sounds in listOf(true, false)) {
            val c = AppConfig("10.0.0.2:9150", "Mia", "Ann", "n.onion:1", sounds, choice)
            val back = config(String(c.encode(), Charsets.UTF_8))
            ok(back.tor == choice && back.sounds == sounds && back.proxy == "10.0.0.2:9150" && back.ownNode == "n.onion:1", "CR-203: the choice ${choice.key} reads back, sounds $sounds")
        }
        ok(String(AppConfig("h:1", "M", "").with(TorChoice.Orbot).encode(), Charsets.UTF_8) == "proxy: h:1\nmanager: M\ncashier: \nownnode: \ntor: orbot\n", "CR-203: the line is the last one")
        ok(AppConfig("h:1", "M", "C").with(TorChoice.Orbot).let { it.proxy == "h:1" && it.cashier == "C" } && AppConfig("h:1", "M", "C", tor = TorChoice.Orbot).withCashier("D").withProxy("h:2").withOwnNode("a.onion:1").withSounds(false).tor == TorChoice.Orbot, "CR-203: changing the choice keeps the address, and every other change keeps the choice")
        for (bad in listOf("tor: off", "tor: ", "tor: Orbot", "sounds: off\ntor: orbot\nsounds: off", "tor: orbot\nsounds: off", "tor: orbot\ntor: orbot")) {
            var refused = false
            try {
                config("proxy: h:1\nmanager: M\ncashier: \nownnode: \n$bad\n")
            } catch (_: ConfigFormatException) {
                refused = true
            }
            ok(refused, "CR-203: refused: ${bad.replace("\n", "|")}")
        }
        ok(TorChoice.values().map { it.label } == listOf("Built-in Tor", "Use Orbot") && TorChoice.fromKey("off") == null, "CR-203: two choices with their labels, and none that turns Tor off")
        ok(TorChoice.BuiltIn.words === AppWords.torBuiltIn && TorChoice.Orbot.words === AppWords.torUsesOrbot, "CR-203: each choice has its words")
        ok(AppWords.torBuiltIn.heading == "Tor" && AppWords.torBuiltIn.what == "This app runs Tor itself, so Orbot is not needed." && AppWords.torBuiltIn.next == "Nothing to do. To use Orbot instead, choose Use Orbot." && AppWords.torBuiltIn.who == Who.NOBODY, "CR-203: the words of the built-in Tor, as written down")
        ok(AppWords.torUsesOrbot.heading == "Tor through Orbot" && AppWords.torUsesOrbot.what == "This app sends everything through Orbot, which must be installed and running." && AppWords.torUsesOrbot.next == "Keep Orbot running. To stop needing it, choose Built-in Tor." && AppWords.torUsesOrbot.who == Who.OWNER, "CR-203: the words of Orbot, as written down")
        for (w in listOf(AppWords.torBuiltIn, AppWords.torUsesOrbot, AppWords.torStarting, AppWords.torFailed)) ok(SayCheck.problem(w) == null, "the words of Tor pass the checks: ${SayCheck.problem(w)}")
        if (appDir == null) return
        val controller = File(appDir, "src/android/Controller.kt").readText()
        ok(controller.contains("if (torOnNow) items.add(RowItem(\"Tor\", onClick = { showTor() }))") && controller.contains("torOnNow = gate.torOn"), "CR-203: Settings has the row Tor only while the shop's settings have Tor on")
        val page = controller.substringAfter("private fun showTor()").substringBefore("/** The choice made")
        ok(page.contains("TorChoice.values()") && page.contains("cfg.tor.words") && page.contains("checkPin(pinInput.text.toString()") && page.contains("showNetwork(choosing = true)"), "CR-203: a button for each choice, the words of the one in use, a PIN for Built-in Tor, the address for Use Orbot")
        val choose = controller.substringAfter("private fun chooseTor(").substringBefore("// ----")
        ok(choose.indexOf("configFile.save(cfg.with(choice))") in 1 until choose.indexOf("begin()"), "CR-203: the choice is saved before the app starts again with it")
        ok(controller.contains("next.with(TorChoice.Orbot)") && controller.contains("checkPin(pinInput.text.toString(), pinInput, { keepNetwork(cfg, text, choosing, msg) }"), "CR-203: Use Orbot is saved behind the manager's PIN with the address")
        ok(controller.contains("configFile.save(AppConfig(AppConfig.DEFAULT_PROXY, managerName, \"\"))"), "CR-203: a new counter's config takes the default, the built-in Tor")
        val rules = File(appDir, "../../docs/COUNTER-RULES.md").readText()
        ok(rules.contains("- **CR-203.**"), "CR-203 is in the rules")
    }

    private fun cr204TorFailsTests(appDir: File?) {
        ok(AppWords.torStarting.heading == "Starting Tor" && AppWords.torStarting.what == "This app is connecting to the Tor network, which can take up to a minute." && AppWords.torStarting.next == "Wait on this screen." && AppWords.torStarting.who == Who.NOBODY, "CR-204: the words while it connects, as written down")
        ok(AppWords.torFailed.heading == "Tor did not start" && AppWords.torFailed.what == "This app could not connect to the Tor network." && AppWords.torFailed.next == "Check that the phone is online, then try again. If it keeps failing, choose Use Orbot." && AppWords.torFailed.who == Who.OWNER && AppWords.torFailed.tone == Tones.STOP, "CR-204: the words when it did not, as written down")
        ok(ScreenModel.pick(null, SetupStatus(true, true, true), AppWords.torFailed, false).let { it is Screen.TorBlocked && it.words === AppWords.torFailed }, "CR-204: a failed Tor is a screen of its own, before the node's")
        ok(AppWords.all().map { it.first }.containsAll(listOf("torBuiltIn", "torUsesOrbot", "torStarting", "torFailed")), "the four words are in the list")
        if (appDir == null) return
        val controller = File(appDir, "src/android/Controller.kt").readText()
        val start = controller.substringAfter("private fun startNode()")
        ok(start.contains("showWorking(AppWords.torStarting)"), "CR-204: the screen says it is connecting while it waits")
        ok(start.contains("catch (_: TorStartException)") && start.contains("torBlocked = AppWords.torFailed") && start.indexOf("torBlocked = AppWords.torFailed") in 1 until start.indexOf("CounterNode.start("), "CR-204: a failure shows its words and comes before the node is started")
        ok(start.contains("torBlocked = if (builtIn) AppWords.torFailed else AppWords.orbotNotRunning"), "CR-204: a Tor that does not answer is not called Orbot")
        ok(controller.contains("ui.primary(\"Try again\") { begin() }, ui.button(\"Tor\") { showTor() }") && controller.contains("const val TOR_WAIT_MS = 90_000L"), "CR-204: Try again starts again, Tor opens the choice, and it waits 90 seconds for a circuit")
        val runner = File(appDir, "../android-common/tor/src/android/TorRunner.kt").readText()
        ok(runner.contains("catch (e: LinkageError)") && runner.contains("class TorStartException") && runner.contains("val run = current?.takeIf { !it.done }"), "CR-204: a library that does not load fails to start, and a run that has ended is not reused")
        ok(controller.contains("PriceBook(LibraryRates, { nowSecs() }, (config?.tor ?: TorChoice.Orbot).missingWords)") && controller.contains("PriceStatus.TorMissing -> b.torMissingWords") && !controller.contains("PriceStatus.TorMissing -> AppWords.orbotNotRunning"), "CR-204: the price words follow the choice")
        ok(!controller.contains("goes through Orbot on this phone") && !controller.contains("so Orbot must be on"), "the About text no longer says Orbot where either choice applies")
        ok(File(appDir, "../../docs/COUNTER-RULES.md").readText().contains("- **CR-204.**"), "CR-204 is in the rules")
    }

    /** CR-219: the small line under the code when a provider is set and this sale has no swap. */
    private fun cr214ShareTests() {
        val root = Files.createTempDirectory("share").toFile()
        val folder = File(root, ShareNames.FOLDER)
        val good = "0123456789abcdef0123456789abcdef"
        // The provider's name check: only 32 lower-case hex digits, and only a file that exists.
        folder.mkdirs()
        File(folder, good).writeText("png")
        File(root, good).writeText("outside")
        File(folder, "request.png").writeText("old")
        ok(ShareNames.valid(good), "CR-214: a name of 16 hex bytes is valid")
        ok(ShareNames.resolve(folder, good)?.readText() == "png", "CR-214: a good name that exists is served")
        ok(ShareNames.resolve(folder, "0123456789abcdef0123456789abcde") == null, "CR-214: a short name is refused")
        ok(ShareNames.resolve(folder, good + "0") == null, "CR-214: a long name is refused")
        ok(ShareNames.resolve(folder, "0123456789abcdef0123456789abcdeg") == null, "CR-214: a non-hex name is refused")
        ok(ShareNames.resolve(folder, "0123456789ABCDEF0123456789ABCDEF") == null, "CR-214: upper case is refused")
        ok(ShareNames.resolve(folder, "request.png") == null, "CR-214: the old fixed name is refused")
        ok(ShareNames.resolve(folder, "..") == null && ShareNames.resolve(folder, "../$good") == null, "CR-214: dot-dot is refused")
        ok(ShareNames.resolve(folder, "..%2f" + good.substring(0, 26)) == null, "CR-214: an escaped path is refused")
        ok(ShareNames.resolve(File(root, "other"), good) == null, "CR-214: another folder serves nothing")
        ok(ShareNames.resolve(folder, "fedcba9876543210fedcba9876543210") == null, "CR-214: a good name with no file is refused")

        // The controller's bookkeeping.
        val revoked = mutableListOf<String>()
        val rnd = java.security.SecureRandom()
        val ledger = ShareLedger(folder, { revoked.add(it) }, { ByteArray(ShareNames.BYTES).also { rnd.nextBytes(it) } })
        ledger.end()
        ok((folder.listFiles()?.size ?: 0) == 0, "CR-214: starting empties the folder")
        val a = ledger.begin { it.writeText("A") }
        ok(ShareNames.valid(a) && File(folder, a).readText() == "A", "CR-214: a share makes a file under a fresh valid name")
        ok(ledger.handedOut() == listOf(a) && revoked.isEmpty(), "CR-214: a share adds one address and revokes nothing")
        val b = ledger.begin { it.writeText("B") }
        ok(a != b, "CR-214: each share has its own name")
        ok(revoked == listOf(a), "CR-214: a new share revokes the earlier address")
        ok(!File(folder, a).exists() && File(folder, b).readText() == "B", "CR-214: a new share deletes the earlier image first")
        ok(ledger.handedOut() == listOf(b), "CR-214: only the new address is held")
        File(folder, "stray").writeText("x")
        ledger.end()
        ok(revoked == listOf(a, b) && ledger.handedOut().isEmpty(), "CR-214: leaving revokes every address")
        ok((folder.listFiles()?.size ?: 0) == 0, "CR-214: leaving empties the folder")
        throws("CR-214: a failed write leaves nothing behind") { ledger.begin { throw IOException("full") } }
        ok((folder.listFiles()?.size ?: 0) == 0 && ledger.handedOut().isEmpty(), "CR-214: a failed share adds no address")
        root.deleteRecursively()
    }

    private fun cr219SwapTests() {
        ok(AppWords.lnNotForThisSale == "Lightning is not available for this sale.", "CR-219: the small line, as written down")
        ok(AppWords.all().none { it.second.what == AppWords.lnNotForThisSale }, "CR-219: the small line has no heading and is not in the list of words")
        ok(SwapLimits.MAX_BYTES == 64 * 1024 && SwapLimits.CONNECT_SECS == 60 && SwapLimits.READ_SECS == 60 && SwapLimits.PORT == 80, "CR-225: the limits of the swap transport")
    }

    /** CR-213 to CR-216: the request to send and the note, in the app. */
    private fun cr213RequestsTests(appDir: File?) {
        val made = AppWords.requestMade("2026-11-14 10:00", "73,554 sats")
        ok(made.heading == "Request ready" && made.what == "The customer can pay this request from anywhere until 2026-11-14 10:00. The amount in bitcoin is fixed at 73,554 sats." && made.next == "Share the link or the code with the customer. The counter shows Paid once the payment is confirmed." && made.who == Who.CASHIER, "CR-214: the words of a request just made, as written down")
        val open = AppWords.requestOpen("2026-11-14 10:00")
        ok(open.heading == "Waiting for payment" && open.what == "This request is open until 2026-11-14 10:00. It is paid once the payment is confirmed." && open.next == "Nothing to do." && open.who == Who.NOBODY, "CR-214: the words of a request that is open again, as written down")
        ok(AppWords.all().map { it.first }.containsAll(listOf("requestMade", "requestOpen")), "the two words are in the list")
        ok(
            WordsOfRequests.REQUEST == "Request to send" && WordsOfRequests.MAKE == "Make the request" && WordsOfRequests.SHARE_LINK == "Share the link" &&
                WordsOfRequests.SHARE_CODE == "Share the code" && WordsOfRequests.OPEN_REQUESTS == "Open requests" && WordsOfRequests.NOTE == "Note" &&
                WordsOfRequests.ADD_NOTE == "Add a note" && WordsOfRequests.NOTE_HINT == "Optional. Like a booking name or date." &&
                WordsOfRequests.LIFETIMES == listOf("1 day" to 1, "3 days" to 3, "7 days" to 7),
            "CR-213, CR-215, CR-216: the labels, as written down",
        )
        ok(SaleModel.buttons(SaleState.NotPaidYet, true).let { !it.requote && it.cancel }, "CR-207: a request is cancelled but takes no new quote")
        ok(SaleState.values().none { SaleModel.buttons(it, true).requote }, "CR-207: no state of a request offers Requote")
        ok(SaleModel.buttons(SaleState.PartPaid, false).requote, "CR-207: a sale is as before")
        if (appDir == null) return
        val controller = File(appDir, "src/android/Controller.kt").readText()
        ok(controller.contains("ui.secondary(WordsOfRequests.REQUEST)") && controller.contains("listOf(open, request)"), "CR-213: the quieter button is under the main one")
        ok(controller.contains("n.openRequest(") && controller.contains("showSale(o.index, o.payment, null, made = true)"), "CR-213: the request is made and its page opens")
        ok(controller.contains("Intent.ACTION_SEND") && controller.contains("setType(\"text/plain\")") && controller.contains("setType(\"image/png\")"), "CR-214: the link and the code go to the share sheet")
        ok(controller.contains("private fun dropSharedImage() = shared.end()") && controller.contains("dropSharedImage()\n        tick = null"), "CR-214: the images are ended when the page is left")
        ok(controller.contains("shared.begin {") && controller.contains("revokeUriPermission("), "CR-214: a share has its own address and the earlier ones are revoked")
        ok(controller.contains("n.requests(nowSecs())") && controller.contains("WordsOfRequests.OPEN_REQUESTS"), "CR-215: Home lists the open requests")
        ok(controller.contains("WordsOfRequests.ADD_NOTE") && controller.contains("n.addNote("), "CR-216: a sale's details add a note")
        val manifest = File(appDir, "AndroidManifest.xml").readText()
        ok(manifest.contains("android:exported=\"false\"") && manifest.contains("ShareImageProvider") && manifest.contains("grantUriPermissions=\"true\""), "CR-214: one provider, not exported")
        val rules = File(appDir, "../../docs/COUNTER-RULES.md").readText()
        ok((205..216).all { rules.contains("- **CR-$it.**") }, "CR-205 to CR-216 are in the rules")
    }

    @JvmStatic
    fun main(args: Array<String>) {
        val screensMd = args.firstOrNull()?.let { File(it) }
        wordsTests(screensMd)
        pinTests()
        pairingTests()
        configTests()
        amountTests()
        qrTests()
        priceTests()
        linkTests()
        torTests()
        lightningTests()
        screenTests()
        cr141CashierRulesTests()
        cr140CashierFileTests()
        cr140NoBackupTests(screensMd?.parentFile?.parentFile?.parentFile?.let { File(it, "apps/counter-android") })
        cr149TillLockTests()
        cr177ShowingTests()
        cr152DayWindowTests()
        cr153TodayLinesTests()
        cr200PinStepsTests(screensMd?.parentFile?.parentFile?.parentFile?.let { File(it, "apps/counter-android") })
        cr188TipTests(screensMd?.parentFile?.parentFile?.parentFile?.let { File(it, "apps/counter-android") })
        cr201BuiltInTorTests(screensMd?.parentFile?.parentFile?.parentFile?.let { File(it, "apps/counter-android") })
        cr202TorFilesTests(screensMd?.parentFile?.parentFile?.parentFile?.let { File(it, "apps/counter-android") })
        cr203TorChoiceTests(screensMd?.parentFile?.parentFile?.parentFile?.let { File(it, "apps/counter-android") })
        cr204TorFailsTests(screensMd?.parentFile?.parentFile?.parentFile?.let { File(it, "apps/counter-android") })
        cr219SwapTests()
        cr214ShareTests()
        cr213RequestsTests(screensMd?.parentFile?.parentFile?.parentFile?.let { File(it, "apps/counter-android") })
        println("AppModelTest (counter): $count checks passed")
    }
}
