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
import org.openbmt.app.UrWords
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
import org.openbmt.owner.PosAlert
import org.openbmt.owner.PosAlertKey
import org.openbmt.owner.PosAlertKind
import org.openbmt.owner.PosMethod
import org.openbmt.owner.PosReply
import org.openbmt.owner.PosRequest
import org.openbmt.owner.PosState
import org.openbmt.owner.PosSubject
import org.openbmt.owner.PosVendor
import org.openbmt.owner.Quoted
import org.openbmt.owner.Route
import org.openbmt.owner.SendKind
import org.openbmt.owner.SendRow
import org.openbmt.owner.SettingsHeld
import org.openbmt.owner.WaitingCounter
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
        for (name in listOf("src/android/Controller.kt", "src/android/Screens.kt", "src/android/Approval.kt", "src/android/AddCounter.kt", "src/android/OwnerUi.kt", "src/android/MainActivity.kt", "src/model/OwnerModel.kt", "src/model/CounterModel.kt", "src/model/ApprovalText.kt")) {
            val src = source(name)
            for (bad in listOf(".signBatch(", ".queueSettings(", "SignedSettings", "Psbt", "psbt", "sign(", ".broadcast(", "Transaction(")) {
                ok(!src.contains(bad), "$name does not call or build $bad")
            }
        }
        // The one way the app causes a settings change and a batch to be queued is the node's own call that adds a counter.
        val add = source("src/android/AddCounter.kt")
        ok(Regex("n\\.counterConfirm\\(").findAll(add).count() == 1 && add.indexOf("ui.button(\"The four words match\")") < add.indexOf("n.counterConfirm("), "the settings and the batch are queued by one call, after the owner says the words match")
        ok(!source("src/android/Screens.kt").contains("counterConfirm") && !source("src/android/Controller.kt").contains("counterConfirm"), "no other screen calls it")
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

    private fun oa_12_four_threads_touch_the_node() {
        val controller = source("src/android/Controller.kt")
        ok(Regex("n\\.pump\\(").findAll(controller).count() == 1, "one place pumps")
        ok(Regex("n\\.linkStep\\(").findAll(controller).count() == 1 && Regex("n\\.deliver\\(").findAll(controller).count() == 1, "one place takes the link's turns")
        val pump = controller.substringAfter("val pump = Thread {").substringBefore("pump.isDaemon")
        ok(pump.contains("n.pump(nowSecs(), 250)"), "the pump waits at most 250 ms")
        val link = controller.substringAfter("val link = Thread {").substringBefore("link.isDaemon")
        ok(link.contains("n.linkStep(now)") && link.contains("n.deliver(now)") && link.contains("LINK_EVERY_MS"), "the link thread takes its turns every few seconds")
        ok(controller.contains("if (setup?.link != null)"), "and only when there is a link")
        ok(Controller_LINK_EVERY_MS_is_5_seconds(controller), "every 5 seconds")
        for (name in listOf("src/android/Screens.kt", "src/android/Approval.kt", "src/android/PosScreen.kt")) {
            val src = source(name)
            ok(!src.contains(".pump(") && !src.contains(".linkStep(") && !src.contains(".deliver(") && !src.contains("Thread"), "$name makes no call of the other threads and starts no thread")
        }
        ok(controller.contains("synchronized(startLock)"), "starts do not overlap")
        ok(Regex("n\\.posRequest\\(").findAll(controller).count() == 1 && Regex("n\\.posReply\\(").findAll(controller).count() == 1, "one place makes the two calls of a read of the till")
        ok(controller.contains("val till = Thread {"), "the till has a thread of its own")
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
        ok(controller.contains("override val label = \"Scan the Key app's code\""), "the camera is a third")
        val approval = source("src/android/Approval.kt")
        ok(approval.contains("responseSources(") && approval.contains("source.begin({ bytes -> takeResponse(n, bytes, msg) })"), "the response is taken through the sources, from a press")
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

    private fun oa_15_the_app_opens_no_connection_of_its_own_but_the_proxys_and_the_tills() {
        for (name in listOf("src/android/Controller.kt", "src/android/Screens.kt", "src/android/Approval.kt", "src/android/OwnerUi.kt", "src/android/MainActivity.kt", "src/model/SetupText.kt", "src/model/AppConfig.kt", "src/model/ApprovalText.kt", "src/model/OwnerModel.kt", "src/model/AppWords.kt", "src/android/PosScreen.kt", "src/model/PosModel.kt")) {
            val src = source(name)
            for (bad in listOf("HttpURLConnection", "HttpsURLConnection", "java.net.URL", "URLConnection", "OkHttp", "Socket(", "DatagramSocket", "InetAddress", "WebView", "DownloadManager")) {
                ok(!src.contains(bad), "$name does not use $bad")
            }
        }
        val gate = source("src/model/TorGate.kt")
        ok(Regex("Socket\\(").findAll(gate).count() == 1 && gate.contains("write(byteArrayOf(5, 1, 0))"), "the one socket is the greeting to the proxy")
        ok(!gate.contains("URL"), "no URL in the gate")
        // The till's fetcher is the one other place, and it opens one connection through the proxy of OA-05.
        val fetcher = source("src/android/HttpsPosFetcher.kt")
        ok(Regex("openConnection\\(").findAll(fetcher).count() == 1 && fetcher.contains("Proxy.Type.SOCKS"), "the fetcher opens one connection, through the proxy")
        for (name in listOf("src/android/Controller.kt", "src/android/Screens.kt", "src/android/Approval.kt", "src/android/MainActivity.kt")) {
            ok(!source(name).contains("openConnection"), "$name opens no connection")
        }
    }

    // ---- the till (OA-16 to OA-19) --------------------------------------------------------------------------

    private fun testSealer(): Sealer {
        val key = ByteArray(32).also { SecureRandom().nextBytes(it) }
        return object : Sealer {
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
    }

    private fun posRequest(url: String = "https://connect.squareup.com/v2/orders/search") = PosRequest(PosMethod.Post, url, ByteArray(0))

    private fun posAlert() = PosAlert(PosAlertKey(PosAlertKind.BitcoinTip, PosSubject.Tender("O", "T")), 5, words())

    private fun posReply(again: Boolean, alerts: Int = 0, moreLeft: Boolean = false) =
        PosReply(again, moreLeft, 1, 100, 0, List(alerts) { posAlert() }, 0, 0)

    /** A link that answers from a script, and remembers what it was asked. */
    private class ScriptedLink(private val replies: ArrayDeque<Any>) : PosLink {
        var requests = 0
        val answers = ArrayList<Triple<Long, Long, Int>>()

        override fun request(now: Long): PosRequest {
            requests++
            return PosRequest(PosMethod.Post, "https://connect.squareup.com/v2/orders/search", ByteArray(0))
        }

        override fun reply(now: Long, status: Long, body: ByteArray): PosReply {
            answers.add(Triple(now, status, body.size))
            return when (val next = replies.removeFirst()) {
                is OwnerException -> throw next
                is PosReply -> next
                else -> throw IllegalStateException("a script of replies")
            }
        }
    }

    private fun oa_16_the_till_token_is_sealed_in_its_own_file_and_never_shown() {
        ok(PosToken.clean("  EAAAabc-DEF_123.~+/=  ") == "EAAAabc-DEF_123.~+/=", "a token is trimmed")
        ok(PosToken.clean("") == null && PosToken.clean("   ") == null, "an empty token is refused")
        ok(PosToken.clean("two words") == null && PosToken.clean("tab\tin") == null && PosToken.clean("new\nline") == null, "a token with a space or a control character is refused")
        ok(PosToken.clean("a".repeat(512)) != null && PosToken.clean("a".repeat(513)) == null, "at most 512 characters")
        ok(PosToken.clean("café") == null, "and printable ASCII only")
        ok((PosForms.token("x y") as? Form.Refused)?.words == AppWords.posTokenNotAccepted, "the form says why")
        val dir = tempDir()
        val store = PosTokenStore(dir, testSealer())
        ok(!store.exists() && store.load() == null, "a phone with no till holds no token")
        store.save("SECRET-token-123")
        val onDisk = String(File(dir, AppFiles.POS_TOKEN_FILE).readBytes(), Charsets.ISO_8859_1)
        ok(!onDisk.contains("SECRET"), "the token is not on the disk in the clear")
        ok(dir.listFiles()!!.map { it.name } == listOf(AppFiles.POS_TOKEN_FILE), "one file and no temporary file")
        ok(store.exists() && store.load() == "SECRET-token-123", "it reads back")
        store.remove()
        ok(!store.exists() && store.load() == null && dir.listFiles()!!.isEmpty(), "taking the till off removes the file")
        store.remove()
        clean(dir)
        ok(AppFiles.POS_KEY_ALIAS != AppFiles.KEY_ALIAS && AppFiles.POS_TOKEN_FILE != AppFiles.SETUP_FILE, "its own key and its own file")
        val controller = source("src/android/Controller.kt")
        ok(controller.contains("PosTokenStore(dir, KeystoreSealer(AppFiles.POS_KEY_ALIAS))") && controller.contains("private val dir: File = act.noBackupFilesDir"), "sealed under its own Keystore key, in the folder that is not backed up")
        // Read for one read only, by the one place that reads, and never put on a screen or in a log.
        for (name in listOf("src/android/Screens.kt", "src/android/Approval.kt", "src/android/PosScreen.kt", "src/android/MainActivity.kt", "src/android/OwnerUi.kt")) {
            ok(!source(name).contains("posTokens.load("), "$name does not read the token")
        }
        ok(Regex("posTokens\\.load\\(").findAll(controller).count() == 1, "one place reads the token")
        val screen = source("src/android/PosScreen.kt")
        ok(Regex("secretField\\(").findAll(screen).count() == 2 && !screen.contains("ui.field(\"Read-only") && !screen.contains("ui.field(\"New read-only"), "both token fields hide what is typed")
        ok(source("src/android/OwnerUi.kt").contains("TYPE_TEXT_VARIATION_PASSWORD"), "a secret field is a password field")
        ok(Regex("forget\\(token\\)").findAll(screen).count() == 2 && screen.contains("clearPrimaryClip()"), "the field and the clipboard are wiped once the token is taken")
        for (name in listOf("src/android/PosScreen.kt", "src/android/HttpsPosFetcher.kt", "src/model/PosModel.kt", "src/android/Controller.kt")) {
            val src = source(name)
            for (bad in listOf("SharedPreferences", "writeText", "writeBytes", "FileOutputStream", "android.util.Log", "Log.d(", "Log.i(", "Log.e(", "println(", "printStackTrace")) {
                ok(!src.contains(bad), "$name does not use $bad")
            }
        }
        ok(!Regex("ui\\.text\\([^)]*token", RegexOption.IGNORE_CASE).containsMatchIn(screen), "no text on the till screen holds the token")
        ok(AppWords.posConnected.what.contains("not shown again"), "the words say it is not shown again")
    }

    private fun oa_17_the_till_is_read_through_tor_from_two_known_systems() {
        for (good in listOf("https://connect.squareup.com/v2/orders/search", "https://api.clover.com/v3/merchants/ABC123/orders?limit=100&expand=payments", "https://connect.squareup.com/v2/payments?begin_time=2026-11-10T00:00:00Z&location_id=LOC1&sort_order=ASC&limit=100&cursor=PAY%3D%3D")) {
            ok(PosUrl.allowed(good), "allowed: $good")
        }
        for (bad in listOf(
            "http://connect.squareup.com/v2/orders/search", "https://connect.squareup.com", "https://connect.squareup.com?x=1",
            "https://connect.squareup.com:444/v2", "https://connect.squareup.com@evil.example/v2", "https://evil.example/v2",
            "https://connect.squareup.com.evil.example/v2", "https://user:pw@api.clover.com/v3", "https://api.clover.com/v3 x",
            "https://api.clover.com/v3\n", "https://api.clover.com/é", "ftp://api.clover.com/v3", "", "//api.clover.com/v3",
        )) {
            ok(!PosUrl.allowed(bad), "refused: ${bad.take(40)}")
        }
        ok(PosLimits.HOSTS == setOf("connect.squareup.com", "api.clover.com"), "two systems and no other")
        ok(PosLimits.MAX_PAGE_BYTES == 8 * 1024 * 1024, "a page is at most 8 MiB")
        ok(PosLimits.TIMEOUT_MS == 20_000 && PosLimits.DEADLINE_MS == 60_000L, "the timeouts")
        ok(readCapped(java.io.ByteArrayInputStream(ByteArray(10)), 10)!!.size == 10, "a page of exactly the most is read")
        ok(readCapped(java.io.ByteArrayInputStream(ByteArray(11)), 10) == null, "a page of one byte more is refused whole")
        ok(readCapped(java.io.ByteArrayInputStream(ByteArray(0)), 10)!!.isEmpty(), "an empty page")
        throws("a page that takes too long") { readCapped(java.io.ByteArrayInputStream(ByteArray(5)), 10) { true } }
        // The loop of one read, with the library and the network scripted.
        val fetched = ArrayList<Pair<String, String>>()
        val link = ScriptedLink(ArrayDeque(listOf(posReply(again = true, alerts = 1), posReply(again = false, alerts = 2))))
        val done = PosPoll.readOnce(link, { r, t -> fetched.add(r.url to t); PosFetch.Reply(200, ByteArray(7)) }, "TOK", { 42L })
        ok(done is PosOutcome.Done && done.newAlerts == 3 && !done.moreLeft && link.requests == 2, "two pages, the alerts of both counted")
        ok(fetched.size == 2 && fetched.all { it.second == "TOK" } && link.answers == listOf(Triple(42L, 200L, 7), Triple(42L, 200L, 7)), "each answer is handed back with its status and its bytes")
        // A Square page with orders is followed by its payments, each a GET with no body that is fetched like the rest.
        val payLink = object : PosLink {
            val sent = ArrayList<PosRequest>()
            var step = 0

            override fun request(now: Long): PosRequest {
                val r = if (step == 1) PosRequest(PosMethod.Get, "https://connect.squareup.com/v2/payments?location_id=LOC1&limit=100", ByteArray(0)) else posRequest()
                sent.add(r)
                return r
            }

            override fun reply(now: Long, status: Long, body: ByteArray): PosReply = posReply(again = ++step < 2)
        }
        val paid = PosPoll.readOnce(payLink, { _, _ -> PosFetch.Reply(200, ByteArray(3)) }, "TOK", { 42L })
        ok(paid is PosOutcome.Done && payLink.sent.map { it.method } == listOf(PosMethod.Post, PosMethod.Get) && payLink.sent[1].body.isEmpty(), "the orders, then the payments, both fetched")
        fun failed(fetch: PosFetch): SayWords? = (PosPoll.readOnce(ScriptedLink(ArrayDeque(listOf(posReply(false)))), { _, _ -> fetch }, "T", { 1L }) as? PosOutcome.Failed)?.words
        ok(failed(PosFetch.TorMissing) == AppWords.orbotNotRunning, "no Orbot: nothing is read, and it says so")
        ok(failed(PosFetch.TooLong) == AppWords.posPageTooLong, "a page that is too long")
        ok(failed(PosFetch.NotReached("SocketTimeoutException")) == AppWords.posNotReached("SocketTimeoutException"), "a till that does not answer")
        // A refusal of the library, with its own words, ends the read.
        val refusing = ScriptedLink(ArrayDeque(listOf(OwnerException("OC-33", "the sign-in was refused", 1, words(Tones.STOP)))))
        val refused = PosPoll.readOnce(refusing, { _, _ -> PosFetch.Reply(401, ByteArray(0)) }, "T", { 1L })
        ok(refused is PosOutcome.Failed && refused.words.heading == "Heading", "a 401 is shown with the library's words")
        // An address that is not a till's is never fetched.
        var asked = false
        val strange = object : PosLink {
            override fun request(now: Long) = posRequest("https://evil.example/x")

            override fun reply(now: Long, status: Long, body: ByteArray): PosReply = posReply(false)
        }
        val blocked = PosPoll.readOnce(strange, { _, _ -> asked = true; PosFetch.Reply(200, ByteArray(0)) }, "T", { 1L })
        ok(!asked && blocked is PosOutcome.Failed && blocked.words == AppWords.posAddressRefused, "an address the library should not give is refused")
        // A library that always wants more is stopped.
        val endless = object : PosLink {
            var n = 0

            override fun request(now: Long): PosRequest {
                n++
                return posRequest()
            }

            override fun reply(now: Long, status: Long, body: ByteArray): PosReply = posReply(true)
        }
        val cut = PosPoll.readOnce(endless, { _, _ -> PosFetch.Reply(200, ByteArray(0)) }, "T", { 1L })
        ok(cut is PosOutcome.Failed && cut.words == AppWords.posReadCutOff && endless.n == PosLimits.MAX_STEPS, "a read ends after the most pages")
        val fetcher = source("src/android/HttpsPosFetcher.kt")
        ok(fetcher.indexOf("PosUrl.allowed(request.url)") in 1 until fetcher.indexOf("gate.available()") && fetcher.indexOf("gate.available()") in 1 until fetcher.indexOf("openConnection(proxy)"), "the address is checked, then Orbot, before anything is opened")
        ok(fetcher.contains("Proxy(Proxy.Type.SOCKS, InetSocketAddress(hp.first, hp.second))") && !fetcher.contains("Proxy.NO_PROXY"), "through the SOCKS proxy of Orbot and never around it")
        ok(Regex("openConnection\\(").findAll(fetcher).count() == 1, "one connection")
        ok(fetcher.contains("instanceFollowRedirects = false") && fetcher.contains("useCaches = false") && fetcher.contains("\"Accept-Encoding\", \"identity\""), "no redirect, no cache, no compression")
        ok(fetcher.contains("connectTimeout = PosLimits.TIMEOUT_MS") && fetcher.contains("readTimeout = PosLimits.TIMEOUT_MS") && fetcher.contains("PosLimits.DEADLINE_MS") && fetcher.contains("PosLimits.MAX_PAGE_BYTES"), "timeouts and the cap of a page")
        ok(fetcher.contains("setRequestProperty(\"Authorization\", \"Bearer \$token\")") && Regex("\\\$token").findAll(fetcher).count() == 1, "the sign-in is set as a header here and used nowhere else")
        ok(fetcher.contains("e.javaClass.simpleName") && !fetcher.contains("e.message"), "a failure says its kind and never its message")
        ok(fetcher.contains("if (code == 200)"), "only an answer of 200 has its body read")
        ok(fetcher.contains("conn.requestMethod = if (request.method == PosMethod.Post) \"POST\" else \"GET\""), "only the method the library asked for")
        for (name in listOf("src/android/Controller.kt", "src/android/Screens.kt", "src/android/Approval.kt", "src/android/PosScreen.kt", "src/model/PosModel.kt")) {
            ok(!source(name).contains("Authorization"), "$name sets no header")
        }
        val screen = source("src/android/PosScreen.kt")
        ok(!screen.contains("n.queue") && !screen.contains("n.cashOut") && !screen.contains("n.queueRefund"), "the till screen reads orders and queues no payment and no refund")
    }

    private fun oa_18_the_till_is_read_while_visible_at_most_once_a_minute() {
        ok(PosLimits.MIN_POLL_MS == 60_000L, "a minute")
        ok(PosSchedule.waitMs(null, 5_000) == 0L, "a first read may start at once")
        ok(PosSchedule.waitMs(1_000, 1_000 + 59_999) == 1L && PosSchedule.waitMs(1_000, 1_000 + 60_000) == 0L && PosSchedule.waitMs(1_000, 500_000) == 0L, "a minute after the start of the last")
        ok(PosSchedule.waitMs(100_000, 10_000) == 60_000L, "a clock that went back waits no more than a minute")
        val controller = source("src/android/Controller.kt")
        ok(Regex("n\\.posRequest\\(").findAll(controller).count() == 1 && Regex("n\\.posReply\\(").findAll(controller).count() == 1, "one place makes the two calls of a read")
        val till = controller.substringAfter("val till = Thread {").substringBefore("till.isDaemon")
        ok(till.contains("PosSchedule.waitMs(lastStart, System.currentTimeMillis())") && till.indexOf("PosSchedule.waitMs(") < till.indexOf("readTill(link, gate)"), "the pace is worked out before every read")
        ok(till.contains("lastStart = System.currentTimeMillis()") && till.contains("posWake.poll(") && till.contains("continue"), "a call for a read now waits out the rest of the minute")
        ok(till.contains("!posConnected"), "nothing is read when no till is connected")
        val adopt = controller.substringAfter("private fun adopt(").substringBefore("private fun readTill(")
        ok(adopt.contains("if (setup?.link != null)") && adopt.contains("posThread = till"), "the thread runs only with a link, and only while the node runs")
        val stop = controller.substringAfter("private fun stopNode()").substringBefore("// ---- pages")
        ok(stop.contains("posThread?.interrupt()") && stop.indexOf("posThread?.interrupt()") < stop.indexOf("it.stop()"), "it is ended before the node is stopped")
        ok(stop.contains("posThread = null"), "and let go of")
        ok(controller.contains("PosPoll.readOnce(link, HttpsPosFetcher(gate), token)") && controller.contains("TorGate(config?.proxy ?: AppConfig.DEFAULT_PROXY)"), "through Orbot's address")
        for (name in listOf("src/android/Screens.kt", "src/android/Approval.kt", "src/android/PosScreen.kt")) {
            val src = source(name)
            ok(!src.contains(".posRequest(") && !src.contains(".posReply(") && !src.contains("Thread"), "$name makes no call of the till thread and starts no thread")
        }
        ok(source("src/android/PosScreen.kt").contains("readTillNow()") && controller.contains("posWake.offer(true)"), "a button asks, and the thread decides")
        ok(controller.contains("POS_IDLE_MS = 5_000L"), "an idle thread looks again every 5 seconds")
    }

    private fun oa_19_the_till_screen_shows_what_the_library_says_and_acknowledges_on_the_phone() {
        val setup = PosForms.setup(1, " MERCH1 ", "Bitcoin\n Lightning ,Bitcoin", "usd EUR, gbp")
        ok(setup.vendor == PosVendor.Clover && setup.location == "MERCH1", "the vendor and the location as typed, trimmed")
        ok(setup.tenderNames == listOf("Bitcoin", "Lightning", "Bitcoin") && setup.currencies == listOf("usd", "EUR", "gbp"), "the lists are split and nothing is judged here")
        ok(PosForms.setup(0, "L", "x", "USD").vendor == PosVendor.Square, "Square is the first choice")
        ok(PosForms.list(" , \n ").isEmpty(), "empty entries are left out")
        val state = PosState(setup, 100, listOf(posAlert()), 1, 4, 3, 1, 0)
        ok(PosLines.lastRead(160, 100) == "Last read 60 seconds ago.", "the time of the last read: ${PosLines.lastRead(160, 100)}")
        ok(PosLines.lastRead(160, null) == "Not read yet since the app was opened.", "or that there was none")
        ok(PosLines.counts(state) == "3 sales matched. 1 of them by order of time, not exactly. 1 alert.", "the matches count and the alerts: ${PosLines.counts(state)}")
        ok(PosLines.counts(PosState(setup, null, emptyList(), 0, 0, 1, 0, 0)) == "1 sale matched. No alerts.", "one match, none by order")
        ok(PosLines.till(setup) == "Clover, MERCH1. Bitcoin tender: Bitcoin, Lightning, Bitcoin. Currencies: usd, EUR, gbp.", "the till in a line")
        val screen = source("src/android/PosScreen.kt")
        ok(screen.contains("n.posState()") && screen.contains("a.words.say()"), "the alerts are shown with the library's words")
        ok(screen.contains("PosLines.lastRead(nowSecs(), s.lastPollAt)") && screen.contains("PosLines.counts(s)"), "the last read and the matches count")
        ok(screen.contains("\"Acknowledge this alert\"") && screen.contains("n.posAcknowledge(nowSecs(), a.key)"), "an alert can be acknowledged")
        ok(!screen.contains("queueSettings") && !screen.contains("signBatch") && !screen.contains("n.clearAlert("), "acknowledging sends no signed command")
        ok(AppWords.posAcknowledged.what.contains("on this phone only") && AppWords.posAcknowledged.what.contains("do not learn"), "and says that it stays on the phone")
        ok(screen.contains("n.posSetup(args)") && screen.indexOf("n.posSetup(args)") < screen.indexOf("sealToken(t, n, rollback = true)"), "the library judges the till before the token is sealed")
        ok(screen.contains("rollback") && screen.contains("n.posRemove()"), "a token that cannot be sealed takes the till off again")
        ok(source("src/android/Screens.kt").contains("ui.button(\"Till\") { showTill() }"), "the home screen has the button")
        ok(source("src/android/Screens.kt").contains("It has not been tried against a real till account yet."), "the About screen says it has not been tried")
        for (w in listOf(AppWords.posNotConnected, AppWords.posNeedsLink, AppWords.posTokenNotAccepted, AppWords.posConnected, AppWords.posRemoved, AppWords.posAcknowledged, AppWords.posTokenMissing, AppWords.posAddressRefused, AppWords.posPageTooLong, AppWords.posReadCutOff, AppWords.posNotReached("x"))) {
            ok(SayCheck.problem(w) == null && (w.tone != Tones.STOP || w.who != Who.NOBODY), "every till state has words that pass the checks: ${w.heading}")
        }
    }

    // ---- the approval by camera and screen (OA-20, OA-21) -----------------------------------------------------

    private fun exchangeTests() {
        ok(ExchangeModel.requestActions.map { it.label } == listOf("Show as code", "Save the request as a file", "Copy the request as text", "Bring back the response"), "the request screen offers the code first")
        ok(ExchangeModel.responseKinds == listOf(ResponseKind.Scan, ResponseKind.File, ResponseKind.Pasted), "the response screen offers the camera first")
        ok(ExchangeModel.TYPE_BYTES == "bytes" && ExchangeModel.RESPONSE_TYPES == setOf("bytes") && ExchangeModel.WHO == Who.OWNER, "ur:bytes, and the owner does the next step")
        ok(ExchangeModel.codeHeight(2000) == 1200 && ExchangeModel.codeHeight(0) == 0, "the code is 60% of the screen's height")
        val still = ExchangeModel.codeStep(made = true, animated = false)
        val moving = ExchangeModel.codeStep(made = true, animated = true)
        val none = ExchangeModel.codeStep(made = false, animated = true)
        ok(still.shown && still.done && still.words === AppWords.requestCodeStill, "one still code: Done is offered")
        ok(moving.shown && moving.done && moving.words === AppWords.requestCodeAnimated, "a code that changes: Done is offered")
        ok(!none.shown && !none.done && none.words === AppWords.requestCodeNotMade, "no code: no Done, and the words say what to do")
        for (w in listOf(still.words, moving.words)) {
            ok(w.next.contains("On the Key app, choose Scan and point it at this code"), "the words say what to do next: ${w.next}")
            ok(w.who == Who.OWNER && w.tone == Tones.INFO, "the owner, as information")
        }
        ok(none.words.tone == Tones.STOP && none.words.next.contains("file") && none.words.next.contains("text"), "no code says to use a file or text")
        ok(ExchangeStep.Response(null).message == null && ExchangeStep.Response(AppWords.scanCancelled).message === AppWords.scanCancelled, "the response screen holds what the last try said")

        val bytes = ByteArray(40) { it.toByte() }
        val given = ScanResult.message(bytes)
        ok(given is SourceResult.Bytes && given.bytes.contentEquals(bytes), "a complete code gives its payload as it is")
        ok(ScanResult.message(ByteArray(ApprovalText.MAX_BYTES)) is SourceResult.Bytes, "the most is accepted")
        val long = ScanResult.message(ByteArray(ApprovalText.MAX_BYTES + 1))
        ok(long is SourceResult.Failed && long.words.heading == "Response text not read" && SayCheck.problem(long.words) == null, "a longer payload is refused before the library sees it")
        val plain = ScanResult.plain()
        ok(plain is SourceResult.Failed && plain.words === AppWords.scanNotAResponse, "a plain code is not the Key app's")
        val cancelled = ScanResult.cancelled()
        ok(cancelled is SourceResult.Failed && cancelled.words === AppWords.scanCancelled, "a cancelled scan says so")
        val off = ScanResult.failed(off = true, problem = null, say = null)
        ok(off is SourceResult.Failed && off.words.heading == "Camera is off for this app" && off.words.who == Who.OWNER, "a denied permission says the camera is off")
        val notStarted = ScanResult.failed(off = false, problem = "no camera", say = null)
        ok(notStarted is SourceResult.Failed && notStarted.words.heading == "Camera did not start" && notStarted.words.detail == "no camera", "a camera that did not start says so, with the system's text")
        val wrong = UrWords.wrongKind(Who.OWNER)
        val said = ScanResult.failed(off = false, problem = null, say = wrong)
        ok(said is SourceResult.Failed && said.words === wrong, "the reading's own words win: a wrong type of code")
        val ofOther = ScanResult.failed(off = true, problem = null, say = UrWords.contentRefused(Who.OWNER))
        ok(ofOther is SourceResult.Failed && ofOther.words.heading == "Code content not usable", "and so a content that was refused")
        for (r in listOf(plain, cancelled, off, notStarted, said, long)) {
            val w = (r as SourceResult.Failed).words
            ok(SayCheck.problem(w) == null && w.next.isNotEmpty() && w.what.isNotEmpty(), "every end of a scan has checked words that say what to do: ${w.heading}")
        }
        val headings = AppWords.all().map { it.second.heading }
        ok(listOf("Camera is off for this app", "Camera did not start", "Reading the animated code", "Not the code expected", "Code content not usable", "Code reader failed").all { it in headings }, "the shared words are in the list of the app's words")
        ok(object : ResponseSource {
            override val label = "x"
            override fun begin(onBytes: (ByteArray) -> Unit, onFailed: (SayWords) -> Unit) {}
        }.leavesScreen == false, "a source stays on its screen unless it says it leaves")
    }

    private fun oa_20_the_request_is_shown_as_a_code_first() {
        val approval = source("src/android/Approval.kt")
        val screen = approval.substringAfter("internal fun Controller.showBuilt(").substringBefore("private fun Controller.showBuiltCode(")
        ok(screen.contains("for (action in ExchangeModel.requestActions)") && screen.contains("RequestAction.ShowCode -> showBuiltCode(built, back)"), "the request screen offers the buttons of the model, in its order")
        ok(screen.contains("ApprovalText.encode(built.request)") && screen.contains("saveFile(name, built.request"), "the file and the text are the request as it is")
        val code = approval.substringAfter("private fun Controller.showBuiltCode(").substringBefore("private fun Controller.showResponse(")
        ok(code.contains("AnimatedQrView.make(") && code.contains("ExchangeModel.WHO, ExchangeModel.TYPE_BYTES, built.request"), "the code is the request, as ur:bytes")
        ok(code.contains("ExchangeModel.codeHeight(act.resources.displayMetrics.heightPixels)"), "at the Key app's size")
        ok(code.contains("ExchangeModel.codeStep(") && code.contains("if (step.done) views.add(ui.button(\"Done\")"), "Done only with a code on the screen")
        ok(code.contains("showResponse(built, back, ExchangeStep.Response(null))"), "Done leads to the response")
        val key = File(appDir, "../key-android/src/android/Controller.kt")
        if (key.isFile) {
            val k = key.readText()
            ok(k.contains("const val TYPE_BYTES = \"bytes\"") && k.contains("heightPixels * ${ExchangeModel.CODE_HEIGHT_SHARE})"), "the Key app draws its answer with the same type and share of the screen")
        }
        ok(ApprovalText.fileName("request", ByteArray(32)).endsWith(".obm"), "a file is still offered")
    }

    private fun oa_21_the_response_is_scanned_from_the_key_apps_code_and_judged_in_rust() {
        val approval = source("src/android/Approval.kt")
        val response = approval.substringAfter("private fun Controller.showResponse(").substringBefore("private fun Controller.takeResponse(")
        ok(response.contains("ExchangeModel.responseKinds.zip(sources)") && response.contains("source.leavesScreen"), "the sources are offered in the model's order, and a scan comes back to this screen with its words")
        ok(Regex("n\\.take\\(").findAll(approval).count() == 1 && approval.contains("takeResponse(n, bytes, msg)"), "every source's bytes go to the one place that takes a response")
        val controller = source("src/android/Controller.kt")
        val scan = controller.substringAfter("private fun scanSource()").substringBefore("internal fun permission(")
        ok(scan.contains("scan.begin(") && scan.contains("UrScan(ExchangeModel.RESPONSE_TYPES, ExchangeModel.WHO)"), "ScanFlow reads ur:bytes parts for the owner")
        ok(scan.contains("ScanResult.message(payload)") && scan.contains("ScanResult.plain()") && scan.contains("ScanResult.cancelled()") && scan.contains("ScanResult.failed(f.off, f.problem, f.words)"), "every end of the scan goes through the model")
        ok(!scan.contains("n.take(") && !scan.contains(".take("), "the scan judges nothing itself")
        ok(Regex("scan\\.begin\\(").findAll(controller).count() == 1 && !controller.contains("requestPermissions"), "the camera is asked for in one place, from the scan button")
        ok(!source("src/android/MainActivity.kt").substringAfter("override fun onCreate").substringBefore("override fun onStart").contains("ermission"), "nothing asks for the camera when the app opens")
        ok(source("src/android/MainActivity.kt").contains("controller.permission(requestCode, grantResults)"), "the permission's answer goes to the scan")
        val hidden = controller.substringAfter("fun hidden()").substringBefore("internal fun begin()")
        ok(hidden.contains("scan.asking") && hidden.contains("scan.stop()"), "a scan stops when the app leaves the screen, but not for its own permission dialog")
        val build = source("build.sh")
        ok(build.contains("qr-jni-build.sh") && build.contains("libopenbmt_qr_jni.so"), "the build carries the QR library the camera and the code use")
    }


    // ---- adding a counter (OA-22 to OA-25) ---------------------------------------------------------------------

    private fun counterTests() {
        ok(CounterModel.explanation.size == 2, "two lines say what the steps are")
        ok(CounterModel.explanation[0].contains("Scan the counter's code") && CounterModel.explanation[0].contains("approve on the Key app"), "the first line: scan, then approve")
        ok(CounterModel.explanation[1].contains("show the counter a code") && CounterModel.explanation[1].contains("Paired"), "the second line: show the counter a code")
        ok(CounterModel.REQUEST_TYPES == setOf("bytes") && CounterModel.MAX_REQUEST_BYTES == 1_024, "ur:bytes and a small request")
        ok(CounterModel.wordsLine(listOf("one", "two", "three", "four")) == "one   two   three   four", "the four words in one line")
        ok(CounterModel.percent(10_000) == "100%" && CounterModel.percent(5_000) == "50%" && CounterModel.percent(3_334) == "33.34%" && CounterModel.percent(3_333) == "33.33%" && CounterModel.percent(500) == "5%" && CounterModel.percent(5) == "0.05%", "a share as a percent")
        val shares = listOf(CounterShare(ByteArray(32) { 1 }, 5_000), CounterShare(ByteArray(32) { 2 }, 5_000))
        ok(CounterModel.shareLines(shares) == listOf("Counter 01010101: 50%", "Counter 02020202: 50%"), "a line for each counter's share, in order")
        ok(CounterModel.nameOf("Front ‮counter") == Format.cleanForeign("Front ‮counter") && !CounterModel.nameOf("Front ‮counter").contains('‮'), "a counter's name is foreign text and is cleaned")
        ok(CounterModel.nameOf("") == "a counter without a name" && CounterModel.nameOf(" \n ") == "a counter without a name", "a name that cleans to nothing is said so")
        ok(CounterModel.windowMinutes(600) == 10L && CounterModel.windowMinutes(30) == 1L && CounterModel.windowMinutes(0) == 1L, "the minutes of the window, never zero")
        ok(CounterModel.windowLine(600).contains("10 minutes") && CounterModel.windowLine(60).contains("1 minute "), "the window in words")
        ok(CounterModel.batchLine(1_000).contains("1000 addresses"), "the first batch's size")
        ok(CounterModel.nextLine.contains("approve there") && CounterModel.nextLine.contains("response"), "what comes next")

        val request = ByteArray(120) { it.toByte() }
        val given = CounterModel.requestResult(request)
        ok(given is SourceResult.Bytes && given.bytes.contentEquals(request), "a counter's code gives its payload as it is")
        ok(CounterModel.requestResult(ByteArray(CounterModel.MAX_REQUEST_BYTES)) is SourceResult.Bytes, "the most is accepted")
        val long = CounterModel.requestResult(ByteArray(CounterModel.MAX_REQUEST_BYTES + 1))
        ok(long is SourceResult.Failed && long.words === AppWords.counterCodeNotRequest, "a longer code is refused before the library sees it")
        ok(CounterModel.rate("1000") is Form.Ok && (CounterModel.rate("1,000") as Form.Ok).value == 1_000L, "a rate is a whole number")
        for (bad in listOf("", "0", "  ", "ten", "-5", "1.5", "1".repeat(19))) {
            val f = CounterModel.rate(bad)
            ok(f is Form.Refused && f.words === AppWords.counterRateNeeded, "a rate of '$bad' is refused with words")
        }

        val still = CounterModel.codeStep(made = true, withBatch = true, animated = false)
        val moving = CounterModel.codeStep(made = true, withBatch = true, animated = true)
        val none = CounterModel.codeStep(made = false, withBatch = true, animated = true)
        val bare = CounterModel.codeStep(made = true, withBatch = false, animated = true)
        ok(still.shown && still.done && still.words === AppWords.counterCodeStill, "one still code")
        ok(moving.shown && moving.done && moving.words === AppWords.counterCodeAnimated, "a code that changes")
        ok(bare.shown && bare.done && bare.words === AppWords.counterCodeNoBatch, "a code without the first addresses says so")
        ok(!none.shown && !none.done && none.words === AppWords.counterCodeNotMade, "no code: no button that says Paired")
        for (w in listOf(still.words, bare.words)) {
            ok(w.next == "Scan this with the counter. It shows Paired when it has taken it." && w.who == Who.OWNER && w.tone == Tones.INFO, "the words say what to do next: ${w.next}")
        }
        ok(
            moving.words.what.contains("about a minute") && moving.words.next.contains("steady") &&
                moving.words.next.contains("Paired") && moving.words.who == Who.OWNER && moving.words.tone == Tones.INFO,
            "a code that changes says how long it takes and to hold steady: ${moving.words.what} ${moving.words.next}",
        )
        ok(none.words.tone == Tones.STOP && none.words.what.contains("kept"), "no code says what waits is kept")

        val waiting = listOf(WaitingCounter(ByteArray(32) { 1 }, "Front"), WaitingCounter(ByteArray(32) { 2 }, "Back"))
        ok(CounterModel.waitingFor(waiting, ByteArray(32) { 2 })?.name == "Back" && CounterModel.waitingFor(waiting, ByteArray(32) { 3 }) == null, "a waiting counter is found by its identity")
        ok(CounterModel.showCodeLabel(waiting[0]) == "Show the code for Front", "the button names the counter")

        val two = AppWords.sharesNote(2)
        ok(two.what == "Each counter's share of the instant limit becomes 1/2." && two.next.contains("Custom shares come later") && two.next.contains("settings editor"), "the shares are equal, and custom shares come later")
        ok(AppWords.sharesNote(3).what.endsWith("1/3."), "the share follows the number of counters")
        ok(AppWords.sharesNote(1).what.contains("only counter"), "one counter has the whole limit")
        ok(AppWords.counterCompare.what.contains("Check that the counter shows these same four words"), "the owner is told to compare the words")
        ok(AppWords.counterDelivered.tone == Tones.GOOD && AppWords.counterNotPaired.tone == Tones.STOP, "tones of the ends")
    }


    private fun oa_22_a_counter_is_read_by_the_camera_and_judged_by_the_node() {
        val add = source("src/android/AddCounter.kt")
        ok(source("src/android/Screens.kt").contains("ui.button(\"Add a counter\") { showAddCounter() }"), "adding a counter starts on the home screen")
        val first = add.substringAfter("internal fun Controller.showAddCounter(").substringBefore("private fun Controller.scanCounter()")
        ok(first.contains("for (line in CounterModel.explanation)") && first.contains("ui.button(\"Scan the counter's code\") { scanCounter() }"), "the explanation, then the button that opens the camera")
        ok(Regex("scan\\.begin\\(").findAll(add).count() == 1 && !add.contains("requestPermissions"), "the camera is asked for in one place, from that button")
        val scan = add.substringAfter("private fun Controller.scanCounter()").substringBefore("private fun Controller.readCounter(")
        ok(scan.contains("UrScan(CounterModel.REQUEST_TYPES, ExchangeModel.WHO)") && scan.contains("CounterModel.requestResult(payload)"), "ur:bytes only, and the length is the model's")
        ok(scan.contains("AppWords.counterNotACode") && scan.contains("AppWords.counterScanCancelled") && scan.contains("ScanResult.failed(f.off, f.problem, f.words)"), "every other end of the scan has words")
        val read = add.substringAfter("private fun Controller.readCounter(").substringBefore("private fun Controller.showCounterCheck(")
        ok(read.contains("AppWords.counterReading") && read.indexOf("page(") < read.indexOf("main.post") && read.indexOf("main.post") < read.indexOf("n.counterScan(bytes)"), "a page says it takes a moment, and is drawn before the call")
        ok(!read.contains("async(") && !read.contains("Thread"), "the call stays on the screen thread")
        ok(read.contains("e.sayWords()"), "a refusal of the node is shown with its words")
        ok(Regex("n\\.counterScan\\(").findAll(add).count() == 1, "one place gives the code to the node")
        for (w in listOf(AppWords.counterNotACode, AppWords.counterScanCancelled, AppWords.counterCodeNotRequest, AppWords.counterReading)) {
            ok(w.who == Who.OWNER && SayCheck.problem(w) == null && w.next.isNotEmpty(), "words: ${w.heading}")
        }
        ok(AppWords.counterNotACode.next.contains("Pair"), "a code that is not a counter's says what to scan")
        ok(source("AndroidManifest.xml").contains("android.permission.CAMERA"), "the camera is declared")
    }

    private fun oa_23_the_owner_compares_the_words_and_confirms_the_settings_and_batch() {
        val add = source("src/android/AddCounter.kt")
        val check = add.substringAfter("private fun Controller.showCounterCheck(").substringBefore("private fun Controller.showCounterQueued(")
        ok(check.contains("CounterModel.nameOf(s.name)") && check.contains("CounterModel.wordsLine(s.sessionWords), 30f, bold = true"), "the name, cleaned, and the four words large")
        ok(check.contains("AppWords.counterCompare") && check.contains("AppWords.sharesNote(s.shares.size)") && check.contains("CounterModel.shareLines(s.shares)") && check.contains("CounterModel.windowLine(s.windowSecs)"), "the words to compare, the shares and the window")
        ok(check.contains("if (s.needsRate)") && check.contains("CounterModel.rate(rate.text.toString())"), "a phone with no settings asks for the rate, and the model checks it")
        ok(check.indexOf("is Form.Refused -> msg.show(") < check.indexOf("n.counterConfirm("), "nothing is queued before the rate is accepted")
        ok(check.contains("ui.button(\"The four words match\")") && check.contains("ui.button(\"The words differ\")") && check.contains("n.cancel(s.pairing)"), "the two answers; a difference takes the request out")
        ok(Regex("n\\.counterConfirm\\(").findAll(check).count() == 1 && !check.contains("n.buildRequest("), "only the first answer confirms, and it builds no request")
        val queued = add.substringAfter("private fun Controller.showCounterQueued(").substringBefore("internal fun Controller.showCounterCode(")
        ok(queued.contains("q.words.say()") && queued.contains("CounterModel.shareLines(q.shares)") && queued.contains("CounterModel.batchLine(q.batchCount)"), "what the node queued is shown with its words")
        ok(queued.contains("showSpend()"), "the approval goes on from the spend screen's request")
        ok(AppWords.sharesNote(2).next == "Custom shares come later, with the settings editor.", "the shares are equal and the words say custom ones come later")
        ok(AppWords.counterCompare.what.contains("Key app shows them too"), "the words are compared on the Key app too")
        ok(AppWords.counterRateNeeded.next.contains("whole number above 0"), "the rate's words say what to type")
        ok(source("src/android/Screens.kt").contains("changing the shop's settings, with shares of the instant limit that are not equal"), "the About screen no longer lists adding a counter as not built")
        ok(!source("src/android/Screens.kt").substringAfter("label(\"Not built yet\")").contains("adding a counter"), "and does not say it is not built")
    }

    private fun oa_24_the_code_for_the_counter_is_the_bundle_the_node_made() {
        val add = source("src/android/AddCounter.kt")
        val code = add.substringAfter("internal fun Controller.showCounterCode(").substringBefore("internal fun Controller.waitingCounterViews(")
        ok(code.contains("n.counterBundle(w.counter)") && code.contains("AnimatedQrView.make(") && code.contains("ExchangeModel.WHO, ExchangeModel.TYPE_BYTES, made.code"), "the code is the bundle, as ur:bytes")
        ok(code.contains("ExchangeModel.codeHeight(act.resources.displayMetrics.heightPixels)"), "at the size of the other codes")
        ok(code.contains("CounterModel.codeStep(view != null, made.withBatch, view?.animated == true)") && code.contains("if (step.done) {"), "the Paired button only with a code on the screen")
        ok(
            AppWords.counterCodeAnimated.next.startsWith("Scan this with the counter") && AppWords.counterCodeAnimated.next.contains("Paired") &&
                AppWords.counterCodeAnimated.what.contains("about a minute"),
            "the words say what to do next and how long it takes",
        )
        val approval = source("src/android/Approval.kt")
        ok(approval.contains("waitingCounterViews(n, Slot(act)) { showHome() }") && approval.substringAfter("private fun Controller.showTaken(").contains("waitingCounterViews("), "a counter approved by the response offers its code")
        val outbox = source("src/android/Screens.kt").substringAfter("internal fun Controller.showOutbox()").substringBefore("// ---- the queue")
        ok(outbox.contains("waitingCounterViews(n, Slot(act)) { showOutbox() }"), "the list of what waits offers it too")
        val rows = add.substringAfter("internal fun Controller.waitingCounterViews(")
        ok(rows.contains("n.countersWaiting()") && rows.contains("CounterModel.showCodeLabel(w)"), "a button for each waiting counter")
        ok(!approval.contains("counterDelivered") && !approval.contains("counterBundle"), "taking a response marks nothing and makes no code itself")
        ok(AppWords.counterNotPaired.what.contains("did not approve") && SayCheck.problem(AppWords.counterNotPaired) == null, "a declined counter has words")
        ok(AppWords.counterCodeNoBatch.what.contains("first addresses are not in this code"), "a code without the first addresses says so")
    }

    private fun oa_25_only_the_owners_word_marks_a_counter_delivered() {
        val add = source("src/android/AddCounter.kt")
        val code = add.substringAfter("internal fun Controller.showCounterCode(").substringBefore("internal fun Controller.waitingCounterViews(")
        ok(Regex("n\\.counterDelivered\\(").findAll(add).count() == 1 && code.contains("ui.button(\"The counter shows Paired\")"), "one call, behind the button that says so")
        ok(code.indexOf("ui.button(\"The counter shows Paired\")") < code.indexOf("n.counterDelivered("), "the call is made by that press")
        ok(code.contains("ui.button(\"Show again\") { showCounterCode(w, back) }") && code.contains("ui.button(\"Back\") { back() }"), "show again, and leaving keeps it waiting")
        ok(!code.contains("File(") && !code.contains("writeBytes") && !code.contains("copy("), "the app keeps no copy of the code")
        ok(AppWords.counterDelivered.what.contains("marked delivered"), "the words say what was done")
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
        ok(text.contains("## 7. What these rules do not do"), "the rules say what they do not do")
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
        exchangeTests()
        counterTests()
        ruleTests(rulesMd)
        println("AppModelTest (owner): $count checks passed")
    }
}
