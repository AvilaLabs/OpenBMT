package org.openbmt.counter.app

import java.io.File
import java.io.IOException
import java.net.InetAddress
import java.net.ServerSocket
import java.nio.file.Files
import java.security.SecureRandom
import org.openbmt.app.SayCheck
import org.openbmt.app.SayWords
import org.openbmt.app.Tones
import org.openbmt.app.Who
import org.openbmt.counter.CounterException
import org.openbmt.counter.CounterNetwork
import org.openbmt.counter.Payment
import org.openbmt.counter.Quoted
import org.openbmt.counter.Rate
import org.openbmt.counter.RateSource
import org.openbmt.counter.SaleState
import org.openbmt.counter.Sample
import org.openbmt.counter.Words

/**
 * The tests of the pure parts of the counter app: its words, the PIN store
 * with a fake clock, the pairing text, the config, the amount keypad, the QR
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
        } catch (_: PairingFormatException) {
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
        ok(AppWords.orbotNotRunning.next == "Open Orbot and turn it on.", "the Orbot next step")
        ok(AppWords.orbotNotRunning.who == Who.MANAGER, "the manager does it")
        ok(AppWords.pinWrong(4).tone == Tones.STOP && AppWords.pinWrong(4).next.contains("4 seconds"), "a wrong PIN says how long")
        ok(AppWords.waitText(1) == "1 second" && AppWords.waitText(59) == "59 seconds", "short waits")
        ok(AppWords.waitText(60) == "1 minute" && AppWords.waitText(300) == "5 minutes" && AppWords.waitText(121) == "3 minutes", "long waits")
        ok(AppWords.priceReady("1.00 USD", 1).tone == Tones.GOOD, "a ready price is green")
        ok(AppWords.cameraOff.who == Who.MANAGER && AppWords.cameraOff.tone == Tones.STOP, "the camera-off words: the manager, stop")
        ok(AppWords.cameraOff.next.contains("settings") && AppWords.cameraOff.next.contains("paste") && AppWords.cameraOff.next.contains("file"), "they say to allow it, paste or choose a file")
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

    // ---- the pairing text -------------------------------------------------------------------

    private fun hex(n: Int, v: Int) = (0 until n).joinToString("") { "%02x".format((v + it) and 0xff) }

    private fun pairingText(
        header: String = Pairing.HEADER,
        extra: String = "",
        drop: String? = null,
        network: String = "regtest",
        eol: String = "\n",
    ): String {
        val lines = listOf(
            header,
            "network: $network",
            "identity-secret: ${hex(32, 1)}",
            "owner-root: ${hex(32, 2)}",
            "approver-root: ${hex(32, 3)}",
            "wallet-checksum: ${hex(32, 4)}",
            "batch-min-count: 5",
            "settings: 0a0b0c",
            "first-batch: 0d0e",
            "peer: 10.0.0.1:8333",
        ).filter { drop == null || !it.startsWith("$drop:") }
        return lines.joinToString(eol) + eol + extra
    }

    private fun pairingTests() {
        val p = Pairing.parse(pairingText())
        ok(p.network == CounterNetwork.Regtest && p.batchMinCount == 5L, "network and batch size")
        ok(p.identitySecret.contentEquals(ByteArray(32) { (1 + it).toByte() }), "the identity secret")
        ok(p.ownerRoot[0].toInt() == 2 && p.approverRoot[0].toInt() == 3 && p.walletChecksum[0].toInt() == 4, "the roots and the wallet check")
        ok(p.settings.contentEquals(byteArrayOf(10, 11, 12)) && p.firstBatch!!.contentEquals(byteArrayOf(13, 14)), "settings and first batch")
        ok(p.peers == listOf("10.0.0.1:8333"), "a peer")
        val c = p.startConfig("/data/node", 1_700_000_000L, "127.0.0.1:9050")
        ok(c.dir == "/data/node" && c.now == 1_700_000_000L && c.proxy == "127.0.0.1:9050" && c.fixedPeers == p.peers, "the start config")
        ok(p.startConfig("d", 1, null).proxy == null, "no proxy with Tor off")
        ok(Pairing.parse(pairingText(drop = "first-batch")).firstBatch == null, "the first batch is optional")
        ok(Pairing.parse(pairingText(drop = "peer")).peers.isEmpty(), "peers are optional")
        ok(Pairing.parse(pairingText().dropLast(1)).batchMinCount == 5L, "the last line end may be missing")
        ok(Pairing.parse(pairingText(eol = "\r\n")).batchMinCount == 5L, "a pasted text with CRLF")
        for (n in listOf("mainnet", "testnet", "signet")) ok(Pairing.parse(pairingText(network = n)).network.name.lowercase() == n, "network $n")
        throws("another network name") { Pairing.parse(pairingText(network = "bitcoin")) }
        throws("another header") { Pairing.parse(pairingText(header = "openbmt-counter-pairing 2")) }
        throws("no header") { Pairing.parse(pairingText().substringAfter('\n')) }
        for (key in listOf("network", "identity-secret", "owner-root", "approver-root", "wallet-checksum", "batch-min-count", "settings")) {
            throws("no $key") { Pairing.parse(pairingText(drop = key)) }
        }
        throws("a repeated key") { Pairing.parse(pairingText(extra = "network: regtest\n")) }
        throws("an unknown key") { Pairing.parse(pairingText(extra = "color: blue\n")) }
        throws("a line with no colon") { Pairing.parse(pairingText(extra = "nonsense\n")) }
        throws("a short secret") { Pairing.parse(pairingText().replace(hex(32, 1), hex(31, 1))) }
        throws("a secret that is not hex") { Pairing.parse(pairingText().replace(hex(32, 1), "zz" + hex(31, 1))) }
        throws("odd settings hex") { Pairing.parse(pairingText().replace("0a0b0c", "0a0b0")) }
        throws("a batch size that is not a number") { Pairing.parse(pairingText().replace("batch-min-count: 5", "batch-min-count: five")) }
        throws("a batch size past 32 bits") { Pairing.parse(pairingText().replace("batch-min-count: 5", "batch-min-count: 4294967296")) }
        throws("a value with spaces around it") { Pairing.parse(pairingText().replace("batch-min-count: 5", "batch-min-count:  5")) }
        throws("an empty value") { Pairing.parse(pairingText().replace("settings: 0a0b0c", "settings: ")) }
        throws("a bad peer") { Pairing.parse(pairingText(extra = "peer: nohost\n")) }
        throws("too many peers") { Pairing.parse(pairingText(extra = (1..8).joinToString("") { "peer: 10.0.0.$it:8333\n" })) }
        throws("a text that is too long") { Pairing.parse(pairingText() + " ".repeat(Pairing.MAX_TEXT)) }
        ok(HostPort.valid("127.0.0.1:9050") && HostPort.valid("localhost:1") && HostPort.valid("[::1]:9050") && HostPort.valid("a.b-c.onion:65535"), "host:port")
        ok(!HostPort.valid("127.0.0.1") && !HostPort.valid(":9050") && !HostPort.valid("h:0") && !HostPort.valid("h:65536") && !HostPort.valid("h:") && !HostPort.valid("h p:1") && !HostPort.valid("[]:1"), "not host:port")
    }

    // ---- the config -----------------------------------------------------------------------------

    private fun configTests() {
        val c = AppConfig("127.0.0.1:9050", "Mia", "Ann")
        val back = AppConfig.decode(c.encode())
        ok(back.proxy == "127.0.0.1:9050" && back.manager == "Mia" && back.cashier == "Ann", "round trip")
        ok(AppConfig.decode(c.withCashier("").encode()).cashier == "", "an empty cashier name")
        ok(AppConfig.decode(AppConfig("h:1", "Méa", "").encode()).manager == "Méa", "a name with a letter outside ASCII")
        ok(AppConfig.validName("Ann Lee") && !AppConfig.validName("") && !AppConfig.validName(" Ann") && !AppConfig.validName("A\nB") && !AppConfig.validName("x".repeat(65)), "names")
        ok(AppConfig.validName("x".repeat(64)) && !AppConfig.validName("é".repeat(64).plus("é".repeat(64))), "name lengths")
        val text = String(c.encode(), Charsets.UTF_8)
        for (bad in listOf("", text.dropLast(1), text + "x: y\n", text.replace("proxy:", "prox:"), text.replace("127.0.0.1:9050", "nope"), text.replace("manager: Mia", "manager: "), "proxy: h:1\nmanager: M\ncashier: \u0007\n")) {
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
        clean(dir)
    }

    // ---- the keypad ---------------------------------------------------------------------------------

    private fun amountTests() {
        val usd = Money.of("USD")!!
        ok(usd.show(0) == "0.00 USD" && usd.show(5) == "0.05 USD" && usd.show(1234) == "12.34 USD", "small amounts")
        ok(usd.show(123456789) == "1,234,567.89 USD" && usd.plain(100) == "1.00", "separators")
        ok(Money.of("JPY")!!.show(1500) == "1,500 JPY" && Money.of("XXX") == null, "currencies")
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
        for (s in listOf(SaleState.PaidInstant, SaleState.Confirmed, SaleState.Final, SaleState.Overridden, SaleState.Expired, SaleState.Cancelled, SaleState.NeedsAPerson)) {
            ok(b(s).let { !it.requote && !it.cancel && !it.override }, "no button for $s")
        }
        val usd = Money.of("USD")!!
        ok(SaleModel.amountLine(usd, 1234, 25_000) == "12.34 USD, 25,000 sats to send", "the amount line gives both (SC-09)")
        ok(SaleModel.amountLine(usd, null, 1) == "1 sat to send", "an amount in sats alone")
        ok(SaleModel.receivedLine(5000, 6000) == "Received 5,000 sats of 6,000 sats", "the received line")
        ok(SaleModel.isEnded(SaleState.Expired) && !SaleModel.isEnded(SaleState.Waiting), "ended sales")
        ok(ShopCurrency.of(listOf(src("a"), src("b")))!!.code == "USD", "the shop's currency is the sources'")
        ok(ShopCurrency.of(listOf(src("a"), RateSource("b", "u", "EUR", "b"))) == null && ShopCurrency.of(emptyList()) == null, "sources in two currencies have none")
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
        torTests()
        screenTests()
        println("AppModelTest (counter): $count checks passed")
    }
}
