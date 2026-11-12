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
import org.openbmt.app.ControlMessage
import org.openbmt.app.ControlReader
import org.openbmt.app.Format
import org.openbmt.app.KeyTextOutcome
import org.openbmt.app.SayCheck
import org.openbmt.app.SayWords
import org.openbmt.app.Sealed
import org.openbmt.app.SealedFile
import org.openbmt.app.Relock
import org.openbmt.app.Sealer
import org.openbmt.app.Tones
import org.openbmt.app.TorControl
import org.openbmt.app.Tries
import org.openbmt.app.TriesFile
import org.openbmt.app.TriesRecord
import org.openbmt.app.UrWords
import org.openbmt.app.Who
import org.openbmt.app.WipeOrder
import org.openbmt.owner.AgreedPrice
import org.openbmt.owner.AgreedPriceIn
import org.openbmt.owner.Alert
import org.openbmt.owner.AlertKind
import org.openbmt.owner.BatchWaiting
import org.openbmt.owner.Books
import org.openbmt.owner.BooksDay
import org.openbmt.owner.BooksFile
import org.openbmt.owner.CashAmount
import org.openbmt.owner.CounterShare
import org.openbmt.owner.CounterState
import org.openbmt.owner.EntryKind
import org.openbmt.owner.Limit
import org.openbmt.owner.LightningAction
import org.openbmt.owner.SwapAction
import org.openbmt.owner.LinkState
import org.openbmt.owner.OutboxKind
import org.openbmt.owner.OutboxRow
import org.openbmt.owner.OwnerException
import org.openbmt.owner.OwnerNetwork
import org.openbmt.owner.PairView
import org.openbmt.owner.PosAlert
import org.openbmt.owner.PosAlertKey
import org.openbmt.owner.PosAlertKind
import org.openbmt.owner.PosMethod
import org.openbmt.owner.PosReply
import org.openbmt.owner.PosRequest
import org.openbmt.owner.PosState
import org.openbmt.owner.PosSubject
import org.openbmt.owner.PosVendor
import org.openbmt.owner.PriceReply
import org.openbmt.owner.PriceSource
import org.openbmt.owner.Quoted
import org.openbmt.owner.Route
import org.openbmt.owner.SendKind
import org.openbmt.owner.SendRow
import org.openbmt.owner.SettingChange
import org.openbmt.owner.SettingsEditArgs
import org.openbmt.owner.SettingsHeld
import org.openbmt.owner.SettingsPreview
import org.openbmt.owner.SetupFields
import org.openbmt.owner.ShopCurrency
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

    /** A file of the Tor part shared with the counter app (`apps/android-common/tor`). */
    private fun torSource(path: String): String = File(appDir, "../android-common/tor/$path").readText()

    /** How many times [part] is in [text]. */
    private fun occurs(text: String, part: String): Int = text.split(part).size - 1

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
        // The words of a counter and of a Box for the price are the same words under two names (OA-101).
        val distinct = all.distinctBy { it.second.heading to it.second.what }
        ok(all.size - distinct.size == 2, "only the price words of the counter and of the Box read the same: ${all.size - distinct.size}")
        ok(SayCheck.distinctProblem(distinct) == null, "no two words read the same: ${SayCheck.distinctProblem(distinct)}")
        ok(AppWords.orbotNotRunning.what == "This app sends everything through Tor, but Orbot is not running.", "the Orbot words, as written down")
        ok(AppWords.orbotNotRunning.next == "Open Orbot and turn it on.", "the Orbot next step")
        ok(AppWords.orbotNotRunning.who == Who.OWNER, "the owner does it")
        ok(all.map { it.first }.toSet().size == all.size, "every name is used once")
        val dirty = AppWords.fileNotRead("bad\u0007 text‮")
        ok(dirty.detail == "bad text", "a system text is cleaned in the details: ${dirty.detail}")
        ok(SayCheck.problem(dirty) == null, "words with a cleaned detail pass")
        val text = AppWords.screensMarkdown()
        ok(text.lines().filter { it.startsWith("| ") }.size == all.size + 1 + FirstRunWords.all().size + 1, "one row for each words and the header, and for each of the first run's")
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
        // LK-67: the counters are the ones in the settings, so a link needs no counter line.
        val noCounters = OwnerSetup.parse(setupText(listOf("relay-route: direct 1.2.3.4:5", "relay-key: ${hex(32, 97)}"))).link
        ok(noCounters != null && noCounters.counters.isEmpty(), "a link with no counter lines")

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
        throws("a counter without a link") { OwnerSetup.parse(setupText(listOf("counter: ${hex(32, 129)}"))) }
        throws("the same counter twice") { OwnerSetup.parse(setupText(linkLines() + listOf("counter: ${hex(32, 129)}"))) }
        throws("a route of another kind") { OwnerSetup.parse(setupText(linkLines("carrier-pigeon 1.2.3.4:5"))) }
        throws("a route without a port") { OwnerSetup.parse(setupText(linkLines("direct 1.2.3.4"))) }
        throws("a text that is too long") { OwnerSetup.parse(setupText(listOf("peer: " + "a".repeat(OwnerSetup.MAX_TEXT)))) }
        throws("an empty value") { OwnerSetup.parse(setupText().replace("birthday: 800000", "birthday: ")) }
    }

    private fun configTests() {
        val c = AppConfig("127.0.0.1:9050", TorChoice.Orbot)
        ok(String(c.encode(), Charsets.UTF_8) == "proxy: 127.0.0.1:9050\ntor: orbot\n", "the config lines")
        ok(AppConfig.decode(c.encode()).proxy == "127.0.0.1:9050" && AppConfig.decode(c.encode()).tor == TorChoice.Orbot, "the config reads back")
        throws("a config without a line end") { AppConfig.decode("proxy: 1.2.3.4:5".toByteArray()) }
        throws("a config with a second line that is not the choice") { AppConfig.decode("proxy: 1.2.3.4:5\nx: y\n".toByteArray()) }
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
        ok(ScreenModel.pick(null, SetupStatus(false, false), null, false) is Screen.Welcome, "nothing saved: the welcome")
        ok(ScreenModel.pick(null, SetupStatus(true, false), null, false) is Screen.Welcome, "half saved: the welcome")
        ok(ScreenModel.pick(null, SetupStatus(false, false), null, false, resuming = true) is Screen.FirstStart, "a ceremony to resume: the setup")
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
        val link = LinkState(true, 0, listOf(Alert(AlertKind.Diverged, ByteArray(32), 1, words())), listOf(counter), emptyList())
        ok(OwnerModel.linkLine(null, 0) == "No link to the counters.", "no link")
        ok(OwnerModel.linkLine(link, 1) == "The relay is reachable. 1 counter is on the list.", "a link")
        ok(OwnerModel.linkLine(LinkState(false, 0, emptyList(), emptyList(), emptyList()), 0) == "The relay is not reachable. No counter is on the list.", "a link that does not answer")

        val batch = OutboxRow(8, OutboxKind.Batch, null, ByteArray(32) { 1 }, null, 1000, 300, words())
        val bl = OwnerModel.outboxLines(batch, 1300)
        ok(OwnerModel.outboxTitle(batch) == "Address batch 8" && bl[0] == "Counter 01010101" && bl[1].endsWith("5 minutes ago.") && bl[2] == "Size: 300 bytes.", "a batch waits: $bl")
        val waiting = BatchWaiting(ByteArray(32) { 1 }, 2, 1000, words())
        val wl = OwnerModel.batchWaitingLines(waiting, 1300)
        ok(wl[0] == "Counter 01010101: its new addresses are on their way." && wl[1].endsWith("5 minutes ago."), "new addresses wait: $wl")
        val tx = OutboxRow(9, OutboxKind.Transaction, "ab12", null, null, null, 200, words())
        val tl = OwnerModel.outboxLines(tx, 1300)
        ok(tl[0] == "Transaction ab12" && tl[1] == "The node keeps no signing time for this one.", "a payment has no time: $tl")
        ok(OwnerModel.outboxTitle(OutboxRow(1, OutboxKind.Settings, null, null, 4, null, 1, words())) == "Settings 1", "a settings title")
        ok(OwnerModel.entryTitle(EntryKind.SettingsChange, 3) == "Settings change 3", "an entry title")

        val s = sampleSettings()
        val sl = OwnerModel.settingsLines(s)
        ok(sl.map { it.first }.toSet().size == sl.size, "every label once")
        ok(sl[0] == ("Version" to "3") && sl[1] == ("Currency" to "US dollar (USD)") && sl[2] == ("Largest instant sale, light check" to "$7 (7,000 sats)"), "the first lines: ${sl.take(3)}")
        val euro = OwnerModel.settingsLines(sampleSettings(currency = ShopCurrency.Eur)).toMap()
        ok(euro["Currency"] == "Euro (EUR)" && euro["Largest instant sale, light check"] == "7 euros (7,000 sats)", "a euro shop: the currency and a limit in euros")
        val sats = OwnerModel.settingsLines(sampleSettings(currency = ShopCurrency.Sats)).toMap()
        ok(sats["Currency"] == "Bitcoin Standard (sats)" && sats["Refunds per day"] == "7 sats", "a Bitcoin Standard shop: a limit in sats, and its satoshis are not said again")
        val byLabel = sl.toMap()
        ok(byLabel["Underpayment still counted"] == "1.05%" && byLabel["Tor"] == "on" && byLabel["Customers can join the Wi-Fi"] == "no", "percent and switches: $byLabel")
        ok(byLabel["Smallest sale a counter opens"] == "no floor" && byLabel["Boxes"] == "0" && byLabel["Devices that write records"] == "2", "floor, boxes, devices")
        ok(byLabel["Tips"] == "Off" && OwnerModel.settingsLines(sampleSettings(tipsOn = true)).toMap()["Tips"] == "10%, 15%, 20%", "the tips line")
        ok(byLabel["Counters and their shares"] == "Counter 02020202 100.00%", "the counters' shares: ${byLabel["Counters and their shares"]}")
        ok(byLabel["Blocks to settle a sale"] == "1" && byLabel["Blocks until a refund may be paid"] == "6", "the depths")
        ok(OwnerModel.settingsLines(sampleSettings(floor = 5000, counters = emptyList())).toMap()["Smallest sale a counter opens"] == "5,000 sats", "a floor")
    }

    private fun sampleSettings(
        floor: Long = 0,
        counters: List<CounterShare> = listOf(CounterShare(ByteArray(32) { 2 }, 10_000)),
        tipsOn: Boolean = false,
        tipPercents: List<Int> = listOf(10, 15, 20),
        currency: ShopCurrency = ShopCurrency.Usd,
    ): SettingsHeld {
        val l = Limit(7000, 7)
        return SettingsHeld(
            3, l, l, l, l, l, l, 105, l, l, l, l, 1, 6, 900, true, false, floor,
            tipsOn, tipPercents, listOf(ByteArray(32) { 1 }, ByteArray(32) { 2 }), counters, emptyList(), null, currency,
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

    private fun oa_03_the_manifest_asks_for_five_permissions_and_exports_one_activity() {
        val manifest = source("AndroidManifest.xml")
        val permissions = Regex("<uses-permission android:name=\"([^\"]+)\"").findAll(manifest).map { it.groupValues[1] }.toList()
        ok(
            permissions == listOf(
                "android.permission.INTERNET",
                "android.permission.CAMERA",
                "android.permission.FOREGROUND_SERVICE",
                "android.permission.FOREGROUND_SERVICE_SPECIAL_USE",
                "android.permission.POST_NOTIFICATIONS",
            ),
            "the permissions: $permissions",
        )
        ok(manifest.contains("android:name=\"android.hardware.camera.any\"") && manifest.contains("android:required=\"false\""), "the camera is a feature that is not required")
        ok(Regex("<uses-feature").findAll(manifest).count() == 1, "one feature")
        ok(Regex("<activity").findAll(manifest).count() == 1 && manifest.contains("android:exported=\"true\"") && manifest.contains("android.intent.category.LAUNCHER"), "one exported launcher activity")
        ok(Regex("<service").findAll(manifest).count() == 1, "one service (OA-89)")
        for (kind in listOf("<receiver", "<provider", "<activity-alias")) ok(!manifest.contains(kind), "no $kind")
        ok(manifest.contains("android:usesCleartextTraffic=\"false\"") && !manifest.contains("debuggable"), "cleartext off, not debuggable")
        ok(manifest.contains("package=\"org.openbmt.owner.app\""), "the package")
        val check = source("tests/check-manifest.sh")
        ok(check.contains("-eq 5 ]") && check.contains("camera.any") && check.contains("minSdkVersion:'29'") && check.contains("targetSdkVersion:'36'"), "the script checks the APK for the same")
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

    private fun oa_06_the_node_runs_while_the_app_is_visible_or_the_background_watch_is_on() {
        val activity = source("src/android/MainActivity.kt")
        ok(activity.substringAfter("override fun onStart").substringBefore("override fun onStop").contains("controller.visible()"), "onStart starts")
        ok(activity.substringAfter("override fun onStop").substringBefore("override fun onDestroy").contains("controller.hidden()"), "onStop stops")
        ok(activity.substringAfter("override fun onDestroy").contains("controller.hidden()"), "onDestroy stops")
        val controller = source("src/android/Controller.kt")
        val stop = controller.substringAfter("private fun stopNode()").substringBefore("// ---- pages")
        ok(stop.indexOf("linkThread") in 0 until stop.indexOf("it.stop()") && stop.indexOf("pumpThread") in 0 until stop.indexOf("it.stop()"), "the threads end before the node stops")
        ok(stop.contains("runId++"), "the loops are told to end")
        // Only the watch's own files use the service, the notification and the manager of them.
        for (name in listOf("src/android/Controller.kt", "src/android/Screens.kt", "src/android/Approval.kt", "src/android/MainActivity.kt", "src/android/OwnerUi.kt")) {
            val src = source(name)
            for (bad in listOf("startForeground", "NotificationManager", "NotificationChannel", "JobScheduler", "WorkManager", "AlarmManager", "WakeLock", "startService")) {
                ok(!src.contains(bad), "$name does not use $bad")
            }
        }
        ok(Regex("<service").findAll(source("AndroidManifest.xml")).count() == 1, "the one service is the watch's (OA-89); the built-in Tor is not a service (OA-97)")
        ok(source("src/android/Screens.kt").contains("unless Watch in the background is on") && source("src/android/Screens.kt").contains("It never starts by itself after the phone restarts"), "the About screen says when the wallet is followed")
        ok(source("README.md").contains("Watch in the background"), "the README says so")
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
        ok(Regex("n\\.counterConfirm\\(").findAll(add).count() == 1 && add.indexOf("ui.primary(\"The four words match\")") < add.indexOf("n.counterConfirm("), "the settings and the batch are queued by one call, after the owner says the words match")
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
        for (field in listOf("health.words", "r.words", "m.words", "r.stage.words", "c.stage.words", "o.stage.words", "a.words", "note.words", "row.words", "o.words", "s.words", "it.words")) {
            ok(screens.contains(field), "the screens show the library's $field")
        }
        ok(AppWords.all().all { SayCheck.problem(it.second) == null }, "every fixed words of the app pass the checks")
        ok(AppWords.responseNotReadable("x").tone == Tones.STOP && AppWords.requestReady.tone == Tones.GOOD, "the tones")
        ok(File(appDir, "../../docs/screens/owner-app.md").isFile, "the list of the app's words is in docs/screens")
    }

    private fun oa_11_a_setup_is_whole_or_it_is_asked_for_again() {
        ok(!SetupStatus(setup = true, config = false).complete && !SetupStatus(setup = false, config = true).complete, "half a setup is not a setup")
        ok(ScreenModel.pick(null, SetupStatus(true, false), null, false) is Screen.Welcome, "half a setup asks again, from the welcome")
        val controller = source("src/android/Controller.kt")
        val save = controller.substringAfter("SealedFile(dir, KeystoreSealer(AppFiles.KEY_ALIAS), AppFiles.SETUP_FILE).save(")
        ok(save.indexOf("configFile.save(") > 0, "the config is written after the setup")
        ok(controller.contains("AppWords.storageUnavailable(e.message)") && controller.contains("AppWords.setupNotReadable(e.message)"), "a file that does not read is a fault with words")
        ok(source("src/android/Lock.kt").contains("OwnerSetup.parse(String(text, Charsets.UTF_8))"), "the setup is parsed at every start (OA-60)")
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
        ok(settings.contains("read-only view") && settings.contains("Change settings"), "and says so, and leads to Change settings")
        val outbox = screens.substringAfter("internal fun Controller.showOutbox()").substringBefore("// ---- the queue")
        ok(outbox.contains("n.outbox()") && !outbox.contains("ui.field(") && !outbox.contains("n.cancel(") && !outbox.contains("n.requeue("), "the outbox screen reads and changes nothing")
        val s = sampleSettings()
        ok(OwnerModel.settingsLines(s) == OwnerModel.settingsLines(s), "a fixed order")
        ok(OwnerModel.settingsLines(s).size == 24, "every field of the settings has a line: ${OwnerModel.settingsLines(s).size}")
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
        // So is the price's fetcher (OA-101): one connection, through the same proxy.
        val rates = source("src/android/HttpsRateFetcher.kt")
        ok(Regex("openConnection\\(").findAll(rates).count() == 1 && rates.contains("Proxy.Type.SOCKS") && rates.contains("!gate.available()"), "the price's fetcher opens one connection, through the proxy, and asks the gate first")
        for (name in listOf("src/android/Controller.kt", "src/android/Screens.kt", "src/android/Approval.kt", "src/android/MainActivity.kt", "src/android/PriceFetch.kt", "src/model/PriceModel.kt")) {
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
        ok(till.contains("PosSchedule.waitMs(lastStart, System.currentTimeMillis())") && till.indexOf("PosSchedule.waitMs(") < till.indexOf("readTill(link, TorGate(proxyNow()))"), "the pace is worked out before every read")
        ok(till.contains("lastStart = System.currentTimeMillis()") && till.contains("posWake.poll(") && till.contains("continue"), "a call for a read now waits out the rest of the minute")
        ok(till.contains("!posConnected"), "nothing is read when no till is connected")
        val adopt = controller.substringAfter("private fun adopt(").substringBefore("private fun readTill(")
        ok(adopt.contains("if (setup?.link != null)") && adopt.contains("posThread = till"), "the thread runs only with a link, and only while the node runs")
        val stop = controller.substringAfter("private fun stopNode()").substringBefore("// ---- pages")
        ok(stop.contains("posThread?.interrupt()") && stop.indexOf("posThread?.interrupt()") < stop.indexOf("it.stop()"), "it is ended before the node is stopped")
        ok(stop.contains("posThread = null"), "and let go of")
        ok(controller.contains("PosPoll.readOnce(link, HttpsPosFetcher(gate), token)") && controller.contains("readTill(link, TorGate(proxyNow()))"), "through the proxy in use: Orbot's address or the built-in Tor's port")
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
        ok(source("src/android/Screens.kt").contains("RowItem(\"Till\") { showTill() }"), "a group page of home has the row")
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
        ok(code.contains("ExchangeModel.codeStep(") && code.contains("if (step.done) actions.add(ui.primary(\"Done\")"), "Done only with a code on the screen")
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
        ok(CounterModel.price("98500") is Form.Ok && (CounterModel.price("98,500") as Form.Ok).value == 98_500L && (CounterModel.price("$98,500") as Form.Ok).value == 98_500L, "a price is a whole number of dollars")
        for (bad in listOf("", "0", "  ", "ten", "-5", "1.5", "98,500.50", "1".repeat(19))) {
            val f = CounterModel.price(bad)
            ok(f is Form.Refused && f.words === AppWords.priceNotRead, "a price of '$bad' is refused with words")
        }
        ok(CounterModel.price("5") is Form.Ok, "a price that is too low is the library's to refuse, not the app's")

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
        ok(two.what == "Each counter's share of the instant limit becomes 1/2." && two.next.contains("Change settings"), "the shares are equal, and custom shares are set in Change settings")
        ok(AppWords.sharesNote(3).what.endsWith("1/3."), "the share follows the number of counters")
        ok(AppWords.sharesNote(1).what.contains("only counter"), "one counter has the whole limit")
        ok(AppWords.counterCompare.what.contains("Check that the counter shows these same four words"), "the owner is told to compare the words")
        ok(AppWords.counterDelivered.tone == Tones.GOOD && AppWords.counterNotPaired.tone == Tones.STOP, "tones of the ends")
    }


    private fun oa_22_a_counter_is_read_by_the_camera_and_judged_by_the_node() {
        val add = source("src/android/AddCounter.kt")
        ok(source("src/android/Screens.kt").contains("RowItem(\"Add a counter\") { showAddCounter() }"), "adding a counter starts on the home screen")
        val first = add.substringAfter("internal fun Controller.showAddCounter(").substringBefore("private fun Controller.scanCounter()")
        ok(first.contains("for (line in CounterModel.explanation)") && first.contains("ui.primary(\"Scan the counter's code\") { scanCounter() }"), "the explanation, then the button that opens the camera")
        ok(Regex("scan\\.begin\\(").findAll(add).count() == 1 && !add.contains("requestPermissions"), "the camera is asked for in one place, from that button")
        val scan = add.substringAfter("private fun Controller.scanCounter()").substringBefore("private fun Controller.readCounter(")
        ok(scan.contains("UrScan(CounterModel.REQUEST_TYPES, ExchangeModel.WHO)") && scan.contains("CounterModel.requestResult(payload)"), "ur:bytes only, and the length is the model's")
        ok(scan.contains("AppWords.counterNotACode") && scan.contains("AppWords.counterScanCancelled") && scan.contains("ScanResult.failed(f.off, f.problem, f.words)"), "every other end of the scan has words")
        val read = add.substringAfter("private fun Controller.readCounter(").substringBefore("private fun Controller.showCounterCheck(")
        ok(read.contains("AppWords.counterReading") && read.indexOf("waiting(") < read.indexOf("main.post") && read.indexOf("main.post") < read.indexOf("n.counterScan(bytes)"), "a page says it takes a moment, and is drawn before the call")
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
        ok(check.contains("CounterModel.nameOf(s.name)") && check.contains("wordCards(CounterModel.wordsLine(s.sessionWords))"), "the name, cleaned, and the four words as cards")
        ok(check.contains("AppWords.counterCompare") && check.contains("AppWords.sharesNote(s.shares.size)") && check.contains("CounterModel.shareLines(s.shares)") && check.contains("CounterModel.windowLine(s.windowSecs)"), "the words to compare, the shares and the window")
        ok(check.contains("if (s.needsRate)") && check.contains("CounterModel.price(price.text.toString())") && check.contains("Dollars for one bitcoin"), "a phone with no settings asks for the price of a bitcoin, and the model checks its shape")
        ok(check.indexOf("is Form.Refused -> msg.show(") < check.indexOf("n.counterConfirm("), "nothing is queued before the price is accepted")
        ok(check.contains("ui.primary(\"The four words match\")") && check.contains("ui.danger(\"The words differ\")") && check.contains("n.cancel(s.pairing)"), "the two answers; a difference takes the request out")
        ok(Regex("n\\.counterConfirm\\(").findAll(check).count() == 1 && !check.contains("n.buildRequest("), "only the first answer confirms, and it builds no request")
        val queued = add.substringAfter("private fun Controller.showCounterQueued(").substringBefore("internal fun Controller.showCounterCode(")
        ok(queued.contains("q.words.say()") && queued.contains("CounterModel.shareLines(q.shares)") && queued.contains("CounterModel.batchLine(q.batchCount)"), "what the node queued is shown with its words")
        ok(queued.contains("showSpend()"), "the approval goes on from the spend screen's request")
        ok(AppWords.sharesNote(2).next.contains("Change settings"), "the shares are equal and the words say where custom ones are set")
        ok(AppWords.counterCompare.what.contains("Key app shows them too"), "the words are compared on the Key app too")
        ok(AppWords.counterPriceNeeded.next.contains("price of one bitcoin in whole dollars") && !check.contains("Satoshis to a dollar"), "the price's words say what to type, and no satoshis are asked for")
        ok(!source("src/android/Screens.kt").contains("changing the shop's settings"), "the About screen no longer lists the settings editor as not built")
        ok(!source("src/android/Screens.kt").substringAfter("\"Not built yet\" to").contains("adding a counter"), "and does not say it is not built")
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
        ok(Regex("n\\.counterDelivered\\(").findAll(add).count() == 1 && code.contains("ui.primary(\"The counter shows Paired\")"), "one call, behind the button that says so")
        ok(code.indexOf("ui.primary(\"The counter shows Paired\")") < code.indexOf("n.counterDelivered("), "the call is made by that press")
        ok(code.contains("ui.button(\"Show again\") { showCounterCode(w, back) }") && code.contains("ui.button(\"Back\") { back() }"), "show again, and leaving keeps it waiting")
        ok(!code.contains("File(") && !code.contains("writeBytes") && !code.contains("copy("), "the app keeps no copy of the code")
        ok(AppWords.counterDelivered.what.contains("marked delivered"), "the words say what was done")
    }

    // ---- making a new shop wallet (OA-26 to OA-31) ---------------------------------------------------------------

    private fun fields(network: OwnerNetwork = OwnerNetwork.Regtest, key3: String? = "tpubKEY3") = SetupFields(
        network = network,
        ownerSecret = ByteArray(32) { (it + 1).toByte() },
        approverRoot = ByteArray(32) { (it + 33).toByte() },
        wallet = "wsh(sortedmulti(2,[a/48h/1h/0h/2h]tpub1/<0;1>/*,[b/48h/1h/0h/2h]tpub2/<0;1>/*,[c/48h/1h/0h/2h]tpub3/<0;1>/*))#abcdefgh",
        key1Account = ByteArray(78) { (it + 65).toByte() },
        key3Xpub = key3,
        batchMinCount = 1_000,
        refundDepth = 6,
        birthday = 967_680,
    )

    private fun ceremonyTests() {
        // The setup text of a finished ceremony is the text the paste path reads, to the same values.
        for (network in OwnerNetwork.values()) {
            for (key3 in listOf("tpubKEY3", null)) {
                val f = fields(network, key3)
                val text = OwnerSetup.format(f)
                val back = OwnerSetup.parse(text)
                ok(back.network == f.network && back.ownerSecret.contentEquals(f.ownerSecret) && back.approverRoot.contentEquals(f.approverRoot), "the network and the secrets round-trip: $network")
                ok(back.wallet == f.wallet && back.key1Account.contentEquals(f.key1Account) && back.key3Xpub == f.key3Xpub, "the wallet and the keys round-trip")
                ok(back.batchMinCount == 1_000L && back.refundDepth == 6L && back.birthday == 967_680L, "the numbers round-trip")
                ok(back.peers.isEmpty() && back.link == null, "a ceremony makes no peers and no link")
                ok(text.startsWith(OwnerSetup.HEADER + "\nnetwork: ") && text.endsWith("\n") && text.length < OwnerSetup.MAX_TEXT, "the header first, a line end last")
                ok(text.contains("key3-xpub: ") == (key3 != null), "Key 3's public key only when there is one")
                ok(!text.contains("seed") && !text.contains("mnemonic") && !text.contains("key2"), "no other key is in the text")
            }
        }
        ok(OwnerSetup.format(fields(OwnerNetwork.Mainnet)).lines()[1] == "network: mainnet", "the network's name")
        ok(OwnerSetup.parse(OwnerSetup.format(fields())).startConfig("/d", 1, "127.0.0.1:9050").network == OwnerNetwork.Regtest, "the node starts on the network of the text")

        // The build's one network, and the names of the files.
        ok(AppConfig.NETWORK == OwnerNetwork.Mainnet, "a wallet made on this phone is for mainnet, the build's one choice")
        ok(AppFiles.CEREMONY_FILE == "ceremony.obks" && AppFiles.CEREMONY_FILE != AppFiles.SETUP_FILE && AppFiles.CEREMONY_FILE != "config.txt", "the ceremony has a file of its own")

        // What the model writes and decides.
        ok(CeremonyModel.fingerprintLines(listOf("aabbccdd", "11223344", "55667788")) == listOf("Key 1: aabbccdd", "Key 2: 11223344", "Key 3: 55667788"), "a line for each fingerprint")
        val pairs = listOf(PairView(1, 2, 1, 2), PairView(1, 3, 1, 3), PairView(2, 3, 2, 3))
        ok(CeremonyModel.pairLines(pairs)[0] == "Keys 1 and 2: this phone and the Key app" && CeremonyModel.pairLines(pairs).size == 3, "a line for each pair and its holders")
        ok(listOf(1, 2, 3, 4).map { CeremonyModel.holder(it) } == listOf("this phone", "the Key app", "another signer", "another wallet app"), "the four holders")
        ok(CeremonyModel.holder(9) == "an unknown holder", "a holder this app does not know is said so")
        ok(CeremonyModel.wordsLine(listOf("a", "b", "c", "d")) == "a   b   c   d", "the words in one line")
        ok(CeremonyModel.ANSWER_TYPES == setOf("bytes") && CeremonyModel.MAX_ANSWER_BYTES == 1_024, "ur:bytes and a small answer")
        val answer = ByteArray(200) { it.toByte() }
        val given = CeremonyModel.answerResult(answer)
        ok(given is SourceResult.Bytes && given.bytes.contentEquals(answer), "a code gives its payload as it is")
        ok(CeremonyModel.answerResult(ByteArray(CeremonyModel.MAX_ANSWER_BYTES)) is SourceResult.Bytes, "the most is accepted")
        val long = CeremonyModel.answerResult(ByteArray(CeremonyModel.MAX_ANSWER_BYTES + 1))
        ok(long is SourceResult.Failed && long.words === AppWords.ceremonyCodeTooLong, "a longer code is refused before the library sees it")

        val text = "[8dfc9b34/48h/1h/0h/2h]tpubDEXiq2SVhhqALktxfVFgj3C9M3T2G7xL11iezYg2LJAf245YkNyq"
        ok((CeremonyModel.key2Text("  $text\n") as Form.Ok).value == text, "key 2's text is trimmed and passed as it is")
        ok((CeremonyModel.key2FromScan(text.toByteArray()) as Form.Ok).value == text, "a scanned plain code is the text")
        for (bad in listOf("", "   \n", "x".repeat(CeremonyModel.MAX_KEY2_TEXT + 1))) {
            val f = CeremonyModel.key2Text(bad)
            ok(f is Form.Refused && f.words === AppWords.ceremonyKey2Text, "a key 2 text of ${bad.length} characters is refused with words")
        }
        ok(CeremonyModel.key2FromScan(byteArrayOf(0xff.toByte(), 0x01)) is Form.Refused, "a code that is not text is not key 2")
        ok(CeremonyModel.key2Text("x".repeat(CeremonyModel.MAX_KEY2_TEXT)) is Form.Ok, "the longest is accepted")

        val still = CeremonyModel.setupCodeStep(made = true, animated = false)
        val moving = CeremonyModel.setupCodeStep(made = true, animated = true)
        val none = CeremonyModel.setupCodeStep(made = false, animated = true)
        ok(still.shown && still.done && still.words === AppWords.ceremonyCodeStill, "one still code")
        ok(moving.shown && moving.done && moving.words === AppWords.ceremonyCodeAnimated, "a code that changes")
        ok(!none.shown && !none.done && none.words === AppWords.ceremonyCodeNotMade, "no code: no button that says the words are the same")
        ok(CeremonyModel.resumeAction(1) == ResumeAction.ScanAnswer && CeremonyModel.resumeAction(2) == ResumeAction.ScanAnswerAgain && CeremonyModel.resumeAction(3) == ResumeAction.SaveSetup, "the stages and what each offers")
        ok(CeremonyModel.resumeAction(0) == null && CeremonyModel.resumeAction(4) == null, "a stage this app does not know offers nothing")
        ok(ResumeAction.values().all { CeremonyModel.resumeWords(it).who == Who.OWNER && SayCheck.problem(CeremonyModel.resumeWords(it)) == null }, "each resume has words")

        ok(AppWords.ceremonyScanNext.what == "Open the Key app on the other phone and choose Set up keys." && AppWords.ceremonyScanNext.next.contains("scan the code it shows"), "the words say what to do on the Key app")
        ok(AppWords.ceremonyStartAgain.what == "The keys made so far are thrown away, and nothing has been paid to them.", "the confirmation says what is thrown away")
        ok(AppWords.ceremonyWordsDiffer.what.contains("not finished") && AppWords.ceremonyWordsDiffer.what.contains("nothing should be paid") && AppWords.ceremonyWordsDiffer.next == "Choose Start again on both phones.", "the words that differ say it is not finished")
        ok(AppWords.ceremonyRootDiffers.what.contains("thrown away") && AppWords.ceremonyRootDiffers.tone == Tones.STOP, "the Key app's words differ: the answer is thrown away, and the words say so")
        ok(AppWords.ceremonyCompareSetup.what.contains("checksum words") && AppWords.ceremonyCompareSetup.what.contains("fingerprints"), "the owner is told to compare the checksum words and the fingerprints")
        ok(AppWords.ceremonyRecordNotRead("x").detail == "x" && AppWords.ceremonyRecordNotRead(null).detail == null, "a record that does not read carries the reason")
        ok(AppWords.notSetUp.next.contains("Make a new shop wallet") && AppWords.setupTextNeeded.next.contains("Paste the setup text"), "the two choices have words")
    }

    private fun ceremonyCode() = source("src/android/Ceremony.kt")

    private fun oa_26_the_first_start_offers_a_new_wallet_and_seals_the_record_at_each_step() {
        val controller = source("src/android/Controller.kt")
        val first = controller.substringAfter("internal fun showFirstStart(").substringBefore("/** The second choice")
        ok(first.indexOf("ui.primary(\"Make a new shop wallet\") { startCeremony() }") in 1 until first.indexOf("ui.button(\"Use setup text\") { showPasteSetup(null) }"), "a new wallet first, the paste path second")
        ok(controller.contains("saveSetup(draftText, { w -> showPasteSetup(w) })"), "the paste path saves as before")
        val c = ceremonyCode()
        ok(c.contains("Key1Args(AppConfig.NETWORK, showWords = false)"), "the build's network, and key 1 has no words to show")
        ok(!c.contains("choice(") && !c.contains("RadioGroup") && !c.contains("OwnerNetwork."), "no network picker")
        ok(c.contains("SealedFile(appDir, KeystoreSealer(AppFiles.KEY_ALIAS), AppFiles.CEREMONY_FILE)") && c.contains("ceremonyFile().save(record)"), "the record is sealed under the key of the setup, in its own file")
        ok(c.indexOf("keep(made.record) {") in 1 until c.indexOf("showKeyMade(made.stateWords.say())"), "key 1's record is saved before the next screen")
        ok(c.indexOf("keep(built.record) {") in 1 until c.indexOf("markKey2(built) { showWalletCheck(d, built) }"), "the wallet's record is saved before the next screen")
        ok(c.indexOf("keep(done.record) {") in 1 until c.indexOf("saveFinished(done.record)"), "the finished record is saved before the setup is")
        ok(Regex("keep\\(").findAll(c).count() == 5, "a record is kept in the three steps, and by one retry")
        ok(c.contains("is IOException -> notKept(") && c.contains("ui.primary(\"Try again\") { keep(record, next) }"), "a record that cannot be saved says so and can be tried again")
        for (bad in listOf("SharedPreferences", "openFileOutput", "writeText", "writeBytes", "FileOutputStream", "externalCacheDir", "cacheDir", "android.util.Log", "Log.d(", "println(", "printStackTrace", "putExtra")) {
            ok(!c.contains(bad), "Ceremony.kt does not use $bad")
        }
        ok(source("src/model/AppConfig.kt").contains("const val CEREMONY_FILE = \"ceremony.obks\""), "the file's name")
        ok(source("src/model/AppConfig.kt").contains("val NETWORK: OwnerNetwork = OwnerNetwork.Mainnet"), "one network")
    }

    private fun oa_27_an_unfinished_ceremony_is_resumed_at_its_stage_or_started_again_after_a_confirmation() {
        val controller = source("src/android/Controller.kt")
        val first = controller.substringAfter("internal fun showFirstStart(").substringBefore("/** The second choice")
        ok(first.contains("File(dir, AppFiles.CEREMONY_FILE).exists() && !File(dir, AppFiles.SETUP_FILE).exists()") && first.contains("showCeremonyResume(message)"), "a record and no setup resumes")
        val c = ceremonyCode()
        val resume = c.substringAfter("internal fun Controller.showCeremonyResume(").substringBefore("/** A record that does not read")
        ok(resume.contains("OwnerCeremony.stage(record)") && resume.contains("at.words.say()") && resume.contains("CeremonyModel.resumeAction(at.stage)"), "the library says the stage, and its words are shown")
        ok(resume.contains("ResumeAction.ScanAnswer, ResumeAction.ScanAnswerAgain -> scanAnswer()") && resume.contains("ResumeAction.SaveSetup -> saveFinished(record)"), "each stage goes on from there")
        ok(resume.contains("ui.button(\"Start again\") { showStartAgain() }"), "start again is offered")
        ok(c.contains("private fun Controller.notReadable(") && c.substringAfter("private fun Controller.notReadable(").substringBefore("/** The confirmation").contains("ui.button(\"Start again\")"), "a record that does not read can only be started again")
        val confirm = c.substringAfter("internal fun Controller.showStartAgain()").substringBefore("/** Deletes the ceremony's record.")
        ok(confirm.contains("AppWords.ceremonyStartAgain") && confirm.contains("ui.danger(\"Start again\") { throwAway() }") && confirm.contains("ui.primary(\"Keep going\") { begin() }"), "the confirmation, and the way to keep going")
        ok(!confirm.contains(".delete()"), "nothing is deleted before the confirmation")
        val away = c.substringAfter("private fun Controller.throwAway()").substringBefore("// ---- the second step")
        ok(away.contains("File(appDir, AppFiles.CEREMONY_FILE)") && Regex("\\.delete\\(\\)").findAll(away).count() == 1 && !away.contains(".SETUP_FILE") && !away.contains("noBackupFilesDir"), "the confirmation deletes the ceremony's file and nothing else")
        ok(Regex("showStartAgain\\(\\)").findAll(c).count() >= 12, "start again is on every screen of the ceremony")
        ok(controller.contains("File(dir, AppFiles.CEREMONY_FILE).delete()") && controller.indexOf("File(dir, AppFiles.CEREMONY_FILE).delete()") > controller.indexOf("if (!setupStatus.complete) return showFirstStart(null)"), "a record found beside a saved setup is deleted")
        ok(controller.contains("ceremony = null\n        stopNode()"), "the start drops what the screens held")
    }

    private fun oa_28_the_key_apps_answer_is_scanned_its_words_compared_and_key_2_asked_only_when_it_is_missing() {
        val c = ceremonyCode()
        val scan = c.substringAfter("private fun Controller.scanAnswer()").substringBefore("/** What the scan or the library said")
        ok(scan.contains("UrScan(CeremonyModel.ANSWER_TYPES, ExchangeModel.WHO)") && scan.contains("CeremonyModel.answerResult(payload)"), "ur:bytes only, and the length is the model's")
        ok(scan.contains("AppWords.ceremonyNotAnAnswer") && scan.contains("AppWords.ceremonyScanCancelled") && scan.contains("ScanResult.failed(f.off, f.problem, f.words)"), "every other end of the scan has words")
        ok(Regex("scan\\.begin\\(").findAll(c).count() == 2 && !c.contains("requestPermissions"), "the camera is asked for in two places, each from a button")
        val read = c.substringAfter("private fun Controller.readAnswer(").substringBefore("private fun Controller.showRootCheck(")
        ok(Regex("OwnerCeremony\\.readAnswer\\(").findAll(c).count() == 1 && read.contains("e.sayWords()"), "one place gives the code to the library, and a refusal is shown with its words")
        val root = c.substringAfter("private fun Controller.showRootCheck(").substringBefore("// ---- the third step")
        ok(root.contains("wordCards(CeremonyModel.wordsLine(read.keyAppRootWords))") && root.contains("AppWords.ceremonyCompareRoot"), "the Key app's identity words, large")
        ok(root.contains("ui.primary(\"The Key app shows the same words\")") && root.contains("ui.danger(\"They differ\")"), "the two answers")
        ok(root.contains("d.answer = null") && root.contains("AppWords.ceremonyRootDiffers"), "a difference throws the answer away, and the words say why")
        ok(root.contains("if (read.key2 == null) showKey2(d, null) else buildWallet(d)"), "key 2 is asked for only when the answer holds none")
        ok(read.contains("d.key2Text = null"), "a new answer starts with no key 2 text, so both are never passed")
        val key2 = c.substringAfter("private fun Controller.showKey2(").substringBefore("// ---- the fourth step")
        ok(key2.contains("AppWords.ceremonyKey2Needed") && key2.contains("CeremonyModel.key2Text(pasted.text.toString())") && key2.contains("CeremonyModel.key2FromScan(bytes)"), "key 2 by paste or by scan, checked for length only")
        ok(key2.contains("UrScan(CeremonyModel.KEY2_TYPES, ExchangeModel.WHO)") && key2.contains("CeremonyModel.key2FromMessage(UrCall.native().keyText(type, payload))"), "key 2's key code goes through the library's key text call")
        ok(CeremonyModel.KEY2_TYPES == setOf("crypto-account", "crypto-hdkey", "bytes"), "the three types")
        val text = "[a1b2c3d4/48h/0h/0h/2h]xpub6Dummy"
        val k = CeremonyModel.key2FromMessage(KeyTextOutcome.Key(text))
        ok(k is Form.Ok && k.value == text, "a key goes on as scanned text does")
        val dec = CeremonyModel.key2FromMessage(KeyTextOutcome.Declined("UR-23", "Not a key this app takes", "The code holds something else.", "Choose the multisig export."))
        ok(dec is Form.Refused && dec.words.heading == "Not a key this app takes" && dec.words.code == "UR-23" && dec.words.who == ExchangeModel.WHO, "a declined code shows the library's words")
        for (o in listOf(KeyTextOutcome.Refused("UR-21"), KeyTextOutcome.Fault)) {
            val r = CeremonyModel.key2FromMessage(o)
            ok(r is Form.Refused && r.words.heading == UrWords.readerFailed(ExchangeModel.WHO).heading, "a failed reader shows its words")
        }
        ok(AppWords.ceremonyKey2Needed.next.contains("a key code from the other wallet app, or its key as text"), "the key 2 step says which codes work")
        ok(c.contains("BuildArgs(d.record, answer, d.key2Text, defaults.batchMinCount)"), "key 2's text goes to the library as it is")
    }

    private fun oa_29_the_wallet_is_compared_on_both_phones_before_the_setup_is_finished_and_saved() {
        val c = ceremonyCode()
        val check = c.substringAfter("private fun Controller.showWalletCheck(").substringBefore("// ---- the fifth step")
        ok(check.contains("wordCards(CeremonyModel.wordsLine(built.checksumWords))") && check.contains("CeremonyModel.fingerprintLines(built.fingerprints)") && check.contains("mono = true"), "the checksum words large and the fingerprints in monospace")
        ok(check.contains("built.pairsWords.say()") && check.contains("CeremonyModel.pairLines(built.pairs)") && check.contains("line(built.firstReceiveAddress, muted = true, mono = true)"), "the pairs words, the pairs and the first address in monospace")
        val code = c.substringAfter("private fun Controller.showSetupCode(").substringBefore("/** The owner says the words differ")
        ok(code.contains("AnimatedQrView.make(") && code.contains("ExchangeModel.WHO, ExchangeModel.TYPE_BYTES, built.walletSetup") && code.contains("ExchangeModel.codeHeight(act.resources.displayMetrics.heightPixels)"), "the setup is shown as ur:bytes, at the size of the other codes")
        ok(code.contains("AppWords.ceremonyCompareSetup") && code.contains("if (step.done) {"), "the words to compare, and the buttons only with a code on the screen")
        ok(code.contains("ui.primary(\"The Key app shows the same words\") { finishCeremony(d) }") && code.contains("ui.danger(\"They differ\") { showWordsDiffer(d, built) }"), "the two answers")
        ok(Regex("OwnerCeremony\\.finish\\(").findAll(c).count() == 1 && Regex("finishCeremony\\(").findAll(c).count() == 3, "only the owner's word finishes, and the retry after a refusal")
        val differ = c.substringAfter("private fun Controller.showWordsDiffer(").substringBefore("// ---- the last step")
        ok(differ.contains("AppWords.ceremonyWordsDiffer") && differ.contains("ui.button(\"Start again\") { showStartAgain() }"), "a difference leads to start again")
        ok(c.contains("OwnerCeremony.finish(FinishArgs(d.record, defaults.refundDepth, defaults.birthday))"), "the library's refund depth and first block")
        val save = c.substringAfter("private fun Controller.saveFinished(").substringBefore("private fun Controller.showSaveFailed(")
        ok(save.indexOf("OwnerCeremony.open(record)") in 1 until save.indexOf("OwnerSetup.format(") + 40 && save.indexOf("saveSetup(") > save.indexOf("OwnerSetup.format("), "the finished record is opened, written as the setup text and saved")
        ok(!save.contains("orbotViews") && save.contains("afterSaved = { File(appDir, AppFiles.CEREMONY_FILE).delete() }"), "no address of Orbot is asked for (the new setup uses the built-in Tor, OA-99), and the record is deleted after the save")
        ok(save.indexOf("val save = {") in 1 until save.indexOf("saveSetup(") && !save.contains("\n    saveSetup(") && save.contains("ui.primary(\"Save and start\") { save() }"), "the save runs only from a button the owner presses")
        ok(!source("src/android/Controller.kt").contains("orbotViews") && source("src/android/Controller.kt").contains("configFile.save(AppConfig.fresh())") && !c.contains("torAlwaysOn"), "the paste screen and the ceremony save a setup with the built-in Tor and ask for no address")
        ok(source("src/android/TorScreen.kt").contains("if (!HostPort.valid(text)) return@primary showTorOrbot(AppWords.torAddressNotAccepted)"), "the address of Orbot is checked where it is asked for")
        val controller = source("src/android/Controller.kt")
        val saveSetup = controller.substringAfter("internal fun saveSetup(").substringBefore("/** The places the setup text")
        ok(saveSetup.indexOf("AppFiles.SETUP_FILE).save(") in 1 until saveSetup.indexOf("configFile.save(") && saveSetup.indexOf("configFile.save(") < saveSetup.indexOf("afterSaved()"), "the setup first, then the config, then the ceremony's record goes")
        ok(saveSetup.contains("showMessage(AppWords.setupSaved, \"Start\") { begin() }"), "the start goes on as after a pasted setup, through the gate of Orbot")
        ok(Regex("File\\(appDir, AppFiles\\.CEREMONY_FILE\\)").findAll(c).count() == 2, "the file is deleted in two places only: after the save, and on a confirmation")
    }

    private fun oa_32_change_settings_is_a_menu_of_seven_screens_and_a_review() {
        val settings = source("src/android/Screens.kt").substringAfter("internal fun Controller.showSettings()").substringBefore("internal fun Controller.showAbout()")
        ok(settings.contains("\"Change settings\"") && settings.contains("showChangeSettings()"), "the settings view offers Change settings")
        val e = source("src/android/SettingsEdit.kt")
        val menu = e.substringAfter("internal fun Controller.showChangeSettings(").substringBefore("internal fun Controller.showEditGroup(")
        for (w in listOf("editGroupShares.heading", "editGroupLightning.heading", "\"Review\"", "\"Start over\"", "settingsDraft = SettingsDraft()")) ok(menu.contains(w), "the menu has $w")
        ok(SettingsEditModel.groups.size == 6 && SettingsEditModel.groups.map { it.words.heading } == listOf("Currency", "Instant sales", "Payments", "Tips", "Refunds and overrides", "Safety"), "the currency, then five screens of fields: ${SettingsEditModel.groups.map { it.words.heading }}")
        ok(AppWords.editGroupShares.heading == "Counters' shares" && AppWords.editGroupLightning.heading == "Lightning node", "the other two")
        ok(SettingsEditModel.fields.map { it.id }.toSet().size == SettingsEditModel.fields.size && SettingsEditModel.fields.size >= 15, "each field once: ${SettingsEditModel.fields.size}")
        ok(e.indexOf("n.queueSettings(") > e.indexOf("n.previewSettings("), "nothing is queued before the preview")
        ok(SettingsDraft().isEmpty, "a new draft is empty")
        ok(source("src/android/Controller.kt").contains("internal var settingsDraft = SettingsDraft()"), "the draft is in memory only")
        for (bad in listOf("SharedPreferences", "File(", "SealedFile")) ok(!e.contains(bad) && !source("src/model/SettingsEditModel.kt").contains(bad), "the draft is never written: $bad")
    }

    private fun oa_33_each_field_shows_its_words_and_value_and_only_the_shape_is_judged_here() {
        for (f in SettingsEditModel.fields) {
            val w = f.words
            ok(w.heading.length <= 28 && w.what.length <= 140 && SayCheck.problem(w) == null, "words of ${f.id}")
        }
        val held = sampleSettings()
        val usd = SettingsEditModel.fields.first { it.kind == FieldKind.Dollars }
        ok(SettingsEditModel.input(usd, held) == "7", "a field shows what the settings hold: ${SettingsEditModel.input(usd, held)}")
        ok(SettingsEditModel.dollars("1,250") == 1250L && SettingsEditModel.dollars("$5") == 5L && SettingsEditModel.dollars("0") == 0L, "dollars")
        ok(SettingsEditModel.dollars("1.5") == null && SettingsEditModel.dollars("") == null && SettingsEditModel.dollars("-3") == null && SettingsEditModel.dollars("abc") == null && SettingsEditModel.dollars("99999999999") == null, "not dollars")
        ok(SettingsEditModel.percentBps("1.05") == 105L && SettingsEditModel.percentBps("50%") == 5000L && SettingsEditModel.percentBps("33.33") == 3333L && SettingsEditModel.percentBps("0.5") == 50L && SettingsEditModel.percentBps("100") == 10_000L, "percent to basis points")
        ok(SettingsEditModel.percentBps("0.001") == null && SettingsEditModel.percentBps("101") == null && SettingsEditModel.percentBps("x") == null && SettingsEditModel.percentBps("") == null && SettingsEditModel.percentBps("-1") == null, "not a percent")
        ok(SettingsEditModel.minutesToSecs("15") == 900L && SettingsEditModel.minutesToSecs("0") == 0L && SettingsEditModel.minutesToSecs("1.5") == null && SettingsEditModel.minutesToSecs("") == null, "minutes to seconds")
        ok(SettingsEditModel.blocks("6") == 6L && SettingsEditModel.blocks("256") == null && SettingsEditModel.blocks("-1") == null, "blocks")
        ok(SettingsEditModel.sats("5000") == 5000L && SettingsEditModel.sats("5,000") == 5000L && SettingsEditModel.sats("5.5") == null && SettingsEditModel.sats("") == null, "satoshis")
        ok(SettingsEditModel.parse(FieldKind.Switch, "1") == 1L && SettingsEditModel.parse(FieldKind.Switch, "0") == 0L && SettingsEditModel.parse(FieldKind.Switch, "yes") == null, "a switch")
        ok(SettingsEditModel.percentInput(105) == "1.05" && SettingsEditModel.percentInput(1000) == "10" && SettingsEditModel.percentInput(50) == "0.5" && SettingsEditModel.percentText(3333) == "33.33%", "percent from basis points")
        ok(SettingsEditModel.tipPercent("15") == 15L && SettingsEditModel.tipPercent(" 1 ") == 1L && SettingsEditModel.tipPercent("50%") == 50L && SettingsEditModel.tipPercent("") == 0L, "a suggested tip is a percent from 1 to 50, or empty")
        ok(listOf("0", "51", "1.5", "-3", "abc", "100", "1,5").all { SettingsEditModel.tipPercent(it) == null }, "not a suggested tip")
        val tipHeld = sampleSettings(tipsOn = true, tipPercents = listOf(5, 12))
        val tipFields = SettingsEditModel.tips.fields
        ok(tipFields.map { it.id } == listOf("tips_on", "tip_1", "tip_2", "tip_3") && tipFields.map { it.label } == listOf("Ask for a tip", "First tip", "Second tip", "Third tip"), "the tips screen: ${tipFields.map { it.label }}")
        ok(tipFields.drop(1).map { SettingsEditModel.input(it, tipHeld) } == listOf("5", "12", "") && tipFields.drop(1).map { SettingsEditModel.now(it, tipHeld) } == listOf("5%", "12%", "none"), "what the tips are now")
        ok(tipFields.all { it.words.what == AppWords.editGroupTips.what } && tipFields.drop(1).all { it.words.next == "A percent from 1 to 50, or empty." }, "the tips' hint")
        ok(AppWords.editValueNotRead(FieldKind.TipPercent, "First tip") === AppWords.editTipNotRead && AppWords.editTipNotRead.heading == "Tip not accepted", "a tip that is not read has its own words")
        ok(SettingsEditModel.tipsNow(sampleSettings()) == "Off" && SettingsEditModel.tipsNow(tipHeld) == "5%, 12%" && SettingsEditModel.tipsNow(sampleSettings(tipsOn = true, tipPercents = emptyList())) == "none", "the read-only line")
        run {
            val td = SettingsDraft()
            ok(SettingsEditModel.args(td, tipHeld).let { it is Form.Ok && it.value.tipPercents == null && it.value.tipsOn == null }, "tips not touched are not sent")
            td.typed["tips_on"] = "0"
            ok((SettingsEditModel.args(td, tipHeld) as Form.Ok).value.let { it.tipsOn == false && it.tipPercents == null }, "the switch alone")
            td.typed["tip_1"] = ""
            td.typed["tip_3"] = "25"
            ok((SettingsEditModel.args(td, tipHeld) as Form.Ok).value.tipPercents == listOf(12, 25), "an empty field is left out, an untouched one is kept, in order")
            td.typed["tip_2"] = "51"
            ok((SettingsEditModel.args(td, tipHeld) as Form.Refused).words === AppWords.editTipNotRead, "a percent above 50 is refused with the tip words")
        }
        val key = "02" + "ab".repeat(32)
        ok(SettingsEditModel.nodeText("  $key  ").let { it is Form.Ok && it.value == key }, "a node key is trimmed")
        ok(SettingsEditModel.nodeText("$key@host.onion:9735").let { it is Form.Ok && it.value == "$key@host.onion:9735" }, "an address goes through whole; the library takes the key")
        ok(SettingsEditModel.nodeText("   ") is Form.Refused, "an empty node text is refused with words")
        ok(SettingsEditModel.nodeText("zz") is Form.Ok, "a text that is not a key is the library's to refuse (OA-07)")
        ok(SettingsEditModel.nodeNow(held) == "none", "no node")
        ok(SettingsEditModel.sharesTotalLine(listOf(5000, 4000)) == "Total 90%. Left over: 10%." && SettingsEditModel.sharesTotalLine(listOf(6000, 5000)).contains("Over the whole by 10%"), "the shares' total and remainder")
        ok(SettingsEditModel.sharesLine(listOf("50", "x")) == "Some share is not a percent yet." && SettingsEditModel.sharesLine(listOf("50", "50")).startsWith("Total 100%"), "the line while typing")
        val d = SettingsDraft()
        d.typed[usd.id] = "1.5"
        ok(SettingsEditModel.args(d, held) is Form.Refused, "a field that is not dollars is refused")
        val e = source("src/android/SettingsEdit.kt")
        ok(e.contains("AppWords.editValueNotRead(") && e.contains("AppWords.editSharesNotPercent") && e.contains("TextWatcher"), "the screens refuse with words and show the total as it is typed")
        for (bad in listOf("Regex", "bech32", "bc1", "secp", "pubkey")) ok(!e.contains(bad), "SettingsEdit.kt judges no key or address: $bad")
    }

    private fun oa_34_review_calls_the_preview_and_queues_only_on_the_button() {
        val e = source("src/android/SettingsEdit.kt")
        val review = e.substringAfter("internal fun Controller.showEditReview(").substringBefore("private fun Controller.showEditPrice(")
        ok(review.contains("n.previewSettings(args)") && review.indexOf("n.previewSettings(") < review.indexOf("\"Queue the change\""), "the preview comes first")
        ok(Regex("n\\.queueSettings\\(").findAll(e).count() == 1 && review.contains("n.queueSettings(p.settings)"), "one queue call, with the bytes the preview gave")
        ok(review.contains("\"Needs the Key app\"") && review.contains("whenDelivered") && review.contains("nothingChanged"), "the two kinds of change are apart and nothing changed is said")
        ok(review.contains("settingsDraft = SettingsDraft()"), "the draft is cleared after the queue")
        ok(e.contains("fun Controller.showEditPrice(") && e.contains("settingsDraft.price = \"\"") && !e.contains("Satoshis to a dollar"), "a price is asked for when the library needs one, empty, and forgotten")
        fun change(f: String, o: String, n: String, loose: Boolean, c: ByteArray? = null) = SettingChange(f, f, c, o, n, loose)
        val p = SettingsPreview(4, listOf(change("Refund cap", "$50", "$100", true), change("Heartbeat", "15 minutes", "10 minutes", false), change("share", "50%", "60%", false, ByteArray(32) { 1 })), true, false, 6000, 98_500L, 1_015L, ByteArray(0))
        val lines = SettingsEditModel.reviewLines(p)
        ok(lines[0].text == "Refund cap: $50 → $100" && lines[2].text == "Share of Counter 01010101: 50% → 60%", "old to new: ${lines.map { it.text }}")
        ok(SettingsEditModel.needKeyApp(p) == listOf("Refund cap: $50 → $100") && SettingsEditModel.whenDelivered(p).size == 2, "loosening apart from tightening")
        val plain = SettingsPreview(4, listOf(change("Heartbeat", "a", "b", false)), false, false, 0, null, null, ByteArray(0))
        ok(SettingsEditModel.reviewWords(p) === AppWords.editNeedsKeyApp && SettingsEditModel.reviewWords(SettingsPreview(4, emptyList(), false, true, 0, null, null, ByteArray(0))) === AppWords.editNothingChanged, "the words above the review")
        ok(SettingsEditModel.reviewWords(plain) === AppWords.editTakesEffect, "a stricter change takes effect when delivered")
        ok(SettingsEditModel.reviewShares(p) == "Total 60%. Left over: 40%." && SettingsEditModel.reviewShares(plain) == null, "the shares line is only for share changes")
        val held = sampleSettings()
        val d = SettingsDraft()
        val usd = SettingsEditModel.fields.first { it.kind == FieldKind.Dollars }
        d.typed[usd.id] = "9"
        d.shares[SettingsEditModel.counterKey(ByteArray(32) { 2 })] = "75.5"
        d.node = NodeChoice.Remove
        val a = (SettingsEditModel.args(d, held) as Form.Ok).value
        ok(a.shares.size == 1 && a.shares[0].bps == 7550L && a.lightning == LightningAction.Remove, "the arguments carry the shares and the removal")
        ok(SettingsEditModel.args(SettingsDraft(), held).let { it is Form.Ok && it.value == SettingsEditArgs() }, "an empty draft is an empty edit")
        d.price = "0"
        ok(SettingsEditModel.args(d, held) is Form.Refused, "a price of 0 is refused with words")
        d.price = "$98,500"
        ok((SettingsEditModel.args(d, held) as Form.Ok).value.usdPerBtc == 98_500L, "the price goes to the node as whole dollars, and the node converts it")
        ok(SettingsEditModel.args(SettingsDraft(), held).let { it is Form.Ok && it.value.usdPerBtc == null }, "no price is sent that was not typed")
        swapSettingTests(held)
    }

    /** CR-218: the swap provider row: the words as written down, the draft turned into the edit, and the address split. */
    private fun swapSettingTests(held: SettingsHeld) {
        ok(AppWords.swapSettingTitle == "Lightning through a swap provider" && AppWords.swapSettingProvider == "Provider" && AppWords.swapSettingOnion == "Onion address", "CR-218: the row's words, as written down")
        ok(AppWords.swapSettingMax == "Largest sale over Lightning" && AppWords.swapSettingOn == "Turn on" && AppWords.swapSettingOff == "Turn off", "CR-218: the row's buttons, as written down")
        ok(
            AppWords.swapSettingText == "Customers can pay over Lightning. A swap provider you pick turns each payment into bitcoin that only your counter can claim, to your own wallet. The provider never holds your money.",
            "CR-218: the row's text, as written down",
        )
        ok(AppWords.swapSettingFee("0.6", 558) == "The provider charges 0.6% plus about 558 sats per sale.", "CR-218: the fee line, as written down")
        ok(AppWords.all().none { it.second.what == AppWords.swapSettingText }, "CR-218: the lines of the row have no heading and are not in the list of words")
        ok(SettingsEditModel.splitOnion(" http://abc.onion/api/ ") == ("abc.onion" to "/api") && SettingsEditModel.splitOnion("abc.onion") == ("abc.onion" to "") && SettingsEditModel.splitOnion("https://abc.onion") == ("abc.onion" to ""), "CR-218: a whole address is split into the name and the path prefix")
        ok(SettingsEditModel.swapNow(held) == "none", "CR-218: no provider is none")
        val d = SettingsDraft()
        ok(d.isEmpty, "an empty draft")
        d.swap = NodeChoice.Set
        d.swapName = " Pretend "
        d.swapOnion = "http://abc.onion/api"
        d.swapMax = "1,000"
        ok(!d.isEmpty && SettingsEditModel.args(d, held) is Form.Refused, "CR-218: a largest sale that is not a number is refused with words")
        d.swapMax = "1000000"
        val a = (SettingsEditModel.args(d, held) as Form.Ok).value
        ok(a.swap == SwapAction.Set && a.swapName == "Pretend" && a.swapOnion == "abc.onion" && a.swapPrefix == "/api" && a.swapMaxSats == 1_000_000L, "CR-218: the arguments carry the provider")
        d.swap = NodeChoice.Remove
        ok((SettingsEditModel.args(d, held) as Form.Ok).value.swap == SwapAction.Remove, "CR-218: turning it off")
    }

    private fun oa_35_after_the_queue_the_existing_screens_take_over() {
        val e = source("src/android/SettingsEdit.kt")
        val review = e.substringAfter("internal fun Controller.showEditReview(").substringBefore("private fun Controller.showEditPrice(")
        ok(review.contains("showOutbox()") && review.contains("showSpend()"), "a stricter change goes to the outbox, a looser one to the request screen")
        ok(!e.contains("approveMerge") && !e.contains("buildRequest(") && !e.contains(".take(") && !e.contains("signBatch"), "the editor has no approval flow of its own")
        ok(AppWords.editQueuedLoosening.what.contains("Key app") && AppWords.editQueuedTightening.what.isNotEmpty(), "the words after the queue")
        ok((AppWords.editLightningNote.what + AppWords.editLightningNote.next).contains("Key app") && (AppWords.editLightningNote.what + AppWords.editLightningNote.next).contains("each counter"), "the Lightning note: the Key app and each counter")
        for ((name, w) in AppWords.all().filter { it.first.startsWith("edit") }) {
            ok((w.who == Who.OWNER || name == "boxPriceFetching") && SayCheck.problem(w) == null && w.heading.length <= 28 && w.what.length <= 140 && w.next.length <= 140, "words of $name")
        }
        ok(AppWords.screensMarkdown().contains("| editIntro |") && File(appDir, "../../docs/screens/owner-app.md").readText().contains("Counters' shares"), "the list of the app's words has them")
        for (bad in listOf(".signBatch(", "Psbt", "psbt", ".broadcast(", "Transaction(", "key2Secret", "key3Secret", "mnemonic")) {
            ok(!e.contains(bad) && !source("src/model/SettingsEditModel.kt").contains(bad), "the editor does not hold or call $bad")
        }
    }

    private fun oa_30_the_slow_calls_draw_a_page_first_and_every_number_is_the_librarys() {
        val c = ceremonyCode()
        val make = c.substringAfter("internal fun Controller.startCeremony()").substringBefore("/** Key 1 is made:")
        ok(make.indexOf("waiting(AppWords.ceremonyMakingKey)") in 1 until make.indexOf("main.post") && make.indexOf("main.post") < make.indexOf("OwnerCeremony.makeKey1("), "a page says it takes a moment, and is drawn before the call")
        val build = c.substringAfter("private fun Controller.buildWallet(").substringBefore("/** The wallet was refused")
        ok(build.indexOf("waiting(AppWords.ceremonyBuilding)") in 1 until build.indexOf("main.post") && build.indexOf("main.post") < build.indexOf("OwnerCeremony.buildWallet("), "the same for the wallet")
        ok(!make.contains("async(") && !build.contains("async(") && !make.contains("Thread") && !build.contains("Thread"), "the calls stay on the screen thread")
        ok(c.contains("OwnerCeremony.defaults(AppConfig.NETWORK)") && c.contains("defaults.batchMinCount") && c.contains("defaults.refundDepth") && c.contains("defaults.birthday"), "the batch minimum, the refund depth and the first block come from the library")
        for (bad in listOf("1000", "1_000", "967", "refundDepth = ", "birthday = ", "BATCH_MIN", "batchMinCount = ")) ok(!c.contains(bad), "Ceremony.kt writes no number of the ceremony: $bad")
        ok(!c.contains("Regex") && !c.contains("bech32") && !c.contains("bcrt1") && !c.contains("bc1"), "the app reads no address and no key")
        ok(source("../../crates/openbmt-owner-ffi/bindings/kotlin/org/openbmt/owner/OwnerNode.kt").contains("fun defaults(network: OwnerNetwork): CeremonyDefaults"), "the binding has the op that gives them")
    }

    private fun oa_31_every_word_of_the_ceremony_is_the_librarys_or_checked_and_no_secret_of_key_2_or_3_is_held() {
        val c = ceremonyCode()
        for (field in listOf("made.stateWords.say()", "at.words.say()", "built.pairsWords.say()", "e.sayWords()")) ok(c.contains(field), "the screens show the library's $field")
        val ceremonyWords = AppWords.all().filter { it.first.startsWith("ceremony") }
        ok(ceremonyWords.size >= 20, "the ceremony has its words: ${ceremonyWords.size}")
        for ((name, w) in ceremonyWords) ok(w.who == Who.OWNER && SayCheck.problem(w) == null && w.next.isNotEmpty(), "words of $name")
        for (w in ceremonyWords.map { it.second }) {
            ok(w.what.length <= 140 && w.next.length <= 140, "each sentence is at most 140 characters: ${w.heading}")
        }
        ok(AppWords.screensMarkdown().contains("| ceremonyStartAgain |") && AppWords.screensMarkdown().contains("| Wallet not finished |"), "the list of the app's words has them")
        ok(File(appDir, "../../docs/screens/owner-app.md").readText().contains("Wallet not finished"), "and so does docs/screens/owner-app.md")
        for (name in listOf("src/android/Ceremony.kt", "src/model/CeremonyModel.kt")) {
            val src = source(name)
            for (bad in listOf(".signBatch(", ".queueSettings(", "sign(", "Psbt", "psbt", ".broadcast(", "Transaction(", "key2Secret", "key3Secret", "mnemonic", "seed")) {
                ok(!src.contains(bad), "$name does not call or hold $bad")
            }
        }
        ok(!ceremonyCode().contains("made.words"), "key 1's words are never shown")
    }

    // ---- the PIN on key 1 (OA-60 to OA-65) ------------------------------------------------------------------

    private fun lockSource(): String = source("src/android/Lock.kt")

    /** The Keystore's stand-in: AES-GCM under a key of the test's own, as in oa_01. */
    private fun aesSealer(key: ByteArray): Sealer = object : Sealer {
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

    private fun oa_60_the_setup_is_locked_behind_a_pin_inside_the_keystores_seal() {
        val lock = lockSource()
        val controller = source("src/android/Controller.kt")
        // The library locks, opens and changes; the app stores what a call returned.
        for (call in listOf("OwnerLock.isLocked(stored)", "OwnerLock.unlock(stored, pin)", "OwnerLock.lock(pin, text)", "OwnerLock.changePin(stored, old, new)")) {
            ok(lock.contains(call), "the app makes the call $call")
        }
        ok(lock.contains("setupFile().save(OwnerLock.changePin(stored, old, new))"), "the file is replaced only with what the change returned")
        ok(controller.contains("AppFiles.SETUP_FILE).save(locked)"), "the setup file holds the locked bytes, not the text")
        ok(!controller.contains("SETUP_FILE).save(text"), "the bare text is never saved")
        ok(lock.contains("text.fill(0)") && controller.contains("bytes.fill(0)"), "the bytes of the text are overwritten after use")
        ok(lock.contains("AppFiles.KEY_ALIAS"), "the Keystore's seal is the one of OA-01")
        // A locked frame goes through the Keystore's seal like any bytes, and nothing of it reads as text.
        val dir = tempDir()
        try {
            val key = ByteArray(32).also { SecureRandom().nextBytes(it) }
            val sealer = aesSealer(key)
            val file = SealedFile(dir, sealer, AppFiles.SETUP_FILE)
            val locked = "OBMx".toByteArray() + ByteArray(80) { (it * 7).toByte() }
            file.save(locked)
            ok(file.load()!!.contentEquals(locked), "the locked bytes come back as they went in")
            val onDisk = File(dir, AppFiles.SETUP_FILE).readBytes()
            ok(onDisk.copyOfRange(0, 4).contentEquals("OBKS".toByteArray()) && !String(onDisk, Charsets.ISO_8859_1).contains("OBMx"), "the file is the Keystore's seal around the frame")
        } finally {
            dir.deleteRecursively()
        }
        ok(source("../../crates/openbmt-owner-ffi/bindings/kotlin/org/openbmt/owner/OwnerNode.kt").contains("object OwnerLock"), "the binding has the four calls")
    }

    private fun oa_61_a_pin_is_chosen_before_the_first_save_and_an_older_setup_gets_one() {
        val controller = source("src/android/Controller.kt")
        val lock = lockSource()
        val save = controller.substringAfter("internal fun saveSetup(")
        ok(save.indexOf("askNewPin(") in 1 until save.indexOf("AppFiles.SETUP_FILE).save(locked)"), "the PIN is asked for before the save")
        ok(save.indexOf("OwnerSetup.parse(text)") in 1 until save.indexOf("askNewPin("), "the text is checked for its shape first")
        ok(source("src/android/Ceremony.kt").contains("backWords = AppWords.ceremonySaveNext"), "the ceremony goes through the same helper")
        ok(controller.contains("saveSetup(draftText, {"), "and so does the paste path")
        ok(SetupLock.gate(0, false) == Gate.SetPin && SetupLock.gate(3, false) == Gate.SetPin, "a bare setup opens only into set-a-PIN")
        ok(lock.contains("Gate.SetPin -> showUpgrade(stored)"), "the start shows the step that sets a PIN")
        val upgrade = lock.substringAfter("private fun Controller.showUpgrade(")
        ok(upgrade.indexOf("readSetup(stored.copyOf())") in 0 until upgrade.indexOf("askNewPin("), "a bare text that is not a setup is a fault, not a PIN")
        ok(upgrade.indexOf("askNewPin(") in 0 until upgrade.indexOf("setupFile().save(locked)"), "the older setup is locked, then saved again")
        for (label in listOf("PIN: 6 digits or more", "Passphrase: 8 characters or more", "The same PIN again", "Set the PIN")) ok(lock.contains(label), "the screen says $label")
        ok(lock.contains("TYPE_NUMBER_VARIATION_PASSWORD") && lock.contains("TYPE_TEXT_FLAG_NO_SUGGESTIONS"), "the fields hide the letters and take no suggestions")
        ok(lock.contains("first.setText(\"\")") && lock.contains("again.setText(\"\")"), "the typed PIN is cleared from the fields")
        ok(AppWords.choosePin.heading == "Choose a PIN" && AppWords.upgradePin.heading == "Set a PIN for key 1", "the words of the two screens")
        ok(AppWords.choosePin.next.contains("6 digits") && AppWords.choosePin.next.contains("8 characters"), "the words say the rule")
        ok(source("src/android/Screens.kt").contains("RowItem(\"Change the PIN\") { showChangePin(null) }"), "the home screen offers the change of PIN")
        ok(lock.contains("triedPin({ showChangePin(it) }"), "a wrong current PIN counts like a try")
    }

    private fun oa_62_the_unlock_comes_first_at_start_and_after_a_minute_away() {
        val controller = source("src/android/Controller.kt")
        val lock = lockSource()
        ok(SetupLock.gate(0, true) == Gate.Unlock && SetupLock.gate(9, true) == Gate.Unlock, "a locked setup asks for the PIN")
        val begin = controller.substringAfter("internal fun begin() {").substringBefore("internal fun render()")
        ok(begin.contains("openStored()"), "the stored setup is opened by the PIN step")
        ok(begin.indexOf("if (setup != null) return startNode()") in 1 until begin.indexOf("openStored()"), "the node starts from memory only while it is unlocked")
        ok(!lock.contains("startNode()"), "nothing in the lock screens starts the node but begin")
        val visible = controller.substringAfter("fun visible() {").substringBefore("fun hidden()")
        ok(visible.contains("Relock.needed(pausedAt, SystemClock.elapsedRealtime())") && visible.indexOf("lockNow()") < visible.indexOf("begin()"), "more than a minute away drops the setup before the start")
        ok(visible.contains("waitedTooLong()") && visible.contains("endWait()"), "a minute in a picker or a permission dialog counts, and the setup is locked")
        val picked = controller.substringAfter("fun picked(").substringBefore("/** At most [limit] bytes")
        ok(picked.indexOf("ExternalWait.decide(") in 0 until picked.indexOf("done(uri)"), "a picker's result is judged before it is read")
        val permission = controller.substringAfter("internal fun permission(").substringBefore("/** Asks the system for a file name")
        ok(permission.indexOf("waitedTooLong()") in 0 until permission.indexOf("scan.permissionResult"), "a permission answer is judged before it is taken")
        val hidden = controller.substringAfter("fun hidden() {").substringBefore("internal fun begin()")
        ok(hidden.contains("pausedAt = SystemClock.elapsedRealtime()") && hidden.contains("relockTimer"), "the time away is kept on the clock that counts since the phone started, and a timer drops the setup")
        ok(!Relock.needed(1000, 1000 + 60_000) && Relock.needed(1000, 1000 + 60_001) && Relock.needed(null, 5) && Relock.needed(1000, 500), "the 60-second rule")
        val slow = lock.substringAfter("private fun Controller.slow(").substringBefore("// ---- the start")
        ok(slow.indexOf("AppWords.working") in 0 until slow.indexOf("postDelayed"), "the working page is drawn before the library is asked")
        ok(lock.contains("slow {") && AppWords.working.heading == "Working", "and the slow steps use it")
        ok(lock.contains("e.rule == RULE_PIN_WRONG") && lock.contains("\"pin-wrong\""), "a wrong PIN is told by its rule")
        ok(AppWords.unlockSetup.heading == "Enter your PIN", "the unlock words")
        // A tap, then the app is hidden before the delayed work: no try, no unlock and no node follow.
        ok(!SetupLock.runsDelayed(false) && SetupLock.runsDelayed(true), "the delayed work runs only while the app is visible")
        ok(slow.contains("if (SetupLock.runsDelayed(visible)) block() else skipped()"), "the delayed block is guarded by the visible flag")
        val triedPin = lock.substringAfter("internal fun Controller.triedPin(").substringBefore("// ---- the wipe")
        ok(triedPin.contains("slow(skipped = { giveBack() })"), "a try that was not run is given back and not counted")
        ok(triedPin.indexOf("op()") > triedPin.indexOf("slow(skipped"), "the PIN is tried only inside the guarded block")
        ok(controller.substringAfter("internal fun startNode() {").substringBefore("val id = ++runId").contains("if (!visible) return"), "the node does not start while hidden")
        ok(controller.contains("internal var visible = false\n        private set") && hidden.contains("visible = false"), "the same flag is set by hidden()")
        ok(lock.substringAfter("internal fun Controller.lockNow() {").substringBefore("}").contains("stopNode()"), "relocking stops a running node")
    }

    private fun oa_63_wrong_tries_are_counted_in_the_shared_file_before_each_try() {
        val waits = listOf(0L, 0, 0, 0, 0, 30, 60, 300, 900, 3600)
        for ((n, w) in waits.withIndex()) ok(Tries.waitSeconds(n) == w, "$n wrong tries wait $w s")
        ok(Tries.check(TriesRecord(10, 5), 5).wipe && !Tries.check(TriesRecord(9, 5), 5).wipe, "the 10th wipes")
        ok(Tries.showsTriesLeft(5) && !Tries.showsTriesLeft(4), "tries left are said from the 5th")
        val lock = lockSource()
        val tried = lock.substringAfter("private fun Controller.triedPin(").substringBefore("// ---- the wipe")
        ok(tried.indexOf("tries.write(before)") in 1 until tried.indexOf("op()"), "the count plus one is written before the try")
        ok(tried.contains("Tries.afterRight()") && tried.contains("giveBack()"), "a right PIN clears the count and another refusal gives the step back")
        ok(tried.contains("before.wrong >= Tries.WIPE_AT"), "the 10th wrong try wipes")
        ok(source("src/android/Controller.kt").contains("TriesFile(dir)"), "the file is the shared TriesFile, in the app's folder")
        ok(!File(appDir, "src/model").listFiles()!!.any { it.readText().contains("object Tries") || it.readText().contains("class TriesFile") }, "the owner app has no copy of the tries code")
        val dir = tempDir()
        try {
            val f = TriesFile(dir)
            ok(f.read(5) == TriesRecord(0, 0), "no file: no wrong tries")
            f.write(TriesRecord(7, 1234))
            ok(f.read(5) == TriesRecord(7, 1234) && File(dir, TriesFile.NAME).length() == 17L, "the record is read back, in 17 bytes")
            File(dir, TriesFile.NAME).writeBytes(ByteArray(16))
            ok(f.read(77) == TriesRecord(9, 77), "a file that is not exact counts as 9 wrong tries just now")
        } finally {
            dir.deleteRecursively()
        }
        ok(AppWords.waitForTry.heading == "Wait before the next try" && AppWords.triesNotSaved("x").heading == "Try not counted", "the words of the waits")
    }

    private fun oa_64_the_tenth_wrong_try_wipes_key_1_in_order() {
        ok(SetupLock.WIPE_FILES == listOf(AppFiles.SETUP_FILE, AppFiles.CEREMONY_FILE, AppFiles.KEY2_SIGNER_FILE, AppFiles.FIRST_ADDRESS_FILE), "the setup, the ceremony's file, then the signer note and the first address record")
        ok(SetupLock.WIPE_FILES.containsAll(listOf("key2-signer.txt", "first-address.obm0")), "the wipe list names both files of a hardware signer")
        ok(SetupLock.gate(10, true) == Gate.Wipe && SetupLock.gate(10, false) == Gate.Wipe && SetupLock.gate(9, true) == Gate.Unlock, "10 wrong tries wipe, whatever the file")
        val dir = tempDir()
        try {
            val setup = File(dir, AppFiles.SETUP_FILE)
            val ceremony = File(dir, AppFiles.CEREMONY_FILE)
            val tries = TriesFile(dir)
            fun setUp() {
                setup.writeBytes(byteArrayOf(1, 2, 3))
                ceremony.writeBytes(byteArrayOf(4, 5, 6))
                File(dir, AppFiles.SETUP_FILE + ".tmp").writeBytes(byteArrayOf(7))
                tries.write(TriesRecord(10, 5))
            }
            setUp()
            val seen = ArrayList<String>()
            WipeOrder.run(dir, SetupLock.WIPE_FILES, tries) {
                seen.add("key")
                ok(!setup.exists() && !ceremony.exists() && tries.exists(), "both files go before the key, the tries file after")
            }
            ok(seen == listOf("key") && !tries.exists() && !File(dir, AppFiles.SETUP_FILE + ".tmp").exists(), "all of it is gone")
            // A ceremony file that is not there is no trouble.
            tries.write(TriesRecord(10, 5))
            setup.writeBytes(byteArrayOf(1))
            WipeOrder.run(dir, SetupLock.WIPE_FILES, tries) { }
            ok(!setup.exists() && !tries.exists(), "a wipe without a ceremony file")
            // A key that stays leaves the tries file, so the next start wipes again.
            setUp()
            try {
                WipeOrder.run(dir, SetupLock.WIPE_FILES, tries) { throw IOException("still there") }
                ok(false, "a key that stays throws")
            } catch (_: IOException) {
            }
            ok(!setup.exists() && !ceremony.exists() && tries.read(0).wrong == 10 && Tries.check(tries.read(0), 0).wipe, "a wipe that stops leaves the count at 10")
        } finally {
            dir.deleteRecursively()
        }
        val lock = lockSource()
        ok(lock.contains("WipeOrder.run(appDir, SetupLock.WIPE_FILES, tries) { OwnerWipe.deleteKey() }"), "the screens wipe through the shared order")
        ok(source("src/android/OwnerWipe.kt").contains("deleteEntry(AppFiles.KEY_ALIAS)"), "the Keystore key of the setup is deleted")
        ok(source("src/android/Controller.kt").contains("if (tries.read(nowSecs()).wrong >= Tries.WIPE_AT) return wipe()"), "a wipe that stopped is finished at the next start")
        val w = AppWords.wiped
        ok(w.heading == "Key 1 wiped from this phone" && w.what.contains("copy of key 1 was wiped after 10 wrong tries"), "the wiped words say what happened")
        ok(w.what.contains("not lost") && w.what.contains("two of the three keys still exist"), "and that the money is not lost")
        ok(w.next.contains("recovery kit") && w.next.contains("move to a new wallet") && w.next.contains("key 2"), "and the two ways on, as the design has them")
        ok(SayCheck.problem(w) == null && SayCheck.problem(AppWords.wipeFailed("x")) == null, "the words of the wipe pass the words check")
    }

    private fun oa_65_the_lock_files_stay_out_of_backups() {
        ok(AppFiles.SETUP_FILE == "setup.obks" && AppFiles.CEREMONY_FILE == "ceremony.obks" && TriesFile.NAME == "tries.obkt", "the three files have their names")
        val manifest = source("AndroidManifest.xml")
        ok(manifest.contains("android:allowBackup=\"false\""), "cloud backup is off in the manifest")
        ok(manifest.contains("@xml/backup_rules") && manifest.contains("@xml/data_extraction_rules"), "both rule files are named")
        val domains = listOf("root", "file", "database", "sharedpref", "external")
        for (name in listOf("backup_rules", "data_extraction_rules")) {
            val xml = source("res/xml/$name.xml")
            val withoutComments = xml.replace(Regex("<!--.*?-->", RegexOption.DOT_MATCHES_ALL), "")
            val parts = if (name == "backup_rules") 1 else 2
            for (d in domains) {
                val n = Regex("<exclude domain=\"$d\" />").findAll(withoutComments).count()
                ok(n == parts, "$name excludes $d $parts time(s)")
            }
            ok(!withoutComments.contains("<include"), "$name includes nothing")
        }
        val controller = source("src/android/Controller.kt")
        ok(controller.contains("private val dir: File = act.noBackupFilesDir") && controller.contains("TriesFile(dir)"), "the tries file is in the no-backup folder")
        for (name in listOf("src/android/Controller.kt", "src/android/Lock.kt", "src/android/Ceremony.kt")) {
            ok(!Regex("[^a-zA-Z]filesDir").containsMatchIn(source(name)), "$name never uses the plain files folder")
        }
        ok(!lockSource().contains("setUserAuthenticationRequired") && !source("../android-common/src/android/Keystore.kt").contains("setUserAuthenticationRequired"), "the wrapping key is not bound to biometrics or the lock screen")
    }

    // ---- connecting a Box (OA-40 to OA-48) ---------------------------------------------------------------------

    private fun connectBox(): String = source("src/android/ConnectBox.kt")

    private fun oa_40_the_home_screen_opens_a_screen_that_saves_nothing() {
        ok(source("src/android/Screens.kt").contains("RowItem(\"Connect your Box\") { showConnectBox() }"), "connecting a Box starts on the home screen")
        val first = connectBox().substringAfter("internal fun Controller.showConnectBox(").substringBefore("private fun Controller.scanBox()")
        ok(first.contains("for (line in ConnectBoxModel.explanation)") && ConnectBoxModel.explanation.size == 2, "two lines of steps")
        ok(first.contains("\"Look for the Box\"") && first.contains("\"Scan the Box's address\""), "the two buttons")
        ok(first.contains("pendingBoxViews("), "the Boxes being added are listed")
        ok(!first.contains("SealedFile") && !first.contains("saveSetup"), "the first screen saves nothing")
        ok(source("src/android/Screens.kt").contains("pendingBoxViews(n, Slot(act)) { showOutbox() }"), "a Box that waits is in the list of what waits to be delivered")
        ok(source("src/android/Approval.kt").contains("pendingBoxViews(n, Slot(act)) { showHome() }"), "and after a response is taken")
    }

    private fun oa_41_the_address_is_judged_by_shape_and_the_fetch_runs_off_the_screen_thread() {
        val onion = "a".repeat(56) + ".onion:8080"
        for (good in listOf("192.168.1.20:8080", "  192.168.1.20:8080 ", onion.uppercase(), onion)) {
            ok(ConnectBoxModel.address(good) is Form.Ok, "an address: ${good.trim().take(24)}")
        }
        ok((ConnectBoxModel.address("  192.168.1.20:8080 ") as Form.Ok).value == "192.168.1.20:8080", "spaces are dropped")
        ok((ConnectBoxModel.address(onion.uppercase()) as Form.Ok).value == onion, "capitals are made small")
        for (bad in listOf("", "192.168.1.20", "box.local", ":80", "a b:80", "host:0", "host:99999", "http://x:80")) {
            ok(ConnectBoxModel.address(bad) is Form.Refused, "not an address: $bad")
        }
        ok(ConnectBoxModel.isOnion(onion) && !ConnectBoxModel.isOnion("192.168.1.20:8080"), "an onion name is told apart")
        ok(ConnectBoxModel.linkFor(onion, "127.0.0.1:9050").proxy == "127.0.0.1:9050", "an onion name goes through the proxy")
        ok(ConnectBoxModel.linkFor("192.168.1.20:8080", "127.0.0.1:9050").proxy.isEmpty(), "an address on the network goes directly")
        val look = connectBox().substringAfter("private fun Controller.lookForBox(").substringBefore("private fun Controller.showBoxFound(")
        ok(look.contains("BoxWords.looking") && look.indexOf("waiting(") < look.indexOf("async("), "a page says it takes a moment, and is drawn first")
        ok(look.contains("async({ n.boxFetchClaim(link) })") && Regex("boxFetchClaim\\(").findAll(connectBox()).count() == 1, "one place fetches, off the screen thread")
        ok(look.contains("refusal(") && look.contains("showConnectBox("), "a refusal is the library's words, on the first screen")
        ok(Regex("scan\\.begin\\(").findAll(connectBox()).count() == 1 && !connectBox().contains("requestPermissions"), "the camera is asked for in one place, from the button")
        ok(BoxWords.addressNotRead.who == Who.OWNER && BoxWords.looking.next.isNotEmpty(), "the words of the address")
    }

    private fun oa_42_the_claim_and_the_settings_are_queued_together_and_the_four_words_are_shown() {
        val found = connectBox().substringAfter("private fun Controller.showBoxFound(").substringBefore("private fun Controller.showBoxQueued(")
        ok(found.contains("ConnectBoxModel.keyGroups(found.boxKey)"), "the Box's key in full")
        ok(found.contains("found.needsRate") && found.contains("CounterModel.price(") && found.contains("BoxWords.priceNeeded"), "a phone with no settings asks for the price")
        ok(found.contains("n.boxQueue(found.claim, f.value)") && Regex("boxQueue\\(").findAll(connectBox()).count() == 1, "one call queues the claim and the settings")
        val queued = connectBox().substringAfter("private fun Controller.showBoxQueued(").substringBefore("internal fun Controller.waitingBoxViews(")
        ok(queued.contains("wordCards(ConnectBoxModel.wordsLine(q.words))") && queued.contains("BoxWords.compare") && queued.contains("showSpend()"), "the four words as cards, to compare, then the request screen")
        ok(ConnectBoxModel.wordsLine(listOf("a", "b", "c", "d")) == "a   b   c   d", "the words on one line")
        ok(ConnectBoxModel.keyGroups(ByteArray(32) { it.toByte() }).split(' ').size == 8, "the key is eight groups")
        ok(ConnectBoxModel.boxLabel(byteArrayOf(1, 2, 3, 4, 5)) == "Box 01020304", "a short name")
    }

    private fun oa_43_the_welcome_is_handed_over_from_the_list_and_a_refusal_keeps_the_approval() {
        val c = connectBox()
        val wait = c.substringAfter("internal fun Controller.pendingBoxViews(").substringBefore("private fun Controller.forgetBox(")
        ok(wait.contains("n.boxesPending()") && wait.contains("p.welcomeReady") && wait.contains("boxAddresses[") && wait.contains("handOver("), "the list of Boxes being added, each with its address")
        val over = c.substringAfter("private fun Controller.handOver(").substringBefore("/** What saving the link did")
        ok(Regex("boxDeliver\\(").findAll(c).count() == 1 && over.contains("async({ n.boxDeliver(link, p.boxKey) })"), "one call hands it over, off the screen thread")
        ok(over.contains("BoxWords.handing") && over.indexOf("waiting(") < over.indexOf("async("), "a page says it takes a moment")
        ok(over.contains("BoxWords.handoverKept") && over.contains("refusal("), "a refusal is the library's words and the app's: the approval is kept")
        ok(over.indexOf("showSaveLink(") > over.indexOf("boxDeliver(") && over.contains("if (delivered != null)"), "the link is saved only for a Box that took it")
        ok(BoxWords.handoverKept.what.contains("stays queued") && BoxWords.handoverKept.next.contains("new one"), "the words say what is kept and what to do")
    }

    private fun oa_44_the_link_is_saved_only_after_the_box_took_its_welcome() {
        val key = ByteArray(32) { (0x70 + it).toByte() }
        val c1 = ByteArray(32) { (0x10 + it).toByte() }
        val c2 = ByteArray(32) { (0x20 + it).toByte() }
        val onion = "a".repeat(56) + ".onion:8080"
        val base = setupText(extra = listOf("peer: 10.0.0.5:8333"))
        val direct = ConnectBoxModel.withLink(base, "192.168.1.20:8080", key, listOf(c1, c2))
        val link = OwnerSetup.parse(direct).link!!
        ok((link.route as RelayRoute.Direct).addr == "192.168.1.20:8080", "the route is the Box's address")
        ok(link.relayKey.contentEquals(key) && link.counters.size == 2 && link.counters[0].contentEquals(c1), "the relay key is the Box's key and the counters are the settings'")
        ok(link.heartbeatEverySecs == 0L && link.checkInGraceSecs == 0L, "the intervals are the defaults")
        ok(OwnerSetup.parse(direct).peers == listOf("10.0.0.5:8333"), "nothing else of the setup changes")
        val viaOnion = OwnerSetup.parse(ConnectBoxModel.withLink(base, onion, key, listOf(c1))).link!!
        val route = viaOnion.route as RelayRoute.Onion
        ok(route.host == "a".repeat(56) + ".onion" && route.port == 8080, "an onion route")
        // A link the setup held is replaced, with its intervals.
        val old = setupText(extra = linkLines() + listOf("heartbeat-every: 30", "check-in-grace: 600"))
        val replaced = OwnerSetup.parse(ConnectBoxModel.withLink(old, "192.168.1.20:8080", key, listOf(c1))).link!!
        ok(replaced.counters.size == 1 && replaced.heartbeatEverySecs == 0L && replaced.relayKey.contentEquals(key), "a link already held is replaced")
        // LK-67: the link follows the settings, so a link needs no counter line.
        val none = OwnerSetup.parse(ConnectBoxModel.withLink(base, "192.168.1.20:8080", key, emptyList())).link!!
        ok(none.counters.isEmpty() && none.relayKey.contentEquals(key), "no counter, the link is still saved")
        val many = OwnerSetup.parse(ConnectBoxModel.withLink(base, "192.168.1.20:8080", key, List(17) { i -> ByteArray(32) { b -> (i + b).toByte() } })).link!!
        ok(many.counters.size == OwnerSetup.MAX_COUNTERS, "more counters than a setup holds: the first ones are written")
        val save = connectBox().substringAfter("private fun Controller.showSaveLink(")
        ok(save.contains("OwnerLock.unlock(stored, typed)") && save.contains("setupFile().save(OwnerLock.lock(typed, bytes))"), "the setup is opened and locked again with the same PIN")
        ok(save.contains("triedPin(") && !save.contains("SealedFile("), "a wrong PIN counts as a try, and nothing is stored but the locked text")
        ok(save.indexOf("OwnerSetup.parse(edited)") < save.indexOf("setupFile().save("), "the text is checked before it is sealed")
        ok(save.contains("setup = next") && save.contains("showMessage(BoxWords.paired, \"Start the link\") { begin() }"), "the app starts again, which starts the link, with no second PIN")
        ok(save.contains("BoxWords.linkNotSaved") && save.contains("\"Not now\"") && save.contains("ConnectBoxModel.linkLines("), "when no link is made the lines are shown")
        ok(Regex("setupFile\\(\\)\\.save\\(").findAll(connectBox()).count() == 1, "the one thing written is the locked setup")
        ok(source("src/android/Lock.kt").contains("internal fun Controller.triedPin(") && BoxWords.savePin.next.contains("PIN"), "the PIN is asked for through the lock's own count of tries")
        ok(ConnectBoxModel.linkLines("192.168.1.20:8080", key, listOf(c1)).size == 3, "the lines of a link")
    }

    private fun oa_45_the_app_builds_no_bytes_of_the_pairing_and_signs_nothing() {
        val c = connectBox() + source("src/model/ConnectBoxModel.kt")
        for (bad in listOf(".signBatch(", "Psbt", "psbt", ".broadcast(", "Transaction(", "key2Secret", "key3Secret", "mnemonic", "SecureRandom", "Clipboard", "copy(")) {
            ok(!c.contains(bad), "connecting a Box does not hold or call $bad")
        }
        ok(connectBox().contains("private val boxAddresses = HashMap<String, String>()"), "the addresses of waiting Boxes are held in memory only")
        ok(!connectBox().contains("java.io.File") && !connectBox().contains("SealedFile"), "no file is opened here but through the lock")
    }

    private fun oa_46_without_a_key_app_the_app_does_not_offer_to_pair_a_box() {
        val c = connectBox() + source("src/android/Screens.kt")
        ok(!c.contains("Pair without the Key app") && !c.contains("showOwnInvitation") && !c.contains("OwnerBoxes") && !c.contains("ownInvitation") && !c.contains("ownCheck"), "no button and no screen for pairing without the Key app")
        ok(!source("src/model/BoxWords.kt").contains("ownIntro") && !source("src/model/ConnectBoxModel.kt").contains("spaced"), "and no words for it")
        ok(File(appDir, "../../docs/OWNER-APP-RULES.md").readText().contains("does not offer to pair a Box without the Key app"), "the rules say so")
        ok(source("../../crates/openbmt-owner-ffi/bindings/kotlin/org/openbmt/owner/OwnerNode.kt").contains("fun ownCheck("), "the library keeps its calls")
    }

    private fun oa_47_a_box_that_was_not_delivered_is_forgotten_in_one_step() {
        val c = connectBox()
        val list = c.substringAfter("internal fun Controller.pendingBoxViews(").substringBefore("private fun Controller.handOver(")
        ok(list.contains("ConnectBoxModel.wordsLine(p.words)") && list.contains("\"Forget ${'$'}{ConnectBoxModel.boxLabel(p.boxKey)}\"") && list.contains("forgetBox(n, p.boxKey, msg, back)"), "a Box being added is listed with its four words and a button that forgets it")
        ok(c.substringAfter("private fun Controller.showBoxQueued(").substringBefore("internal fun Controller.pendingBoxViews(").contains("\"Forget this Box\""), "also on the screen after queuing")
        val forget = c.substringAfter("private fun Controller.forgetBox(").substringBefore("/**\n * Hands the Box its welcome")
        ok(Regex("boxForget\\(").findAll(c).count() == 1 && forget.contains("attempt(msg) { n.boxForget(key) }") && forget.contains("BoxWords.forgotten"), "one call forgets, a refusal is the library's words")
        ok(BoxWords.forgotten.what.contains("not told") && BoxWords.forgotten.next.contains("new invitation"), "the words say what was done and how to add the Box again")
    }

    private fun oa_48_every_word_of_connecting_a_box_is_checked_and_listed() {
        val all = BoxWords.all()
        ok(all.size == 13 && all.all { (n, _) -> n.startsWith("box") }, "the words of the Box screens")
        for ((name, w) in all) {
            ok((w.who == Who.OWNER || name == "boxPriceFetching") && SayCheck.problem(w) == null && w.heading.length <= 28 && w.what.length <= 140 && w.next.length <= 140, "words of $name")
        }
        val names = AppWords.all().map { it.first }
        ok(all.all { it.first in names }, "they are in the app's list of words")
        val md = File(appDir, "../../docs/screens/owner-app.md").readText()
        ok(md.contains("| boxPaired |") && md.contains("| boxForgotten |"), "the list of the app's words has them")
    }

    // ---- a hardware signer as key 2 (OA-70 to OA-74) ------------------------------------------------------------

    private fun registrationOf(textFits: Boolean, descriptorFits: Boolean) = org.openbmt.owner.Registration(
        "Corner Cafe", "Name: Corner Cafe\n", "wsh(sortedmulti(2,a,b,c))#aaaaaaaa", "wsh(sortedmulti(2,a,b,c))#bbbbbbbb",
        "wsh(sortedmulti(2,a,b,c))#cccccccc", ByteArray(4), "bc1qfirst", textFits, descriptorFits,
    )

    private fun oa_70_the_person_picks_how_a_payment_reaches_the_signer() {
        screensOfTheTransport()
        val t = HardwareModel.transports
        ok(t.map { it.transport } == listOf(Transport.Ur, Transport.Bbqr, Transport.File), "three transports, in order")
        ok(t.all { it.examples.startsWith("For example ") && it.examples.endsWith(".") }, "each names devices as examples")
        ok(t[0].examples.contains("Keystone") && t[1].examples.contains("Coldcard Q") && t[2].examples.contains("microSD"), "the examples are devices that use it")
        ok(t.map { it.label } == listOf("Moving QR codes (UR)", "Moving QR codes (BBQr)", "A file on a memory card"), "the labels")
        ok(t[0].examples == "For example Keystone, Passport, Jade, Krux and SeedSigner." && t[1].examples == "For example Coldcard Q and SeedSigner.", "the examples of the codes")
        ok(t[2].examples == "For example Coldcard Mk4 with a microSD card.", "the example of the file")
        ok(HardwareModel.RETURN_UR_TYPES == setOf("crypto-psbt", "psbt") && HardwareModel.RETURN_BBQR_KINDS == setOf('P', 'T'), "what is read back")
        ok(HardwareWords.chooseTransport.heading == "How to reach the signer" && HardwareWords.chooseTransport.next == "Pick the one your signer uses. The names under each choice are examples.", "the words of the choice")
        ok(HardwareModel.UR_PSBT == "crypto-psbt" && HardwareModel.BBQR_PSBT == 'P' && HardwareModel.BBQR_TRANSACTION == 'T', "the types of the codes")
        ok(HardwareModel.BBQR_PART_CHARS in 16..4296 && HardwareModel.UR_FRAGMENT_BYTES in 1..0xffff, "the sizes are in the library's range")
        ok(SayCheck.problem(HardwareWords.chooseTransport) == null && HardwareWords.chooseTransport.who == Who.OWNER, "the words say who picks")
    }

    private fun oa_71_the_wallet_is_shown_to_the_signer_in_the_ways_it_reads() {
        val fits = registrationOf(true, true)
        val long = registrationOf(false, false)
        ok(HardwareModel.textWays(fits) == listOf(RegistrationWay.PlainCode, RegistrationWay.UrBytes, RegistrationWay.BbqrText, RegistrationWay.TextFile), "a text that fits is offered as one code first")
        ok(HardwareModel.textWays(long) == listOf(RegistrationWay.UrBytes, RegistrationWay.BbqrText, RegistrationWay.TextFile), "a long text has no plain code")
        ok(HardwareModel.descriptorWays(fits) == listOf(RegistrationWay.DescriptorCode, RegistrationWay.CryptoOutput, RegistrationWay.DescriptorFile), "the descriptor as a code, a crypto-output and a file")
        ok(HardwareModel.descriptorWays(long) == listOf(RegistrationWay.CryptoOutput, RegistrationWay.DescriptorFile), "a long descriptor has no plain code")
        ok(HardwareModel.textFileName(fits) == "Corner_Cafe.txt" && HardwareModel.descriptorFileName(fits) == "Corner_Cafe.descriptor.txt", "the file names")
        ok(SayCheck.problem(HardwareWords.registerIntro) == null, "the introduction passes the checks")
        ok(HardwareModel.textWay(Transport.Ur, fits) == RegistrationWay.PlainCode && HardwareModel.textWay(Transport.Ur, long) == RegistrationWay.UrBytes, "moving UR codes: one plain code when it fits, else ur:bytes")
        ok(HardwareModel.textWay(Transport.Bbqr, fits) == RegistrationWay.BbqrText && HardwareModel.textWay(Transport.File, long) == RegistrationWay.TextFile, "BBQr type U, or the .txt file")
        ok(HardwareModel.UR_OUTPUT == "crypto-output" && HardwareModel.UR_TEXT == "bytes" && HardwareModel.BBQR_TEXT == 'U', "the types of the wallet's codes")
        ok(HardwareWords.registerIntro.heading == "Register the wallet" && HardwareWords.registerIntro.next == "Show the signer this code or file, then check the first address.", "the words of the registration")
    }

    private fun hardwareCode() = source("src/android/Hardware.kt")

    private fun screensOfTheTransport() {
        val h = hardwareCode()
        for (label in listOf("Save the file", "Next: bring it back", "Scan the signer's code", "Open a file", "Show the wallet code", "Save the wallet file", "Show the descriptor code", "Check the first address", "They match", "They differ", "Check later")) {
            ok(h.contains("\"$label\""), "the button $label")
        }
        ok(h.contains("ui.primary(\"They match\")") && h.contains("ui.danger(\"They differ\")") && h.contains("ui.button(\"Check later\")"), "primary, danger and secondary")
        ok(h.contains("ui.primary(\"Scan the signer's code\")") && h.contains("ui.button(\"Open a file\")"), "scan is the main way back")
        ok(h.contains("HardwareModel.RETURN_UR_TYPES") && h.contains("HardwareModel.RETURN_BBQR_KINDS") && h.contains("n.signerTake("), "what is read goes to the library as bytes")
        ok(h.contains("MAX_RETURNED + 1"), "a file one byte over the library's limit is handed over to be refused")
        ok(h.contains("HardwareModel.UR_PSBT") && h.contains("HardwareModel.BBQR_PSBT") && h.contains("file.fileName"), "the payment's codes and its file name")
        ok(h.contains("TODO(OA-60)"), "the relock across the scan and the pickers is named as not done")
        val screens = source("src/android/Screens.kt")
        ok(screens.contains("if (key2Signer) {") && screens.contains("n.signerBuild(nowSecs(), f.value)") && screens.contains("n.buildRequest(nowSecs(), f.value)"), "a spend goes to the signer only when key 2 is one")
        ok(screens.contains("RowItem(\"Register the wallet on your signer\")"), "the wallet screen's row")
        val c = ceremonyCode()
        ok(c.contains("HardwareModel.key2IsSigner(built.pairs)") && c.contains("keepsSignerMark = true"), "the ceremony notes key 2's holder")
        ok(source("src/android/Controller.kt").contains("if (!keepsSignerMark) {"), "a pasted setup clears the note")
    }

    private fun oa_72_the_first_address_check_is_done_or_skipped_with_the_time() {
        val at = { t: Long -> "day $t" }
        ok(HardwareModel.firstAddressLine(null, at).contains("has not been checked"), "not done yet")
        ok(HardwareModel.firstAddressLine(org.openbmt.owner.FirstAddress(true, 5), at) == "The first address was checked on the signer on day 5.", "checked, with the time")
        ok(HardwareModel.firstAddressLine(org.openbmt.owner.FirstAddress(false, 6), at).contains("Skipped on day 6."), "skipped, with the time")
        ok(SayCheck.problem(HardwareWords.firstAddressCompare) == null && HardwareWords.firstAddressCompare.what.contains("address list"), "the compare words name the address list")
        ok(HardwareModel.grouped("bc1qar0srrr7xfkvy5l643lydnw9re59gtzzwf5mdq") == "bc1q ar0s rrr7 xfkv y5l6 43ly dnw9 re59 gtzz wf5m dq", "an address in groups of four")
        ok(HardwareModel.grouped("abcd") == "abcd" && HardwareModel.grouped("") == "", "no stray space at the ends")
        val pairs = { b: Int -> listOf(org.openbmt.owner.PairView(1, 2, 1, b), org.openbmt.owner.PairView(1, 3, 1, 4), org.openbmt.owner.PairView(2, 3, b, 4)) }
        ok(HardwareModel.key2IsSigner(pairs(3)) && !HardwareModel.key2IsSigner(pairs(2)), "key 2 is a signer only when its holder is another signer")
        ok(!HardwareModel.key2IsSigner(listOf(org.openbmt.owner.PairView(1, 3, 1, 3))), "another signer holding key 3 does not count")
        val d = HardwareWords.firstAddressDiffers
        ok(d.tone == Tones.STOP && d.who == Who.OWNER && d.heading == "Do not use with this signer" && SayCheck.problem(d) == null, "a differing address is a stop for the owner")
        ok(d.next == "Register the wallet again on the signer, then check again. If it still differs, do not take money into this wallet.", "and says what to do")
        val saved = HardwareWords.fileSaved
        ok(saved.heading == "File saved" && saved.what == "The payment for the signer is in the file named below." && saved.who == Who.OWNER && saved.tone == Tones.INFO, "the words of a saved file")
        ok(saved.next == "Put the file on the signer's memory card and sign it there. Then bring the signed file back.", "and what to do with it")
        ok(HardwareModel.signerSession(true, null) == HardwareModel.SignerSession.Exported, "a session with its file was exported")
        ok(HardwareModel.signerSession(false, "AS-92") == HardwareModel.SignerSession.NotExported, "AS-92 is a signer session not yet given")
        ok(HardwareModel.signerSession(false, "AS-90") == HardwareModel.SignerSession.NotSigner && HardwareModel.signerSession(false, null) == HardwareModel.SignerSession.NotSigner, "any other refusal is the Key app's session")
        val screens = source("src/android/Screens.kt")
        ok(screens.contains("ui.button(\"Continue on the signer\")") && hardwareCode().contains("showSignerReturn(run, null)"), "an exported session continues at the way back")
        val ceremony = ceremonyCode()
        ok(ceremony.contains("ui.primary(\"Register the wallet on your signer\")") && ceremony.contains("ui.button(\"Save and start\")"), "the last ceremony screen: register first, save second")
        ok(HardwareWords.firstAddressChecked.next == "Nothing to do." && HardwareWords.firstAddressSkipped.next == "Check it from the wallet screen before the shop takes money.", "checked and skipped")
        ok(HardwareWords.firstAddressSkipped.tone == Tones.INFO && HardwareWords.firstAddressChecked.tone == Tones.GOOD, "skipped is not good, checked is")
    }

    private fun oa_73_the_owner_compares_every_line_with_the_signers_screen() {
        val lines = HardwareModel.compareLines(listOf("bcrt1qpay" to 40_000L), "bcrt1qchange" to 150_000L, 473)
        ok(lines.size == 3 && lines[0].startsWith("Pay ") && lines[0].endsWith(" to bcrt1qpay"), "each payment with its address: $lines")
        ok(lines[1].startsWith("Change ") && lines[1].endsWith(" at bcrt1qchange"), "the change back to the wallet")
        ok(lines[2].startsWith("Fee "), "the fee")
        ok(HardwareModel.compareLines(listOf("a" to 1L), null, 2).size == 2, "no change, no change line")
        ok(HardwareWords.signerReview.what.contains("second check") && HardwareWords.signerReview.next.contains("only if they all match"), "the words say the signer is the second check")
        ok(SayCheck.problem(HardwareWords.signerReview) == null, "the words pass the checks")
        ok(HardwareWords.signerReview.heading == "Check it on the signer" && HardwareWords.signerReview.what == "The signer's own screen is the second check. It shows each address, each amount and the fee.", "the words of the review")
    }

    private fun oa_74_a_refused_step_changes_nothing_and_a_missing_part_is_said() {
        ok(SayCheck.problem(HardwareWords.partsMissing) == null && HardwareWords.partsMissing.what.contains("any order"), "a missing part is said, and the order is free")
        ok(SayCheck.problem(HardwareWords.returnedNeeded) == null, "what to bring back is said")
        ok(HardwareWords.returnedNeeded.heading == "Bring the payment back" && HardwareWords.partsMissing.heading == "Keep scanning", "the headings")
        ok(HardwareModel.partsLine(3, 9) == "3 of 9 parts read", "the progress line")
        val own = org.openbmt.app.SayWords(Tones.STOP, Who.OWNER, "h", "w", "n", null, null, null, null)
        ok(HardwareModel.scanFailure(false, null, own) === own, "a reading's own words are shown")
        ok(HardwareModel.scanFailure(true, null, null).heading == org.openbmt.app.CameraWords.off(Who.OWNER).heading, "a camera that is off for the app")
        ok(HardwareModel.scanFailure(false, "no camera", null).heading == org.openbmt.app.CameraWords.notStarted(Who.OWNER, "no camera").heading, "a camera that did not start")
        ok(HardwareWords.all().all { SayCheck.problem(it.second) == null }, "every words of the hardware steps pass the checks")
        ok(HardwareWords.all().map { it.first }.toSet().size == HardwareWords.all().size, "the names differ")
        ok(AppWords.all().map { it.first }.containsAll(HardwareWords.all().map { it.first }), "they are in the list that is checked and written")
    }

    // ---- watching in the background (OA-80 to OA-89) --------------------------------------------------------------

    /** The Keystore's stand-in: AES-GCM under a key of the test's own. */
    private fun aesSealer(): Sealer {
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

    private fun oa_80_the_setting_is_off_until_it_is_turned_on_and_has_its_words() {
        val dir = tempDir()
        val store = WatchStore(dir, aesSealer())
        ok(!store.enabled() && !store.hasKey(), "off by default, with no key")
        store.setEnabled(true)
        ok(store.enabled() && File(dir, AppFiles.WATCH_ON_FILE).exists(), "the marker file is the setting")
        store.setEnabled(true)
        store.setEnabled(false)
        ok(!store.enabled(), "and off again")
        clean(dir)
        val w = AppWords.watchSetting
        ok(w.heading == "Watch in the background", "the heading")
        ok(w.what == "The phone keeps following the shop's wallet while the app is closed, and tells the counters it is watching.", "what it does")
        ok(AppWords.watchSettingMore == "Without this, instant sales stop about ten minutes after you close the app.", "and the rest of it")
        ok(w.next == "Turn it on to keep instant sales working. Android shows a notification while it runs.", "the next step")
        ok(SayCheck.problem(w) == null && SayCheck.problem(AppWords.watchNotificationsOff) == null, "the words pass the checks")
        val screen = source("src/android/WatchScreen.kt")
        ok(screen.contains("AppWords.watchSetting") && screen.contains("AppWords.watchSettingMore") && source("src/android/Screens.kt").contains("RowItem(AppWords.watchSetting.heading) { showWatch() }"), "the home screen opens it")
        val controller = source("src/android/Controller.kt")
        ok(controller.contains("watchStore.enabled()") && !controller.contains("setEnabled(true)"), "only the setting's own screen turns it on")
        val order = screen.substringAfter("private fun Controller.makeWatchReady(")
        ok(order.indexOf("watchStore.saveSecret(") < order.indexOf("n.watchSettingsPreview(true") && order.indexOf("n.watchExport(") < order.indexOf("watchStore.setEnabled(true)"), "the key and the contents come before the marker, so a failure leaves it off")
        ok(screen.contains("is java.io.IOException -> AppWords.fileNotSaved") && screen.contains("showWatch(watchRefusal(e))"), "a failure is words")
    }

    private fun oa_81_the_watch_key_is_random_sealed_on_its_own_and_holds_no_wallet() {
        val dir = tempDir()
        val store = WatchStore(dir, aesSealer())
        val made = WatchSecret.draw(SecureRandom()) { it.copyOf() }!!
        val again = WatchSecret.draw(SecureRandom()) { it.copyOf() }!!
        ok(made.first.size == 32 && !made.first.contentEquals(again.first), "32 random bytes, a new draw each time")
        var refused = 0
        ok(WatchSecret.draw(SecureRandom()) { refused++; throw org.openbmt.owner.OwnerException("watch-key", "no", 4) } == null && refused == 3, "a secret the library refuses is drawn again, three times")
        store.saveSecret(made.first)
        store.saveContents(byteArrayOf(1, 2, 3, 4))
        ok(store.hasKey() && store.loadSecret()!!.contentEquals(made.first) && store.loadContents()!!.contentEquals(byteArrayOf(1, 2, 3, 4)), "both read back")
        ok(dir.listFiles()!!.map { it.name }.sorted() == listOf(AppFiles.WATCH_FILE, AppFiles.WATCH_SECRET_FILE).sorted(), "two files, and no temporary file")
        val onDisk = String(File(dir, AppFiles.WATCH_SECRET_FILE).readBytes(), Charsets.ISO_8859_1)
        ok(!onDisk.contains(String(made.first, Charsets.ISO_8859_1)), "the secret is not on the disk in the clear")
        store.setEnabled(true)
        store.remove()
        ok(!store.enabled() && !store.hasKey() && dir.listFiles()!!.isEmpty(), "removing leaves nothing")
        clean(dir)
        ok(AppFiles.WATCH_KEY_ALIAS == "openbmt-owner-watch-v1" && AppFiles.WATCH_KEY_ALIAS != AppFiles.KEY_ALIAS && AppFiles.WATCH_KEY_ALIAS != AppFiles.POS_KEY_ALIAS, "a Keystore key of its own")
        val model = source("src/model/WatchModel.kt")
        ok(model.contains("SecureRandom") && model.contains("random.nextBytes(secret)"), "drawn from the random source")
        for (bad in listOf("digest", "Mac.", "pbkdf", "SETUP_FILE", "KEY_ALIAS,", "ownerSecret", "key1")) ok(!model.contains(bad), "the watch key is derived from nothing: no $bad")
        val lock = source("src/model/SetupLock.kt")
        ok(!lock.contains("WATCH"), "the wipe order of OA-64 is as it was")
        ok(source("src/android/Lock.kt").contains("watchStore.remove()"), "the wipe removes the watch files")
        ok(source("src/model/AppConfig.kt").contains("no PIN"), "the files say they ask for no PIN")
    }

    private fun oa_82_the_service_is_foreground_special_use_with_an_ongoing_notification() {
        val service = source("src/android/WatchService.kt")
        ok(service.contains("FOREGROUND_SERVICE_TYPE_SPECIAL_USE") && service.contains("startForeground("), "a foreground service of type specialUse")
        ok(service.contains("setOngoing(true)"), "an ongoing notification")
        ok(service.contains("AppWords.watchNotificationTitle") && service.contains("WatchNotice.text(state)") && service.contains("AppWords.watchNotificationStop"), "its title, text and action are the app's words")
        ok(service.contains("return START_NOT_STICKY") && !service.contains("START_STICKY") && !service.contains("START_REDELIVER"), "never sticky")
        ok(AppWords.watchNotificationTitle == "Watching the shop" && AppWords.watchNotificationText == "Instant sales stay on while this runs." && AppWords.watchNotificationStop == "Stop", "the notification's words")
        ok(WatchNotice.text(WatchNotice.State.Watching) == "Instant sales stay on while this runs.", "the text while it runs")
        val controller = source("src/android/Controller.kt")
        val adopt = controller.substringAfter("private fun adopt(").substringBefore("val pump")
        ok(adopt.contains("watchStore.enabled()") && adopt.contains("WatchService.start(act)"), "it is started while the app is on the screen, when the setting is on")
        ok(service.contains("fun start(context: Context)") && !service.contains("BOOT_COMPLETED") && !source("AndroidManifest.xml").contains("BOOT_COMPLETED"), "nothing starts it after a boot")
    }

    private fun oa_83_the_notification_permission_is_asked_and_a_refusal_leaves_it_off() {
        ok(!WatchPermission.mustAsk(32, false) && !WatchPermission.mustAsk(29, false), "not asked before Android 13")
        ok(WatchPermission.mustAsk(33, false) && WatchPermission.mustAsk(36, false), "asked from Android 13")
        ok(!WatchPermission.mustAsk(34, true), "not asked again when it is granted")
        val w = WatchPermission.refused
        ok(w.heading == "Notifications are off", "the heading")
        ok(w.what == "Android needs to show a notification while the phone watches in the background, and notifications are off for this app.", "what happened")
        ok(w.next == "Allow notifications for OpenBMT Owner in Android's settings, then turn this on again.", "the next step")
        val screen = source("src/android/WatchScreen.kt")
        ok(screen.contains("Manifest.permission.POST_NOTIFICATIONS") && screen.contains("WatchPermission.mustAsk(Build.VERSION.SDK_INT, granted)"), "asked by the model's rule")
        val result = screen.substringAfter("fun Controller.watchPermissionResult").substringBefore("private fun Controller.watchTurnOnGranted")
        ok(result.contains("else showWatch(WatchPermission.refused)"), "a refusal shows the words and the setting stays off")
        ok(source("src/android/Controller.kt").contains("if (code == REQUEST_NOTIFY) return watchPermissionResult(results)"), "the answer reaches it")
        // Naming the key is a change of the settings that the Key app approves, through the existing queue.
        ok(screen.contains("n.queueSettings(p.settings)") && screen.contains("AppWords.editQueuedLoosening") && screen.contains("showSpend()"), "the key is named through the queue and the Key app's approval")
        ok(screen.contains("n.watchSettingsPreview(true, key)"), "from the library's own preview")
    }

    private fun oa_84_the_node_and_the_watcher_never_run_together() {
        val controller = source("src/android/Controller.kt")
        val hidden = controller.substringAfter("fun hidden()").substringBefore("internal fun begin()")
        ok(hidden.indexOf("handOverContents()") in 0 until hidden.indexOf("stopNode()") && hidden.indexOf("stopNode()") < hidden.indexOf("WatchRunner.handOver()"), "the contents are written, the node stops, then the watcher takes over")
        val visible = controller.substringAfter("fun visible()").substringBefore("fun hidden()")
        ok(visible.indexOf("WatchRunner.reclaim()") in 0 until visible.indexOf("begin()"), "the watcher lets go before the node starts")
        ok(controller.contains("n.watchExport(secret)") && controller.contains("secret.fill(0)"), "the export is made from the secret read for it only")
        val service = source("src/android/WatchService.kt")
        ok(service.contains("private val lock = Any()") && service.contains("fun reclaim() = synchronized(lock)") && service.contains("synchronized(lock) {"), "a step, a hand-over and a reclaim are one at a time")
        ok(SetupLock.gate(0, true) == Gate.Unlock, "the lock still governs the screens")
        ok(!service.contains("SETUP_FILE") && !service.contains("AppFiles.KEY_ALIAS") && !service.contains("unlock"), "the service asks for no PIN and holds no setup")
    }

    private fun oa_85_stop_and_turning_it_off_end_the_watch_and_remove_the_key_from_the_settings() {
        val service = source("src/android/WatchService.kt")
        val stop = service.substringAfter("private fun turnOff()").substringBefore("private fun show(")
        ok(service.contains("ACTION_STOP") && stop.contains("setEnabled(false)") && stop.contains("WatchRunner.end()") && stop.contains("stopSelf()"), "Stop turns the setting off and ends the service")
        ok(service.contains("addAction("), "the notification has the action")
        val screen = source("src/android/WatchScreen.kt")
        val off = screen.substringAfter("private fun Controller.watchTurnOff()").substringBefore("private fun Controller.watchRefusal")
        ok(off.indexOf("WatchService.stop(act)") in 0 until off.indexOf("watchStore.remove()") && off.indexOf("watchStore.remove()") < off.indexOf("n.watchSettingsPreview(false"), "the service stops, the files go, then the removal is queued")
        ok(off.contains("AppWords.editQueuedTightening"), "a tightening, in the words of one")
    }

    private fun oa_86_the_watcher_signs_heartbeats_and_nothing_else() {
        val files = listOf("src/android/WatchService.kt", "src/android/WatchScreen.kt", "src/model/WatchModel.kt")
        val service = source("src/android/WatchService.kt")
        ok(service.contains("OwnerWatch.start(bytes)") && !service.contains("OwnerNode.start("), "the service starts the watcher, not a node")
        for (name in files) {
            val src = source(name)
            for (bad in listOf("signBatch", "counterConfirm", "counterScan", "approveMerge", ".take(", "buildRequest", "recoverKey1", "queueRefund", "cashOut", "OwnerSetup", "SETUP_FILE", "ownerSecret")) {
                ok(!src.contains(bad), "$name does not use $bad")
            }
        }
        ok(!service.contains("queueSettings") && !service.contains("watchSettingsPreview"), "the service changes no settings")
        val screen = source("src/android/WatchScreen.kt")
        ok(Regex("\\.queueSettings\\(p\\.settings\\)").findAll(screen).count() == 2 && !screen.contains("SignedSettings"), "the screen queues the bytes of a preview and builds none")
        ok(screen.contains("{ watchTurnOn() }") && screen.contains("{ watchTurnOff() }"), "on a press")
    }

    private fun oa_87_the_watcher_takes_no_wake_lock_and_no_alarm_and_keeps_the_links_pace() {
        ok(WatchSchedule.STEP_WAIT_MS == 1000L && WatchSchedule.IDLE_MS == 500L, "a step waits at most 1000 milliseconds")
        for (name in listOf("src/android/WatchService.kt", "src/android/WatchScreen.kt", "src/model/WatchModel.kt", "src/android/Controller.kt")) {
            val src = source(name)
            for (bad in listOf("WakeLock", "PowerManager", "AlarmManager", "JobScheduler", "WorkManager", "setExact", "REQUEST_IGNORE_BATTERY", "setAlarmClock")) ok(!src.contains(bad), "$name does not use $bad")
        }
        val manifest = source("AndroidManifest.xml")
        ok(!manifest.contains("WAKE_LOCK") && !manifest.contains("ALARM") && !manifest.contains("BATTERY"), "no permission for them")
        ok(source("src/android/WatchService.kt").contains("WatchSchedule.STEP_WAIT_MS"), "the loop's wait is the schedule's")
    }

    private fun oa_88_the_watcher_asks_the_proxy_first_and_says_when_it_does_not_answer() {
        ok(WatchSchedule.orbotWaitMs(null, 1_000) == 0L, "the first ask is at once")
        ok(WatchSchedule.orbotWaitMs(1_000, 1_000 + 10_000) == 20_000L, "then every 30 seconds")
        ok(WatchSchedule.orbotWaitMs(1_000, 1_000 + 30_000) == 0L && WatchSchedule.orbotWaitMs(1_000, 0) == WatchSchedule.ORBOT_RETRY_MS, "and never longer than that")
        ok(WatchNotice.text(WatchNotice.State.NoOrbot) == "Not watching: Tor is not connected. Instant sales are off.", "the words")
        val service = source("src/android/WatchService.kt")
        ok(service.indexOf("TorGate(config.proxy).available()") in 1 until service.indexOf("OwnerWatch.start(bytes)"), "the gate is asked before the watcher starts")
        ok(service.contains("WatchNotice.State.NoOrbot") && !service.contains("withoutTor"), "the notification says so, and nothing goes around the proxy")
    }

    private fun oa_89_the_manifest_has_the_three_permissions_and_the_one_service_of_the_watch() {
        val manifest = source("AndroidManifest.xml")
        val service = manifest.substringAfter("<service").substringBefore("</service>")
        ok(service.contains("android:name=\"org.openbmt.owner.app.WatchService\"") && service.contains("android:exported=\"false\"") && service.contains("android:foregroundServiceType=\"specialUse\""), "the service, not exported, of type specialUse")
        ok(service.contains("android:name=\"android.app.PROPERTY_SPECIAL_USE_FGS_SUBTYPE\"") && service.contains("android:value=\"Follows the shop's own wallet and tells the shop's counters it is watching\""), "and its use")
        ok(Regex("<property").findAll(manifest).count() == 1 && !manifest.substring(manifest.indexOf("<service")).contains("<intent-filter"), "one property, and the service has no filter")
        for (bad in listOf("BOOT_COMPLETED", "<receiver", "<provider", "<activity-alias", "WAKE_LOCK", "SCHEDULE_EXACT_ALARM", "USE_EXACT_ALARM")) ok(!manifest.contains(bad), "no $bad")
        val check = source("tests/check-manifest.sh")
        ok(check.contains("foregroundServiceType.*0x40000000") && check.contains("PROPERTY_SPECIAL_USE_FGS_SUBTYPE") && check.contains("-eq 5 ]") && check.contains("for kind in receiver provider activity-alias"), "the script checks the built APK for all of it")
        for (other in listOf("../key-android/AndroidManifest.xml", "../counter-android/AndroidManifest.xml")) {
            val m = source(other)
            ok(!m.contains("<service") && !m.contains("FOREGROUND_SERVICE") && !m.contains("POST_NOTIFICATIONS"), "$other is not changed")
        }
    }

    // ---- the look of the screens (OA-91 to OA-93), the currency (OA-95, OA-96) -------------------------------------------------------------

    private fun oa_91_every_screen_is_a_page_of_the_shared_look_and_adds_no_word_but_the_group_titles() {
        val screens = listOf(
            "Screens.kt", "Controller.kt", "Lock.kt", "Ceremony.kt", "Approval.kt", "Hardware.kt", "AddCounter.kt", "ConnectBox.kt",
            "SettingsEdit.kt", "WatchScreen.kt", "PosScreen.kt",
        )
        for (name in screens) {
            val src = source("src/android/$name")
            ok(!src.contains("ui.page(") && !src.contains("page(listOf(") && !src.contains("page(views)"), "$name has no screen of the old look")
            ok(!src.contains("ui.words(WordsModel") && !src.contains("wordsBelowBand") && !src.contains("SaleBand"), "$name uses no band of the old look")
            ok(!src.contains("Feedback.outcome") && !src.contains("Sound."), "$name plays no outcome sound")
        }
        val ui = source("src/android/OwnerUi.kt")
        ok(ui.contains("ui.resultPanel(") && ui.contains("ui.working(") && ui.contains("ui.compactWords(") && ui.contains("ui.cameraFrame(") && ui.contains("ui.titleBlock("), "the types of screen are the shared ones")
        ok(source("src/android/Controller.kt").contains("showScan(v)") && occurs(source("src/android/Ceremony.kt"), "showScan(v)") == 2, "the scanners go through the camera frame")
        val withSnap = screens.filter { source("src/android/$it").contains("LogoSnapView(") }
        ok(withSnap == listOf("Ceremony.kt") && occurs(source("src/android/Ceremony.kt"), "LogoSnapView(") == 1, "the logo clicks together in one place, the new wallet's screen: $withSnap")
        ok(source("src/android/Ceremony.kt").contains("ceremonyLogoShown"), "and once")
        val shaking = listOf("Lock.kt", "ConnectBox.kt").filter { source("src/android/$it").contains("Feedback.shake(") }
        ok(shaking == listOf("Lock.kt", "ConnectBox.kt") && occurs(source("src/android/Lock.kt"), "Feedback.shake(") == 1, "a refused PIN shakes its field")
        val newWords = setOf(OwnerLook.WAITING_TITLE, OwnerLook.MONEY, OwnerLook.DEVICES, OwnerLook.ALERTS, OwnerLook.SETTINGS)
        ok(newWords == setOf("Open approvals and queues", "Money", "Counters and devices", "Alerts and notes", "Settings and this phone"), "the group titles are as written")
    }

    private fun oa_92_home_is_one_status_card_one_card_of_four_rows_and_a_card_for_the_counts_it_has() {
        for (sats in listOf(0L, 1L, 1_204_500L, 12_345_678_901L)) {
            val b = OwnerLook.balance(Format.sats(sats), 912_345, 912_400)
            val whole = "The wallet can spend ${Format.sats(sats)}. Checked to block 912345 of 912400."
            ok(b.sentence() == whole, "the balance sentence is the three parts joined: ${b.sentence()}")
            ok(b.lead == "The wallet can spend" && b.amount == Format.sats(sats) && b.checked == "Checked to block 912345 of 912400.", "the parts")
        }
        ok(OwnerLook.moneyTitles.take(3) == listOf("Receipts and merge", "Cash out", "Old addresses"), "the first three entries of Money")
        ok(OwnerLook.moneyTitles.last() == "Waiting to be delivered" && OwnerLook.moneyTitles.toSet().size == OwnerLook.moneyTitles.size, "the rest, each once")
        ok(OwnerLook.moneySub(0, OwnerModel.sendsLine(0)) == "Receipts and merge, Cash out, Old addresses", "no payments line when none is in progress")
        ok(OwnerLook.moneySub(2, OwnerModel.sendsLine(2)) == "Receipts and merge, Cash out, Old addresses\n2 payments in progress.", "the payments line under the sub")
        val rows = OwnerLook.waitingRows(listOf("Open approvals" to 1, "Waiting to be delivered" to 0, "Refund queue" to null, "Refund notes" to 3))
        ok(rows.map { it.title to it.count } == listOf("Open approvals" to 1, "Refund notes" to 3), "a row for a count above zero only")
        ok(OwnerLook.waitingRows(listOf("Open approvals" to null, "Refund queue" to null)).isEmpty(), "no count, no row")
        val home = source("src/android/Screens.kt").substringAfter("internal fun Controller.showHome()").substringBefore("/** The screen a row of the waiting card opens. */")
        for (call in listOf(".sessions()", ".outbox()", ".queue()", ".mergeOffer()", ".receiptsLine()", ".refundNotes()")) ok(!home.contains(call), "home makes no call $call")
        for (call in listOf(".status()", ".linkState()", ".sendStates()")) ok(home.contains(call), "home keeps its call $call")
        ok(home.contains("showSpend()") && home.contains("ui.primary(\"Spend\")"), "Spend is the footer's primary")
    }

    private fun oa_93_settings_cut_the_instant_lines_into_a_table_and_fold_the_rest() {
        val lines = OwnerModel.settingsLines(sampleSettings())
        val (table, rest) = OwnerLook.instantTable(lines)
        ok(table.columns == listOf("Largest instant sale", "Instant total"), "the two columns")
        ok(table.rows.map { it.first } == listOf("Light check", "Own node", "Box reports"), "a row for each check, its first letter in upper case: ${table.rows}")
        ok(table.rows.all { it.second == "$7 (7,000 sats)" && it.third == "$7 (7,000 sats)" }, "the values are the lines' own")
        ok(table.tolerance == ("Underpayment still counted" to "1.05%"), "the tolerance")
        // The only change to a label is the first letter of the row name: "Box reports" keeps its capital in the line.
        val back = lines.filter { l -> table.rows.any { (name, perSale, total) -> table.columns.any { c -> l.first.equals("$c, $name", ignoreCase = true) && l.second in listOf(perSale, total) } } }
        ok(back.size == 6 && table.rows.size * 2 == back.size, "every row cuts back into the two lines it came from: ${back.map { it.first }}")
        ok(back.all { l -> table.rows.any { r -> l.first.substringAfter(", ").replaceFirstChar { it.uppercase() } == r.first } }, "and only the first letter differs")
        ok(rest.none { it.first.startsWith("Largest instant sale") || it.first.startsWith("Instant total") }, "the lines of the table are not left in the rest")
        val folded = OwnerLook.settingsFolded(rest)
        ok(folded.map { it.first } == listOf("Currency", "Payments", "Tips", "Refunds and overrides", "Safety", "Counters' shares", "Lightning node"), "the groups: ${folded.map { it.first }}")
        val every = folded.flatMap { it.second }
        val expected = lines.filter { it.first != "Version" && it !in back && it.first != "Underpayment still counted" }
        ok(every.toSet() == expected.toSet() && every.size == expected.size, "every other line is in a group once, and the version is the bar's")
        ok(OwnerLook.settingsSub(3) == "Settings version 3", "the sub-line")
        val withWatch = OwnerLook.settingsFolded(rest + ("Watch in the background" to "on"))
        ok(withWatch.last() == ("Watch in the background" to listOf("Watch in the background" to "on")), "a line about the watch has its group")
        ok(OwnerLook.capital("light check") == "Light check" && OwnerLook.capital("") == "", "the first letter")
        val page = source("src/android/Screens.kt").substringAfter("internal fun Controller.showSettings()").substringBefore("internal fun Controller.showAbout()")
        ok(page.contains("instantCard(table)") && page.contains("foldedCard(folded)") && page.contains("ui.primary(\"Change settings\")"), "the page has the card, the folds and the footer's primary")
    }

    // ---- the till's token queue (OA-94) -------------------------------------------------------------------------

    private fun oa_94_the_till_token_file_has_one_queue_and_a_stale_callback_draws_nothing() {
        val dir = tempDir()
        val posted = java.util.concurrent.LinkedBlockingQueue<() -> Unit>()
        val queue = PosTokenQueue(PosTokenStore(dir, testSealer())) { posted.put(it) }
        ok(!queue.exists() && queue.load() == null, "no token at first")
        // A remove asked, then a save: the order asked is the order done, so the new token stays.
        val results = ArrayList<String>()
        queue.save("OLD-token") { }
        queue.remove { results.add("removed") }
        queue.save("NEW-token") { results.add("saved") }
        ok(queue.isRemoving(), "a removal in flight is known to the screen")
        ok(queue.load() == "NEW-token" && queue.exists(), "a remove then a save leaves the token")
        repeat(3) { posted.take()() }
        ok(results == listOf("removed", "saved") && !queue.isRemoving(), "the callbacks come in the order asked: $results")
        ok(dir.listFiles()!!.map { it.name } == listOf(AppFiles.POS_TOKEN_FILE), "one file")
        queue.remove { }
        posted.take()()
        ok(!queue.exists() && queue.load() == null, "a lone remove removes it")
        // Many from many threads still run one at a time, and each load sees the last save asked before it.
        val q = PosTokenQueue(PosTokenStore(dir, testSealer())) { posted.put(it) }
        for (i in 0 until 20) {
            q.save("t$i") { }
            ok(q.load() == "t$i", "load $i follows its save")
        }
        repeat(20) { posted.take()() }
        clean(dir)
        // A callback draws only under the number it started with.
        val epoch = TillEpoch()
        val first = epoch.current()
        var drawn = 0
        ok(epoch.draw(first) { drawn++ } && drawn == 1, "a current callback draws")
        val after = epoch.advance()
        ok(!epoch.draw(first) { drawn++ } && drawn == 1, "a stale callback does nothing")
        ok(epoch.draw(after) { drawn++ } && drawn == 2 && after > first, "and the new number draws")
        val pos = source("src/android/PosScreen.kt")
        ok(pos.contains("posTokens.remove { r ->") && pos.contains("posTokens.save(token) { r ->") && !pos.contains("async("), "the screen goes through the queue")
        ok(pos.contains("tillEpoch.advance()") && occurs(pos, "tillEpoch.draw(started)") == 2, "the number goes up and both callbacks are guarded")
        ok(pos.contains("posTokens.isRemoving()") && source("src/android/Controller.kt").contains("PosTokenQueue(PosTokenStore(dir"), "no actions while the removal runs")
        ok(source("src/android/Controller.kt").contains("posTokens.load()") && !source("src/android/Controller.kt").contains("PosTokenStore.load"), "the till thread reads through the queue")
    }

    private fun oa_95_the_currency_is_the_first_group_and_a_new_currency_asks_for_every_limit_again() {
        val e = source("src/android/SettingsEdit.kt")
        ok(AppWords.editGroupCurrency.heading == "Currency" && SettingsEditModel.groups.first() === SettingsEditModel.currency, "the currency is the first group")
        ok(AppWords.editGroupCurrency.what == "The money the counters price sales in, and the limits are set in." && AppWords.editGroupCurrency.next == "Choose one. Bitcoin Standard prices in sats and uses no price source.", "the words of the group")
        ok(AppWords.editCurrencyRetype.heading == "New currency, new limits" && AppWords.editCurrencyRetype.what == "Every limit is set again in the new currency, so the change shows them empty." && AppWords.editCurrencyRetype.next == "Type each limit in the new currency, then review. The change needs the Key app.", "the words of the new currency")
        ok(AppWords.editCurrencyRetype.who == Who.OWNER && AppWords.editGroupCurrency.who == Who.OWNER, "both are the owner's")
        ok(ShopCurrency.entries.map { it.label } == listOf("US dollar (USD)", "Euro (EUR)", "Pound sterling (GBP)", "Japanese yen (JPY)", "Australian dollar (AUD)", "Indian rupee (INR)", "South Korean won (KRW)", "Indonesian rupiah (IDR)", "Singapore dollar (SGD)", "Bitcoin Standard (sats)"), "the ten choices in order")
        ok(e.contains("ShopCurrency.entries") && e.contains("ui.chips(choices.map { it.label }") && e.contains("settingsDraft.chooseCurrency(") && e.contains("AppWords.editCurrencyRetype"), "the screen offers the choices and says every limit is typed again")
        val held = sampleSettings()
        val limit = SettingsEditModel.fields.first { it.kind == FieldKind.Dollars }
        val d = SettingsDraft()
        ok(d.currency == null && SettingsEditModel.input(limit, held, d) == "7", "a shop that keeps its currency shows its limits")
        d.typed[limit.id] = "9"
        d.price = "98500"
        d.chooseCurrency(ShopCurrency.Usd, held.currency)
        ok(d.currency == null && d.typed[limit.id] == "9" && d.price == "98500", "choosing the held currency changes nothing")
        d.chooseCurrency(ShopCurrency.Eur, held.currency)
        ok(d.currency == ShopCurrency.Eur && d.typed.isEmpty() && d.price.isEmpty() && !d.isEmpty, "a new currency forgets the limits and the price typed in the old one")
        ok(SettingsEditModel.input(limit, held, d) == "" && SettingsEditModel.now(limit, held) == "$7", "its limits start empty and the line Now: is still the old one")
        val bps = SettingsEditModel.fields.first { it.kind == FieldKind.Percent }
        ok(SettingsEditModel.input(bps, held, d) == "1.05", "a field that is not a limit keeps what it held")
        d.typed[limit.id] = "7"
        val a = (SettingsEditModel.args(d, held) as Form.Ok).value
        ok(a.currency == ShopCurrency.Eur && SettingsEditModel.args(SettingsDraft(), held).let { it is Form.Ok && it.value.currency == null }, "the edit names the currency only when it changed")
        d.price = "98,500"
        ok((SettingsEditModel.args(d, held) as Form.Ok).value.usdPerBtc == 98_500L, "the price is the one typed")
        d.price = "x"
        ok((SettingsEditModel.args(d, held) as Form.Refused).words.what.contains("whole number of euros"), "a price that is not a number says which currency")
        d.chooseCurrency(ShopCurrency.Usd, held.currency)
        ok(d.currency == null && d.isEmpty, "choosing the old currency again leaves no change")
    }

    private fun oa_96_the_editor_names_the_shops_currency_where_it_said_dollars_and_the_settings_show_it() {
        val usd = AppWords.editFields.toMap()
        for (c in ShopCurrency.entries) {
            val here = AppWords.editFieldsIn(c).toMap()
            for ((id, w) in usd) {
                val was = listOf(w.heading, w.what, w.next)
                val now = listOf(here.getValue(id).heading, here.getValue(id).what, here.getValue(id).next)
                ok(now == was.map { it.replace("Whole dollars", "Whole ${c.plural}") }, "the words of $id differ only where dollars stood: $c")
                ok(SayCheck.problem(here.getValue(id)) == null, "the words of $id in $c pass the checks")
            }
            val pairs = listOf(
                AppWords.editGroupInstant to AppWords.editGroupInstantIn(c),
                AppWords.editGroupRefunds to AppWords.editGroupRefundsIn(c),
            )
            for ((was, now) in pairs) {
                ok(now.heading == was.heading && now.what == was.what && now.next == was.next.replace("Whole dollars", "Whole ${c.plural}"), "the group's words differ only where dollars stood: $c")
            }
            val bad = AppWords.editValueNotRead(FieldKind.Dollars, "Refunds per day", c)
            val base = AppWords.editValueNotRead(FieldKind.Dollars, "Refunds per day")
            ok(bad.what == base.what.replace("dollars", c.plural) && bad.next == base.next.replace("dollars", c.plural) && SayCheck.problem(bad) == null, "the value not read says $c")
            val price = AppWords.priceNotReadIn(c)
            ok(price.what == AppWords.priceNotRead.what.replace("dollars", c.plural) && price.next == AppWords.priceNotRead.next.replace("dollars", c.plural) && SayCheck.problem(price) == null, "the price not read says $c")
        }
        ok(AppWords.editGroupInstantIn(ShopCurrency.Usd) === AppWords.editGroupInstant && AppWords.priceNotReadIn(ShopCurrency.Usd) === AppWords.priceNotRead, "dollars are the words as before")
        ok(AppWords.editFieldsIn(ShopCurrency.Sats).toMap().getValue("refund_cap").next == "Whole sats. 0 turns it off.", "Bitcoin Standard: whole sats")
        ok(AppWords.editFieldsIn(ShopCurrency.Eur).toMap().getValue("refund_cap").next == "Whole euros. 0 turns it off.", "euros: whole euros")
        val held = sampleSettings(currency = ShopCurrency.Eur)
        val limit = SettingsEditModel.fields.first { it.kind == FieldKind.Dollars }
        ok(SettingsEditModel.now(limit, held) == "7 euros" && SettingsEditModel.moneyText(ShopCurrency.Sats, 1250) == "1,250 sats" && SettingsEditModel.moneyText(ShopCurrency.Jpy, 0) == "off", "Now: names the currency")
        ok(OwnerLook.settingsGroups.first() == ("Currency" to listOf("Currency")) && OwnerModel.settingsLines(held)[1] == ("Currency" to "Euro (EUR)"), "the settings view shows Currency as a key and the choice as its value")
        val e = source("src/android/SettingsEdit.kt")
        ok(e.contains("OwnerLook.capital(currency.plural) + \" for one bitcoin\"") && !e.contains("\"Dollars for one bitcoin\""), "the price field names the currency")
    }

    // ---- the built-in Tor (OA-97 to OA-100) -----------------------------------------------------------------------

    private fun reply(text: String): ControlMessage? = ControlReader(java.io.ByteArrayInputStream(text.toByteArray(Charsets.ISO_8859_1))).next()

    private fun oa_97_the_app_runs_tor_itself_and_reads_its_control_socket_strictly() {
        // Good replies.
        val okReply = reply("250 OK\r\n")!!
        ok(okReply.code == 250 && okReply.lines == listOf("OK") && !okReply.isEvent, "a plain success")
        TorControl.expectOk(okReply)
        val socks = reply("250-net/listeners/socks=\"127.0.0.1:41233\"\r\n250 OK\r\n")!!
        ok(TorControl.socksPort(socks) == 41233, "the port of the one SOCKS listener")
        val phase = reply("250-status/bootstrap-phase=NOTICE BOOTSTRAP PROGRESS=45 TAG=loading_descriptors SUMMARY=\"Loading\"\r\n250 OK\r\n")!!
        ok(TorControl.bootstrapProgress(phase) == 45, "the progress of the bootstrap")
        val done = reply("250-status/bootstrap-phase=NOTICE BOOTSTRAP PROGRESS=100 TAG=done SUMMARY=\"Done\"\r\n250 OK\r\n")!!
        ok(TorControl.bootstrapProgress(done) == 100, "done is 100")
        val circuit = reply("650 STATUS_CLIENT NOTICE CIRCUIT_ESTABLISHED\r\n")!!
        ok(circuit.isEvent && TorControl.status(circuit) == TorControl.Status.CircuitEstablished, "the first circuit")
        val step = TorControl.status(reply("650 STATUS_CLIENT NOTICE BOOTSTRAP PROGRESS=80 TAG=x SUMMARY=\"y\"\r\n")!!)
        ok(step is TorControl.Status.Bootstrap && step.percent == 80, "a step of the bootstrap")
        ok(TorControl.status(reply("650 STATUS_CLIENT NOTICE ENOUGH_DIR_INFO\r\n")!!) == TorControl.Status.Other, "another status is nothing to do")
        // Two messages in a row, and the end of the stream between them.
        val two = ControlReader(java.io.ByteArrayInputStream("250 OK\r\n650 STATUS_CLIENT NOTICE CIRCUIT_ESTABLISHED\r\n".toByteArray()))
        ok(two.next() != null && two.next() != null && two.next() == null, "the stream ends cleanly between messages")
        ok(reply("") == null, "an empty stream is the end")
        // Error codes are replies, and the commands refuse them.
        val refused = reply("515 Authentication failed\r\n")!!
        ok(refused.code == 515, "an error code is read")
        throws("an error code taken as success") { TorControl.expectOk(refused) }
        throws("a success whose last line is not OK") { TorControl.expectOk(reply("250 done\r\n")!!) }
        throws("a SOCKS reply with an error code") { TorControl.socksPort(reply("552 Unrecognized key\r\n")!!) }
        // Refusals of the shape.
        throws("a line with no terminator") { reply("250 OK") }
        throws("a line feed alone") { reply("250 OK\n") }
        throws("a carriage return alone") { reply("250 OK\rX") }
        throws("a reply that stops after a dash line") { reply("250-a=b\r\n") }
        throws("a line longer than the cap") { reply("250 " + "a".repeat(TorControl.MAX_LINE) + "\r\n") }
        ok(reply("250 " + "a".repeat(TorControl.MAX_LINE - 4) + "\r\n") != null, "a line at the cap is read")
        throws("a reply of too many lines") { reply("250-a\r\n".repeat(TorControl.MAX_LINES) + "250 OK\r\n") }
        throws("a data reply") { reply("250+info=\r\nline\r\n.\r\n250 OK\r\n") }
        throws("a code that changes in a reply") { reply("250-a\r\n251 OK\r\n") }
        throws("a code that is not digits") { reply("2x0 OK\r\n") }
        throws("a line that is too short") { reply("250\r\n") }
        throws("a separator that is not a space or a dash") { reply("250*OK\r\n") }
        throws("a byte that is not printable ASCII") { reply("250 O\u0001K\r\n") }
        throws("a byte above ASCII") { reply("250 O\u00e9K\r\n") }
        // Refusals of the meaning.
        for (bad in listOf("\"192.168.0.1:9050\"", "\"127.0.0.1:0\"", "\"127.0.0.1:65536\"", "\"127.0.0.1:x\"", "\"127.0.0.1:9050\" \"127.0.0.1:9051\"", "127.0.0.1:9050", "\"\"", "\"127.0.0.1:\"")) {
            throws("the SOCKS listener $bad") { TorControl.socksPort(reply("250-net/listeners/socks=$bad\r\n250 OK\r\n")!!) }
        }
        throws("a reply for another key") { TorControl.socksPort(reply("250-net/listeners/dns=\"127.0.0.1:9050\"\r\n250 OK\r\n")!!) }
        throws("a bootstrap phase with no progress") { TorControl.bootstrapProgress(reply("250-status/bootstrap-phase=NOTICE BOOTSTRAP\r\n250 OK\r\n")!!) }
        throws("a progress above 100") { TorControl.bootstrapProgress(reply("250-status/bootstrap-phase=NOTICE BOOTSTRAP PROGRESS=101\r\n250 OK\r\n")!!) }
        throws("an event that is not a status of the client") { TorControl.status(reply("650 CIRC 1 BUILT\r\n")!!) }
        throws("an event of several lines") { TorControl.status(reply("650-STATUS_CLIENT NOTICE CIRCUIT_ESTABLISHED\r\n650 OK\r\n")!!) }
        throws("a reply taken for an event") { TorControl.status(okReply) }
        // The five commands, and no others.
        val consts = Regex("const val ([A-Z_]+) = \"([^\"]*)\\\\r\\\\n\"").findAll(torSource("src/model/TorControl.kt")).map { it.groupValues[2] }.toList()
        ok(consts == listOf("AUTHENTICATE", "SETEVENTS STATUS_CLIENT", "GETINFO status/bootstrap-phase", "GETINFO net/listeners/socks", "SIGNAL SHUTDOWN"), "the five commands: $consts")
        val svc = torSource("src/android/TorService.kt")
        ok(Regex("\\.write\\(").findAll(svc).count() == 2 && Regex("TorControl\\.[A-Z_]+\\b").findAll(svc).map { it.value }.toSet().containsAll(setOf("TorControl.AUTHENTICATE", "TorControl.SET_EVENTS", "TorControl.GET_SOCKS", "TorControl.GET_BOOTSTRAP", "TorControl.SHUTDOWN")), "the service writes only those commands")
        val code = svc.lines().filter { l -> l.trimStart().let { !it.startsWith("*") && !it.startsWith("/**") && !it.startsWith("//") } }.joinToString("\n")
        ok(svc.contains("package org.torproject.jni") && code.contains("class TorService(") && !code.contains(": Service") && !code.contains("androidx") && !code.contains("jtorctl") && !code.contains("sendBroadcast"), "the class has the library's name, is not an Android service and uses neither androidx nor jtorctl")
        ok(svc.contains("BSD 3-Clause") && svc.contains("github.com/guardianproject/tor-android") && svc.contains("follow"), "the header says whose call order it follows, and its licence")
        ok(svc.contains("\"--SocksPort\", \"127.0.0.1:auto\""), "one SOCKS listener, at 127.0.0.1, on a port Tor picks")
        ok(svc.contains("--ControlSocket") && !svc.contains("--ControlPort"), "the control socket is the one the natives give")
        ok(source("AndroidManifest.xml").let { !it.contains("TorService") }, "the manifest has no entry for it: it is not a component")
        val controller = source("src/android/Controller.kt")
        ok(controller.contains("ensureTor().awaitPort(TOR_WAIT_MS)") && controller.indexOf("ensureTor().awaitPort") in 1 until controller.indexOf("OwnerNode.start("), "the node starts only after the built-in Tor has a port")
        val hidden = controller.substringAfter("fun hidden()").substringBefore("/**\n     * OA-90")
        ok(hidden.indexOf("stopNode()") in 0 until hidden.indexOf("releaseTor()") && hidden.indexOf("WatchRunner.handOver()") in 0 until hidden.indexOf("releaseTor()"), "Tor is let go of when the app is hidden, after the node and the hand-over")
        val runner = torSource("src/android/TorRunner.kt")
        ok(runner.contains("run.stop()") && runner.contains("run.holders == 0"), "the last lease shuts Tor down")
        ok(source("src/android/WatchService.kt").contains("torLease = TorRunner.acquire(dir)") && source("src/android/WatchService.kt").contains("l?.close()"), "the watcher holds a lease from its service's start to its end")
    }

    private fun oa_98_the_built_in_tor_keeps_its_files_in_the_no_backup_folder_and_reads_no_configuration() {
        val svc = torSource("src/android/TorService.kt")
        val runner = torSource("src/android/TorRunner.kt")
        ok(runner.contains("const val TOR_DIR = \"tor\"") && runner.contains("File(dir, TorRunner.TOR_DIR)") && runner.contains("File(torDir, \"data\")") && runner.contains("File(torDir, \"cache\")"), "the folders are under the app's folder")
        ok(source("src/android/Controller.kt").contains("private val dir: File = act.noBackupFilesDir") && source("src/android/Controller.kt").contains("TorRunner.acquire(dir)"), "and that folder is noBackupFilesDir")
        ok(source("src/android/WatchService.kt").contains("WatchRunner.begin(noBackupFilesDir)"), "the watcher's is the same folder")
        ok(runner.contains("setReadable(true, true)") && runner.contains("setWritable(true, true)") && runner.contains("setExecutable(true, true)"), "the folders are the app's alone")
        ok(!runner.contains("cacheDir") && !runner.contains("getCacheDir") && !svc.contains("getCacheDir") && !runner.contains("filesDir.") , "nothing of Tor is in a cache folder that is not the no-backup folder")
        val options = Regex("\"(--[A-Za-z-]+)\"").findAll(svc).map { it.groupValues[1] }.toList()
        ok(options == listOf("--verify-config", "--RunAsDaemon", "--defaults-torrc", "--ignore-missing-torrc", "--CacheDirectory", "--DataDirectory", "--ControlSocket", "--CookieAuthentication", "--SocksPort"), "the options of the command line: $options")
        ok(svc.contains("\"-f\", torrc.absolutePath") && !svc.contains("FileWriter") && !svc.contains("writeText") && !svc.contains("PrintWriter"), "the files named for configuration are never written, so none is read")
        ok(svc.contains("socket.delete()"), "a control socket left by an earlier run is removed before Tor starts")
        val extract = source("AndroidManifest.xml")
        ok(extract.contains("android:allowBackup=\"false\""), "nothing of it is backed up (OA-02)")
        val build = source("build.sh")
        val script = torSource("tor-library.sh")
        ok(script.contains("aar_sha256=\"28ff2bb6d4d42eabe57183dedd7a97fce77e8ce4b6f5af6fa03993b3ca054edd\"") && script.contains("is not the pinned tor-android 0.4.9.14 AAR"), "the shared script checks the library's hash and stops on a mismatch")
        ok(build.contains("tor-library.sh\" check") && build.contains("tor-library.sh\" unpack") && !build.contains("unzip -q"), "the build takes libtor.so only through that script, and checks the AAR before anything else is built")
        ok(script.contains("jni/\$abi/libtor.so") && Regex("unzip [^\\n]*\"\\\$tor_aar\"").findAll(script).count() == 1, "only libtor.so is taken from the AAR, by one unzip")
    }

    private fun oa_99_the_owner_chooses_between_the_built_in_tor_and_orbot() {
        ok(AppConfig.fresh().tor == TorChoice.BuiltIn && AppConfig.fresh().proxy == AppConfig.DEFAULT_PROXY, "a new setup uses the built-in Tor")
        val legacy = AppConfig.decode("proxy: 10.0.0.2:9150\n".toByteArray())
        ok(legacy.tor == TorChoice.Orbot && legacy.proxy == "10.0.0.2:9150", "a setup from before the choice keeps Use Orbot, at its address")
        for (choice in TorChoice.values()) {
            val c = AppConfig("10.0.0.2:9150", choice)
            val back = AppConfig.decode(c.encode())
            ok(back.tor == choice && back.proxy == "10.0.0.2:9150", "the choice ${choice.key} reads back")
        }
        ok(AppConfig("127.0.0.1:9050", TorChoice.BuiltIn).with(TorChoice.Orbot).let { it.tor == TorChoice.Orbot && it.proxy == "127.0.0.1:9050" }, "changing the choice keeps the address of Orbot")
        ok(AppConfig("10.0.0.2:9150", TorChoice.Orbot).with(TorChoice.BuiltIn).proxy == "10.0.0.2:9150", "and going back to Orbot finds it")
        throws("a choice that is not one of the two") { AppConfig.decode("proxy: 1.2.3.4:5\ntor: off\n".toByteArray()) }
        throws("a second line that is not the choice") { AppConfig.decode("proxy: 1.2.3.4:5\nmanager: a\n".toByteArray()) }
        throws("a third line") { AppConfig.decode("proxy: 1.2.3.4:5\ntor: orbot\nx: y\n".toByteArray()) }
        throws("a choice with no line end") { AppConfig.decode("proxy: 1.2.3.4:5\ntor: orbot".toByteArray()) }
        ok(TorChoice.values().map { it.label } == listOf("Built-in Tor", "Use Orbot") && TorChoice.fromKey("off") == null, "the two choices have their labels, and there is no third (no way to turn Tor off)")
        ok(TorChoice.BuiltIn.words === AppWords.torBuiltIn && TorChoice.Orbot.words === AppWords.torUsesOrbot, "each choice has its words")
        ok(AppWords.torBuiltIn.heading == "Tor" && AppWords.torBuiltIn.what == "This app runs Tor itself, so Orbot is not needed." && AppWords.torBuiltIn.next == "Nothing to do. To use Orbot instead, choose Use Orbot." && AppWords.torBuiltIn.who == Who.NOBODY, "the words of the built-in Tor, as written down")
        ok(AppWords.torUsesOrbot.heading == "Tor through Orbot" && AppWords.torUsesOrbot.what == "This app sends everything through Orbot, which must be installed and running." && AppWords.torUsesOrbot.next == "Keep Orbot running. To stop needing it, choose Built-in Tor." && AppWords.torUsesOrbot.who == Who.OWNER, "the words of Orbot, as written down")
        val screens = source("src/android/Screens.kt")
        ok(screens.substringAfter("private fun Controller.showSettingsAndPhone()").substringBefore("private fun Controller.rescan()").contains("RowItem(AppWords.torBuiltIn.heading) { showTor() }"), "Settings and this phone has the row Tor")
        val page = source("src/android/TorScreen.kt")
        ok(page.contains("TorChoice.values()") && page.contains("chooseTor(") && page.contains("words = now.words"), "the page has a button for each choice and the words of the one in use")
        ok(source("src/android/Controller.kt").indexOf("configFile.save(next)") in 1 until source("src/android/Controller.kt").indexOf("config = next"), "the choice is saved before it is taken up")
        ok(source("src/android/Controller.kt").contains("val old = config ?: return"), "the choice is changed only on a saved setup")
        ok(!source("src/android/Controller.kt").contains("withoutTor = true") && !page.contains("withoutTor"), "neither choice sends around Tor")
    }

    private fun oa_100_when_the_built_in_tor_does_not_start_nothing_is_sent_and_the_app_says_so() {
        ok(AppWords.torStarting.heading == "Starting Tor" && AppWords.torStarting.what == "This app is connecting to the Tor network, which can take up to a minute." && AppWords.torStarting.next == "Wait on this screen." && AppWords.torStarting.who == Who.NOBODY, "the words while it connects, as written down")
        ok(AppWords.torFailed.heading == "Tor did not start" && AppWords.torFailed.what == "This app could not connect to the Tor network." && AppWords.torFailed.next == "Check that the phone is online, then try again. If it keeps failing, choose Use Orbot." && AppWords.torFailed.who == Who.OWNER && AppWords.torFailed.tone == Tones.STOP, "the words when it did not, as written down")
        ok(ScreenModel.pick(null, SetupStatus(true, true), AppWords.torFailed, false).let { it is Screen.TorBlocked && it.words === AppWords.torFailed }, "a failed Tor is a screen of its own, before the node's")
        ok(ScreenModel.pick(null, SetupStatus(true, true), AppWords.torFailed, true) is Screen.TorBlocked, "and it stays until the owner tries again")
        val c = source("src/android/Controller.kt")
        ok(c.contains("waiting(if (cfg.tor == TorChoice.BuiltIn) AppWords.torStarting else AppWords.starting)"), "the screen says it is connecting while it waits")
        ok(c.contains("torBlocked = AppWords.torFailed") && c.contains("\"Try again\" else \"Check again\""), "a failure shows its words and the button Try again")
        ok(c.indexOf("torBlocked = AppWords.torFailed") in 1 until c.indexOf("OwnerNode.start("), "the failure comes before the node is started")
        ok(c.contains("const val TOR_WAIT_MS = 90_000L"), "it waits 90 seconds for a circuit")
        ok(c.contains("outcome.words === AppWords.orbotNotRunning") && c.contains("PosOutcome.Failed(AppWords.torFailed)"), "a till read that finds no Tor says so in the built-in Tor's words")
        ok(c.contains("fun proxyNow()") && c.contains("return \"\"") && c.contains("\"127.0.0.1:\$port\""), "with no Tor up there is no proxy address for a fetch to use")
        val runner = torSource("src/android/TorRunner.kt")
        ok(runner.contains("catch (e: LinkageError)") && runner.contains("class TorStartException"), "a library that does not load is a failure to start, with the same words")
        ok(runner.contains("val run = current?.takeIf { !it.done }"), "a run that has ended is not reused: trying again starts a new one")
        ok(source("src/android/WatchService.kt").contains("TorChoice.BuiltIn -> torLease?.port() != null"), "the watcher is not started while the built-in Tor has no circuit")
    }

    // ---- the price of a bitcoin, fetched (OA-101) -----------------------------------------------------------------------

    /** A library that agrees when two replies came, with the price in the replies' body, and refuses otherwise. */
    private class FakePrice(val ids: List<String>, val refuse: Boolean = false) : PricePort {
        var got: List<PriceReply> = emptyList()
        var at = 0L
        var asked = ""

        override fun sources(): List<PriceSource> = ids.map { PriceSource(it, "https://$it.example/price") }

        override fun agree(replies: List<PriceReply>, now: Long): AgreedPrice {
            got = replies
            at = now
            if (refuse || replies.size < 2) throw OwnerException("RT-31", "too few", 1, words())
            return AgreedPrice(98_500, 9_850_000, "med.kc")
        }

        override fun sourcesFor(currency: String): List<PriceSource> {
            asked = currency
            return sources()
        }

        override fun agreeFor(currency: String, replies: List<PriceReply>, now: Long): AgreedPriceIn {
            got = replies
            at = now
            if (refuse || replies.size < 2) throw OwnerException("RT-31", "too few", 1, words())
            return AgreedPriceIn(91_000, 9_100_000, "med.kc")
        }
    }

    private fun oa_101_the_price_is_asked_of_the_sources_and_the_owner_can_type_another() {
        // Fetched: both sources answered, each reply stamped when it arrived, and the price is the library's.
        val port = FakePrice(listOf("kraken", "coinbase"))
        val fetched = PriceModel.fetch(port, { RateFetch.Body("{}".toByteArray()) }) { 1_700_000_000L }
        ok(fetched is PriceOutcome.Fetched && fetched.usd == 98_500L, "the sources agree: the library's price")
        ok(port.got.map { it.source }.sorted() == listOf("coinbase", "kraken") && port.got.all { it.fetchedAt == 1_700_000_000L } && port.at == 1_700_000_000L, "every reply is handed over, stamped by the phone's clock")
        ok(PriceModel.fieldText(fetched) == "98500" && (CounterModel.price(PriceModel.fieldText(fetched)) as? Form.Ok)?.value == 98_500L, "the price is in the field as whole dollars, and the field's own check takes it")
        ok(AppWords.counterPriceFetched(98_500).what.contains("98,500 dollars for one bitcoin"), "the words say the price with its separator")
        ok(PriceModel.dollars(98_500) == "98,500" && PriceModel.dollars(999) == "999" && PriceModel.dollars(1_234_567) == "1,234,567" && PriceModel.dollars(1_000) == "1,000", "thousands separators")
        // Refused: the sources do not agree, or no source answered, or the proxy did not answer. The price is typed.
        val disagree = PriceModel.fetch(FakePrice(listOf("kraken", "coinbase"), refuse = true), { RateFetch.Body("{}".toByteArray()) }) { 1L }
        ok(disagree is PriceOutcome.Missing && PriceModel.fieldText(disagree) == "", "sources that disagree leave the field empty")
        val silent = FakePrice(listOf("kraken", "coinbase"))
        ok(PriceModel.fetch(silent, { RateFetch.NotAnswered }) { 1L } is PriceOutcome.Missing && silent.got.isEmpty(), "sources that did not answer give the library no reply")
        ok(PriceModel.fetch(FakePrice(listOf("kraken", "coinbase")), { RateFetch.TorMissing }) { 1L } is PriceOutcome.Missing, "a proxy that does not answer leaves the field empty")
        val broken = PriceModel.fetch(FakePrice(listOf("kraken", "coinbase")), { if (it.id == "kraken") throw IllegalStateException("x") else RateFetch.Body("{}".toByteArray()) }) { 1L }
        ok(broken is PriceOutcome.Missing, "a fetch that failed is a missing reply, and one reply is not enough")
        ok(AppWords.counterPriceNeeded.heading == "Price needed" && BoxWords.priceNeeded.heading == "Price for the Box", "the words of a price that must be typed are as they were")
        // The owner types over the fetched price: what is in the field when they say the words match is what is used.
        ok((CounterModel.price("101000") as? Form.Ok)?.value == 101_000L && CounterModel.price("") is Form.Refused, "another price typed over it is the one used, and an empty field is refused as before")
        val helper = source("src/android/PriceFetch.kt")
        ok(helper.contains("if (field.text.isNullOrBlank()) field.setText(") && helper.contains("async("), "what was typed while the sources were asked is kept, and the call is off the screen thread")
        ok(helper.indexOf("slot.show(ui.words(fetching))") < helper.indexOf("async(") && helper.contains("slot.show(ui.words(fetched(outcome.usd)))") && helper.contains("slot.show(ui.words(needed))"), "the fetching words first, then the fetched words or the needed words")
        val add = source("src/android/AddCounter.kt").substringAfter("private fun Controller.showCounterCheck(").substringBefore("private fun Controller.showCounterQueued(")
        ok(add.contains("fillPrice(note, it, AppWords.counterPriceFetching, AppWords::counterPriceFetched, AppWords.counterPriceNeeded)"), "the counter's screen asks for the price when the phone has no settings")
        val box = connectBox().substringAfter("private fun Controller.showBoxFound(").substringBefore("private fun Controller.showBoxQueued(")
        ok(box.contains("fillPrice(note, it, BoxWords.priceFetching, BoxWords::priceFetched, BoxWords.priceNeeded)"), "the Box's screen does the same")
        ok(AppWords.counterPriceFetching.who == Who.NOBODY && AppWords.counterPriceFetched(1).who == Who.OWNER && BoxWords.priceFetching === AppWords.counterPriceFetching, "the words are the same for a counter and for a Box")
        ok(!source("src/model/PriceModel.kt").contains("usdPerBtc *") && !source("src/model/PriceModel.kt").contains("/ 100"), "no price is worked out here: the library's dollars are used")
    }

    // ---- the price of a cash-out, and the books (OA-102 to OA-104) --------------------------------------------------------

    private fun oa_102_a_cash_out_is_recorded_at_the_price_of_the_shops_currency() {
        val port = FakePrice(listOf("kraken", "coinbase"))
        val fetched = PriceModel.fetchIn("EUR", port, { RateFetch.Body("{}".toByteArray()) }) { 1_700_000_000L }
        ok(fetched is PriceOutcome.FetchedIn && fetched.whole == 91_000L && port.asked == "EUR", "the sources of the shop's currency are asked, and the price is the library's")
        ok(port.got.size == 2 && port.got.all { it.fetchedAt == 1_700_000_000L } && port.at == 1_700_000_000L, "every reply is handed over, stamped by the phone's clock")
        ok(PriceModel.fieldText(fetched) == "91000", "the price is in the field")
        val refused = PriceModel.fetchIn("EUR", FakePrice(listOf("kraken", "coinbase"), refuse = true), { RateFetch.Body("{}".toByteArray()) }) { 1L }
        ok(refused is PriceOutcome.Missing && PriceModel.fieldText(refused) == "", "sources that disagree leave the field empty")
        ok(PriceModel.fetchIn("EUR", FakePrice(listOf("kraken", "coinbase")), { RateFetch.NotAnswered }) { 1L } is PriceOutcome.Missing, "sources that did not answer leave it empty too")
        // The owner types over it: the field is what is used, in the shop's currency.
        val typed = Forms.cashOutPrice("101,000", ShopCurrency.Eur) as Form.Ok
        ok(typed.value.currency == "EUR" && typed.value.wholePerBtc == 101_000L, "the typed price is the one used, with the shop's currency code")
        ok(Forms.cashOutPrice("", ShopCurrency.Eur) is Form.Refused && Forms.cashOutPrice("0", ShopCurrency.Eur) is Form.Refused, "an empty price and 0 are refused")
        ok((Forms.cashOutPrice("x", ShopCurrency.Gbp) as Form.Refused).words.what.contains("pounds"), "the refusal names the shop's currency")
        val w = AppWords.cashOutPrice(91_000, ShopCurrency.Eur)
        ok(w.heading == "Today's price" && w.what == "The price sources give 91,000 euros for one bitcoin. The books record this cash-out at that price." && w.next == "If it looks right, go on. To use another price, type it below." && w.who == Who.OWNER, "the words of the fetched price")
        val needed = AppWords.cashOutPriceNeeded
        ok(needed.heading == "Price needed" && needed.what == "The price sources did not answer, and the books need what this cash-out is worth." && needed.next == "Type the price of one bitcoin, then go on." && needed.who == Who.OWNER, "the words of the price to type")
        val screens = source("src/android/Screens.kt")
        val flow = screens.substringAfter("private fun Controller.showCashOutPrice(").substringBefore("// ---- old addresses")
        ok(flow.contains("OwnerLook.capital(currency.plural) + \" for one bitcoin\"") && flow.contains("fillPriceIn(") && flow.contains("Forms.cashOutPrice(") && flow.indexOf("Forms.cashOutPrice(") < flow.indexOf("n.cashOut(priced)"), "the field is labelled as the editor's, and the price is checked before the cash-out is queued")
        ok(screens.contains("currency != ShopCurrency.Sats") && screens.contains("showCashOutPrice(n, f.value, currency)"), "Bitcoin Standard asks nothing")
        val fetch = source("src/android/PriceFetch.kt")
        ok(fetch.contains("PriceModel.fetchIn(currency.code") && fetch.contains("if (field.text.isNullOrBlank()) field.setText("), "the sources of the currency are asked off the screen thread, and what was typed is kept")
    }

    private fun books(
        days: List<BooksDay> = emptyList(),
        accounting: List<BooksFile> = emptyList(),
        ledger: ByteArray = ByteArray(0),
        refusal: Words? = null,
    ) = Books("EUR", days, accounting, ledger, refusal)

    private fun oa_103_the_books_page_lists_the_days_the_library_gives_in_its_order() {
        val b = books(
            days = listOf(BooksDay("2026-11-10", 3, 0, "45.50"), BooksDay("2026-11-09", 1, 0, "7.00"), BooksDay("2026-10-31", 12, 1, "1234.00")),
            accounting = listOf(BooksFile("2026-10", "a".toByteArray()), BooksFile("2026-11", "b".toByteArray())),
            ledger = "ledger".toByteArray(),
        )
        val page = BooksModel.page(b)
        ok(page.rows.map { it.date } == listOf("2026-11-10", "2026-11-09", "2026-10-31"), "the days are in the library's order, newest first")
        ok(page.rows.map { it.totals } == listOf("3 sales, 45.50 EUR", "1 sale, 7.00 EUR", "12 sales, 1234.00 EUR"), "a day's totals are \"{n} sales, {amount}\" in the shop's currency")
        ok(page.empty == null && page.refusal == null, "a page with days has no empty words")
        // Nothing yet: the empty words, and no file to save.
        val none = BooksModel.page(books())
        ok(none.empty === AppWords.booksEmpty && none.rows.isEmpty() && none.accounting == null && none.ledger == null, "an empty node shows the empty words and offers no file")
        ok(AppWords.booksEmpty.heading == "No sales yet" && AppWords.booksEmpty.what == "No counter has sent a sale to this phone yet." && AppWords.booksEmpty.next == "Nothing to do. Sales appear here as the counters report them." && AppWords.booksEmpty.who == Who.NOBODY, "the empty words, as written down")
        ok(OwnerLook.BOOKS == "Books" && OwnerLook.BOOKS_SUB == "Sales, refunds and totals by day, and the files for your bookkeeper." && OwnerLook.moneyTitles.last() == "Waiting to be delivered", "the Money row is Books, after the others")
        val screens = source("src/android/Screens.kt")
        ok(screens.contains("rows + RowItem(OwnerLook.BOOKS, OwnerLook.BOOKS_SUB, onClick = { showBooks() })"), "the row is the last of the Money page and opens the page")
        val page2 = source("src/android/Books.kt")
        ok(page2.contains("screen(OwnerLook.BOOKS, views, footer, back = { showHome() })") && page2.contains("ui.rows(") && page2.contains("n.books(offsetMinutes)") && page2.contains("TimeZone.getDefault().getOffset("), "the page is a title page with a back, one rows card, and the phone's offset")
    }

    private fun oa_104_the_files_are_saved_exactly_as_the_library_made_them() {
        val acct = byteArrayOf(0, 1, 2, 13, 10, 127, -1)
        val full = byteArrayOf(9, 8, 7, -128)
        val b = books(
            days = listOf(BooksDay("2026-11-10", 3, 0, "45.50")),
            accounting = listOf(BooksFile("2026-10", "old".toByteArray()), BooksFile("2026-11", acct)),
            ledger = full,
        )
        val page = BooksModel.page(b)
        ok(page.accounting!!.name == "openbmt-accounting-2026-11.csv" && page.accounting.bytes.contentEquals(acct), "the accounting file is the library's bytes, named for the newest month")
        ok(page.ledger!!.name == "openbmt-ledger-2026-11.csv" && page.ledger.bytes.contentEquals(full), "the full ledger is the library's bytes, named for the newest month")
        // A refusal: its words, and nothing to save for what was refused.
        val refusal = words(Tones.STOP)
        val refused = BooksModel.page(books(days = b.days, refusal = refusal))
        ok(refused.refusal != null && refused.refusal.heading == "Heading" && refused.accounting == null && refused.ledger == null && refused.empty == null, "a refusal shows the library's words and offers no file")
        ok(OwnerLook.BOOKS_NOTE == "The files follow US rules only and are not tax advice." && OwnerLook.BOOKS_SAVE_ACCOUNTING == "Save the accounting file" && OwnerLook.BOOKS_SAVE_LEDGER == "Save the full ledger", "the footnote and the buttons, as written down")
        val booksSaved = AppWords.booksSaved
        ok(booksSaved.heading == "File saved" && booksSaved.what == "The file is saved where you chose. It holds sale numbers, times and amounts, and no addresses." && booksSaved.next == "Give it to your bookkeeper." && booksSaved.who == Who.OWNER, "the saved words, as written down")
        val src = source("src/android/Books.kt")
        ok(src.contains("ui.primary(OwnerLook.BOOKS_SAVE_ACCOUNTING)") && src.contains("ui.button(OwnerLook.BOOKS_SAVE_LEDGER)") && src.contains("line(OwnerLook.BOOKS_NOTE, muted = true)"), "a primary and a secondary button, and the footnote in 13 sp secondary")
        ok(src.contains("file.name, file.bytes, \"text/csv\"") && src.contains("AppWords.booksSaved"), "the save writes the bytes it was given, through the picker, and says it did")
    }

    // ---- the rules and their tests -----------------------------------------------------------------------------

    /** Rules that have no test of this app, each with the reason. Every other rule has a test named for it. */
    private val notHere: Map<String, String> = mapOf(
        "OA-90" to "the redraw after a change of light and dark is the activity's and the window's, and needs a device; the screens it draws are those of OA-62",
    )

    private fun ruleTests(rulesMd: File?) {
        val methods = AppModelTest::class.java.declaredMethods.filter { it.name.startsWith("oa_") && !it.name.contains('$') }.sortedBy { it.name }
        for (m in methods) {
            m.isAccessible = true
            if (m.parameterCount != 0) throw AssertionError("test method ${m.name} has parameters")
            m.invoke(AppModelTest)
        }
        ok(methods.size >= 15, "the rules' tests ran: ${methods.size}")
        val ids = methods.map { it.name.removePrefix("oa_").substringBefore('_') }
        ok(ids.size == ids.toSet().size, "one test for each number")
        if (rulesMd == null) return
        val rules = Regex("^- \\*\\*OA-(\\d{2,3})\\.\\*\\*", RegexOption.MULTILINE).findAll(rulesMd.readText()).map { it.groupValues[1] }.toList()
        ok(rules.first() == "01" && rules.map { it.toInt() } == rules.map { it.toInt() }.sorted() && rules.toSet().size == rules.size, "the rules are numbered upward from 01, each once: $rules")
        // OA-36 to OA-39 are kept for other work, OA-49 to OA-59 for the next, and OA-66 to OA-69 and OA-75 to OA-79 for more.
        val numbered = (1..35) + (40..48) + (60..65) + (70..74) + (80..89) + (90..104)
        ok(rules.map { it.toInt() }.all { it in numbered } && rules.map { it.toInt() } == numbered.filter { it.toString().padStart(2, '0') in rules }, "the rules are numbered from 01 without a gap but the kept numbers: $rules")
        for (id in rules) {
            ok(id in ids || "OA-$id" in notHere, "OA-$id has a test named oa_${id}_... or a reason")
        }
        for (id in ids) ok(id in rules, "oa_${id}_... tests a rule that is in docs/OWNER-APP-RULES.md")
        for (line in rulesMd.readLines()) ok(line.length <= 80 || line.startsWith("|"), "a line of the rules is at most 80 characters: ${line.take(40)}")
        val text = rulesMd.readText()
        ok(text.contains("## 9. What these rules do not do"), "the rules say what they do not do")
        ok(text.lines().any { it.startsWith("Status:") }, "the status line")
    }


    // ---- the first run ----------------------------------------------------------------------------

    private fun firstRunTests() {
        // The welcome is for a phone with no setup, and only then.
        val none = SetupStatus(false, false)
        val done = SetupStatus(true, true)
        ok(ScreenModel.pick(null, none, null, false) is Screen.Welcome, "no setup: the welcome")
        ok(ScreenModel.pick(null, done, null, false) is Screen.Starting, "after setup: not the welcome")
        ok(ScreenModel.pick(null, done, null, true) is Screen.Home, "after setup, running: home")
        ok(ScreenModel.pick(null, none, null, false, resuming = true) !is Screen.Welcome, "a ceremony to resume skips the welcome")

        // The first run: Welcome, Shop, Till, Money, then Why for the savings wallet only, then the notice.
        val r = FirstRun(ShopPrefs(), "DE")
        ok(r.step == FirstRunStep.Welcome && r.currency == ShopCurrency.Eur, "the money is guessed from the locale")
        r.next(); ok(r.step == FirstRunStep.Shop, "shop")
        r.name = "Cafe Anna"; r.currency = ShopCurrency.Gbp; r.next(); ok(r.step == FirstRunStep.Till, "till")
        r.till = Till.Toast; r.query = "to"; r.next(); ok(r.step == FirstRunStep.Money, "money")
        r.choose(MoneyChoice.Make); ok(r.step == FirstRunStep.Why, "the savings wallet shows the why first")
        r.next(); ok(r.step == FirstRunStep.Guide && r.guide.step == GuideStep.Need, "then the key maker guide, at its first step")
        r.back(); ok(r.step == FirstRunStep.Why, "back from the first step of the guide to the why")
        r.openRealCases(); ok(r.step == FirstRunStep.RealCases, "the button on the why screen opens the real cases")
        r.back(); ok(r.step == FirstRunStep.Why, "back from the real cases to the why screen")
        val shown = FirstRunWords.all().toMap()
        ok(shown["whyRealCases"] == "Real cases" && shown["realTitle"] == "When someone else holds the money" &&
            shown["realCase4Place"] == "Lebanon, since 2019" && shown["whyText2"]!!.contains("with a coin or a die."), "the real cases words are the ones of the words file")
        val screensMdText = File(appDir, "../../docs/screens/owner-app.md").readText()
        for (k in listOf("whyRealCases", "realTitle", "realIntro", "realCase1Place", "realCase1Text", "realCase2Place", "realCase2Text",
            "realCase3Place", "realCase3Text", "realCase4Place", "realCase4Text", "realBitcoinTitle", "realBitcoinText", "realKeysTitle", "realKeysText", "realCoinTitle", "realCoinText", "realMore")) {
            ok(shown.containsKey(k) && screensMdText.contains("| $k |"), "docs/screens/owner-app.md has $k")
        }
        r.next(); r.back()
        FirstRun(ShopPrefs(), "DE").let { f -> f.openRealCases(); ok(f.step == FirstRunStep.Welcome, "the real cases open from the why screen only") }
        r.next(); r.guide.next(); ok(r.step == FirstRunStep.Guide && r.guide.step == GuideStep.Tails, "the guide goes on by its own steps")
        r.back(); ok(r.step == FirstRunStep.Guide && r.guide.step == GuideStep.Need, "back from step 2 stays in the guide")
        r.back(); r.next(); ok(r.guide.step == GuideStep.Need, "the guide is made new each time it is entered")
        for (i in 1..5) r.guide.next()
        r.guide.scanned("818bc9b6", "bc1qaaaa"); r.guide.same(); r.guide.same()
        ok(r.guide.step == GuideStep.Done, "the guide ends at its last screen")
        r.next(); ok(r.step == FirstRunStep.NotYet, "Done leads to the notice")
        r.back(); ok(r.step == FirstRunStep.Why, "back from the notice to the why")
        r.back(); ok(r.step == FirstRunStep.Money, "back to the money")
        r.back(); r.back()
        ok(r.step == FirstRunStep.Shop && r.name == "Cafe Anna" && r.currency == ShopCurrency.Gbp, "back to the shop keeps what was entered")
        r.next()
        ok(r.till == Till.Toast && r.query == "to", "back to the till keeps the choice and the search")
        r.next(); r.choose(MoneyChoice.Own); ok(r.step == FirstRunStep.NotYet, "own wallet: the notice directly")
        r.back(); ok(r.step == FirstRunStep.Money, "back from the notice to the money")
        r.choose(MoneyChoice.Later); ok(r.step == FirstRunStep.NotYet, "later: the notice directly")
        r.toWelcome(); r.back(); ok(r.step == FirstRunStep.Welcome, "the welcome has no step before it")

        // The key maker guide: six steps in order, Back, the scan, and the two comparisons.
        val g = KeyGuide()
        val order = listOf(GuideStep.Need, GuideStep.Tails, GuideStep.Page, GuideStep.Start, GuideStep.Open, GuideStep.Follow)
        ok(order.map { it.number } == (1..6).toList() && KeyGuide.TOTAL == 6, "the steps are numbered 1 to 6")
        for ((i, want) in order.withIndex()) {
            ok(g.step == want, "guide step ${i + 1}")
            if (i < order.size - 1) g.next()
        }
        g.next(); ok(g.step == GuideStep.Follow, "step 6 goes on only by a scan")
        g.back(); ok(g.step == GuideStep.Open, "back from step 6")
        g.next(); g.scanned("818bc9b6", "bc1qyjassxtmzl02s8de8jw9365qmuvwg74udtp3vj")
        ok(g.step == GuideStep.Compare && g.fingerprint == "818bc9b6", "a scan gives the comparison")
        ok(KeyGuide.groups("bc1qyjassxtmzl02") == "bc1q yjas sxtm zl02", "the address in groups of four")
        g.back(); ok(g.step == GuideStep.Follow && g.fingerprint == null && g.firstAddress == null, "back to the scan forgets what was scanned")
        g.scanned("818bc9b6", "bc1qaaaa"); g.same()
        ok(g.step == GuideStep.Electrum && g.fingerprint == "818bc9b6", "the same: on to Electrum, with the scan kept")
        g.back(); ok(g.step == GuideStep.Compare, "back from Electrum")
        g.same(); g.same(); ok(g.step == GuideStep.Done, "Electrum the same: the last screen")
        g.back(); ok(g.step == GuideStep.Electrum, "back from the last screen")
        g.different(); ok(g.step == GuideStep.Stopped, "Electrum different: stop")
        g.restart(); repeat(5) { g.next() }; g.scanned("818bc9b6", "bc1qaaaa"); g.different(); ok(g.step == GuideStep.Stopped, "the page different: stop")
        KeyGuide().also { it.different() }.let { ok(it.step == GuideStep.Need, "different has meaning only as the answer of a comparison") }
        g.restart(); ok(g.step == GuideStep.Need && g.fingerprint == null && g.firstAddress == null, "after the stop the guide starts again with nothing kept")
        val h = KeyGuide(); h.next(); h.next(); h.next(); h.next(); h.next(); h.scanned("1", "a"); h.different()
        ok(h.step == GuideStep.Stopped, "different at the first comparison stops")
        h.back(); ok(h.step == GuideStep.Need && h.fingerprint == null, "Back from the stop starts again")
        val m = KeyGuide(); repeat(5) { m.next() }
        ok(!m.scanned(null, "bc1qaaaa") && m.step == GuideStep.Follow && m.fingerprint == null && m.firstAddress == null, "a wallet with more than one key gives no comparison and stays on step 6")
        ok(m.scanned("818bc9b6", "bc1qaaaa") && m.step == GuideStep.Compare, "a single-key wallet gives the comparison")
        ok(KeyGuideWords.kgNotKeyMaker.heading == "Not the key maker's code" && KeyGuideWords.kgCameraOff.next == "Allow the camera for OpenBMT in the phone's settings, then press Scan the page's code again.", "the added words, exactly")
        ok(KeyGuideWords.kgStopTitle == "Stop" && KeyGuideWords.kgStartAgain == "Start again", "the stop screen's title and button")
        val e = KeyGuide(); e.scanned("1", "a"); ok(e.step == GuideStep.Need && e.fingerprint == null, "a scan is taken only at step 6")
        ok(!KeyGuide().back(), "the first step has nothing before it")
        ok(KeyGuideWords.badge(3, 6) == "Step 3 of 6", "the badge")
        ok(KeyGuideWords.kgNeedList.size == 4 && KeyGuideWords.kgScanButton == "Scan the page's code", "the guide's words")
        ok(KeyGuideWords.kgDifferentText == "Stop. Do not use this wallet. On the page, press Clear and close, shut down Tails, and start again.", "the stop text, exactly")
        ok(KeyGuideWords.kgNeedCard == "No card? Any paper will do for now. Write \"TEMPORARY. PUT THIS ON STEEL.\" at the top.", "the card note, exactly")

        // The locale guess: the nine of the app, the euro countries, and the dollar for the rest.
        val guesses = mapOf("US" to ShopCurrency.Usd, "GB" to ShopCurrency.Gbp, "FR" to ShopCurrency.Eur, "JP" to ShopCurrency.Jpy, "AU" to ShopCurrency.Aud,
            "IN" to ShopCurrency.Inr, "KR" to ShopCurrency.Krw, "ID" to ShopCurrency.Idr, "SG" to ShopCurrency.Sgd, "" to ShopCurrency.Usd, "ZZ" to ShopCurrency.Usd)
        for ((c, want) in guesses) ok(ShopPrefs.guessCurrency(c) == want, "the money of $c")
        ok(FirstRun(ShopPrefs(currency = ShopCurrency.Sats), "US").currency == ShopCurrency.Sats, "a saved money wins over the guess")

        // The till search.
        ok(Till.listed.map { it.title } == listOf("Square", "Toast", "Shopify POS", "Loyverse", "Odoo", "Hike", "StoreHub"), "the seven tills, in order")
        ok(Till.search("").map { it.key } == Till.listed.map { it.key } + "none" + "other", "no search: all seven, then no till and another till")
        ok(Till.search("SHOP").map { it.title } == listOf("Shopify POS", FirstRunWords.tillNone, FirstRunWords.tillOther), "a search matches in any case, and None and Other stay")
        ok(Till.search("zzz").size == 2, "a search with no match still lists No till and Another till")
        ok(Till.search("o").map { it.title }.containsAll(listOf("Toast", "Loyverse", "Odoo", "StoreHub", "Shopify POS")), "a search matches inside a name")

        // The steps screen, for each of the nine choices.
        for (t in Till.values()) {
            val page = TillStepsPage.of(t)
            ok(page.showsDone && page.lines.isNotEmpty(), "${t.key}: a page with a Done and lines")
            ok(page.lines.none { it.text.contains("{till}") } && page.title.none { it == '{' }, "${t.key}: every {till} is replaced")
        }
        val sq = TillStepsPage.of(Till.Square)
        ok(sq.title == "Add Bitcoin to Square", "Square's title")
        ok(sq.lines.first().text == "Do this once, on the computer or tablet where you manage Square. It takes about two minutes.", "Square's intro")
        ok(sq.lines.filter { it.kind == TillStepsPage.Kind.Step }.map { it.text } == Till.Square.steps.mapIndexed { i, s -> "${i + 1}. $s" } && Till.Square.steps.size == 4, "Square's four numbered steps")
        ok(sq.lines.any { it.text == FirstRunWords.tillSquareCashier } && sq.lines.any { it.text == FirstRunWords.tillSquareReports }, "Square's cashier and report lines")
        ok(sq.lines.last().kind == TillStepsPage.Kind.Small && sq.lines.last().text.startsWith("Steps read on Square's own help pages on 11 November 2026."), "the checked line is last and small")
        ok(sq.lines.any { it.text == "Square does not move the money, so it charges no fee for this payment type." }, "the no-fee line")
        ok(TillStepsPage.of(Till.StoreHub).lines.count { it.kind == TillStepsPage.Kind.Step } == 4, "StoreHub: four steps")
        ok(TillStepsPage.of(Till.Toast).lines.count { it.kind == TillStepsPage.Kind.Step } == 3, "Toast: three steps")
        ok(TillStepsPage.of(Till.Shopify).lines.any { it.text == FirstRunWords.tillShopifyNote }, "Shopify's note")
        ok(TillStepsPage.of(Till.Loyverse).lines.any { it.text == FirstRunWords.tillLoyverseReports }, "Loyverse's report line")
        ok(TillStepsPage.of(Till.None).lines.map { it.text } == listOf(FirstRunWords.tillNoneText), "No till shows its own text")
        ok(TillStepsPage.of(Till.Other).lines.map { it.text } == listOf(FirstRunWords.tillOtherText, FirstRunWords.tillOtherAsk), "Another till shows its own text")

        // What is kept, and where.
        val dir = tempDir()
        try {
            val f = ShopFile(dir)
            ok(f.load().name == "" && f.load().till == null && !f.load().stepsShown, "nothing kept: an empty shop")
            f.save(ShopPrefs("Cafe Anna", ShopCurrency.Sats, Till.Hike, false))
            val back = f.load()
            ok(back.name == "Cafe Anna" && back.currency == ShopCurrency.Sats && back.till == Till.Hike && !back.stepsShown, "name, money and till come back")
            f.save(back.with(stepsShown = true))
            ok(f.load().stepsShown && f.load().till == Till.Hike, "the steps are marked shown")
            ok(String(f.load().encode()) == "name: Cafe Anna\ncurrency: SAT\ntill: hike\nsteps-shown: yes\n", "the file's form")
            ok(ShopPrefs.clean("a\nb\u0007c") == "abc", "a name is one clean line")
            ok(ShopPrefs.clean("x".repeat(100)).length == ShopPrefs.MAX_NAME, "a name is cut at its length")
            File(dir, AppFiles.SHOP_FILE).writeText("garbage")
            ok(ShopFile(dir).load().till == null, "a file that is not the shop file is an empty shop")
            for (bad in listOf("name: a\ncurrency: XXX\ntill: \nsteps-shown: no\n", "name: a\ncurrency: \ntill: nope\nsteps-shown: no\n", "name: a\ncurrency: \ntill: \nsteps-shown: maybe\n")) {
                throws("a shop file with a value the app does not have") { ShopPrefs.decode(bad.toByteArray()) }
            }
        } finally {
            clean(dir)
        }

        // The practice shop: the order, the amount, the books, Back, and nothing written.
        var clock = 1_700_000_000L
        val p = Practice(ShopCurrency.Eur) { clock }
        ok(p.step == PracticeStep.Intro, "the practice starts at the intro")
        ok(!p.back(), "the intro has no step before it")
        p.next(); ok(p.step == PracticeStep.Amount, "amount")
        p.next(); ok(p.step == PracticeStep.Amount && !p.canCharge, "Charge needs an amount")
        p.digit(0); ok(p.digits == "", "no leading zero")
        for (d in listOf(1, 2, 5)) p.digit(d)
        ok(p.amount == 125L && p.amountText() == "125 euros", "the amount typed: ${p.amountText()}")
        p.delete(); p.digit(0); ok(p.amount == 120L, "delete and type again")
        repeat(10) { p.digit(9) }
        ok(p.digits.length == Practice.MAX_DIGITS, "the amount has a length limit")
        p.delete(); repeat(p.digits.length) { p.delete() }
        for (d in listOf(4, 5, 0)) p.digit(d)
        p.next(); ok(p.step == PracticeStep.Qr, "the code")
        p.back(); ok(p.step == PracticeStep.Amount && p.amount == 450L, "back to the amount keeps it")
        p.next(); clock += 60; p.next(); ok(p.step == PracticeStep.Paid && p.paidAt == clock, "paid at the time of the tap")
        p.back(); ok(p.step == PracticeStep.Qr, "back from paid to the code")
        p.next(); p.next(); ok(p.step == PracticeStep.Books, "books")
        ok(p.booksRow().endsWith("  450 euros  Paid") && p.booksRow().startsWith(Format.localTime(p.paidAt!!)), "the books have one row: time, amount, Paid: ${p.booksRow()}")
        p.next(); ok(p.step == PracticeStep.Refund && !p.refunded, "refund")
        p.next(); ok(p.step == PracticeStep.Done && p.refunded, "done, refunded in cash")
        p.back(); ok(p.step == PracticeStep.Refund && !p.refunded, "back from done un-does the refund")
        p.next()
        val again = p.again()
        ok(again.step == PracticeStep.Amount && again.digits == "" && again.paidAt == null, "try again starts a new sale")
        ok(Practice.QR_TEXT == "OPENBMT PRACTICE - NOT A PAYMENT", "the code's text is exactly the practice line")
        ok(Practice(ShopCurrency.Usd) { 0 }.also { it.next(); it.digit(7) }.amountText() == "$7", "the dollar")

        // The practice and the first run write nothing and call nothing of the library: no file, no store, no signer.
        val model = source("src/model/FirstRunModel.kt")
        val practiceSource = model.substringAfter("class Practice(").substringBefore("companion object")
        for (bad in listOf("File(", "DurableFile", "SealedFile", "OwnerNode", "node", "Native", "writeText", "writeBytes", "SharedPreferences", "save(", "sign", "Sign", "send", "Socket", "URL(")) {
            ok(!practiceSource.contains(bad), "the practice shop reaches nothing outside memory: $bad")
        }
        val screens = source("src/android/FirstRun.kt")
        val practiceScreens = screens.substringAfter("// ---- the practice shop").substringBefore("// ---- your shop")
        for (bad in listOf("shopFile\\.save", "File\\(", "\\bnode\\b", "\\bn\\.", "withNode", "async", "posTokens", "\\bsetup\\b", "OwnerNode", "ceremony", "writeText", "FileOutputStream", "SharedPreferences")) {
            ok(!Regex(bad).containsMatchIn(practiceScreens), "the practice screens write and call nothing: $bad")
        }
        ok(practiceScreens.contains("FirstRunWords.practiceBanner") && practiceScreens.contains("Practice.QR_TEXT"), "every practice page carries the banner, and the code is the practice line")
        ok(screens.substringAfter("private fun Controller.practicePage").substringBefore("private fun Controller.showPractice").contains("tone = ui.pal.warn"), "the banner is the tinted bar")
        ok(source("src/android/Controller.kt").contains("ScreenModel.pick(fault, status, torBlocked, node != null, ceremonyPending())") && source("src/android/Controller.kt").contains("return showEntry()"), "the app opens on the welcome when nothing is saved")
        ok(source("src/android/MainActivity.kt").contains("controller.systemBack()") && source("src/android/MainActivity.kt").contains("controller.firstRunBack = null"), "the system's Back does what the arrow does on a first-run screen, and a screen of any other kind clears it")
        ok(source("src/android/AddCounter.kt").contains("afterFirstCounter()") && source("src/android/Screens.kt").contains("tillStepsRow"), "the steps follow the first counter and are in Settings")

        // The key maker guide: nothing from it reaches a file, the node or the ceremony, and it never asks for words.
        val guideScreens = source("src/android/KeyGuide.kt")
        val guideModel = source("src/model/FirstRunModel.kt").substringAfter("class KeyGuide").substringBefore("/** The screens of the practice shop")
        for ((name, text) in listOf("screens" to guideScreens, "model" to guideModel)) {
            for (bad in listOf("shopFile", "File\\(", "\\bnode\\b", "withNode", "async", "posTokens", "\\bsetup\\b", "OwnerNode", "OwnerCeremony", "ceremony", "writeText", "writeBytes", "FileOutputStream", "SharedPreferences", "DurableFile", "KeystoreSealer", "SealedFile", "\\.save\\(", "mnemonic", "seed")) {
                ok(!Regex(bad).containsMatchIn(text), "the guide $name reach nothing outside memory: $bad")
            }
        }
        ok(guideScreens.contains("OwnerImport.read(") && guideScreens.contains("bytes.fill(0)"), "the scan goes to the one library call and the scanned bytes are cleared")
        ok(source("src/model/FirstRunWords.kt").contains("KeyGuideWords.all()"), "the guide's words are in the list of screens")

        val keys = FirstRunWords.all().map { it.first }
        ok(keys.size == keys.toSet().size, "every first-run key is used once")
        ok(FirstRunWords.moneyNotYet == "This part is still being built. For now, setup continues with the test setup.", "the notice, exactly")
        ok(FirstRunWords.with(FirstRunWords.tillStepsTitle, "Odoo") == "Add Bitcoin to Odoo", "{till} is the till's name")
    }

    private fun pictureTests() {
        for (p in PictureTiming.Picture.values()) {
            ok(p.loopMs in 2000..3000, "${p.name}: a loop of two to three seconds")
            ok(PictureTiming.position(p, 0, true) == 0f, "${p.name}: starts at 0")
            ok(PictureTiming.position(p, p.loopMs.toLong(), true) == 0f && PictureTiming.position(p, p.loopMs * 5L + 750, true) == 750f / p.loopMs, "${p.name}: the loop repeats")
            ok((0 until p.loopMs step 10).all { PictureTiming.position(p, it.toLong(), true) in 0f..0.999f }, "${p.name}: the position stays inside the loop")
            ok(PictureTiming.position(p, 1234, false) == PictureTiming.STILL && PictureTiming.position(p, 0, false) == PictureTiming.STILL, "${p.name}: with animations off it is the last frame at any time")
        }
        ok(PictureTiming.seg(1f, 0.3f, PictureTiming.DONE_BY) == 1f && PictureTiming.seg(PictureTiming.STILL, 0.1f, 0.5f) == 1f, "the last frame is past the end of every move")
        ok(PictureTiming.seg(0f, 0.2f, 0.6f) == 0f && Math.abs(PictureTiming.seg(0.4f, 0.2f, 0.6f) - 0.5f) < 1e-6f, "a move is 0 before it and linear in it")
        ok(PictureTiming.seg(0.5f, 0.5f, 0.5f) == 1f && PictureTiming.seg(0.4f, 0.5f, 0.5f) == 0f, "an empty stretch is a step")
        ok(PictureTiming.ease(0f) == 0f && PictureTiming.ease(1f) == 1f && Math.abs(PictureTiming.ease(0.5f) - 0.5f) < 1e-6f && PictureTiming.ease(2f) == 1f, "ease keeps its ends")
        ok(PictureTiming.bump(0f, 0.2f, 0.6f) == 0f && PictureTiming.bump(1f, 0.2f, 0.6f) == 0f && PictureTiming.bump(0.9f, 0.2f, 0.6f) == 0f && PictureTiming.bump(0.4f, 0.2f, 0.6f) > 0.99f, "a bump is 0 at both ends")
        val src = source("src/android/FirstRunPictures.kt")
        ok(!Regex("drawText|StaticLayout|TextView|\"[A-Za-z]{2,}").containsMatchIn(src.replace(Regex("//.*"), "").replace(Regex("/\\*[\\s\\S]*?\\*/"), "")), "the pictures draw shapes only: no text and no string of words")
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
        ceremonyTests()
        firstRunTests()
        pictureTests()
        ruleTests(rulesMd)
        println("AppModelTest (owner): $count checks passed")
    }
}
