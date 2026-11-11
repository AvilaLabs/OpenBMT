package org.openbmt.owner.app

import java.io.File
import java.io.IOException
import java.net.InetAddress
import java.net.ServerSocket
import java.nio.file.Files
import java.security.SecureRandom
import javax.crypto.Cipher
import javax.crypto.spec.GCMParameterSpec
import javax.crypto.spec.SecretKeySpec
import org.openbmt.app.Format
import org.openbmt.app.SayCheck
import org.openbmt.app.SayWords
import org.openbmt.app.Sealed
import org.openbmt.app.SealedFile
import org.openbmt.app.Sealer
import org.openbmt.app.Tones
import org.openbmt.app.Who
import org.openbmt.owner.Alert
import org.openbmt.owner.AlertKind
import org.openbmt.owner.CashAmount
import org.openbmt.owner.CounterShare
import org.openbmt.owner.CounterState
import org.openbmt.owner.EntryKind
import org.openbmt.owner.Limit
import org.openbmt.owner.LinkState
import org.openbmt.owner.OutboxKind
import org.openbmt.owner.OutboxRow
import org.openbmt.owner.OwnerException
import org.openbmt.owner.OwnerNetwork
import org.openbmt.owner.Quoted
import org.openbmt.owner.Route
import org.openbmt.owner.SendKind
import org.openbmt.owner.SendRow
import org.openbmt.owner.SettingsHeld
import org.openbmt.owner.Words

/**
 * The tests of the pure parts of the owner app: its words, the setup text,
 * the config, the text form of a request and a response, the Tor gate, the
 * screen choice, the lines and forms of the screens; and, named `oa_NN_...`,
 * the rules of docs/OWNER-APP-RULES.md. They run on the desktop JVM, where no
 * Android class exists. The main takes the path of docs/screens/owner-app.md
 * and the path of docs/OWNER-APP-RULES.md; the environment variable
 * OPENBMT_WRITE_SCREENS=1 writes the first file first.
 */
object AppModelTest {
    private var count = 0
    private lateinit var appDir: File

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
        } catch (_: SetupFormatException) {
            return
        } catch (_: ApprovalTextException) {
            return
        }
        throw AssertionError("FAILED: no error for $what")
    }

    private fun tempDir(): File = Files.createTempDirectory("owner-test").toFile()

    private fun clean(dir: File) {
        dir.listFiles()?.forEach { it.delete() }
        dir.delete()
    }

    private fun source(path: String): String = File(appDir, path).readText()

    private fun hex(n: Int, v: Int) = (0 until n).joinToString("") { "%02x".format((v + it) and 0xff) }

    /** A setup text, with the lines of [extra] added and the lines named in [without] left out. */
    private fun setupText(extra: List<String> = emptyList(), without: Set<String> = emptySet(), header: String = OwnerSetup.HEADER): String {
        val lines = listOf(
            "network: regtest",
            "owner-secret: ${hex(32, 1)}",
            "approver-root: ${hex(32, 33)}",
            "wallet: wsh(multi(2,[a/1h]xpub1,[b/1h]xpub2,[c/1h]xpub3))#abcdefgh",
            "key1-account: ${hex(78, 65)}",
            "batch-min-count: 20",
            "refund-depth: 6",
            "birthday: 800000",
        ).filter { l -> without.none { l.startsWith("$it:") } }
        return (listOf(header) + lines + extra).joinToString("\n") + "\n"
    }

    private fun linkLines(route: String = "onion abcdefghij.onion:8080") = listOf(
        "relay-route: $route",
        "relay-key: ${hex(32, 97)}",
        "counter: ${hex(32, 129)}",
        "counter: ${hex(32, 161)}",
    )

    private fun words(tone: Int = Tones.INFO) = Words(tone, Who.OWNER, "Heading", "What happened.", "Do this.", null, null, null, null)

    // ---- the words ------------------------------------------------------------------------

    private fun wordsTests(screensMd: File?) {
        val all = AppWords.all()
        for ((name, w) in all) ok(SayCheck.problem(w) == null, "words of $name: ${SayCheck.problem(w)}")
        ok(SayCheck.distinctProblem(all) == null, "no two words read the same: ${SayCheck.distinctProblem(all)}")
        ok(AppWords.orbotNotRunning.what == "This app sends everything through Tor, but Orbot is not running.", "the Orbot words, as written down")
        ok(AppWords.orbotNotRunning.next == "Open Orbot and turn it on.", "the Orbot next step")
        ok(AppWords.orbotNotRunning.who == Who.OWNER, "the owner does it")
        ok(all.map { it.first }.toSet().size == all.size, "every name is used once")
        val dirty = AppWords.fileNotRead("bad\u0007 text‮")
        ok(dirty.detail == "bad text", "a system text is cleaned in the details: ${dirty.detail}")
        ok(SayCheck.problem(dirty) == null, "words with a cleaned detail pass")
        val text = AppWords.screensMarkdown()
        ok(text.lines().filter { it.startsWith("| ") }.size == all.size + 1, "one row for each words and the header")
        ok(text.lines().filter { !it.startsWith("|") }.all { it.length <= 80 }, "lines are at most 80 characters")
        if (screensMd != null) {
            if (System.getenv("OPENBMT_WRITE_SCREENS") != null) screensMd.writeText(text)
            ok(screensMd.isFile && screensMd.readText() == text, "docs/screens/owner-app.md is what screensMarkdown makes (run with OPENBMT_WRITE_SCREENS=1)")
        }
    }

    private fun conversionTests() {
        val w = Words(Tones.STOP, Who.KEY_HOLDER, "H", "W.", "N.", "AS-1", 5L, Quoted("the Key app", "text"), "d").say()
        ok(w.tone == Tones.STOP && w.who == Who.KEY_HOLDER && w.code == "AS-1" && w.at == 5L, "the fields carry over")
        ok(w.quoted?.from == "the Key app" && w.quoted?.text == "text" && w.detail == "d", "the quoted text and the detail carry over")
        val withWords = OwnerException("r", "t", 1, words())
        ok(withWords.sayWords().heading == "Heading", "a refusal of the library is shown with its words")
        val own = OwnerException("response-null", "the library gave no response", 4)
        ok(own.sayWords().heading == "Answer not readable" && own.sayWords().detail!!.startsWith("response-null"), "an error the app made itself has the app's words")
    }

    // ---- the setup text -------------------------------------------------------------------------

    private fun setupTests() {
        val s = OwnerSetup.parse(setupText())
        ok(s.network == OwnerNetwork.Regtest, "the network")
        ok(s.ownerSecret.size == 32 && s.approverRoot.size == 32 && s.key1Account.size == 78, "the key sizes")
        ok(s.ownerSecret[0] == 1.toByte() && s.key1Account[0] == 65.toByte(), "the key bytes")
        ok(s.wallet.startsWith("wsh(") && s.key3Xpub == null && s.link == null && s.peers.isEmpty(), "wallet, no Key 3, no link, no peers")
        ok(s.batchMinCount == 20L && s.refundDepth == 6L && s.birthday == 800000L, "the numbers")
        ok(OwnerSetup.parse(setupText().trimEnd('\n')).birthday == 800000L, "a missing final line end is allowed")
        ok(OwnerSetup.parse(setupText().replace("\n", "\r\n")).refundDepth == 6L, "line ends of a text file are allowed")

        val full = OwnerSetup.parse(setupText(listOf("key3-xpub: xpubABC", "peer: 10.0.0.2:8333", "peer: node.example:8333") + linkLines() + listOf("heartbeat-every: 30", "check-in-grace: 600")))
        ok(full.key3Xpub == "xpubABC" && full.peers == listOf("10.0.0.2:8333", "node.example:8333"), "Key 3's public key and the peers")
        val link = full.link!!
        ok(link.route is RelayRoute.Onion && link.counters.size == 2 && link.relayKey.size == 32, "the link")
        ok(link.heartbeatEverySecs == 30L && link.checkInGraceSecs == 600L, "the link's times")
        ok(OwnerSetup.parse(setupText(linkLines("direct 192.168.1.5:9000"))).link!!.route is RelayRoute.Direct, "a direct route")

        // The config of a start.
        val c = full.startConfig("/data/node", 1_700_000_000, "127.0.0.1:9050")
        ok(c.dir == "/data/node" && c.now == 1_700_000_000L && c.network == OwnerNetwork.Regtest, "the start's folder, time and network")
        ok(c.proxy == "127.0.0.1:9050" && c.broadcastProxy == "127.0.0.1:9050" && !c.withoutTor, "Tor for the peers and for sending")
        val route = c.link!!.route as Route.Onion
        ok(route.proxy == "127.0.0.1:9050" && route.host == "abcdefghij.onion" && route.port == 8080L, "the onion route goes through the proxy")
        ok(c.fixedPeers == full.peers && c.key3Xpub == "xpubABC" && c.refundDepth == 6L, "the rest of the start")
        val direct = OwnerSetup.parse(setupText(linkLines("direct 192.168.1.5:9000"))).startConfig("/d", 1, "127.0.0.1:9050")
        ok((direct.link!!.route as Route.Direct).addr == "192.168.1.5:9000", "a direct route is kept as it is")

        throws("a text without the header") { OwnerSetup.parse(setupText(header = "openbmt-owner-setup 2")) }
        throws("a pairing text of the counter") { OwnerSetup.parse(setupText(header = "openbmt-counter-pairing 1")) }
        throws("an empty text") { OwnerSetup.parse("") }
        for (key in listOf("network", "owner-secret", "approver-root", "wallet", "key1-account", "batch-min-count", "refund-depth", "birthday")) {
            throws("a text without $key") { OwnerSetup.parse(setupText(without = setOf(key))) }
        }
        throws("an unknown key") { OwnerSetup.parse(setupText(listOf("passphrase: x"))) }
        throws("a repeated key") { OwnerSetup.parse(setupText(listOf("birthday: 5"))) }
        throws("no space after the colon") { OwnerSetup.parse(setupText().replace("birthday: ", "birthday:")) }
        throws("a bad network") { OwnerSetup.parse(setupText().replace("regtest", "moonnet")) }
        throws("a short secret") { OwnerSetup.parse(setupText().replace("owner-secret: ${hex(32, 1)}", "owner-secret: ${hex(31, 1)}")) }
        throws("a secret that is not hex") { OwnerSetup.parse(setupText().replace("owner-secret: ${hex(32, 1)}", "owner-secret: " + "zz".repeat(32))) }
        throws("a short account key") { OwnerSetup.parse(setupText().replace("key1-account: ${hex(78, 65)}", "key1-account: ${hex(77, 65)}")) }
        throws("a number that is not digits") { OwnerSetup.parse(setupText().replace("refund-depth: 6", "refund-depth: six")) }
        throws("a number that is too large") { OwnerSetup.parse(setupText().replace("refund-depth: 6", "refund-depth: 4294967296")) }
        throws("a wallet with a space") { OwnerSetup.parse(setupText().replace("wsh(multi(2,", "wsh(multi(2, ")) }
        throws("a key 3 with a space") { OwnerSetup.parse(setupText(listOf("key3-xpub: xpub ABC"))) }
        throws("a peer that is not host:port") { OwnerSetup.parse(setupText(listOf("peer: nowhere"))) }
        throws("more than 8 peers") { OwnerSetup.parse(setupText((1..9).map { "peer: 10.0.0.$it:8333" })) }
        throws("a relay key without a route") { OwnerSetup.parse(setupText(listOf("relay-key: ${hex(32, 97)}"))) }
        throws("a route without a relay key") { OwnerSetup.parse(setupText(listOf("relay-route: direct 1.2.3.4:5"))) }
        throws("a route without a counter") { OwnerSetup.parse(setupText(listOf("relay-route: direct 1.2.3.4:5", "relay-key: ${hex(32, 97)}"))) }
        throws("a counter without a link") { OwnerSetup.parse(setupText(listOf("counter: ${hex(32, 129)}"))) }
        throws("the same counter twice") { OwnerSetup.parse(setupText(linkLines() + listOf("counter: ${hex(32, 129)}"))) }
        throws("a route of another kind") { OwnerSetup.parse(setupText(linkLines("carrier-pigeon 1.2.3.4:5"))) }
        throws("a route without a port") { OwnerSetup.parse(setupText(linkLines("direct 1.2.3.4"))) }
        throws("a text that is too long") { OwnerSetup.parse(setupText(listOf("peer: " + "a".repeat(OwnerSetup.MAX_TEXT)))) }
        throws("an empty value") { OwnerSetup.parse(setupText().replace("birthday: 800000", "birthday: ")) }
    }

    private fun configTests() {
        val c = AppConfig("127.0.0.1:9050")
        ok(String(c.encode(), Charsets.UTF_8) == "proxy: 127.0.0.1:9050\n", "the config line")
        ok(AppConfig.decode(c.encode()).proxy == "127.0.0.1:9050", "the config reads back")
        throws("a config without a line end") { AppConfig.decode("proxy: 1.2.3.4:5".toByteArray()) }
        throws("a config with two lines") { AppConfig.decode("proxy: 1.2.3.4:5\nx: y\n".toByteArray()) }
        throws("a config with a bad proxy") { AppConfig.decode("proxy: nowhere\n".toByteArray()) }
        throws("a config of the counter app") { AppConfig.decode("proxy: 1.2.3.4:5\nmanager: a\ncashier: b\n".toByteArray()) }
        throws("a config that is too long") { AppConfig.decode(ByteArray(300) { 'a'.code.toByte() }) }
        throws("a config that is not UTF-8") { AppConfig.decode(byteArrayOf(0xff.toByte(), 10)) }
        val dir = tempDir()
        val file = ConfigFile(dir)
        ok(file.load() == null, "a first start has no config")
        file.save(c)
        ok(file.load()!!.proxy == "127.0.0.1:9050", "a saved config is read back")
        ok(dir.listFiles()!!.none { it.name.endsWith(".tmp") }, "no temporary file is left")
        clean(dir)
        ok(HostPort.valid("127.0.0.1:9050") && HostPort.valid("[::1]:9050") && HostPort.valid("orbot.local:1"), "good hosts and ports")
        ok(!HostPort.valid("127.0.0.1") && !HostPort.valid("127.0.0.1:0") && !HostPort.valid("127.0.0.1:65536") && !HostPort.valid(":9050") && !HostPort.valid("a b:1"), "bad hosts and ports")
    }

    // ---- requests and responses ---------------------------------------------------------------------

    private fun approvalTextTests() {
        val rnd = SecureRandom()
        for (n in listOf(1, 2, 31, 32, 33, 100, 1000, 4097)) {
            val bytes = ByteArray(n).also { rnd.nextBytes(it) }
            val text = ApprovalText.encode(bytes)
            ok(ApprovalText.decode(text).contentEquals(bytes), "$n bytes round-trip through the text")
            ok(text.lines().filter { it.isNotEmpty() }.all { it.length <= 64 } && text.endsWith("\n"), "lines of at most 64 digits")
            ok(text == text.lowercase(), "lower case")
        }
        ok(ApprovalText.decode("  00 ff\r\n10\t20 ").contentEquals(byteArrayOf(0, -1, 16, 32)), "spaces and line ends are ignored")
        throws("an empty text") { ApprovalText.decode(" \n") }
        throws("an odd number of digits") { ApprovalText.decode("abc") }
        throws("a digit that is not hex") { ApprovalText.decode("abcg") }
        throws("a text for more than the most") { ApprovalText.decode("00".repeat(ApprovalText.MAX_BYTES + 1)) }
        ok(ApprovalText.decode("00".repeat(ApprovalText.MAX_BYTES)).size == ApprovalText.MAX_BYTES, "the most is accepted")
        ok(ApprovalText.preview(byteArrayOf(1, 2, 3)) == "010203", "a short request is shown whole")
        val big = ApprovalText.preview(ByteArray(1000))
        ok(big.startsWith("0000") && big.endsWith("(1000 bytes in all)") && big.length < 400, "a long request is cut and says how long it is")
        ok(ApprovalText.fileName("request", ByteArray(32) { it.toByte() }) == "request-00010203.obm", "the file name")

        var got: ByteArray? = null
        var failed: SayWords? = null
        PastedResponse { "0a0b" }.begin({ got = it }, { failed = it })
        ok(got!!.contentEquals(byteArrayOf(10, 11)) && failed == null, "the pasted source gives the bytes")
        PastedResponse { "0a0" }.begin({ got = null }, { failed = it })
        ok(failed != null && failed!!.heading == "Response text not read" && SayCheck.problem(failed!!) == null, "the pasted source says why it could not")
    }

    // ---- the screens -------------------------------------------------------------------------------------

    private fun screenTests() {
        val set = SetupStatus(setup = true, config = true)
        ok(set.complete && !SetupStatus(true, false).complete && !SetupStatus(false, true).complete, "a setup is complete when both are saved")
        ok(ScreenModel.pick(null, SetupStatus(false, false), null, false) is Screen.FirstStart, "nothing saved: the first screen")
        ok(ScreenModel.pick(null, SetupStatus(true, false), null, false) is Screen.FirstStart, "half saved: the first screen")
        ok(ScreenModel.pick(null, set, null, false) is Screen.Starting, "saved, not running: starting")
        ok(ScreenModel.pick(null, set, null, true) is Screen.Home, "running: home")
        ok(ScreenModel.pick(null, set, AppWords.orbotNotRunning, false) is Screen.TorBlocked, "no Orbot: the Tor screen")
        ok(ScreenModel.pick(AppWords.stopped, set, AppWords.orbotNotRunning, true) is Screen.Fault, "a fault comes first")
        ok(ScreenModel.pick(AppWords.stopped, SetupStatus(false, false), null, false) is Screen.Fault, "a fault comes before the first screen")
    }

    private fun modelTests() {
        ok(OwnerModel.alertsLine(0) == "No alerts." && OwnerModel.alertsLine(1) == "1 alert." && OwnerModel.alertsLine(3) == "3 alerts.", "the alerts line")
        ok(OwnerModel.sendsLine(0) == "No payments in progress." && OwnerModel.sendsLine(1) == "1 payment in progress." && OwnerModel.sendsLine(2) == "2 payments in progress.", "the sends line")
        val rows = SendKind.values().map { SendRow("ab", it, 0, words()) }
        ok(OwnerModel.sendsInProgress(rows) == 6, "done and replaced are not in progress: ${OwnerModel.sendsInProgress(rows)}")
        ok(OwnerModel.sendsInProgress(emptyList()) == 0, "no sends")
        ok(OwnerModel.counterLabel(ByteArray(32) { 0xab.toByte() }) == "Counter abababab", "a counter's label")
        ok(OwnerModel.saleNumber(0) == 1L, "the first sale is 1")
        ok(OwnerModel.ago(100, 100) == "just now" && OwnerModel.ago(100, 200) == "just now", "now and the future")
        ok(OwnerModel.ago(160, 100) == "60 seconds ago" && OwnerModel.ago(400, 100) == "5 minutes ago", "seconds and minutes")
        ok(OwnerModel.ago(100 + 3 * 3600, 100) == "3 hours ago" && OwnerModel.ago(100 + 3 * 86400, 100) == "3 days ago", "hours and days")

        val counter = CounterState(ByteArray(32) { 1 }, 5, ByteArray(32) { 2 }, 3, true, 1000, words())
        val lines = OwnerModel.counterLines(counter, 1300)
        ok(lines[0] == "Counter 01010101: 5 records." && lines[1] == "Head: 0202020202020202..." && lines[2] == "Last check-in: number 3, 5 minutes ago, held.", "a counter's lines: $lines")
        ok(OwnerModel.counterLines(CounterState(ByteArray(32), 0, ByteArray(32), null, null, null, words()), 0)[2] == "No check-in has been taken yet.", "a counter that never checked in")
        val link = LinkState(true, 0, listOf(Alert(AlertKind.Diverged, ByteArray(32), 1, words())), listOf(counter))
        ok(OwnerModel.linkLine(null, 0) == "No link to the counters.", "no link")
        ok(OwnerModel.linkLine(link, 1) == "The relay is reachable. 1 counter is on the list.", "a link")
        ok(OwnerModel.linkLine(LinkState(false, 0, emptyList(), emptyList()), 0) == "The relay is not reachable. No counter is on the list.", "a link that does not answer")

        val batch = OutboxRow(8, OutboxKind.Batch, null, ByteArray(32) { 1 }, null, 1000, 300, words())
        val bl = OwnerModel.outboxLines(batch, 1300)
        ok(OwnerModel.outboxTitle(batch) == "Address batch 8" && bl[0] == "Counter 01010101" && bl[1].endsWith("5 minutes ago.") && bl[2] == "Size: 300 bytes.", "a batch waits: $bl")
        val tx = OutboxRow(9, OutboxKind.Transaction, "ab12", null, null, null, 200, words())
        val tl = OwnerModel.outboxLines(tx, 1300)
        ok(tl[0] == "Transaction ab12" && tl[1] == "The node keeps no signing time for this one.", "a payment has no time: $tl")
        ok(OwnerModel.outboxTitle(OutboxRow(1, OutboxKind.Settings, null, null, 4, null, 1, words())) == "Settings 1", "a settings title")
        ok(OwnerModel.entryTitle(EntryKind.SettingsChange, 3) == "Settings change 3", "an entry title")

        val s = sampleSettings()
        val sl = OwnerModel.settingsLines(s)
        ok(sl.map { it.first }.toSet().size == sl.size, "every label once")
        ok(sl[0] == ("Version" to "3") && sl[1] == ("Largest instant sale, light check" to "$7 (7,000 sats)"), "the first lines: ${sl.take(2)}")
        val byLabel = sl.toMap()
        ok(byLabel["Underpayment still counted"] == "1.05%" && byLabel["Tor"] == "on" && byLabel["Customers can join the Wi-Fi"] == "no", "percent and switches: $byLabel")
        ok(byLabel["Smallest sale a counter opens"] == "no floor" && byLabel["Boxes"] == "0" && byLabel["Devices that write records"] == "2", "floor, boxes, devices")
        ok(byLabel["Counters and their shares"] == "Counter 02020202 100.00%", "the counters' shares: ${byLabel["Counters and their shares"]}")
        ok(byLabel["Blocks to settle a sale"] == "1" && byLabel["Blocks until a refund may be paid"] == "6", "the depths")
        ok(OwnerModel.settingsLines(sampleSettings(floor = 5000, counters = emptyList())).toMap()["Smallest sale a counter opens"] == "5,000 sats", "a floor")
    }

    private fun sampleSettings(floor: Long = 0, counters: List<CounterShare> = listOf(CounterShare(ByteArray(32) { 2 }, 10_000))): SettingsHeld {
        val l = Limit(7000, 7)
        return SettingsHeld(
            3, l, l, l, l, l, l, 105, l, l, l, l, 1, 6, 900, true, false, floor,
            listOf(ByteArray(32) { 1 }, ByteArray(32) { 2 }), counters, emptyList(),
        )
    }

    private fun formTests() {
        ok(Forms.number("12,345") == 12345L && Forms.number(" 7 ") == 7L, "numbers")
        ok(Forms.number("") == null && Forms.number("1.5") == null && Forms.number("-1") == null && Forms.number("1".repeat(19)) == null, "bad numbers")
        ok(Forms.number32("4294967295") == 4294967295L && Forms.number32("4294967296") == null, "a u32")
        ok(Forms.counter(hex(32, 1))!!.size == 32 && Forms.counter(hex(31, 1)) == null && Forms.counter("zz".repeat(32)) == null, "a counter identity")
        ok((Forms.feeRate("12") as Form.Ok).value == 12L, "a fee rate")
        ok((Forms.feeRate("x") as Form.Refused).words == AppWords.notANumber, "a bad fee rate")
        ok((Forms.feeRate("0") as Form.Ok).value == 0L, "zero is the library's to refuse")

        val counter = ByteArray(32) { 1 }
        val r = Forms.refund(counter, "3", " Mia ", " bcrt1qxyz ", "10,000") as Form.Ok
        ok(r.value.sale == 2L && r.value.cashier == "Mia" && r.value.address == "bcrt1qxyz" && r.value.sats == 10_000L, "a refund's arguments: sale 3 is index 2")
        ok((Forms.refund(null, "3", "Mia", "a", "1") as Form.Refused).words == AppWords.counterNeeded, "no counter")
        ok((Forms.refund(ByteArray(5), "3", "Mia", "a", "1") as Form.Refused).words == AppWords.counterNotAccepted, "a counter of the wrong size")
        ok((Forms.refund(counter, "0", "Mia", "a", "1") as Form.Refused).words == AppWords.notANumber, "there is no sale 0")
        ok((Forms.refund(counter, "x", "Mia", "a", "1") as Form.Refused).words == AppWords.notANumber, "a sale that is not a number")
        ok((Forms.refund(counter, "1", "Mia", "a", "") as Form.Refused).words == AppWords.notANumber, "no amount")
        val odd = Forms.refund(counter, "1", "", "this is not an address", "1") as Form.Ok
        ok(odd.value.address == "this is not an address" && odd.value.cashier == "", "an address and a name are the library's to judge")

        val c1 = Forms.cashOut(" bcrt1qabc ", false, "5000", " till ") as Form.Ok
        ok(c1.value.address == "bcrt1qabc" && (c1.value.amount as CashAmount.Sats).sats == 5000L && c1.value.label == "till", "a cash-out of an amount")
        ok((Forms.cashOut("a", true, "junk", "l") as Form.Ok).value.amount is CashAmount.All, "a cash-out of everything ignores the amount")
        ok((Forms.cashOut("a", false, "junk", "l") as Form.Refused).words == AppWords.notANumber, "a cash-out amount that is not a number")
    }

    // ---- the Tor gate (OA-05) ------------------------------------------------------------------------------

    private fun serve(reply: ByteArray?): Pair<ServerSocket, Thread> {
        val server = ServerSocket(0, 1, InetAddress.getByName("127.0.0.1"))
        val t = Thread {
            try {
                server.accept().use { s ->
                    val greeting = ByteArray(3)
                    var n = 0
                    while (n < 3) {
                        val k = s.getInputStream().read(greeting, n, 3 - n)
                        if (k < 0) return@Thread
                        n += k
                    }
                    if (reply != null) s.getOutputStream().apply { write(reply); flush() }
                }
            } catch (_: IOException) {
            }
        }
        t.isDaemon = true
        t.start()
        return server to t
    }

    // ---- the rules ---------------------------------------------------------------------------------------------

    private fun oa_01_the_secrets_are_sealed_in_one_file_and_nowhere_else() {
        val dir = tempDir()
        val key = ByteArray(32).also { SecureRandom().nextBytes(it) }
        val sealer = object : Sealer {
            override fun seal(plain: ByteArray, aad: ByteArray): Sealed {
                val iv = ByteArray(12).also { SecureRandom().nextBytes(it) }
                val cipher = Cipher.getInstance("AES/GCM/NoPadding")
                cipher.init(Cipher.ENCRYPT_MODE, SecretKeySpec(key, "AES"), GCMParameterSpec(128, iv))
                cipher.updateAAD(aad)
                return Sealed(iv, cipher.doFinal(plain))
            }

            override fun open(sealed: Sealed, aad: ByteArray): ByteArray {
                try {
                    val cipher = Cipher.getInstance("AES/GCM/NoPadding")
                    cipher.init(Cipher.DECRYPT_MODE, SecretKeySpec(key, "AES"), GCMParameterSpec(128, sealed.iv))
                    cipher.updateAAD(aad)
                    return cipher.doFinal(sealed.ciphertext)
                } catch (e: java.security.GeneralSecurityException) {
                    throw IOException("does not decrypt", e)
                }
            }
        }
        val text = setupText()
        val file = SealedFile(dir, sealer, AppFiles.SETUP_FILE)
        ok(file.load() == null, "a first start has no setup")
        file.save(text.toByteArray(Charsets.UTF_8))
        val onDisk = File(dir, AppFiles.SETUP_FILE).readBytes()
        val asText = String(onDisk, Charsets.ISO_8859_1)
        ok(!asText.contains(hex(32, 1)) && !asText.contains("owner-secret") && !asText.contains(hex(78, 65)), "no secret is on the disk in the clear")
        ok(dir.listFiles()!!.map { it.name } == listOf(AppFiles.SETUP_FILE), "one file and no temporary file")
        ok(String(file.load()!!, Charsets.UTF_8) == text, "the setup reads back")
        ok(OwnerSetup.parse(String(file.load()!!, Charsets.UTF_8)).ownerSecret[0] == 1.toByte(), "and parses")
        val tampered = onDisk.copyOf().also { it[it.size - 1] = (it[it.size - 1].toInt() xor 1).toByte() }
        File(dir, AppFiles.SETUP_FILE).writeBytes(tampered)
        throws("a file that was changed") { file.load() }
        clean(dir)
        // No other place to keep anything, and no log.
        for (name in listOf("src/android/Controller.kt", "src/android/Screens.kt", "src/android/Approval.kt", "src/android/MainActivity.kt", "src/android/OwnerUi.kt")) {
            val src = source(name)
            for (bad in listOf("SharedPreferences", "getSharedPreferences", "openFileOutput", "writeText", "writeBytes", "FileOutputStream", "getExternalFilesDir", "externalCacheDir", "cacheDir", "android.util.Log", "Log.d(", "Log.i(", "Log.e(", "println(", "printStackTrace")) {
                ok(!src.contains(bad), "$name does not use $bad")
            }
        }
        val controller = source("src/android/Controller.kt")
        ok(controller.contains("SealedFile(dir, KeystoreSealer(AppFiles.KEY_ALIAS), AppFiles.SETUP_FILE).save("), "the setup is saved sealed")
        ok(controller.contains("draftText = \"\"") && controller.contains("clearPrimaryClip()"), "the draft and the clipboard are cleared after the save")
        ok(AppFiles.KEY_ALIAS != "openbmt-key-state-v1", "the key of the owner app is not the Key app's")
        ok(controller.contains("act.noBackupFilesDir"), "the folder is the one that is not backed up")
    }

    private fun oa_02_nothing_is_backed_up_or_moved() {
        val manifest = source("AndroidManifest.xml")
        ok(manifest.contains("android:allowBackup=\"false\""), "allowBackup is false")
        ok(manifest.contains("android:dataExtractionRules=\"@xml/data_extraction_rules\"") && manifest.contains("android:fullBackupContent=\"@xml/backup_rules\""), "the rules are named")
        val extraction = source("res/xml/data_extraction_rules.xml")
        ok(!extraction.contains("<include"), "nothing is included")
        for (place in listOf("cloud-backup", "device-transfer")) {
            val part = extraction.substringAfter("<$place>").substringBefore("</$place>")
            for (domain in listOf("root", "file", "database", "sharedpref", "external")) {
                ok(part.contains("<exclude domain=\"$domain\""), "$place excludes $domain")
            }
        }
        val old = source("res/xml/backup_rules.xml")
        ok(!old.contains("<include") && old.contains("<exclude domain=\"file\""), "the old rules include nothing")
    }

    private fun oa_03_the_manifest_asks_for_two_permissions_and_exports_one_activity() {
        val manifest = source("AndroidManifest.xml")
        val permissions = Regex("<uses-permission android:name=\"([^\"]+)\"").findAll(manifest).map { it.groupValues[1] }.toList()
        ok(permissions == listOf("android.permission.INTERNET", "android.permission.CAMERA"), "the permissions: $permissions")
        ok(manifest.contains("android:name=\"android.hardware.camera.any\"") && manifest.contains("android:required=\"false\""), "the camera is a feature that is not required")
        ok(Regex("<uses-feature").findAll(manifest).count() == 1, "one feature")
        ok(Regex("<activity").findAll(manifest).count() == 1 && manifest.contains("android:exported=\"true\"") && manifest.contains("android.intent.category.LAUNCHER"), "one exported launcher activity")
        for (kind in listOf("<service", "<receiver", "<provider", "<activity-alias")) ok(!manifest.contains(kind), "no $kind")
        ok(manifest.contains("android:usesCleartextTraffic=\"false\"") && !manifest.contains("debuggable"), "cleartext off, not debuggable")
        ok(manifest.contains("package=\"org.openbmt.owner.app\""), "the package")
        val check = source("tests/check-manifest.sh")
        ok(check.contains("-eq 2 ]") && check.contains("camera.any") && check.contains("minSdkVersion:'29'") && check.contains("targetSdkVersion:'36'"), "the script checks the APK for the same")
        val build = source("build.sh")
        ok(build.contains("min_sdk=29") && build.contains("target_sdk=36") && build.contains("arm64-v8a:aarch64-linux-android") && build.contains("x86_64:x86_64-linux-android"), "the build's levels and ABIs")
    }

    private fun oa_04_every_window_is_secure_before_it_shows_anything() {
        val activity = source("src/android/MainActivity.kt")
        val create = activity.substringAfter("override fun onCreate").substringBefore("override fun onStart")
        ok(create.contains("FLAG_SECURE"), "onCreate sets FLAG_SECURE")
        ok(create.indexOf("FLAG_SECURE") < create.indexOf("Controller(this)"), "before the controller can show anything")
        ok(!activity.contains("clearFlags"), "nothing clears it")
        for (name in listOf("src/android/Controller.kt", "src/android/Screens.kt", "src/android/Approval.kt")) {
            ok(!source(name).contains("clearFlags") && !source(name).contains("Dialog"), "$name opens no window of its own and clears no flag")
        }
        ok(!activity.contains("FLAG_KEEP_SCREEN_ON"), "the owner app does not keep the screen on")
    }

    private fun oa_05_everything_goes_through_tor_and_nothing_goes_around_it() {
        val up = TorGate("127.0.0.1:9050") { _, _ -> true }
        ok(up.available() && up.proxy == "127.0.0.1:9050", "a proxy that answers")
        ok(!TorGate("127.0.0.1:9050") { _, _ -> false }.available(), "a proxy that does not answer: no node")
        ok(!TorGate("nowhere") { _, _ -> true }.available(), "an address that is not host:port: no node")
        var asked: Pair<String, Int>? = null
        TorGate("orbot.local:9150") { h, p -> asked = h to p; true }.available()
        ok(asked == ("orbot.local" to 9150), "the probe is asked at the proxy's address")
        // The real probe, against a server that speaks SOCKS5, one that does not and one that is not there.
        val (good, t1) = serve(byteArrayOf(5, 0))
        ok(Socks5Probe(1000).answers("127.0.0.1", good.localPort), "a SOCKS5 proxy is running")
        t1.join(2000); good.close()
        val (bad, t2) = serve(byteArrayOf(5, -1))
        ok(!Socks5Probe(1000).answers("127.0.0.1", bad.localPort), "a proxy that refuses the method is not a running proxy")
        t2.join(2000); bad.close()
        val (http, t3) = serve("HTTP/1.1 400".toByteArray())
        ok(!Socks5Probe(1000).answers("127.0.0.1", http.localPort), "another server on the port is not a proxy")
        t3.join(2000); http.close()
        val (silent, t4) = serve(null)
        ok(!Socks5Probe(500).answers("127.0.0.1", silent.localPort), "a server that says nothing is not a proxy")
        t4.join(2000); silent.close()
        val closed = ServerSocket(0, 1, InetAddress.getByName("127.0.0.1")).also { it.close() }
        ok(!Socks5Probe(500).answers("127.0.0.1", closed.localPort), "nothing at the port")
        // The start: Tor for every connection, and the gate before the node.
        val setup = OwnerSetup.parse(setupText(linkLines()))
        val cfg = setup.startConfig("/d", 1, "127.0.0.1:9050")
        ok(cfg.proxy == "127.0.0.1:9050" && cfg.broadcastProxy == "127.0.0.1:9050" && !cfg.withoutTor, "the start is given the proxy for peers and for sending, and never withoutTor")
        ok((cfg.link!!.route as Route.Onion).proxy == "127.0.0.1:9050", "and for an onion route")
        val controller = source("src/android/Controller.kt")
        ok(controller.indexOf("gate.available()") in 1 until controller.indexOf("OwnerNode.start("), "the gate is asked before the node starts")
        ok(!controller.contains("withoutTor = true") && !source("src/model/SetupText.kt").contains("withoutTor = true"), "no code sets withoutTor")
        ok(AppWords.orbotNotRunning.what == "This app sends everything through Tor, but Orbot is not running." && AppWords.orbotNotRunning.next == "Open Orbot and turn it on.", "the words say so")
    }

    private fun oa_06_the_node_runs_only_while_the_app_is_visible() {
        val activity = source("src/android/MainActivity.kt")
        ok(activity.substringAfter("override fun onStart").substringBefore("override fun onStop").contains("controller.visible()"), "onStart starts")
        ok(activity.substringAfter("override fun onStop").substringBefore("override fun onDestroy").contains("controller.hidden()"), "onStop stops")
        ok(activity.substringAfter("override fun onDestroy").contains("controller.hidden()"), "onDestroy stops")
        val controller = source("src/android/Controller.kt")
        val stop = controller.substringAfter("private fun stopNode()").substringBefore("// ---- pages")
        ok(stop.indexOf("linkThread") in 0 until stop.indexOf("it.stop()") && stop.indexOf("pumpThread") in 0 until stop.indexOf("it.stop()"), "the threads end before the node stops")
        ok(stop.contains("runId++"), "the loops are told to end")
        for (name in listOf("src/android/Controller.kt", "src/android/Screens.kt", "src/android/Approval.kt", "src/android/MainActivity.kt", "src/android/OwnerUi.kt", "AndroidManifest.xml")) {
            val src = source(name)
            for (bad in listOf("startForeground", "NotificationManager", "NotificationChannel", "JobScheduler", "WorkManager", "AlarmManager", "WakeLock", "startService", "<service")) {
                ok(!src.contains(bad), "$name does not use $bad")
            }
        }
        ok(source("src/android/Screens.kt").contains("There is no service in the background and no notification"), "the About screen says so")
        ok(source("README.md").contains("no service"), "the README says so")
    }

    private fun oa_07_what_has_a_meaning_is_judged_in_rust() {
        // An address and a name go through as typed: the shape of a number is all that is checked.
        val a = Forms.refund(ByteArray(32), "1", "x", "not even close", "5") as Form.Ok
        ok(a.value.address == "not even close", "an address is not judged in Kotlin")
        ok((Forms.cashOut("junk", false, "0", "") as Form.Ok).value.address == "junk", "a cash-out address is not judged in Kotlin")
        val shapeOnly = OwnerSetup.parse(setupText().replace(hex(32, 1), "00".repeat(32)))
        ok(shapeOnly.ownerSecret.all { it == 0.toByte() }, "a secret that is not a key passes the shape check, and the node refuses it")
        ok(ApprovalText.decode("00").size == 1, "the text of a response is only unwrapped")
        for (name in listOf("src/model/OwnerModel.kt", "src/android/Controller.kt", "src/android/Screens.kt", "src/android/Approval.kt")) {
            val src = source(name)
            for (bad in listOf("Regex", "bech32", "bc1", "bcrt1", "tb1", "base58", "Base58", "fromBase58")) ok(!src.contains(bad), "$name does not read an address ($bad)")
        }
        ok(!source("src/model/ApprovalText.kt").contains("Regex"), "a response is not read with a pattern")
    }

    private fun oa_08_the_app_holds_key_1_and_no_other_secret_key() {
        val names = OwnerSetup::class.java.declaredFields.map { it.name.lowercase() }
        ok(names.none { it.contains("key2") || (it.contains("key3") && !it.contains("xpub")) }, "no field for another key's secret: $names")
        for (line in listOf("key2-secret", "key2-account", "key3-secret", "key3-account", "key3-seed", "mnemonic", "seed")) {
            throws("a line $line") { OwnerSetup.parse(setupText(listOf("$line: ${hex(32, 1)}"))) }
        }
        ok(OwnerSetup.parse(setupText(listOf("key3-xpub: xpubABC"))).key3Xpub == "xpubABC", "Key 3's public key is accepted")
        ok(OwnerSetup.parse(setupText()).key3Xpub == null, "and is optional")
        val doc = source("src/model/SetupText.kt")
        ok(doc.contains("nothing\n * of Key 2 or Key 3 but Key 3's public key"), "the format says so")
    }

    private fun oa_09_the_app_never_signs_a_spend_by_itself() {
        for (name in listOf("src/android/Controller.kt", "src/android/Screens.kt", "src/android/Approval.kt", "src/android/OwnerUi.kt", "src/android/MainActivity.kt", "src/model/OwnerModel.kt", "src/model/ApprovalText.kt")) {
            val src = source(name)
            for (bad in listOf(".signBatch(", ".queueSettings(", "SignedSettings", "Psbt", "psbt", "sign(", ".broadcast(", "Transaction(")) {
                ok(!src.contains(bad), "$name does not call or build $bad")
            }
        }
        val screens = source("src/android/Screens.kt") + source("src/android/Approval.kt")
        ok(screens.contains("n.buildRequest(") && screens.contains("n.approveMerge(") && screens.contains("n.take(") && screens.contains("n.queueRefund(") && screens.contains("n.cashOut("), "the calls that lead to a spend are the queue, the request and the response")
        // The request is shown and saved before anything could be taken, and taking is an explicit press.
        val approval = source("src/android/Approval.kt")
        ok(approval.indexOf("AppWords.requestReady") < approval.indexOf("source.begin("), "the request comes before the response")
        ok(AppWords.requestReady.what.contains("nothing has been signed yet"), "the words say nothing is signed")
        ok(source("src/android/Screens.kt").contains("It never holds Key 2 or Key 3, and it never signs a spend by itself"), "the About screen says so")
    }

    private fun oa_10_every_word_a_person_reads_is_the_librarys_or_checked() {
        // The words of the app are checked in wordsTests; here, that the screens show the library's words and write none of their own.
        val screens = source("src/android/Screens.kt") + source("src/android/Approval.kt")
        for (field in listOf("health.words", "receipts.words", "m.words", "r.stage.words", "c.stage.words", "o.stage.words", "a.words", "note.words", "row.words", "o.words", "s.words", "it.words")) {
            ok(screens.contains(field), "the screens show the library's $field")
        }
        ok(AppWords.all().all { SayCheck.problem(it.second) == null }, "every fixed words of the app pass the checks")
        ok(AppWords.responseNotReadable("x").tone == Tones.STOP && AppWords.requestReady.tone == Tones.GOOD, "the tones")
        ok(File(appDir, "../../docs/screens/owner-app.md").isFile, "the list of the app's words is in docs/screens")
    }

    private fun oa_11_a_setup_is_whole_or_it_is_asked_for_again() {
        ok(!SetupStatus(setup = true, config = false).complete && !SetupStatus(setup = false, config = true).complete, "half a setup is not a setup")
        ok(ScreenModel.pick(null, SetupStatus(true, false), null, false) is Screen.FirstStart, "half a setup asks again")
        val controller = source("src/android/Controller.kt")
        val save = controller.substringAfter("SealedFile(dir, KeystoreSealer(AppFiles.KEY_ALIAS), AppFiles.SETUP_FILE).save(")
        ok(save.indexOf("configFile.save(") > 0, "the config is written after the setup")
        ok(controller.contains("AppWords.storageUnavailable(e.message)") && controller.contains("AppWords.setupNotReadable(e.message)"), "a file that does not read is a fault with words")
        ok(controller.contains("OwnerSetup.parse(String(text, Charsets.UTF_8))"), "the setup is parsed at every start")
    }

    private fun oa_12_three_threads_touch_the_node() {
        val controller = source("src/android/Controller.kt")
        ok(Regex("n\\.pump\\(").findAll(controller).count() == 1, "one place pumps")
        ok(Regex("n\\.linkStep\\(").findAll(controller).count() == 1 && Regex("n\\.deliver\\(").findAll(controller).count() == 1, "one place takes the link's turns")
        val pump = controller.substringAfter("val pump = Thread {").substringBefore("pump.isDaemon")
        ok(pump.contains("n.pump(nowSecs(), 250)"), "the pump waits at most 250 ms")
        val link = controller.substringAfter("val link = Thread {").substringBefore("link.isDaemon")
        ok(link.contains("n.linkStep(now)") && link.contains("n.deliver(now)") && link.contains("LINK_EVERY_MS"), "the link thread takes its turns every few seconds")
        ok(controller.contains("if (setup?.link != null)"), "and only when there is a link")
        ok(Controller_LINK_EVERY_MS_is_5_seconds(controller), "every 5 seconds")
        for (name in listOf("src/android/Screens.kt", "src/android/Approval.kt")) {
            val src = source(name)
            ok(!src.contains(".pump(") && !src.contains(".linkStep(") && !src.contains(".deliver(") && !src.contains("Thread"), "$name makes no call of the other threads and starts no thread")
        }
        ok(controller.contains("synchronized(startLock)"), "starts do not overlap")
    }

    private fun Controller_LINK_EVERY_MS_is_5_seconds(controller: String) = controller.contains("LINK_EVERY_MS = 5_000L")

    private fun oa_13_a_request_and_a_response_are_exactly_their_bytes() {
        val rnd = SecureRandom()
        for (n in 1..200) {
            val bytes = ByteArray(n).also { rnd.nextBytes(it) }
            ok(ApprovalText.decode(ApprovalText.encode(bytes)).contentEquals(bytes), "$n bytes")
        }
        ok(ApprovalText.encode(ByteArray(100)).lines().first().length == 64, "64 digits to a line")
        ok(ApprovalText.MAX_BYTES == 1_000_000, "the most is 1,000,000 bytes")
        throws("a response over the most") { ApprovalText.decode("ab".repeat(ApprovalText.MAX_BYTES + 1)) }
        val controller = source("src/android/Controller.kt")
        ok(controller.contains("ApprovalText.MAX_BYTES + 1") && controller.contains("bytes.size > ApprovalText.MAX_BYTES"), "a file longer than the most is refused before the library sees it")
        ok(controller.contains("PastedResponse(pasted)"), "the pasted text is one source")
        ok(controller.contains("override val label = \"Choose the response file\""), "a file is another")
        val approval = source("src/android/Approval.kt")
        ok(approval.contains("for (source in responseSources(") && approval.contains("source.begin({ bytes -> takeResponse(n, bytes, msg) }"), "the response is taken through the sources, from a press")
        ok(approval.contains("ApprovalText.encode(built.request)") && approval.contains("saveFile(name, built.request"), "the text and the file are of the request as it is")
        ok(Regex("n\\.take\\(").findAll(approval).count() == 1, "one place takes a response")
        var failed: SayWords? = null
        PastedResponse { "zz" }.begin({ }, { failed = it })
        ok(failed != null, "a source that cannot give bytes says why")
    }

    private fun oa_14_the_settings_and_the_outbox_are_shown_and_not_changed() {
        val screens = source("src/android/Screens.kt")
        val settings = screens.substringAfter("internal fun Controller.showSettings()").substringBefore("internal fun Controller.showAbout()")
        ok(settings.contains("n.settings()") && !settings.contains("ui.field(") && !settings.contains("numberField(") && !settings.contains("n.queue"), "the settings screen reads and has no input")
        ok(settings.contains("read-only view"), "and says so")
        val outbox = screens.substringAfter("internal fun Controller.showOutbox()").substringBefore("// ---- the queue")
        ok(outbox.contains("n.outbox()") && !outbox.contains("ui.field(") && !outbox.contains("n.cancel(") && !outbox.contains("n.requeue("), "the outbox screen reads and changes nothing")
        val s = sampleSettings()
        ok(OwnerModel.settingsLines(s) == OwnerModel.settingsLines(s), "a fixed order")
        ok(OwnerModel.settingsLines(s).size == 21, "every field of the settings has a line: ${OwnerModel.settingsLines(s).size}")
        val noTime = OwnerModel.outboxLines(OutboxRow(1, OutboxKind.Pairing, null, null, null, null, 10, words()), 5)
        ok(noTime.contains("The node keeps no signing time for this one."), "an object with no time says so")
        ok(AppWords.noSettingsYet.who == Who.OWNER && SayCheck.problem(AppWords.noSettingsYet) == null, "no settings yet has words")
    }

    private fun oa_15_the_app_opens_no_connection_of_its_own() {
        for (name in listOf("src/android/Controller.kt", "src/android/Screens.kt", "src/android/Approval.kt", "src/android/OwnerUi.kt", "src/android/MainActivity.kt", "src/model/SetupText.kt", "src/model/AppConfig.kt", "src/model/ApprovalText.kt", "src/model/OwnerModel.kt", "src/model/AppWords.kt")) {
            val src = source(name)
            for (bad in listOf("HttpURLConnection", "HttpsURLConnection", "java.net.URL", "URLConnection", "OkHttp", "Socket(", "DatagramSocket", "InetAddress", "WebView", "DownloadManager")) {
                ok(!src.contains(bad), "$name does not use $bad")
            }
        }
        val gate = source("src/model/TorGate.kt")
        ok(Regex("Socket\\(").findAll(gate).count() == 1 && gate.contains("write(byteArrayOf(5, 1, 0))"), "the one socket is the greeting to the proxy")
        ok(!gate.contains("URL"), "no URL in the gate")
    }

    // ---- the rules and their tests -----------------------------------------------------------------------------

    /** Rules that have no test of this app, each with the reason. Every other rule has a test named for it. */
    private val notHere: Map<String, String> = emptyMap()

    private fun ruleTests(rulesMd: File?) {
        val methods = AppModelTest::class.java.declaredMethods.filter { it.name.startsWith("oa_") && !it.name.contains('$') }.sortedBy { it.name }
        for (m in methods) {
            m.isAccessible = true
            if (m.parameterCount != 0) throw AssertionError("test method ${m.name} has parameters")
            m.invoke(AppModelTest)
        }
        ok(methods.size >= 15, "the rules' tests ran: ${methods.size}")
        val ids = methods.map { it.name.substring(3, 5) }
        ok(ids.size == ids.toSet().size, "one test for each number")
        if (rulesMd == null) return
        val rules = Regex("^- \\*\\*OA-(\\d\\d)\\.\\*\\*", RegexOption.MULTILINE).findAll(rulesMd.readText()).map { it.groupValues[1] }.toList()
        ok(rules == (1..rules.size).map { "%02d".format(it) }, "the rules are numbered from 01 without a gap: $rules")
        for (id in rules) {
            ok(id in ids || "OA-$id" in notHere, "OA-$id has a test named oa_${id}_... or a reason")
        }
        for (id in ids) ok(id in rules, "oa_${id}_... tests a rule that is in docs/OWNER-APP-RULES.md")
        for (line in rulesMd.readLines()) ok(line.length <= 80 || line.startsWith("|"), "a line of the rules is at most 80 characters: ${line.take(40)}")
        val text = rulesMd.readText()
        ok(text.contains("## 4. What these rules do not do"), "the rules say what they do not do")
        ok(text.lines().any { it.startsWith("Status:") }, "the status line")
    }

    @JvmStatic
    fun main(args: Array<String>) {
        val screensMd = args.firstOrNull()?.let { File(it) }
        val rulesMd = args.getOrNull(1)?.let { File(it) }
        appDir = File((rulesMd ?: File("docs/OWNER-APP-RULES.md").absoluteFile).absoluteFile.parentFile.parentFile, "apps/owner-android")
        wordsTests(screensMd)
        conversionTests()
        setupTests()
        configTests()
        approvalTextTests()
        screenTests()
        modelTests()
        formTests()
        ruleTests(rulesMd)
        println("AppModelTest (owner): $count checks passed")
    }
}
