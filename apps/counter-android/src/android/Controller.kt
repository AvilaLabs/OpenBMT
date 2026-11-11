package org.openbmt.counter.app

import android.content.ClipData
import android.content.ClipboardManager
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.view.View
import android.view.ViewGroup
import android.widget.Button
import android.widget.EditText
import android.widget.LinearLayout
import android.widget.RadioGroup
import android.widget.TextView
import java.io.File
import java.io.IOException
import java.security.GeneralSecurityException
import java.time.ZoneId
import java.util.concurrent.LinkedBlockingQueue
import java.util.concurrent.TimeUnit
import org.openbmt.app.AnimatedQrView
import org.openbmt.app.Colour
import org.openbmt.app.Format
import org.openbmt.app.KeystoreSealer
import org.openbmt.app.SayWords
import org.openbmt.app.ScanFlow
import org.openbmt.app.SaleBand
import org.openbmt.app.SealedFile
import org.openbmt.app.Ui
import org.openbmt.app.UrScan
import org.openbmt.app.UrWords
import org.openbmt.app.Who
import org.openbmt.app.WordsModel
import org.openbmt.counter.Actor
import org.openbmt.counter.ActorRole
import org.openbmt.counter.CounterException
import org.openbmt.counter.CounterNode
import org.openbmt.counter.LnRequest
import org.openbmt.counter.PairStarted
import org.openbmt.counter.Pairing
import org.openbmt.counter.Payment
import org.openbmt.counter.RateSource
import org.openbmt.counter.Rates
import org.openbmt.counter.RefundAsk
import org.openbmt.counter.RefundState
import org.openbmt.counter.RelayKind
import org.openbmt.counter.SaleState
import org.openbmt.counter.Shop
import org.openbmt.counter.OwnNodeSetting
import org.openbmt.counter.StartConfig

/**
 * The screens and what they do. The node runs while the activity is visible:
 * [visible] starts it and [hidden] stops it, releasing its folder. One
 * background thread pumps the node (`pump(250)`) and posts what it reports to
 * the screen thread; when the pairing holds a relay (CR-130, CR-131) one more thread takes a turn of the
 * link to the owner every five seconds (CR-135) and a line on the home screen says how it is going
 * (CR-136); every other call into the library is made on the screen
 * thread, except the start, which runs on a worker thread before the pump
 * begins. The price is fetched by one worker thread for each source on the
 * schedule of RT-40 to RT-45, and the bytes go to the library. The shop's own
 * Lightning node is reached by one more thread, which asks the library for the
 * next request (CR-112), sends it through [LightningTransport] and gives back
 * the answer; the test of a new connection runs on that same thread.
 */
class Controller(private val act: MainActivity) {
    private val ui = Ui(act)
    private val scan = ScanFlow(act, ui)
    private val main = Handler(Looper.getMainLooper())
    private val dir: File = act.noBackupFilesDir
    private val nodeDir = File(dir, "node")
    private val pin = PinStore(dir, { nowSecs() })
    private val configFile = ConfigFile(dir)
    private val staff = StaffStore(dir, { nowSecs() }, { config?.manager ?: "" })

    // Who is at the till: in memory only (CR-149), with the list of cashiers as read at the last start.
    private val locks = TillLock { SystemClock.elapsedRealtime() / 1000 }
    private var cashiers: List<String> = emptyList()

    // What is saved, as read at the last start.
    private var record: ByteArray? = null
    private var config: AppConfig? = null
    private var lightningRecord: ByteArray? = null
    private var lightningFault: SayWords? = null

    // What the Lightning screen holds while the camera has the screen.
    private var draftLnCode = ""
    private var draftLnAddress = ""
    private var draftLnRune = ""
    private var draftLnCert = ""

    // What the first screen holds while the pairing runs, and where the pairing has got to (CR-98).
    private val progress = PairingProgress()
    private var draftCounterName = "Counter"
    private var draftName = ""

    // What runs.
    @Volatile
    private var runId = 0
    private var visible = false
    private var fault: SayWords? = null
    private var torBlocked: SayWords? = null
    private var node: CounterNode? = null
    private var money: Money? = null
    private var book: PriceBook? = null
    private var pumpFault: SayWords? = null
    private var pumpThread: Thread? = null
    private var linkThread: Thread? = null
    private val workers = ArrayList<Thread>()
    private var lightningThread: Thread? = null
    private var transport: LightningTransport? = null
    private val lightningJobs = LinkedBlockingQueue<Runnable>()
    private val startLock = Any()

    // What the open screen redraws as the node reports.
    private var tick: (() -> Unit)? = null
    private var lastTick = 0L

    // What this run knows of its sales: the amount in the shop's currency of each, by index.
    private val amounts = HashMap<Long, Long>()

    private fun nowSecs(): Long = System.currentTimeMillis() / 1000

    // ---- visible, hidden -------------------------------------------------------------------------

    fun visible() {
        visible = true
        if (scan.asking) return
        locks.backInView()
        begin()
    }

    fun hidden() {
        visible = false
        if (scan.asking) return
        locks.leftView()
        forgetCeremony()
        scan.stop()
        stopNode()
    }

    /** CR-91, CR-98: a ceremony open when the app is left is forgotten, in the library too. */
    private fun forgetCeremony() {
        if (!progress.left()) return
        try {
            Pairing.cancel()
        } catch (_: CounterException) {
            // The library has no ceremony: it was ended already.
        } catch (_: LinkageError) {
            // No library, no ceremony.
        }
    }

    private fun begin() {
        fault = null
        torBlocked = null
        stopNode()
        val setup = try {
            config = configFile.load()
            val paired = File(dir, PAIRING_FILE).exists()
            SetupStatus(paired, pin.isSet(), config != null)
        } catch (e: IOException) {
            fault = AppWords.fileNotRead(e.message)
            return render()
        }
        if (!setup.complete) return showFirstStart(if (progress.takeForgotten()) AppWords.pairingForgotten else null)
        // CR-140: the list of cashiers is read whole or not at all; a file that does not read is a fault, and nobody is signed in.
        try {
            cashiers = staff.names()
        } catch (e: IOException) {
            cashiers = emptyList()
            locks.forget()
            fault = AppWords.cashiersNotRead(e.message)
            return render()
        }
        if (cashiers.isEmpty()) locks.forget()
        try {
            record = SealedFile(dir, KeystoreSealer(KEY_ALIAS), PAIRING_FILE).load()
                ?: throw IOException("the pairing is missing")
        } catch (e: IOException) {
            fault = AppWords.storageUnavailable(e.message)
            return render()
        } catch (e: GeneralSecurityException) {
            fault = AppWords.storageUnavailable(e.message)
            return render()
        }
        // CR-115: the Lightning connection, sealed by its own key; a file that does not read leaves Lightning off, and the Lightning screen says why.
        lightningFault = null
        lightningRecord = try {
            SealedFile(dir, KeystoreSealer(LightningSetup.KEY_ALIAS), LightningSetup.FILE).load()
        } catch (e: IOException) {
            lightningFault = AppWords.storageUnavailable(e.message)
            null
        } catch (e: GeneralSecurityException) {
            lightningFault = AppWords.storageUnavailable(e.message)
            null
        }
        startNode()
    }

    private fun render() {
        val setup = SetupStatus(File(dir, PAIRING_FILE).exists(), pin.isSet(), config != null)
        when (val s = ScreenModel.pick(fault, setup, torBlocked, node != null, locked())) {
            is Screen.Fault -> showMessage(s.words, "Try again") { begin() }
            is Screen.FirstStart -> showFirstStart(null)
            is Screen.TorBlocked -> page(listOf(ui.words(s.words), ui.primary("Check again") { begin() }, ui.button("Network") { showNetwork() }))
            is Screen.Starting -> page(listOf(ui.words(AppWords.starting)))
            is Screen.WhoAtTill -> showWho(null)
            is Screen.Home -> showHome()
        }
    }

    /** CR-147: cashiers are listed and nobody is signed in. */
    private fun locked(): Boolean = cashiers.isNotEmpty() && locks.who == null

    /**
     * CR-147, CR-149: before a screen that is not a sale's, the lock that waited for a sale's screen to close
     * happens, and a locked till shows "Who is at the till?" instead. True when it did.
     */
    private fun lockedOut(): Boolean {
        locks.saleScreen(null)
        if (!locked()) return false
        showWho(null)
        return true
    }

    // ---- running the node ----------------------------------------------------------------------------

    private fun startNode() {
        val id = ++runId
        page(listOf(ui.words(AppWords.starting)))
        val rec = record ?: return
        val cfg = config ?: return
        val lightning = lightningRecord
        val thread = Thread {
            try {
                val sources = Rates.sources()
                // For display only: the library checks the record again at the start (CR-97).
                val pairing = Shop.pairing(rec)
                val gate = TorGate(pairing.settings.torOn, cfg.proxy)
                if (!gate.available()) {
                    main.post {
                        if (id == runId) {
                            torBlocked = AppWords.orbotNotRunning
                            render()
                        }
                    }
                    return@Thread
                }
                synchronized(startLock) {
                    if (id != runId) return@Thread
                    nodeDir.mkdirs()
                    // CR-134: an onion relay with Tor off in the settings goes through Orbot, when it answers.
                    val linkProxy = LinkProxy.pick(pairing.relay == RelayKind.Onion, pairing.settings.torOn, cfg.proxy, Socks5Probe())
                    val n = CounterNode.start(
                        StartConfig(nodeDir.path, nowSecs(), rec, gate.proxy, cfg.ownNode.ifEmpty { null }, lightning, linkProxy),
                    )
                    if (id != runId) {
                        n.close()
                        return@Thread
                    }
                    main.post {
                        if (id == runId) adopt(id, n, sources, gate) else n.close()
                    }
                }
            } catch (e: CounterException) {
                main.post { if (id == runId) { fault = e.sayWords(); render() } }
            } catch (e: LinkageError) {
                main.post { if (id == runId) { fault = AppWords.libraryMissing(e.message); render() } }
            }
        }
        thread.isDaemon = true
        thread.start()
    }

    private fun adopt(id: Int, n: CounterNode, sources: List<RateSource>, gate: TorGate) {
        val m = ShopCurrency.of(sources)
        if (m == null) {
            n.close()
            fault = AppWords.fault(CounterException("shop-currency", "the price sources have no one currency", 4))
            return render()
        }
        node = n
        money = m
        pumpFault = null
        val b = PriceBook(LibraryRates, { nowSecs() })
        book = b
        val runner = PriceRunner(b, HttpsFetcher(gate))
        val schedule = PriceSchedule(sources.map { it.id }, System.currentTimeMillis())
        for (s in sources) {
            val w = Thread {
                while (runId == id) {
                    val wait = schedule.waitMs(s.id, System.currentTimeMillis())
                    if (wait > 0) {
                        try {
                            Thread.sleep(minOf(wait, 1000L))
                        } catch (_: InterruptedException) {
                            return@Thread
                        }
                        continue
                    }
                    runner.turn(s)
                    schedule.taken(s.id, System.currentTimeMillis())
                    main.post { if (id == runId) tick?.invoke() }
                }
            }
            w.isDaemon = true
            workers.add(w)
            w.start()
        }
        val pump = Thread {
            while (runId == id) {
                try {
                    val p = n.pump(nowSecs(), 250)
                    main.post { if (id == runId) onPump(p.reports.isNotEmpty()) }
                    if (p.stopped) break
                } catch (e: CounterException) {
                    main.post {
                        if (id == runId) {
                            pumpFault = e.sayWords()
                            tick?.invoke()
                        }
                    }
                    break
                }
            }
        }
        pump.isDaemon = true
        pumpThread = pump
        pump.start()
        startLightning(id, n, gate)
        // CR-135: one thread for the link, only when the pairing holds a relay.
        if (hasLink(n)) {
            val link = Thread {
                while (runId == id) {
                    try {
                        n.linkStep(nowSecs())
                    } catch (_: CounterException) {
                        break
                    }
                    main.post { if (id == runId) tick?.invoke() }
                    var waited = 0L
                    while (runId == id && waited < LinkProxy.EVERY_MS) {
                        try {
                            Thread.sleep(250)
                        } catch (_: InterruptedException) {
                            return@Thread
                        }
                        waited += 250
                    }
                }
            }
            link.isDaemon = true
            linkThread = link
            link.start()
        }
        // CR-149: a person still signed in, who left a sale's screen open, comes back to it; the lock that waited happens when it closes.
        val back = if (cashiers.isNotEmpty()) locks.reopen() else null
        if (back != null) showSale(back, null, null) else showHome()
    }

    /** CR-136: whether the counter has a link to the owner, which is when its pairing holds a relay. */
    private fun hasLink(n: CounterNode): Boolean =
        try {
            n.linkState() != null
        } catch (_: CounterException) {
            false
        }

    /**
     * CR-112, CR-114: the one thread that talks to the shop's node. It runs a queued job (the test of a
     * new connection) or else asks the library for the next request, sends it and hands back the answer.
     * Without a connection `ln_next` has nothing, and the thread only waits.
     */
    private fun startLightning(id: Int, n: CounterNode, gate: TorGate) {
        val t = LightningTransport(gate.torOn, config?.proxy ?: AppConfig.DEFAULT_PROXY)
        transport = t
        lightningJobs.clear()
        val worker = Thread {
            while (runId == id) {
                try {
                    val job = lightningJobs.poll(500, TimeUnit.MILLISECONDS)
                    if (job != null) {
                        job.run()
                        continue
                    }
                    val request = n.lnNext(nowSecs()) ?: continue
                    when (val out = t.send(request) { der -> pinned(n, request, der) }) {
                        is LnOutcome.Answer -> n.lnReply(request.id, out.status.toLong(), out.body)
                        is LnOutcome.Failed -> n.lnFailed(request.id, out.why)
                    }
                } catch (_: InterruptedException) {
                    return@Thread
                } catch (_: CounterException) {
                    // The library keeps its own account of a sale's request that failed (LN-22).
                } catch (_: LinkageError) {
                    return@Thread
                }
            }
        }
        worker.isDaemon = true
        lightningThread = worker
        worker.start()
    }

    /** Whether the library says this is the pinned certificate; a library that cannot say is a no. */
    private fun pinned(n: CounterNode, request: LnRequest, der: ByteArray): Boolean =
        try {
            n.lnPin(request.id, der)
        } catch (_: CounterException) {
            false
        }

    private fun onPump(reported: Boolean) {
        val now = System.currentTimeMillis()
        if (reported || now - lastTick >= 1000) {
            lastTick = now
            tick?.invoke()
        }
    }

    /** Stops the pump and the price workers, then the node, which lets go of its folder (NR-79). */
    private fun stopNode() {
        runId++
        tick = null
        pumpThread?.let { try { it.join(1500) } catch (_: InterruptedException) { } }
        pumpThread = null
        linkThread?.let {
            it.interrupt()
            try { it.join(1500) } catch (_: InterruptedException) { }
        }
        linkThread = null
        for (w in workers) w.interrupt()
        workers.clear()
        lightningThread?.interrupt()
        lightningThread = null
        transport = null
        lightningJobs.clear()
        node?.let {
            try {
                it.stop()
            } catch (_: CounterException) {
                // Already stopped.
            } catch (_: LinkageError) {
            }
        }
        node = null
        book = null
    }

    // ---- pages -----------------------------------------------------------------------------------------------

    private fun page(views: List<View>) {
        tick = null
        act.show(ui.page(views))
    }

    private fun showMessage(words: SayWords, label: String = "Back", back: () -> Unit) {
        page(listOf(ui.words(words), ui.primary(label, WordsModel.buttonColour(words.tone)) { back() }))
    }

    private fun <T> async(work: () -> T, done: (Result<T>) -> Unit) {
        val t = Thread {
            val r = runCatching(work)
            main.post { done(r) }
        }
        t.isDaemon = true
        t.start()
    }

    private fun label(text: String): TextView = ui.text(text, 14f, bold = true)

    // ---- first start ---------------------------------------------------------------------------------------------

    private fun showFirstStart(message: SayWords?) {
        val views = ArrayList<View>()
        views.add(ui.words(message ?: AppWords.notSetUp))
        views.add(label("Name of this counter"))
        val counterName = ui.field("Name of this counter").also { it.setText(draftCounterName) }
        views.add(counterName)
        views.add(label("Manager's name"))
        val name = ui.field("Name of the manager").also { it.setText(draftName) }
        views.add(name)
        views.add(label("Manager's PIN, 6 to 12 digits"))
        val pin1 = pinField(act, "PIN")
        val pin2 = pinField(act, "PIN again")
        views.add(pin1)
        views.add(pin2)
        views.add(ui.primary("Pair with the Key app") {
            draftCounterName = counterName.text.toString().trim()
            draftName = name.text.toString().trim()
            startPairing(draftCounterName, draftName, pin1.text.toString(), pin2.text.toString())
        })
        page(views)
    }

    /**
     * What was typed is checked and kept first (the PIN and the config; the pairing is what completes the
     * setup), then the pairing begins with the camera for the Key app's offer.
     */
    private fun startPairing(counterName: String, managerName: String, pin1: String, pin2: String) {
        FirstStartCheck.problem(counterName, managerName, pin1, pin2)?.let { return showFirstStart(it) }
        page(listOf(ui.words(AppWords.starting)))
        async({
            pin.set(pin1)
            configFile.save(AppConfig(AppConfig.DEFAULT_PROXY, managerName, ""))
        }) { r ->
            when (val e = r.exceptionOrNull()) {
                null -> scanOffer()
                is IOException -> showFirstStart(AppWords.fileNotSaved(e.message))
                is GeneralSecurityException -> showFirstStart(AppWords.storageUnavailable(e.message))
                else -> throw e
            }
        }
    }

    // ---- pairing: scan the offer, compare the words, scan the bundle (CR-91 to CR-98) ---------------------

    /** A page with [words] and a Scan button: the camera is asked for only when Scan is tapped (CR-90). */
    private fun scanPage(words: SayWords, scanNow: () -> Unit, cancel: () -> Unit) {
        page(listOf(ui.words(words), ui.primary("Scan") { scanNow() }, ui.button("Cancel") { cancel() }))
    }

    private fun failedScan(f: org.openbmt.app.ScanFailure) {
        endCeremony()
        showFirstStart(f.words ?: if (f.off) AppWords.cameraOff else AppWords.cameraNotStarted(f.problem))
    }

    /** The ceremony is over, here and in the library. */
    private fun endCeremony() {
        progress.cancelled()
        try {
            Pairing.cancel()
        } catch (_: CounterException) {
            // No ceremony in the library.
        } catch (_: LinkageError) {
            // No library, no ceremony.
        }
    }

    private fun scanOffer() {
        progress.cancelled()
        progress.start()
        scanPage(AppWords.pairingScanOffer, {
            scan.begin(
                show = { v -> tick = null; act.show(v) },
                onBytes = { bytes -> offerRead(bytes) },
                onFailed = { f -> failedScan(f) },
                onCancel = { endCeremony(); showFirstStart(null) },
                // The offer may be longer than one code: ur:bytes, whole or in parts (UR-20).
                ur = UrScan(setOf(TYPE_BYTES), Who.MANAGER) { _, payload -> offerRead(payload) },
            )
        }, { endCeremony(); showFirstStart(null) })
    }

    private fun offerRead(offer: ByteArray) {
        page(listOf(ui.words(AppWords.starting)))
        async({ Pairing.begin(offer, draftCounterName, nowSecs()) }) { r ->
            val started = r.getOrNull()
            if (started != null && progress.offerRead()) return@async compare(started)
            endCeremony()
            when (val e = r.exceptionOrNull()) {
                is CounterException -> showMessage(e.sayWords(), "Back") { showFirstStart(null) }
                is LinkageError -> showMessage(AppWords.libraryMissing(e.message), "Back") { showFirstStart(null) }
                else -> showFirstStart(null)
            }
        }
    }

    /** The request as a code, and the four words, the most prominent thing on the screen (CR-98). */
    private fun compare(started: PairStarted) {
        val views = ArrayList<View>()
        views.add(ui.text(FirstStartCheck.wordsLine(started.words), 34f, bold = true))
        views.add(ui.words(AppWords.pairingCompare))
        val metrics = act.resources.displayMetrics
        val made = AnimatedQrView.make(act, Who.MANAGER, TYPE_BYTES, started.request, (metrics.heightPixels * 0.5).toInt())
        val view = made.view
        if (view == null) {
            views.add(ui.words(made.words ?: UrWords.cannotShow(Who.MANAGER)))
        } else {
            views.add(ui.words(if (view.animated) UrWords.showingAnimated(Who.MANAGER) else UrWords.showingStill(Who.MANAGER)))
            views.add(view)
        }
        views.add(ui.primary("They match") { theyMatch() })
        views.add(ui.danger("They differ") { theyDiffer() })
        page(views)
    }

    private fun theyMatch() {
        if (!progress.matched()) return
        try {
            Pairing.match(nowSecs())
        } catch (e: CounterException) {
            endCeremony()
            return showMessage(e.sayWords(), "Back") { showFirstStart(null) }
        } catch (e: LinkageError) {
            endCeremony()
            return showMessage(AppWords.libraryMissing(e.message), "Back") { showFirstStart(null) }
        }
        scanBundle()
    }

    /** CR-92: the ceremony ends, the key and the number are forgotten, and the words say to start again. */
    private fun theyDiffer() {
        if (!progress.differed()) return
        val words = try {
            Pairing.differ()
        } catch (e: CounterException) {
            e.words
        } catch (e: LinkageError) {
            null
        }
        showMessage(words?.say() ?: AppWords.pairingForgotten, "Back") { showFirstStart(null) }
    }

    private fun scanBundle() {
        scanPage(AppWords.pairingScanBundle, {
            scan.begin(
                show = { v -> tick = null; act.show(v) },
                onBytes = { bytes -> bundleRead(bytes) },
                onFailed = { f -> failedScan(f) },
                onCancel = { endCeremony(); showFirstStart(null) },
                ur = UrScan(setOf(TYPE_BYTES), Who.MANAGER) { _, payload -> bundleRead(payload) },
            )
        }, { endCeremony(); showFirstStart(null) })
    }

    /** The bundle is given to the library; what comes back is the record, sealed here with the Keystore (CR-97). */
    private fun bundleRead(bundle: ByteArray) {
        page(listOf(ui.words(AppWords.starting)))
        async({
            val kept = Pairing.finish(nowSecs(), bundle)
            SealedFile(dir, KeystoreSealer(KEY_ALIAS), PAIRING_FILE).save(kept)
        }) { r ->
            val e = r.exceptionOrNull()
            // Whatever the outcome, the library's ceremony is over once it has taken a bundle.
            if (e == null) progress.finished() else endCeremony()
            when (e) {
                null -> {
                    draftName = ""
                    showMessage(AppWords.pairingSaved, "Start") { begin() }
                }
                is CounterException -> showMessage(e.sayWords(), "Back") { showFirstStart(null) }
                is LinkageError -> showMessage(AppWords.libraryMissing(e.message), "Back") { showFirstStart(null) }
                is IOException -> showFirstStart(AppWords.fileNotSaved(e.message))
                is GeneralSecurityException -> showFirstStart(AppWords.storageUnavailable(e.message))
                else -> throw e
            }
        }
    }

    /** The answer of the system's permission dialog. */
    fun permission(code: Int, results: IntArray) = scan.permissionResult(code, results)

    // ---- home ----------------------------------------------------------------------------------------------------------

    private fun showHome() {
        if (lockedOut()) return
        val n = node ?: return render()
        val cfg = config ?: return render()
        val health = Slot(act)
        val priceSlot = Slot(act)
        val ownSlot = Slot(act)
        val lightningSlot = Slot(act)
        val linkSlot = Slot(act)
        val linked = hasLink(n)
        val views = ArrayList<View>()
        // CR-147: who is at the till, and a way to lock it.
        locks.who?.takeIf { cashiers.isNotEmpty() }?.let { who ->
            views.add(ui.text("At the till: ${who.name}", 20f, bold = true))
            views.add(ui.button("Lock") {
                locks.lock()
                render()
            })
        }
        views.add(label("Is Bitcoin ready?"))
        views.add(health)
        // CR-116: the Lightning line is there only while a connection is saved.
        views.add(lightningSlot)
        if (cfg.ownNode.isNotEmpty()) {
            // CR-107: the own node's line, and a way to ask the driver to dial it again.
            views.add(label("Your own node"))
            views.add(ownSlot)
            views.add(ui.button("Try again") {
                try {
                    n.ownNodeRetry()
                } catch (e: CounterException) {
                    ownSlot.show(ui.words(e.sayWords()))
                }
            })
        }
        if (linked) {
            // CR-136: the link to the owner, and a way to call the Box now.
            views.add(label("The Box"))
            views.add(linkSlot)
            views.add(ui.button("Try again") {
                async({ n.linkStep(nowSecs(), true) }) { r ->
                    r.exceptionOrNull()?.let { if (it is CounterException) linkSlot.show(ui.words(it.sayWords())) }
                    if (r.isSuccess) tick?.invoke()
                }
            })
        }
        views.add(label("Price"))
        views.add(priceSlot)
        views.add(ui.primary("New sale") { showNewSale() })
        views.add(ui.button("Sales") { showSales() })
        views.add(ui.button("Today") { showToday(false) })
        if (cashiers.isEmpty()) {
            // CR-146: with no cashier listed the cashier is the one name of this phone, as before.
            views.add(label("Cashier"))
            val name = ui.field("Your name").also { it.setText(cfg.cashier) }
            views.add(name)
            val saved = Slot(act)
            views.add(ui.button("Save my name") {
                val v = name.text.toString().trim()
                if (!AppConfig.validName(v)) {
                    saved.show(ui.words(AppWords.nameNotAccepted))
                } else {
                    try {
                        val updated = cfg.withCashier(v)
                        configFile.save(updated)
                        config = updated
                        saved.show()
                    } catch (e: IOException) {
                        saved.show(ui.words(AppWords.fileNotSaved(e.message)))
                    }
                }
            })
            views.add(saved)
        }
        views.add(ui.button("Staff") { showStaffEntry() })
        views.add(ui.button("Your own node") { showOwnNode() })
        views.add(ui.button("Network") { showNetwork() })
        views.add(ui.button("Lightning") { showLightning(null) })
        views.add(ui.button("About") { showAbout() })
        page(views)
        tick = {
            health.show(ui.words(healthWords(n)))
            lightningWords(n)?.let { lightningSlot.show(label("Lightning"), ui.words(it)) } ?: lightningSlot.show()
            if (cfg.ownNode.isNotEmpty()) ownSlot.show(ui.words(ownNodeWords(n)))
            if (linked) linkSlot.show(ui.words(linkWords(n)))
            priceSlot.show(*priceViews())
        }
        tick?.invoke()
    }

    private fun healthWords(n: CounterNode): SayWords {
        pumpFault?.let { return it }
        return try {
            val s = n.status()
            if (s.stopped) AppWords.stopped else s.health?.words?.say() ?: AppWords.starting
        } catch (e: CounterException) {
            e.sayWords()
        }
    }

    /** CR-116: the Lightning line, or null when no connection is saved. */
    private fun lightningWords(n: CounterNode): SayWords? =
        try {
            n.status().lightning?.words?.say()
        } catch (e: CounterException) {
            e.sayWords()
        }

    /** CR-136: the link's line, from the library's words. */
    private fun linkWords(n: CounterNode): SayWords =
        try {
            n.linkState()?.words?.say() ?: AppWords.starting
        } catch (e: CounterException) {
            e.sayWords()
        }

    /** CR-107: the own node's health line, or the library's words for why there is none. */
    private fun ownNodeWords(n: CounterNode): SayWords =
        try {
            n.status().ownNode?.words?.say() ?: AppWords.starting
        } catch (e: CounterException) {
            e.sayWords()
        }

    // ---- the own node's address (CR-100 to CR-102) ----------------------------------------------------

    /** The counter's own setting, behind the manager's PIN: save an address, or remove it. */
    private fun showOwnNode() {
        if (lockedOut()) return
        val cfg = config ?: return render()
        val rec = record ?: return render()
        val msg = Slot(act)
        val address = ui.field("Tor address of your node", mono = true).also { it.setText(cfg.ownNode) }
        val pinInput = pinField(act, "Manager's PIN")
        page(
            listOf(
                ui.words(AppWords.ownNodeAbout),
                label("Tor address of your node"),
                address,
                label("Manager's PIN"),
                pinInput,
                msg,
                ui.primary("Save") {
                    val text = address.text.toString().trim()
                    checkPin(pinInput.text.toString(), { saveOwnNode(cfg, rec, text, msg) }, { msg.show(ui.words(it)) })
                },
                ui.danger("Remove") {
                    checkPin(pinInput.text.toString(), { keepOwnNode(cfg, "", AppWords.ownNodeRemoved, msg) }, { msg.show(ui.words(it)) })
                },
                ui.button("Back") { showHome() },
            ),
        )
    }

    /** The library reads the address (a home-network address, no Tor and so on are refused with words) and the app keeps what it made. */
    private fun saveOwnNode(cfg: AppConfig, rec: ByteArray, text: String, msg: Slot) {
        async({
            val pairing = Shop.pairing(rec)
            OwnNodeSetting.check(text, pairing.network, pairing.settings.torOn)
        }) { r ->
            when (val e = r.exceptionOrNull()) {
                null -> keepOwnNode(cfg, r.getOrThrow(), AppWords.ownNodeSaved, msg)
                is CounterException -> msg.show(ui.words(e.sayWords()))
                is LinkageError -> msg.show(ui.words(AppWords.libraryMissing(e.message)))
                else -> throw e
            }
        }
    }

    private fun keepOwnNode(cfg: AppConfig, address: String, done: SayWords, msg: Slot) {
        try {
            configFile.save(cfg.withOwnNode(address))
        } catch (e: IOException) {
            return msg.show(ui.words(AppWords.fileNotSaved(e.message)))
        }
        showMessage(done, "Start") { begin() }
    }

    // ---- the network: the address of Orbot (CR-87) -----------------------------------------------

    /** The address Tor is reached at, behind the manager's PIN. Open from home, and from the screen that says Orbot did not answer. */
    private fun showNetwork() {
        if (node != null && lockedOut()) return
        val cfg = config ?: return render()
        val msg = Slot(act)
        val address = ui.field("host:port", mono = true).also { it.setText(cfg.proxy) }
        val pinInput = pinField(act, "Manager's PIN")
        page(
            listOf(
                ui.words(AppWords.networkAbout),
                label("Address of Orbot"),
                address,
                label("Manager's PIN"),
                pinInput,
                msg,
                ui.primary("Save") {
                    val text = address.text.toString().trim()
                    val problem = NetworkCheck.problem(text)
                    if (problem != null) {
                        msg.show(ui.words(problem))
                    } else {
                        checkPin(pinInput.text.toString(), { keepNetwork(cfg, text, msg) }, { msg.show(ui.words(it)) })
                    }
                },
                ui.button("Back") { render() },
            ),
        )
    }

    private fun keepNetwork(cfg: AppConfig, address: String, msg: Slot) {
        try {
            configFile.save(cfg.withProxy(address))
        } catch (e: IOException) {
            return msg.show(ui.words(AppWords.fileNotSaved(e.message)))
        }
        showMessage(AppWords.networkSaved, "Start") { begin() }
    }

    // ---- Lightning through the shop's own node (CR-112 to CR-116) -------------------------------------

    /** The connection to the shop's node, behind the manager's PIN: scan or type it, test it, save it sealed, or remove it. */
    private fun showLightning(notice: SayWords?) {
        if (lockedOut()) return
        if (node == null) return render()
        val msg = Slot(act)
        (notice ?: lightningFault)?.let { msg.show(ui.words(it)) }
        val code = ui.field("lndconnect:// code", lines = 3, mono = true).also { it.setText(draftLnCode) }
        val address = ui.field("host:port", mono = true).also { it.setText(draftLnAddress) }
        val rune = ui.field("Rune", mono = true).also { it.setText(draftLnRune) }
        val cert = ui.field("Certificate, or its SHA-256 fingerprint", lines = 4, mono = true).also { it.setText(draftLnCert) }
        val pinInput = pinField(act, "Manager's PIN")
        fun keep() {
            draftLnCode = code.text.toString()
            draftLnAddress = address.text.toString()
            draftLnRune = rune.text.toString()
            draftLnCert = cert.text.toString()
        }
        page(
            listOf(
                ui.words(AppWords.lightningAbout),
                ui.words(AppWords.lightningCare),
                ui.text(if (lightningRecord != null) "A connection is saved on this phone." else "No connection is saved.", 16f),
                label("LND"),
                code,
                ui.button("Scan the code") {
                    keep()
                    scan.begin(
                        show = { v -> tick = null; act.show(v) },
                        onBytes = { bytes -> draftLnCode = String(bytes, Charsets.UTF_8); showLightning(null) },
                        onFailed = { f -> showLightning(f.words ?: if (f.off) AppWords.cameraOff else AppWords.cameraNotStarted(f.problem)) },
                        onCancel = { showLightning(null) },
                    )
                },
                ui.primary("Test and save the LND connection") {
                    keep()
                    testLightning(LightningSetup.lnd(draftLnCode), pinInput.text.toString(), msg)
                },
                label("Core Lightning"),
                address,
                rune,
                cert,
                ui.button("Test and save the Core Lightning connection") {
                    keep()
                    testLightning(LightningSetup.cln(draftLnAddress, draftLnRune, draftLnCert), pinInput.text.toString(), msg)
                },
                label("Manager's PIN"),
                pinInput,
                msg,
                ui.danger("Remove the connection") {
                    checkPin(pinInput.text.toString(), { removeLightning(msg) }, { msg.show(ui.words(it)) })
                },
                ui.button("Back") { forgetLightningDraft(); showHome() },
            ),
        )
    }

    private fun forgetLightningDraft() {
        draftLnCode = ""
        draftLnAddress = ""
        draftLnRune = ""
        draftLnCert = ""
    }

    /**
     * CR-115: the PIN first, then the test of LN-20 on the Lightning thread against what the shop's
     * settings name; the connection is sealed and saved only when it passes. A library that says the
     * owner has not named the node yet gives words, and nothing is tested or saved.
     */
    private fun testLightning(input: LnInput?, pinText: String, msg: Slot) {
        if (input == null) return msg.show(ui.words(AppWords.lightningFieldsNeeded))
        checkPin(pinText, {
            val n = node
            val t = transport
            if (n == null || t == null) return@checkPin render()
            val id = runId
            msg.show(ui.words(AppWords.lightningTesting))
            lightningJobs.add(Runnable {
                val result = runCatching {
                    val first = n.lnSetup(input.kind, input.address, input.secret, input.cert)
                    val sealed = LightningSetup.drive(
                        first,
                        { r -> t.send(r) { der -> pinned(n, r, der) } },
                        { i, status, body -> n.lnReply(i, status, body) },
                        { i, why -> n.lnFailed(i, why) },
                    )
                    if (sealed != null) SealedFile(dir, KeystoreSealer(LightningSetup.KEY_ALIAS), LightningSetup.FILE).save(sealed)
                    sealed != null
                }
                main.post { if (id == runId) testDone(result, msg) }
            })
        }, { msg.show(ui.words(it)) })
    }

    private fun testDone(result: Result<Boolean>, msg: Slot) {
        when (val e = result.exceptionOrNull()) {
            null ->
                if (result.getOrThrow()) {
                    forgetLightningDraft()
                    showMessage(AppWords.lightningSaved, "Start") { begin() }
                } else {
                    msg.show(ui.words(AppWords.lightningNotReached))
                }
            is CounterException -> msg.show(ui.words(e.sayWords()))
            is LinkageError -> msg.show(ui.words(AppWords.libraryMissing(e.message)))
            is IOException -> msg.show(ui.words(AppWords.fileNotSaved(e.message)))
            is GeneralSecurityException -> msg.show(ui.words(AppWords.storageUnavailable(e.message)))
            else -> throw e
        }
    }

    /** CR-115: the saved connection is deleted at once, and the library stops using it. */
    private fun removeLightning(msg: Slot) {
        val n = node ?: return render()
        val file = File(dir, LightningSetup.FILE)
        if (file.exists() && !file.delete()) return msg.show(ui.words(AppWords.fileNotSaved("the file could not be deleted")))
        lightningRecord = null
        lightningFault = null
        forgetLightningDraft()
        try {
            n.lnRemove()
        } catch (e: CounterException) {
            return msg.show(ui.words(e.sayWords()))
        }
        showMessage(AppWords.lightningRemoved, "Back") { showHome() }
    }

    private fun priceViews(): Array<View> {
        val b = book ?: return emptyArray()
        val m = money ?: return emptyArray()
        val words = when (val s = b.status()) {
            is PriceStatus.Ready -> AppWords.priceReady(m.show(s.rate.rate), s.sources)
            is PriceStatus.Refused -> s.words
            PriceStatus.TorMissing -> AppWords.orbotNotRunning
        }
        val notes = b.notes().joinToString("\n") { n ->
            "${n.source}: " + (n.price?.let { "${m.show(it)} at ${Format.localTime(n.atSecs)}" } ?: n.words?.heading.orEmpty())
        }
        val out = ArrayList<View>()
        out.add(ui.words(words))
        if (notes.isNotEmpty()) out.add(ui.text(notes, 13f, mono = true))
        return out.toTypedArray()
    }

    // ---- a new sale ------------------------------------------------------------------------------------------------------

    private fun showNewSale() {
        if (lockedOut()) return
        val m = money ?: return render()
        val entry = AmountEntry(m)
        val display = ui.text(entry.show(), 32f, bold = true)
        val msg = Slot(act)
        val views = ArrayList<View>()
        views.add(label("Amount of the sale"))
        views.add(display)
        views.add(msg)
        fun update() {
            display.text = entry.show()
            msg.show()
        }
        for (row in listOf("123", "456", "789")) {
            views.add(row(row.map { d -> keypadButton(d.toString()) { entry.press(d); update() } }))
        }
        views.add(
            row(
                listOf(
                    keypadButton("C") { entry.clear(); update() },
                    keypadButton("0") { entry.press('0'); update() },
                    keypadButton("<") { entry.backspace(); update() },
                ),
            ),
        )
        views.add(ui.primary("Open the sale") {
            if (entry.minor <= 0) msg.show(ui.words(AppWords.amountNeeded)) else openSale(entry.minor) { msg.show(ui.words(it)) }
        })
        views.add(ui.button("Back") { showHome() })
        page(views)
    }

    private fun keypadButton(text: String, onClick: () -> Unit): Button =
        ui.button(text, onClick).also { it.textSize = 24f }

    private fun row(buttons: List<Button>): View = LinearLayout(act).apply {
        orientation = LinearLayout.HORIZONTAL
        for (b in buttons) addView(b, LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f))
    }

    /** The price a sale is quoted at: the newest `combine` of the samples already collected (RT-41), or the words of why there is none. */
    private fun quote(): Pair<org.openbmt.counter.Rate?, SayWords?> {
        val b = book ?: return null to AppWords.stopped
        val r = b.rate()
        val e = r.exceptionOrNull()
        if (e is CounterException) return null to e.sayWords()
        return r.getOrNull() to null
    }

    private fun openSale(amountMinor: Long, refused: (SayWords) -> Unit) {
        val n = node ?: return render()
        val m = money ?: return render()
        val (rate, words) = quote()
        if (rate == null) return refused(words ?: AppWords.stopped)
        try {
            val o = n.open(nowSecs(), amountMinor, m.code, rate.rate, rate.sourceLabel, rate.sourceFetchedAt, atTheTill())
            amounts[o.index] = amountMinor
            showSale(o.index, o.payment, null)
        } catch (e: CounterException) {
            refused(e.sayWords())
        }
    }

    // ---- the sale ------------------------------------------------------------------------------------------------------------

    private fun showSale(index: Long, first: Payment?, notice: SayWords?) {
        val n = node ?: return render()
        val m = money ?: return render()
        locks.saleScreen(index)
        val views = ArrayList<View>()
        // The state in large type and the amount at 44 sp fill the top third, so that it reads across the counter.
        val band = SaleBand(act)
        views.add(band)
        val msg = Slot(act)
        if (notice != null) msg.show(ui.words(notice))
        views.add(msg)
        val maxPx = (act.resources.displayMetrics.heightPixels * 0.45f).toInt()
        var qr: QrView? = first?.let { QrView(act, it, maxPx) }
        qr?.let { views.add(it) }
        val received = ui.text("", 16f)
        val state = Slot(act)
        val buttons = Slot(act)
        val done = Slot(act)
        val lightning = Slot(act)
        views.add(received)
        views.add(state)
        views.add(lightning)
        views.add(buttons)
        var uri: String? = first?.copyUri
        val slotForQr = LinearLayout(act).apply { orientation = LinearLayout.VERTICAL }
        if (qr == null) views.add(2, slotForQr)
        views.add(ui.button("Copy the request") {
            val u = uri
            if (u != null) {
                act.getSystemService(ClipboardManager::class.java)?.setPrimaryClip(ClipData.newPlainText("payment request", u))
                msg.show(ui.words(AppWords.requestCopied))
            }
        })
        views.add(ui.button("Details and refunds") { showSaleDetails(index) })
        done.show(ui.button("Done") { showHome() })
        views.add(done)
        page(views)
        var shown: SaleState? = null
        tick = {
            try {
                val s = n.judge(nowSecs(), index)
                val p = s.payment
                if (p != null) {
                    uri = p.copyUri
                    val existing = qr
                    if (existing == null) {
                        val created = QrView(act, p, maxPx)
                        qr = created
                        slotForQr.addView(created)
                    } else {
                        existing.show(p)
                    }
                }
                val says = WordsModel.of(s.words.say())
                band.show(says, SaleModel.amountBig(m, amounts[index], s.needsSats), SaleModel.amountSmall(amounts[index], s.needsSats))
                received.text = SaleModel.receivedLine(s.receivedSats, s.needsSats)
                state.show(
                    ui.wordsBelowBand(says),
                    *listOfNotNull(s.check).map { c -> ui.words(c.say()) }.toTypedArray(),
                    *s.flags.map { f -> ui.words(f.say()) }.toTypedArray(),
                )
                // CR-113: the line that says Lightning is not available for this sale, or that its code is on the way.
                val ln = s.lightning
                val unavailable = ln?.unavailable
                when {
                    unavailable != null -> lightning.show(ui.words(unavailable.say()))
                    ln != null && ln.waiting -> lightning.show(ui.words(AppWords.lightningWaiting))
                    else -> lightning.show()
                }
                if (shown != s.state) {
                    shown = s.state
                    val b = SaleModel.buttons(s.state)
                    val list = ArrayList<View>()
                    if (b.requote) list.add(ui.primary("Requote") { requote(index, msg) })
                    if (b.cancel) list.add(ui.danger("Cancel the sale") { cancelSale(index, msg) })
                    if (b.override) list.add(ui.button(if (cashiers.isEmpty()) "Accept by hand (manager)" else "Accept by hand") { showOverride(index) })
                    buttons.show(*list.toTypedArray())
                    // Once the sale is paid, the one thing left to do is to finish.
                    done.show(if (SaleModel.isPaid(s.state)) ui.primary("Done", Colour.Green) { showHome() } else ui.button("Done") { showHome() })
                }
            } catch (e: CounterException) {
                state.show(ui.words(e.sayWords()))
            }
        }
        tick?.invoke()
    }

    private fun requote(index: Long, msg: Slot) {
        val n = node ?: return render()
        val (rate, words) = quote()
        if (rate == null) return msg.show(ui.words(words ?: AppWords.stopped))
        try {
            val o = n.requote(nowSecs(), index, rate.rate, rate.sourceLabel, rate.sourceFetchedAt)
            showSale(index, o.payment, null)
        } catch (e: CounterException) {
            msg.show(ui.words(e.sayWords()))
        }
    }

    private fun cancelSale(index: Long, msg: Slot) {
        val n = node ?: return render()
        try {
            n.cancel(nowSecs(), index, atTheTill())
            tick?.invoke()
        } catch (e: CounterException) {
            msg.show(ui.words(e.sayWords()))
        }
    }

    // ---- who acts (CR-21, CR-67 to CR-69) ----------------------------------------------------------------------------------------

    /** The choice of who acts, with the PIN field a manager types into; [signedIn] is the person at the till when cashiers are listed (CR-148). */
    private class ActorBlock(val group: RadioGroup, val pin: EditText, val signedIn: Actor?)

    /** CR-148: who is signed in at the till, when cashiers are listed; otherwise null and the cashier is the phone's one name. */
    private fun atTheTill(): Actor? = if (cashiers.isNotEmpty()) locks.who else null

    private fun actorBlock(views: MutableList<View>): ActorBlock {
        val who = atTheTill()
        views.add(label("Who is asking"))
        if (who != null) {
            // The signed-in person acts without typing a PIN; a cashier may ask a manager instead, with the manager's PIN (CR-86).
            val cashier = who.role == ActorRole.Cashier
            val group = ui.choice(if (cashier) listOf("Me, ${who.name}", "Manager (needs the PIN)") else listOf("Me, ${who.name}"))
            views.add(group)
            val pinInput = pinField(act, "Manager's PIN")
            if (cashier) views.add(pinInput)
            return ActorBlock(group, pinInput, who)
        }
        val group = ui.choice(listOf("Cashier", "Manager (needs the PIN)"))
        views.add(group)
        val pinInput = pinField(act, "Manager's PIN")
        views.add(pinInput)
        return ActorBlock(group, pinInput, null)
    }

    /** The person who acts: the signed-in person, or the cashier by name, or the manager when the PIN is right (CR-21, CR-148). */
    private fun resolveActor(block: ActorBlock, done: (Actor) -> Unit, failed: (SayWords) -> Unit) {
        val cfg = config ?: return render()
        val who = block.signedIn
        if (who != null) {
            if (ui.chosen(block.group) != 1) return done(who)
            return checkPin(block.pin.text.toString(), { done(Actor(ActorRole.Manager, cfg.manager)) }, failed)
        }
        if (ui.chosen(block.group) != 1) {
            if (cfg.cashier.isEmpty()) return failed(AppWords.nameNeeded)
            return done(Actor(ActorRole.Cashier, cfg.cashier))
        }
        checkPin(block.pin.text.toString(), { done(Actor(ActorRole.Manager, cfg.manager)) }, failed)
    }

    private fun checkPin(text: String, ok: () -> Unit, failed: (SayWords) -> Unit) {
        async({ pin.check(text) }) { r ->
            val e = r.exceptionOrNull()
            if (e != null) {
                failed(if (e is IOException) AppWords.fileNotSaved(e.message) else AppWords.fault(CounterException("pin", e.javaClass.simpleName, 4)))
            } else {
                when (val v = r.getOrNull()) {
                    PinResult.Ok -> ok()
                    is PinResult.Wrong -> failed(AppWords.pinWrong(v.waitSecs))
                    is PinResult.Locked -> failed(AppWords.pinLocked(v.waitSecs))
                    null -> failed(AppWords.fault(CounterException("pin", "no answer", 4)))
                }
            }
        }
    }

    private fun showOverride(index: Long) {
        val n = node ?: return render()
        val cfg = config ?: return render()
        locks.saleScreen(index)
        val msg = Slot(act)
        if (atTheTill() != null) {
            // CR-148: a cashier's override is the signed-in cashier's, with no second PIN; the manager's still needs the PIN.
            val views = ArrayList<View>()
            views.add(ui.words(AppWords.overrideAbout))
            val actor = actorBlock(views)
            views.add(msg)
            views.add(ui.primary("Accept this sale") {
                resolveActor(actor, { who ->
                    try {
                        n.overrideSale(nowSecs(), index, who)
                        showSale(index, null, null)
                    } catch (e: CounterException) {
                        msg.show(ui.words(e.sayWords()))
                    }
                }, { msg.show(ui.words(it)) })
            })
            views.add(ui.button("Back") { showSale(index, null, null) })
            return page(views)
        }
        val pinInput = pinField(act, "Manager's PIN")
        page(
            listOf(
                ui.words(AppWords.managerNeeded),
                pinInput,
                msg,
                ui.primary("Accept this sale") {
                    checkPin(pinInput.text.toString(), {
                        try {
                            n.overrideSale(nowSecs(), index, Actor(ActorRole.Manager, cfg.manager))
                            showSale(index, null, null)
                        } catch (e: CounterException) {
                            msg.show(ui.words(e.sayWords()))
                        }
                    }, { msg.show(ui.words(it)) })
                },
                ui.button("Back") { showSale(index, null, null) },
            ),
        )
    }

    // ---- sales and refunds ------------------------------------------------------------------------------------------------------------

    private fun showSales() {
        if (lockedOut()) return
        val n = node ?: return render()
        val views = ArrayList<View>()
        views.add(label("Sales the counter holds"))
        try {
            val sales = n.status().sales.reversed().take(MAX_LISTED)
            if (sales.isEmpty()) views.add(ui.text("No sales yet.", 16f))
            for (i in sales) {
                val s = n.judge(nowSecs(), i)
                val amount = amounts[i]?.let { money?.show(it) + ", " }.orEmpty()
                views.add(ui.button("Sale ${i + 1}: ${s.words.heading}, $amount${Format.sats(s.needsSats)}") { showSaleDetails(i) })
            }
        } catch (e: CounterException) {
            views.add(ui.words(e.sayWords()))
        }
        views.add(ui.button("Back") { showHome() })
        page(views)
    }

    private fun showSaleDetails(index: Long, notice: SayWords? = null) {
        val n = node ?: return render()
        val m = money ?: return render()
        locks.saleScreen(index)
        val views = ArrayList<View>()
        notice?.let { views.add(ui.words(it)) }
        views.add(label("Sale ${index + 1}"))
        try {
            val s = n.judge(nowSecs(), index)
            views.add(ui.text(SaleModel.amountLine(m, amounts[index], s.needsSats), 18f, bold = true))
            views.add(ui.text(SaleModel.receivedLine(s.receivedSats, s.needsSats), 16f))
            views.add(ui.words(s.words.say()))
            s.check?.let { views.add(ui.words(it.say())) }
            for (f in s.flags) views.add(ui.words(f.say()))
            views.add(label("Refunds"))
            val rows = n.refunds(index)
            if (rows.isEmpty()) views.add(ui.text("No refund was asked for.", 16f))
            for (r in rows) {
                val by = "${if (r.byRole == ActorRole.Manager) "manager" else "cashier"} ${Format.cleanForeign(r.byName)}"
                views.add(ui.text("Asked ${Format.sats(r.askedSats)}${r.valueMinor?.let { ", " + m.show(it) } ?: ""} by $by at ${Format.localTime(r.at)}", 14f))
                views.add(ui.words(r.words.say()))
                val number = r.number
                if (number != null && (r.state == RefundState.Requested || r.state == RefundState.WaitingForDepth)) {
                    views.add(ui.danger("Withdraw this request") { showWithdraw(index, number) })
                }
            }
            views.add(ui.button("Ask for a refund") { showRefundRequest(index) })
            views.add(ui.button("Refund in cash from the till") { showCashRefund(index) })
        } catch (e: CounterException) {
            views.add(ui.words(e.sayWords()))
        }
        views.add(ui.button("Back to the sale") { showSale(index, null, null) })
        views.add(ui.button("Back to sales") { showSales() })
        page(views)
    }

    private fun showRefundRequest(index: Long) {
        val n = node ?: return render()
        val m = money ?: return render()
        locks.saleScreen(index)
        val views = ArrayList<View>()
        val msg = Slot(act)
        views.add(msg)
        views.add(label("Amount to refund"))
        val kind = ui.choice(listOf("In ${m.code}", "In sats"))
        views.add(kind)
        val amount = amountField(act, "Amount")
        views.add(amount)
        views.add(label("Address the customer wants it sent to"))
        val destination = ui.field("Paste the address or payment request", lines = 2)
        views.add(destination)
        views.add(label("Reason"))
        val reason = ui.field("A short reason")
        views.add(reason)
        val actor = actorBlock(views)
        views.add(ui.primary("Ask for the refund") {
            val text = amount.text.toString()
            val dest = destination.text.toString().trim()
            val why = reason.text.toString().trim()
            val inSats = ui.chosen(kind) == 1
            val sats = Format.toAmount(text)
            val minor = m.parse(text)
            when {
                (inSats && (sats == null || sats == 0L)) || (!inSats && (minor == null || minor == 0L)) ->
                    msg.show(ui.words(AppWords.notANumber))
                dest.isEmpty() -> msg.show(ui.words(AppWords.destinationNeeded))
                why.isEmpty() -> msg.show(ui.words(AppWords.reasonNeeded))
                else -> resolveActor(actor, { who ->
                    val ask: RefundAsk? = if (inSats) {
                        sats?.let { RefundAsk.Sats(it) }
                    } else {
                        val (rate, words) = quote()
                        if (rate == null) {
                            msg.show(ui.words(words ?: AppWords.stopped))
                            null
                        } else {
                            minor?.let { RefundAsk.Value(it, rate.rate, rate.sourceLabel, rate.sourceFetchedAt) }
                        }
                    }
                    if (ask != null) {
                        try {
                            n.requestRefund(nowSecs(), index, ask, dest, who, why)
                            showSaleDetails(index, AppWords.refundAsked)
                        } catch (e: CounterException) {
                            msg.show(ui.words(e.sayWords()))
                        }
                    }
                }, { msg.show(ui.words(it)) })
            }
        })
        views.add(ui.button("Back") { showSaleDetails(index) })
        page(views)
    }

    private fun showCashRefund(index: Long) {
        val n = node ?: return render()
        val m = money ?: return render()
        locks.saleScreen(index)
        val views = ArrayList<View>()
        val msg = Slot(act)
        views.add(msg)
        views.add(label("Cash to give back, in ${m.code}"))
        val amount = amountField(act, "Amount")
        views.add(amount)
        views.add(label("Reason"))
        val reason = ui.field("A short reason")
        views.add(reason)
        val actor = actorBlock(views)
        views.add(ui.primary("Save the cash refund") {
            val minor = m.parse(amount.text.toString())
            val why = reason.text.toString().trim()
            when {
                minor == null || minor == 0L -> msg.show(ui.words(AppWords.notANumber))
                why.isEmpty() -> msg.show(ui.words(AppWords.reasonNeeded))
                else -> resolveActor(actor, { who ->
                    try {
                        n.refundInCash(nowSecs(), index, who, minor, why)
                        showSaleDetails(index, AppWords.cashRefunded)
                    } catch (e: CounterException) {
                        msg.show(ui.words(e.sayWords()))
                    }
                }, { msg.show(ui.words(it)) })
            }
        })
        views.add(ui.button("Back") { showSaleDetails(index) })
        page(views)
    }

    private fun showWithdraw(index: Long, number: Long) {
        val n = node ?: return render()
        locks.saleScreen(index)
        val views = ArrayList<View>()
        val msg = Slot(act)
        views.add(msg)
        views.add(ui.text("Withdraw refund request $number of sale ${index + 1}?", 18f, bold = true))
        val actor = actorBlock(views)
        views.add(ui.danger("Withdraw it") {
            resolveActor(actor, { who ->
                try {
                    n.withdrawRefund(nowSecs(), index, number, who)
                    showSaleDetails(index, AppWords.refundWithdrawn)
                } catch (e: CounterException) {
                    msg.show(ui.words(e.sayWords()))
                }
            }, { msg.show(ui.words(it)) })
        })
        views.add(ui.button("Back") { showSaleDetails(index) })
        page(views)
    }

    // ---- who is at the till (CR-147 to CR-150) ------------------------------------------------------------------------------

    /** "Who is at the till?": the cashiers' names and the manager's. */
    private fun showWho(notice: SayWords?) {
        val cfg = config ?: return render()
        val views = ArrayList<View>()
        views.add(ui.words(notice ?: AppWords.whoAtTill))
        for (name in cashiers) views.add(ui.button(name) { showSignIn(Actor(ActorRole.Cashier, name)) })
        views.add(ui.button("${cfg.manager} (manager)") { showSignIn(Actor(ActorRole.Manager, cfg.manager)) })
        page(views)
    }

    private fun showSignIn(person: Actor) {
        val msg = Slot(act)
        val pinInput = pinField(act, "PIN")
        page(
            listOf(
                ui.text(person.name, 22f, bold = true),
                label("PIN"),
                pinInput,
                msg,
                ui.primary("Sign in") { signIn(person, pinInput.text.toString(), msg) },
                ui.button("Back") { showWho(null) },
            ),
        )
    }

    /** CR-150: the PIN check of CR-85 for that person, counted and stored before it is compared; a held PIN is not looked at. */
    private fun signIn(person: Actor, text: String, msg: Slot) {
        async({ if (person.role == ActorRole.Manager) pin.check(text) else staff.check(person.name, text) }) { r ->
            val e = r.exceptionOrNull()
            if (e is IOException) return@async msg.show(ui.words(AppWords.fileNotSaved(e.message)))
            if (e != null) throw e
            when (val v = r.getOrThrow()) {
                PinResult.Ok -> {
                    locks.signIn(person)
                    showHome()
                }
                is PinResult.Wrong -> msg.show(ui.words(AppWords.signInWrong(v.waitSecs)))
                is PinResult.Locked -> msg.show(ui.words(AppWords.signInHeld(v.waitSecs)))
            }
        }
    }

    // ---- staff: the cashiers (CR-142) ------------------------------------------------------------------------------------------------

    /** The manager's PIN opens Staff. */
    private fun showStaffEntry() {
        if (lockedOut()) return
        val msg = Slot(act)
        val pinInput = pinField(act, "Manager's PIN")
        page(
            listOf(
                ui.words(AppWords.staffPinNeeded),
                pinInput,
                msg,
                ui.primary("Open") { checkPin(pinInput.text.toString(), { showStaff(emptyList()) }, { msg.show(ui.words(it)) }) },
                ui.button("Back") { showHome() },
            ),
        )
    }

    private fun showStaff(notices: List<SayWords>) {
        if (lockedOut()) return
        val cfg = config ?: return render()
        if (node == null) return render()
        val views = ArrayList<View>()
        views.add(ui.words(AppWords.staffAbout))
        for (w in notices) views.add(ui.words(w))
        views.add(label("Cashiers"))
        if (cashiers.isEmpty()) views.add(ui.text("No cashier is listed, so nobody has to sign in.", 16f))
        for (name in cashiers) {
            views.add(ui.text(name, 18f, bold = true))
            views.add(ui.button("New PIN for $name") { showCashierPin(name) })
            views.add(ui.danger("Remove $name") { showRemoveCashier(name) })
        }
        views.add(label("Add a cashier"))
        val nameField = ui.field("Name of the cashier")
        val pin1 = pinField(act, "PIN, 4 to 8 digits (the cashier types it)")
        val pin2 = pinField(act, "PIN again")
        val msg = Slot(act)
        views.add(nameField)
        views.add(pin1)
        views.add(pin2)
        views.add(msg)
        views.add(ui.primary("Add the cashier") {
            addCashier(cfg, nameField.text.toString().trim(), pin1.text.toString(), pin2.text.toString(), msg)
        })
        views.add(ui.button("Back") { showHome() })
        page(views)
    }

    /** CR-142: the file first, then the record for the owner; the list is changed even when the record cannot be written, and the app says so. */
    private fun addCashier(cfg: AppConfig, name: String, pin1: String, pin2: String, msg: Slot) {
        (StaffCheck.nameProblem(name, cashiers, cfg.manager) ?: StaffCheck.pinProblem(pin1, pin2))?.let { return msg.show(ui.words(it)) }
        msg.show(ui.words(AppWords.starting))
        async({
            staff.add(name, pin1)
            staff.names()
        }) { r ->
            val e = r.exceptionOrNull()
            if (e is IOException) return@async msg.show(ui.words(AppWords.fileNotSaved(e.message)))
            if (e != null) throw e
            cashiers = r.getOrThrow()
            // The manager's PIN opened this screen, so the manager who turned the lock on is the person at the till (CR-146).
            if (locks.who == null) locks.signIn(Actor(ActorRole.Manager, cfg.manager))
            recordStaff(true, name, AppWords.cashierAdded)
        }
    }

    private fun showRemoveCashier(name: String) {
        if (lockedOut()) return
        val msg = Slot(act)
        page(
            listOf(
                ui.text("Remove $name?", 20f, bold = true),
                msg,
                ui.danger("Remove") { removeCashier(name, msg) },
                ui.button("Back") { showStaff(emptyList()) },
            ),
        )
    }

    private fun removeCashier(name: String, msg: Slot) {
        async({
            staff.remove(name)
            staff.names()
        }) { r ->
            val e = r.exceptionOrNull()
            if (e is IOException) return@async msg.show(ui.words(AppWords.fileNotSaved(e.message)))
            if (e != null) throw e
            cashiers = r.getOrThrow()
            // CR-146: a person who was removed is signed out, and the last one removed turns the lock off.
            val who = locks.who
            if (who != null && who.role == ActorRole.Cashier && who.name == name) locks.lock()
            if (cashiers.isEmpty()) locks.forget()
            recordStaff(false, name, AppWords.cashierRemoved)
        }
    }

    private fun showCashierPin(name: String) {
        if (lockedOut()) return
        val msg = Slot(act)
        val pin1 = pinField(act, "New PIN, 4 to 8 digits (the cashier types it)")
        val pin2 = pinField(act, "New PIN again")
        page(
            listOf(
                ui.text("New PIN for $name", 20f, bold = true),
                pin1,
                pin2,
                msg,
                ui.primary("Save the new PIN") { saveCashierPin(name, pin1.text.toString(), pin2.text.toString(), msg) },
                ui.button("Back") { showStaff(emptyList()) },
            ),
        )
    }

    /** CR-142: a new PIN is not a record; it clears that cashier's count and hold. */
    private fun saveCashierPin(name: String, pin1: String, pin2: String, msg: Slot) {
        StaffCheck.pinProblem(pin1, pin2)?.let { return msg.show(ui.words(it)) }
        msg.show(ui.words(AppWords.starting))
        async({ staff.setPin(name, pin1) }) { r ->
            val e = r.exceptionOrNull()
            if (e is IOException) return@async msg.show(ui.words(AppWords.fileNotSaved(e.message)))
            if (e != null) throw e
            showStaff(listOf(AppWords.cashierPinSaved))
        }
    }

    /** CR-143: the record of the change goes to the library; when it cannot be written the screen says the owner was not told. */
    private fun recordStaff(added: Boolean, name: String, done: SayWords) {
        val n = node ?: return render()
        val cfg = config ?: return render()
        try {
            n.staffRecord(nowSecs(), added, name, cfg.manager)
            showStaff(listOf(done))
        } catch (e: CounterException) {
            showStaff(listOf(AppWords.ownerNotTold, e.sayWords()))
        }
    }

    // ---- today (CR-152 to CR-155) ----------------------------------------------------------------------------------------------------

    /** The day's totals from the counter's own records, from local midnight to now; the lines per person only after the manager's PIN. */
    private fun showToday(people: Boolean) {
        if (lockedOut()) return
        val n = node ?: return render()
        val m = money ?: return render()
        val now = nowSecs()
        val from = DayWindow.start(now, ZoneId.systemDefault())
        val views = ArrayList<View>()
        views.add(label("Today"))
        views.add(ui.text("From ${Format.localTime(from)} until now", 14f))
        try {
            val t = n.dayTotals(from, DayWindow.end(now), people)
            views.add(ui.words(AppWords.todayAbout))
            for (line in TodayModel.lines(t, m)) views.add(ui.text(line, 17f))
            if (t.waiting.count > 0) views.add(ui.words(AppWords.todayWaiting))
            if (t.stopped > 0) views.add(ui.words(AppWords.todayStopped))
            if (people) {
                views.add(label("Lines per person"))
                val lines = TodayModel.people(t)
                if (lines.isEmpty()) views.add(ui.text("Nobody is named in today's records.", 16f))
                for (line in lines) views.add(ui.text(line, 16f))
            } else {
                val msg = Slot(act)
                val pinInput = pinField(act, "Manager's PIN")
                views.add(ui.words(AppWords.todayPeopleNeedPin))
                views.add(pinInput)
                views.add(msg)
                views.add(ui.primary("Show the lines") {
                    checkPin(pinInput.text.toString(), { showToday(true) }, { msg.show(ui.words(it)) })
                })
            }
        } catch (e: CounterException) {
            views.add(ui.words(e.sayWords()))
        }
        views.add(ui.button("Back") { showHome() })
        page(views)
    }

    // ---- about ----------------------------------------------------------------------------------------------------------------------

    private fun showAbout() {
        if (lockedOut()) return
        page(
            listOf(
                ui.text("OpenBMT Counter $VERSION", 22f, bold = true),
                ui.text("Not for real funds. No independent security audit has been done. First build: it has not been run on a phone yet.", 16f, bold = true),
                label("Keep the app open at the counter"),
                ui.text(
                    "The counter looks for payments only while this app is on the screen, and it stops when you leave the app or the screen goes off. " +
                        "There is no service in the background and no notification in this version. The screen is kept on while the app is open.",
                    16f,
                ),
                label("Not built yet"),
                ui.text(
                    "Several managers with their own PINs (there is one manager PIN for this phone); " +
                        "receipts; changing the shop's settings after pairing.",
                    16f,
                ),
                label("Cashiers and the till"),
                ui.text(
                    "The manager can list up to 30 cashiers, each with a name and a PIN, under Staff. Once one is listed the till is locked: a person taps their name and types " +
                        "their PIN, and sales, cancels and refunds are written down with that name. The app locks when Lock is tapped and after more than five minutes out of view, " +
                        "never while a sale is open. The list stays on this phone, is not backed up, and the owner is sent a record of each cashier added or removed. " +
                        "A PIN on a phone stops a casual hand and nobody else.",
                    16f,
                ),
                label("Today"),
                ui.text(
                    "Today counts this counter's own records from midnight until now: sales paid, waiting and cancelled, overrides, refunds and cash refunds. " +
                        "The lines per person need the manager's PIN. The owner's phone has the full books and the export.",
                    16f,
                ),
                label("Tor"),
                ui.text(
                    "When the shop's settings turn Tor on, every connection goes through Orbot on this phone. If Orbot is not running the counter sends nothing and says so.",
                    16f,
                ),
                label("Lightning"),
                ui.text(
                    "The manager can give the counter a connection to a Lightning node of the shop's own, with the PIN. The key it holds can only make and look up invoices. " +
                        "A payment by Lightning is final at once and is kept with its payment number and time. If the node does not answer, the sale shows the on-chain code only. " +
                        "Running a node means watching its channels, its updates and its backups.",
                    16f,
                ),
                label("Your own node"),
                ui.text(
                    "The manager can give the counter the Tor address of a node of the shop's own, with the PIN. The counter then asks it about each payment " +
                        "as well, and a payment both see is held to the higher limits. If the node cannot be reached the sale screen says it used the light check. " +
                        "A home-network address is refused, and the node needs Tor, so Orbot must be on.",
                    16f,
                ),
                ui.button("Back") { showHome() },
            ),
        )
    }

    companion object {
        const val VERSION = "0.1.0"
        const val KEY_ALIAS = "openbmt-counter-pairing-v1"
        const val PAIRING_FILE = "pairing.obks"
        const val TYPE_BYTES = "bytes"
        private const val MAX_LISTED = 50
    }
}
