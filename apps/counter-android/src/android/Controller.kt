package org.openbmt.counter.app

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Intent
import android.graphics.Bitmap
import android.graphics.Canvas
import android.net.Uri
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.view.View
import android.view.ViewGroup
import android.widget.Button
import android.widget.FrameLayout
import android.widget.EditText
import android.widget.LinearLayout
import android.widget.RadioGroup
import android.widget.TextView
import java.io.File
import java.io.IOException
import java.security.GeneralSecurityException
import java.security.SecureRandom
import java.time.ZoneId
import java.util.concurrent.LinkedBlockingQueue
import java.util.concurrent.TimeUnit
import org.openbmt.app.AnimatedQrView
import org.openbmt.app.CameraFrame
import org.openbmt.app.Colour
import org.openbmt.app.Feedback
import org.openbmt.app.Format
import org.openbmt.app.LogoSnapView
import org.openbmt.app.Palette
import org.openbmt.app.RowItem
import org.openbmt.app.SnapTiming
import org.openbmt.app.KeystoreSealer
import org.openbmt.app.SayWords
import org.openbmt.app.ScanFlow
import org.openbmt.app.SealedFile
import org.openbmt.app.TorLease
import org.openbmt.app.TorRunner
import org.openbmt.app.TorStartException
import org.openbmt.app.Ui
import org.openbmt.app.UrScan
import org.openbmt.app.UrWords
import org.openbmt.app.Who
import org.openbmt.app.WordsModel
import org.openbmt.counter.Actor
import org.openbmt.counter.ActorRole
import org.openbmt.counter.BillKind
import org.openbmt.counter.BillState
import org.openbmt.counter.PartWay
import org.openbmt.counter.ChangeState
import org.openbmt.counter.CounterException
import org.openbmt.counter.CounterNode
import org.openbmt.counter.LnRequest
import org.openbmt.counter.OwnWalletKind
import org.openbmt.counter.PairStarted
import org.openbmt.counter.Pairing
import org.openbmt.counter.Payment
import org.openbmt.counter.RateSource
import org.openbmt.counter.Rates
import org.openbmt.counter.RefundAsk
import org.openbmt.counter.RefundState
import org.openbmt.counter.RelayKind
import org.openbmt.counter.SaleInfo
import org.openbmt.counter.SaleState
import org.openbmt.counter.StarterState
import org.openbmt.counter.SwitchState
import org.openbmt.counter.TillRequest
import org.openbmt.counter.Shop
import org.openbmt.counter.ShopSettings
import org.openbmt.counter.TipChoice
import org.openbmt.counter.OwnNodeSetting
import org.openbmt.counter.ShopLightningStatus
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
    /** CR-261: the record of a half-done switch is being kept (off the screen thread), so the home tick does not start it twice. */
    private var switchKeeping = false
    private var config: AppConfig? = null
    private var lightningRecord: ByteArray? = null
    private var lightningFault: SayWords? = null
    /** SL-76: the sealed seed record of Shop Lightning, when one is kept; given to the library at each start. */
    private var shopSeedRecord: ByteArray? = null
    private var shopSeedDeleting = false

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
    @Volatile
    private var visible = false
    private var fault: SayWords? = null
    private var torBlocked: SayWords? = null

    /** Whether the settings in force, as the running node has them, have Tor on: the row Tor of Settings shows then (CR-203). */
    private var torOnNow = false
    private var node: CounterNode? = null
    private var money: Money? = null
    private var book: PriceBook? = null
    private var pumpFault: SayWords? = null
    private var pumpThread: Thread? = null
    private var linkThread: Thread? = null
    private val workers = ArrayList<Thread>()
    private var lightningThread: Thread? = null
    private var transport: LightningTransport? = null
    private var swapTransport: SwapTransport? = null
    private val lightningJobs = LinkedBlockingQueue<Runnable>()
    private val startLock = Any()

    // Which screen this run shows (CR-177), and the one it was showing when the app left the screen (null in a new process, which asks the library).
    private var showing: Showing = Showing.Home
    private var resumeTo: Showing? = null

    // Where the person goes once signed in, when the till was locked on the way to a screen (CR-177).
    private var afterSignIn: Showing? = null

    // The sales that have played their outcome sound in this run, and the lines of home a tap has opened (CR-178 keeps neither across a restart).
    private val outcomePlays = OutcomePlays()
    private val openLines = HashSet<String>()

    // What the open screen redraws as the node reports.
    private var tick: (() -> Unit)? = null

    // TP-20: Android sends a reader's commands to the tag service only while a sale's payment screen is in front.
    private val tapPreference = TapPreference { on -> TapPreferred.set(act, on) }

    /** The activity came to the front or left it (TP-20). */
    fun tapFront(front: Boolean) {
        tapPreference.resumed(front)
    }

    /** TP-05: the tag of the sale on the screen, set with its code and again when the request changes; null clears it. */
    private fun tapSet(tag: ByteArray?) {
        TapTag.set(tag)
        tapPreference.wanted(TapTag.current() != null)
    }

    /** The tick of the Home screen: Home is showing when [tick] is this one (CR-164). */
    private var homeTick: (() -> Unit)? = null

    // ---- the built-in Tor (CR-201 to CR-204) -------------------------------------------------------------

    private val torLock = Any()
    private var torLease: TorLease? = null

    /** The lease on the built-in Tor, taken now if this controller has none or its run has ended; not while the app is hidden. */
    private fun ensureTor(): TorLease = synchronized(torLock) {
        if (!visible) throw TorStartException("the app is hidden")
        torLease?.takeIf { !it.dead } ?: run {
            torLease?.close()
            TorRunner.acquire(dir).also { torLease = it }
        }
    }

    /** Lets go of the built-in Tor; it shuts down at once, as nothing else holds it (CR-201). */
    private fun releaseTor() {
        val l = synchronized(torLock) { torLease.also { torLease = null } }
        l?.close()
    }

    /** CR-87: set when the start was refused because the settings the owner sent turned Tor on. */
    private var torForced = false
    private var lastTick = 0L

    // What this run knows of its sales: the amount in the shop's currency of each, by index.
    private val amounts = HashMap<Long, Long>()

    private fun nowSecs(): Long = System.currentTimeMillis() / 1000

    // ---- visible, hidden -------------------------------------------------------------------------

    fun visible() {
        visible = true
        Feedback.init(act)
        if (scan.asking) return
        locks.backInView()
        begin()
    }

    fun hidden() {
        visible = false
        tapSet(null)
        if (scan.asking) return
        locks.leftView()
        if (node != null) resumeTo = showing
        forgetCeremony()
        scan.stop()
        stopNode()
        // CR-201: the built-in Tor runs only while the app is shown.
        releaseTor()
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
        torForced = false
        stopNode()
        val setup = try {
            config = configFile.load()
            Feedback.soundOn = config?.sounds ?: true
            val paired = File(dir, PAIRING_FILE).exists()
            SetupStatus(paired, pin.isSet(), config != null)
        } catch (e: IOException) {
            fault = AppWords.fileNotRead(e.message)
            return render()
        }
        if (!setup.complete) return showFirstStart(if (progress.takeForgotten()) (if (progress.own) AppWords.pairingOwnForgotten else AppWords.pairingForgotten) else null)
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
        // SL-76: the Lightning key, sealed by its own key; a file that does not read is as no key, and the library asks for it again.
        shopSeedRecord = try {
            SealedFile(dir, KeystoreSealer(ShopLightningWords.KEY_ALIAS), ShopLightningWords.FILE).load()
        } catch (e: IOException) {
            null
        } catch (e: GeneralSecurityException) {
            null
        }
        shopSeedDeleting = false
        dropSharedImage()
        startNode()
    }

    private fun render() {
        val setup = SetupStatus(File(dir, PAIRING_FILE).exists(), pin.isSet(), config != null)
        when (val s = ScreenModel.pick(fault, setup, torBlocked, node != null, locked())) {
            is Screen.Fault -> showMessage(s.words, "Try again") { begin() }
            is Screen.FirstStart -> showFirstStart(null)
            is Screen.TorBlocked ->
                // CR-204: the built-in Tor did not start: try again, or choose Orbot. Orbot did not answer: check again, or its address.
                if (s.words === AppWords.torFailed) {
                    showCenter(s.words, listOf(ui.primary("Try again") { begin() }, ui.button("Tor") { showTor() }))
                } else {
                    showCenter(s.words, listOf(ui.primary("Check again") { begin() }, ui.button("Network") { showNetwork() }))
                }
            is Screen.Starting -> showWorking()
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
        showWorking()
        val rec = record ?: return
        val cfg = config ?: return
        val lightning = lightningRecord
        val shopSeed = shopSeedRecord
        val thread = Thread {
            try {
                val sources = Rates.sources()
                // For display only: the library checks the record again at the start (CR-97).
                // CR-163: Tor as the settings in force have it, which the owner may have changed since pairing.
                val pairing = Shop.pairing(rec, nodeDir.path)
                val torOn = pairing.settings.torOn || torForced
                // CR-201: Tor of the app's own runs only when the settings have Tor on and it is the choice; else a run of it is let go of.
                val builtIn = torOn && cfg.tor == TorChoice.BuiltIn
                val address: String
                if (builtIn) {
                    main.post { if (id == runId && node == null) showWorking(AppWords.torStarting) }
                    val port = try {
                        ensureTor().awaitPort(TOR_WAIT_MS)
                    } catch (_: TorStartException) {
                        // CR-204: no Tor, no node: nothing is sent around the proxy.
                        main.post {
                            if (id == runId) {
                                torBlocked = AppWords.torFailed
                                render()
                            }
                        }
                        return@Thread
                    }
                    address = "127.0.0.1:$port"
                } else {
                    releaseTor()
                    address = cfg.proxy
                }
                val gate = TorGate(torOn, address)
                if (!gate.available()) {
                    main.post {
                        if (id == runId) {
                            torBlocked = if (builtIn) AppWords.torFailed else AppWords.orbotNotRunning
                            render()
                        }
                    }
                    return@Thread
                }
                synchronized(startLock) {
                    if (id != runId) return@Thread
                    nodeDir.mkdirs()
                    // CR-134: an onion relay with Tor off in the settings goes through Orbot, when it answers.
                    val linkProxy = LinkProxy.pick(pairing.relay == RelayKind.Onion, torOn, cfg.proxy, Socks5Probe())
                    val n = CounterNode.start(
                        StartConfig(nodeDir.path, nowSecs(), rec, gate.proxy, cfg.ownNode.ifEmpty { null }, lightning, linkProxy, shopSeed),
                    )
                    if (id != runId) {
                        n.close()
                        return@Thread
                    }
                    main.post {
                        if (id == runId) adopt(id, n, sources, gate, pairing.settings) else n.close()
                    }
                }
            } catch (e: CounterException) {
                main.post {
                    if (id != runId) return@post
                    // CR-87: the settings the owner sent after pairing have Tor on: start again with the proxy.
                    if (e.rule == "CR-87" && !torForced) {
                        torForced = true
                        return@post startNode()
                    }
                    fault = e.sayWords()
                    render()
                }
            } catch (e: LinkageError) {
                main.post { if (id == runId) { fault = AppWords.libraryMissing(e.message); render() } }
            }
        }
        thread.isDaemon = true
        thread.start()
    }

    private fun adopt(id: Int, n: CounterNode, allSources: List<RateSource>, gate: TorGate, settings: ShopSettings) {
        // CR-193: the shop's currency is in its signed settings, and its price comes from the sources of that currency only (RT-40).
        val m = Money.of(settings.currency)
        if (m == null) {
            n.close()
            fault = AppWords.fault(CounterException("shop-currency", "the settings name a currency the app cannot write", 4))
            return render()
        }
        node = n
        torOnNow = gate.torOn
        money = m
        pumpFault = null
        // CR-197: Bitcoin Standard has no price: no source is asked and there is no book.
        val sources = ShopPrices.sources(m, allSources)
        val b = PriceBook(LibraryRates, { nowSecs() }, (config?.tor ?: TorChoice.Orbot).missingWords)
        book = if (m.isSats) null else b
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
                    // CR-164: new settings that change Tor or the Lightning node take effect when the node starts again, which is done when Home is showing and no sale screen is open.
                    if (restartNeeded(n) || currencyChanged()) main.post { if (id == runId && tick != null && tick === homeTick) begin() }
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
        // CR-177: the screen this run was showing comes back, drawn from the library's state; a new process opens on the open sale.
        val screen = Resume.choose(resumeTo) { openSales(n) }
        resumeTo = null
        // CR-149: a locked till asks who is at the till first; the lock that waited for a sale's screen happens when it closes.
        if (locked()) {
            afterSignIn = screen
            return showWho(null)
        }
        open(screen)
    }

    /** CR-177: the indexes of the sales the library lists that are not yet ended. A library that cannot say lists none. */
    private fun openSales(n: CounterNode): List<Long> =
        try {
            n.status().sales.filter { Resume.open(n.judge(nowSecs(), it).state) }
        } catch (e: CounterException) {
            emptyList()
        }

    /** Shows [screen] again, drawn fresh from the library's state. */
    private fun open(screen: Showing) {
        when (screen) {
            is Showing.Sale -> showSale(screen.index, null, null)
            Showing.Sales -> showSales()
            Showing.Home, Showing.Other -> showHome()
        }
    }

    /**
     * A change between light and dark with the activity kept (CR-178): the screen is drawn again, in place, from
     * the state. A screen that is not a sale, the list or home is not drawn again from state, and home takes its place;
     * a locked till asks who is at the till again.
     */
    fun redraw() {
        if (!visible || scan.asking) return
        if (node == null) return render()
        if (locked()) return showWho(null)
        open(showing)
    }

    /** CR-164: whether the settings in force need the node started again. */
    private fun restartNeeded(n: CounterNode): Boolean =
        try {
            n.linkState()?.restartNeeded == true
        } catch (_: CounterException) {
            false
        }

    /**
     * CR-195, CR-196: whether the settings now in force name another currency than the one this run prices in. The
     * node is started again, so that the keypad, the price sources and the words follow the new currency.
     */
    private fun currencyChanged(): Boolean {
        val rec = record ?: return false
        val code = money?.code ?: return false
        return try {
            Shop.pairing(rec, nodeDir.path).settings.currency != code
        } catch (_: CounterException) {
            false
        }
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
        val t = LightningTransport(gate.torOn, gate.address)
        transport = t
        // CR-225: the swap provider is reached through the same Tor, by its onion name only.
        val swapTransport = SwapTransport(gate.address)
        this.swapTransport = swapTransport
        lightningJobs.clear()
        val worker = Thread {
            while (runId == id) {
                try {
                    val job = lightningJobs.poll(500, TimeUnit.MILLISECONDS)
                    if (job != null) {
                        job.run()
                        continue
                    }
                    val request = n.lnNext(nowSecs())
                    if (request != null) {
                        when (val out = t.send(request) { der -> pinned(n, request, der) }) {
                            is LnOutcome.Answer -> n.lnReply(request.id, out.status.toLong(), out.body)
                            is LnOutcome.Failed -> n.lnFailed(request.id, out.why)
                        }
                        continue
                    }
                    // CR-219: the swap provider's request, one at a time like the node's.
                    val swap = n.swapNext(nowSecs()) ?: continue
                    when (val out = swapTransport.send(swap.hostPort, swap.bytes)) {
                        is SwapOutcome.Answer -> n.swapReply(swap.id, out.bytes)
                        is SwapOutcome.Failed -> n.swapFailed(swap.id)
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
        swapTransport = null
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

    /** Puts [view] on the screen, with the open screen's tick cleared and [shown] remembered (CR-177). */
    private fun put(view: View, shown: Showing = Showing.Other) {
        dropSharedImage()
        tick = null
        tapSet(null)
        showing = shown
        act.show(view)
    }

    /**
     * A page of the new look: the [bar], the [body] that scrolls and the [actions] fixed at the bottom. [tone]
     * tints the bar.
     */
    private fun show(bar: View, body: List<View>, actions: List<Button> = emptyList(), shown: Showing = Showing.Other, tone: org.openbmt.app.Tone? = null) =
        put(ui.page(bar, body, actions, tone), shown)

    /**
     * A page for forms, lists and detail pages: the bar with [barTitle] (and a back arrow to [back] when given),
     * the [words] as the title block, the [body], and the next line after it as a hint. [actions] sit in the footer.
     */
    private fun titled(
        barTitle: String,
        words: SayWords?,
        body: List<View>,
        actions: List<Button> = emptyList(),
        back: (() -> Unit)? = null,
        shown: Showing = Showing.Other,
        logo: Boolean = false,
    ) {
        val model = words?.let { WordsModel.of(it) }
        val views = ArrayList<View>()
        if (model != null) views.add(ui.titleBlock(model, barTitle))
        views.addAll(body)
        if (model != null) views.add(ui.hint(model))
        show(ui.bar(barTitle, back = back, logo = logo), views, actions, shown)
    }

    /** A screen that is one message: the words as a result panel, and its buttons in the footer. */
    private fun showCenter(words: SayWords, actions: List<Button>, snap: Boolean = false) {
        val model = WordsModel.of(words)
        val page = ui.page(ui.bar("Counter", logo = true), listOf(ui.resultPanel(model)), actions, ui.pal.of(model.colour))
        if (!snap || !Feedback.motionOn(act)) return put(page)
        // The logo clicks together once over the screen, then the screen is there again.
        val overlay = FrameLayout(act).apply { setBackgroundColor((ui.pal.background and 0x00FFFFFF) or 0xF2000000.toInt()) }
        overlay.addView(LogoSnapView(act), FrameLayout.LayoutParams(ViewGroup.LayoutParams.WRAP_CONTENT, ViewGroup.LayoutParams.WRAP_CONTENT, android.view.Gravity.CENTER))
        val holder = FrameLayout(act)
        holder.addView(page, FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT))
        holder.addView(overlay, FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT))
        put(holder)
        (overlay.getChildAt(0) as LogoSnapView).play()
        main.postDelayed({
            overlay.animate().alpha(0f).setDuration(250).withEndAction { (overlay.parent as? ViewGroup)?.removeView(overlay) }.start()
        }, (SnapTiming.total * 1000).toLong() + 400)
    }

    private fun showMessage(words: SayWords, label: String = "Back", snap: Boolean = false, back: () -> Unit) {
        showCenter(words, listOf(ui.primary(label) { back() }), snap)
    }

    /** The screen that only waits: the ring, and the words of [AppWords.starting]. */
    private fun showWorking(words: SayWords = AppWords.starting) {
        show(ui.bar("Counter", logo = true), listOf(ui.working(WordsModel.of(words))))
    }

    /** A message for a Slot: the words as a tinted notice. */
    private fun note(words: SayWords): View = ui.notice(WordsModel.of(words))

    private fun <T> async(work: () -> T, done: (Result<T>) -> Unit) {
        val t = Thread {
            val r = runCatching(work)
            main.post { done(r) }
        }
        t.isDaemon = true
        t.start()
    }

    private fun label(text: String): TextView = ui.label(text)


    /** [view] at the left edge of the body, as wide as it is. */
    private fun left(view: View): View = LinearLayout(act).apply {
        addView(view, LinearLayout.LayoutParams(ViewGroup.LayoutParams.WRAP_CONTENT, ViewGroup.LayoutParams.WRAP_CONTENT))
    }

    /** The next step in a card with an arrow, as the first screen shows it. */
    private fun nextCard(model: WordsModel): View = LinearLayout(act).apply {
        orientation = LinearLayout.HORIZONTAL
        background = ui.roundedFill(ui.pal.surface, 14f, 1f, ui.pal.divider)
        setPadding(ui.px(16), ui.px(14), ui.px(16), ui.px(14))
        addView(
            View(act).apply { background = org.openbmt.app.IconDrawable(org.openbmt.app.Icon.Arrow, ui.pal.text, 2f) },
            LinearLayout.LayoutParams(ui.px(20), ui.px(20)).also {
                it.rightMargin = ui.px(12)
                it.topMargin = ui.px(2)
            },
        )
        addView(ui.text(model.next, 15f, medium = true), LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f))
    }

    // ---- first start ---------------------------------------------------------------------------------------------

    private fun showFirstStart(message: SayWords?) {
        val model = WordsModel.of(AppWords.notSetUp)
        val views = ArrayList<View>()
        views.add(left(ui.whoChip(model)))
        views.add(ui.titleBlock(model, "Counter"))
        views.add(nextCard(model))
        if (message != null) views.add(note(message))
        val counterName = ui.field("").also { it.setText(draftCounterName) }
        views.add(ui.labelled("Name of this counter", counterName))
        val name = ui.field("").also { it.setText(draftName) }
        views.add(ui.labelled("Manager's name", name))
        val pins = ui.pinPair("Manager's PIN, 6 to 12 digits", "PIN again")
        views.add(pins.view)
        val pair = ui.primary("Pair with the shop") {
            draftCounterName = counterName.text.toString().trim()
            draftName = name.text.toString().trim()
            startPairing(draftCounterName, draftName, pins.first.text.toString(), pins.second.text.toString())
        }
        show(ui.bar("Counter", logo = true), views, listOf(pair))
    }

    /**
     * What was typed is checked and kept first (the PIN and the config; the pairing is what completes the
     * setup), then the pairing begins with the camera for the offer of the owner phone or the Key app.
     */
    private fun startPairing(counterName: String, managerName: String, pin1: String, pin2: String) {
        FirstStartCheck.problem(counterName, managerName, pin1, pin2)?.let { return showFirstStart(it) }
        showWorking()
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
        titled(words.heading, words, emptyList(), listOf(ui.primary("Scan") { scanNow() }, ui.button("Cancel") { cancel() }), back = cancel)
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

    /**
     * The page the scan puts on the screen, made of what the scan built: the preview fills the body on black with
     * the frame over it, the words of the scan sit over the bottom, and its progress line is the pill. Its Cancel
     * button is the footer's and the bar's back arrow.
     */
    private fun showScanner(column: View) {
        val col = column as? LinearLayout ?: return put(column)
        var surface: View? = null
        var cancel: Button? = null
        for (i in 0 until col.childCount) {
            val c = col.getChildAt(i)
            if (c is android.view.SurfaceView) surface = c
            if (c is Button) cancel = c
        }
        val status = col.getChildAt(2) as? LinearLayout
        val title = (col.getChildAt(0) as? TextView)?.text?.toString().orEmpty()
        val hint = (col.getChildAt(1) as? TextView)?.text?.toString().orEmpty()
        val preview = surface?.let { s ->
            val lp = s.layoutParams
            col.removeView(s)
            CoverLayout(act, lp.width, lp.height).also { it.addView(s, FrameLayout.LayoutParams(ViewGroup.LayoutParams.WRAP_CONTENT, ViewGroup.LayoutParams.WRAP_CONTENT)) }
        }
        val frame = ui.cameraFrame(preview)
        val first = WordsModel(Colour.Blue, title, hint, "", WordsModel.whoText(Who.NOBODY), null, null, emptyList())
        frame.words(first)
        fun refresh() {
            val st = status ?: return
            val texts = ArrayList<String>()
            fun collect(v: View) {
                if (v is TextView) texts.add(v.text.toString()) else if (v is ViewGroup) for (i in 0 until v.childCount) collect(v.getChildAt(i))
            }
            var line: String? = null
            for (i in 0 until st.childCount) {
                val c = st.getChildAt(i)
                if (c is org.openbmt.app.WordsView) collect(c) else if (c is TextView) line = c.text.toString()
            }
            if (texts.size >= 4) {
                frame.words(WordsModel(Colour.Blue, texts[0], texts[1], texts[2], texts[3], null, null, emptyList()))
            } else {
                frame.words(first)
            }
            frame.progress(line)
        }
        status?.setOnHierarchyChangeListener(
            object : ViewGroup.OnHierarchyChangeListener {
                override fun onChildViewAdded(parent: View?, child: View?) {
                    main.post { refresh() }
                }

                override fun onChildViewRemoved(parent: View?, child: View?) {}
            },
        )
        val stop = { cancel?.performClick(); Unit }
        val holder = FillsFrame(act).also { it.addView(frame, FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT)) }
        val bar = ui.bar("Scan", back = stop, onBlack = true)
        put(ui.page(bar, listOf(holder), listOf(ui.secondary("Cancel") { stop() }), scroll = false))
    }

    private fun scanOffer() {
        progress.cancelled()
        progress.start()
        scanPage(AppWords.pairingScanOffer, {
            scan.begin(
                show = { v -> showScanner(v) },
                onBytes = { bytes -> offerRead(bytes) },
                onFailed = { f -> failedScan(f) },
                onCancel = { endCeremony(); showFirstStart(null) },
                // The offer may be longer than one code: ur:bytes, whole or in parts (UR-20).
                ur = UrScan(setOf(TYPE_BYTES), Who.MANAGER) { _, payload -> offerRead(payload) },
            )
        }, { endCeremony(); showFirstStart(null) })
    }

    private fun offerRead(offer: ByteArray) {
        showWorking()
        async({ Pairing.begin(offer, draftCounterName, nowSecs()) }) { r ->
            val started = r.getOrNull()
            // CR-237: the same scan reads either kind of offer; the library says which ceremony follows.
            if (started != null && started.wallet != OwnWalletKind.None && progress.offerReadOwn()) return@async checkAddress(started)
            if (started != null && started.wallet == OwnWalletKind.None && progress.offerRead()) return@async compare(started)
            endCeremony()
            when (val e = r.exceptionOrNull()) {
                is CounterException -> showMessage(e.sayWords(), "Back") { showFirstStart(null) }
                is LinkageError -> showMessage(AppWords.libraryMissing(e.message), "Back") { showFirstStart(null) }
                else -> showFirstStart(null)
            }
        }
    }

    /** CR-228, CR-237: the wallet's first receive address, the largest thing on the screen, in groups of four, and the kind of wallet beneath it. */
    private fun checkAddress(started: PairStarted) {
        val words = AppWords.pairingOwnAddress
        val model = WordsModel.of(words)
        val views = ArrayList<View>()
        views.add(ui.compactWords(model))
        views.add(ui.text(FirstAddress.groups(started.address), 28f, bold = true, mono = true))
        views.add(ui.text(FirstAddress.kindLine(started.wallet), 16f))
        views.add(ui.hint(model))
        show(ui.bar(words.heading, back = { endCeremony(); showFirstStart(null) }), views, listOf(ui.primary("Same address") { addressSame() }, ui.quiet("Different") { addressDifferent() }))
    }

    /** CR-229: only now does the library make the counter's key and number; the request and the four words come back. */
    private fun addressSame() {
        if (!progress.addressSame()) return
        showWorking()
        async({ Pairing.addressSame(nowSecs(), draftCounterName) }) { r ->
            val started = r.getOrNull()
            if (started != null) return@async compare(started, AppWords.pairingOwnCompare)
            endCeremony()
            when (val e = r.exceptionOrNull()) {
                is CounterException -> showMessage(e.sayWords(), "Back") { showFirstStart(null) }
                is LinkageError -> showMessage(AppWords.libraryMissing(e.message), "Back") { showFirstStart(null) }
                else -> showFirstStart(null)
            }
        }
    }

    /** CR-228: the address is not the wallet's: the ceremony ends and nothing is kept. */
    private fun addressDifferent() {
        if (!progress.addressDifferent()) return
        try {
            Pairing.addressDifferent()
        } catch (_: CounterException) {
            // The ceremony is forgotten whatever the library says; the words below are the ones to show.
        } catch (_: LinkageError) {
            // No library, no ceremony.
        }
        showMessage(AppWords.pairingOwnDifferent, "Back") { showFirstStart(null) }
    }

    /** The request as a code, and the four words, the most prominent thing on the screen (CR-98). */
    private fun compare(started: PairStarted, words: SayWords = AppWords.pairingCompare) {
        val model = WordsModel.of(words)
        val views = ArrayList<View>()
        views.add(ui.compactWords(model))
        views.add(ui.wordCards(started.words))
        val metrics = act.resources.displayMetrics
        val made = AnimatedQrView.make(act, Who.MANAGER, TYPE_BYTES, started.request, (metrics.heightPixels * 0.5).toInt())
        val view = made.view
        if (view == null) {
            views.add(note(made.words ?: UrWords.cannotShow(Who.MANAGER)))
        } else {
            views.add(note(if (view.animated) UrWords.showingAnimated(Who.MANAGER) else UrWords.showingStill(Who.MANAGER)))
            views.add(ui.codeCard(view))
        }
        views.add(ui.hint(model))
        show(ui.bar(words.heading, back = { endCeremony(); showFirstStart(null) }), views, listOf(ui.primary("They match") { theyMatch() }, ui.danger("They differ") { theyDiffer() }))
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
                show = { v -> showScanner(v) },
                onBytes = { bytes -> bundleRead(bytes) },
                onFailed = { f -> failedScan(f) },
                onCancel = { endCeremony(); showFirstStart(null) },
                ur = UrScan(setOf(TYPE_BYTES), Who.MANAGER) { _, payload -> bundleRead(payload) },
            )
        }, { endCeremony(); showFirstStart(null) })
    }

    /** The bundle is given to the library; what comes back is the record, sealed here with the Keystore (CR-97). */
    private fun bundleRead(bundle: ByteArray) {
        showWorking()
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
                    showMessage(AppWords.pairingSaved, "Start", snap = true) { begin() }
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
        val lines = Slot(act)
        val noteSlot = Slot(act)
        val requestsSlot = Slot(act)
        val tillSlot = Slot(act)
        val starterSlot = Slot(act)
        val linked = hasLink(n)
        // CR-147: who is at the till, and a way to lock it.
        val who = locks.who?.takeIf { cashiers.isNotEmpty() }
        val bar = ui.bar("Counter", sub = who?.let { "Cashier: ${it.name}" }, logo = true, gear = { showSettings() })
        if (who != null) {
            val row = (bar as org.openbmt.app.BarView).row
            row?.addView(
                ui.barButton("Lock") {
                    locks.lock()
                    render()
                },
                row.childCount - 1,
                LinearLayout.LayoutParams(ViewGroup.LayoutParams.WRAP_CONTENT, ViewGroup.LayoutParams.WRAP_CONTENT).also { it.rightMargin = ui.px(4) },
            )
        }
        val tiles = LinearLayout(act).apply { orientation = LinearLayout.VERTICAL }
        tiles.addView(ui.tile("New sale", Glyph.Plus, true) { showNewSale() }, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ui.px(104)))
        val pair = LinearLayout(act).apply { orientation = LinearLayout.HORIZONTAL }
        pair.addView(ui.tile("Sales", Glyph.List, false) { showSales() }, LinearLayout.LayoutParams(0, ui.px(104), 1f).also { it.rightMargin = ui.px(6) })
        pair.addView(ui.tile("Today", Glyph.Calendar, false) { showToday(false) }, LinearLayout.LayoutParams(0, ui.px(104), 1f).also { it.leftMargin = ui.px(6) })
        tiles.addView(pair, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT).also { it.topMargin = ui.px(12) })
        // CR-246: bills and tabs, one tile under the others.
        tiles.addView(ui.tile(WordsOfBills.BILLS, Glyph.List, false) { showBills() }, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ui.px(104)).also { it.topMargin = ui.px(12) })
        show(bar, listOf(health, tillSlot, starterSlot, lines, noteSlot, tiles, requestsSlot), emptyList(), Showing.Home)
        var healthSig: String? = null
        var linesSig: String? = null
        var requestsSig: String? = null
        var tillSig: String? = null
        var starterSig: String? = null
        tick = {
            val h = healthWords(n)
            val hs = sig(h)
            if (hs != healthSig) {
                healthSig = hs
                health.show(ui.healthCard(h))
            }
            val items = ArrayList<HomeLine>()
            // CR-116: the Lightning line is there only while a connection is saved.
            lightningWords(n)?.let { items.add(HomeLine("lightning", it)) }
            // SL-41, SL-76, SL-77: on the Lightning counter only, the reason Lightning is not offered and the call for the key.
            shopLightning(n)?.let { sl ->
                sl.why?.let { items.add(HomeLine("shop-lightning-why", it.say())) }
                if (sl.needsSeed) items.add(HomeLine("shop-lightning-key", sl.scan.say()))
                if (sl.deleteSeed) deleteShopSeed()
            }
            if (cfg.ownNode.isNotEmpty()) {
                // CR-107: the own node's line, and a way to ask the driver to dial it again.
                items.add(
                    HomeLine("ownnode", ownNodeWords(n), retry = {
                        try {
                            n.ownNodeRetry()
                        } catch (e: CounterException) {
                            noteSlot.show(note(e.sayWords()))
                        }
                    }),
                )
            }
            if (linked) {
                // CR-136: the link to the owner, and a way to call the Box now.
                items.add(
                    HomeLine("box", linkWords(n), retry = {
                        async({ n.linkStep(nowSecs(), true) }) { r ->
                            r.exceptionOrNull()?.let { if (it is CounterException) noteSlot.show(note(it.sayWords())) }
                            if (r.isSuccess) tick?.invoke()
                        }
                    }),
                )
                // CR-164, CR-165: what the owner's settings mean for this counter, when anything.
                settingsLine(n)?.let { items.add(HomeLine("settings", it)) }
                // CR-244: a change that waits for a manager, and the words of one that came to nothing.
                val change = changeState(n)
                if (change?.pending?.confirmed == false) items.add(HomeLine("change", AppWords.changeWaits))
                change?.notice?.let { items.add(HomeLine("change-notice", it.say())) }
                // CR-260, CR-261, CR-264: a switch from the starter wallet that waits for a manager, the words of one that was
                // dropped, and the new pairing record a half-done switch still needs kept.
                val moved = switchState(n)
                keepSwitchRecord(n, moved?.recordUpdate)
                if (moved?.pending != null) items.add(HomeLine("switch", SwitchWords.homeLine))
                moved?.notice?.let { items.add(HomeLine("switch-notice", it.say())) }
            }
            // CR-273: a stop, or a line that cannot be checked, with the library's words.
            val starter = starterNow(n)
            StarterModel.homeLine(starter)?.let { items.add(HomeLine("starter", it)) }
            priceWords()?.let { items.add(HomeLine("price", it, priceNotes())) }
            val ls = items.joinToString("\u0002") { sig(it.words) + (it.extra ?: "") }
            if (ls != linesSig) {
                linesSig = ls
                lines.show(ui.homeLines(items, openLines))
            }
            // CR-267: the requests from the till, oldest first, each on its own card above the other lines.
            val tillRequests = TillModel.shown(tillWaiting(n))
            val ts = TillModel.signature(tillRequests)
            if (ts != tillSig) {
                tillSig = ts
                val m = money
                if (tillRequests.isEmpty() || m == null) {
                    tillSlot.show()
                } else {
                    val cards = tillRequests.map { r ->
                        ui.tillCard(TillModel.lines(m, r), start = { startTillSale(n, r) { w -> noteSlot.show(note(w)) } }, notNow = {
                            try {
                                n.tillNotNow(nowSecs(), r.sale)
                            } catch (e: CounterException) {
                                noteSlot.show(note(e.sayWords()))
                            }
                            tillSig = null
                            tick?.invoke()
                        }).also { it.layoutParams = ui.margins(bottom = 12) }
                    }
                    tillSlot.show(*cards.toTypedArray())
                }
            }
            // CR-274: each payment from the starter wallet nobody has seen, on its own card until someone taps "seen".
            val unseen = StarterModel.unseen(starter)
            val ss = StarterModel.signature(unseen)
            if (ss != starterSig) {
                starterSig = ss
                if (unseen.isEmpty()) {
                    starterSlot.show()
                } else {
                    val heading = starter?.cards?.heading.orEmpty()
                    val cards = unseen.map { p ->
                        ui.starterCard(heading, StarterModel.lines(p), seen = {
                            try {
                                n.starterSeen(nowSecs(), p.txid)
                            } catch (e: CounterException) {
                                noteSlot.show(note(e.sayWords()))
                            }
                            starterSig = null
                            tick?.invoke()
                        }).also { it.layoutParams = ui.margins(bottom = 12) }
                    }
                    starterSlot.show(*cards.toTypedArray())
                }
            }
            // CR-215: the open requests, newest first, under their own heading.
            val open = try {
                n.requests(nowSecs())
            } catch (e: CounterException) {
                emptyList()
            }
            val rs = open.joinToString("\u0002") { "${it.index}\u0001${it.note}\u0001${it.amountMinor}" }
            if (rs != requestsSig) {
                requestsSig = rs
                if (open.isEmpty()) {
                    requestsSlot.show()
                } else {
                    val m = money
                    val rows = open.map { r ->
                        amounts[r.index] = r.amountMinor
                        val shown = r.note ?: m?.show(r.amountMinor) ?: Format.sats(r.amountMinor)
                        RowItem(shown, Format.localTime(r.expiresAt), null, onClick = { showSale(r.index, null, null) })
                    }
                    requestsSlot.show(label(WordsOfRequests.OPEN_REQUESTS).also { it.setPadding(0, ui.px(20), 0, ui.px(8)) }, ui.rows(rows))
                }
            }
        }
        homeTick = tick
        tick?.invoke()
    }

    /** CR-267: the requests from the till that wait; none when the library has none to list. */
    private fun tillWaiting(n: CounterNode): List<TillRequest> =
        try {
            n.tillWaiting(nowSecs())
        } catch (e: CounterException) {
            emptyList()
        } catch (e: LinkageError) {
            emptyList()
        }

    /**
     * CR-268: Start. The counter opens the sale for the request's amount at its own price, and the sale's screen is
     * shown as for a sale typed here. A refusal is shown on Home and the request stays.
     */
    private fun startTillSale(n: CounterNode, request: TillRequest, refused: (SayWords) -> Unit) {
        starterStop()?.let { return refused(it) }
        val (rate, words) = quote()
        if (rate == null) return refused(words ?: AppWords.stopped)
        try {
            val o = n.tillStart(nowSecs(), request.sale, rate.rate, rate.sourceLabel, rate.sourceFetchedAt, atTheTill())
            amounts[o.index] = request.amountMinor
            showSale(o.index, o.payment, null)
        } catch (e: CounterException) {
            refused(e.sayWords())
        }
    }

    /** What a words shows, as one text: a screen redraws only when it changes. */
    private fun sig(w: SayWords): String = "${w.tone}\u0001${w.who}\u0001${w.heading}\u0001${w.what}\u0001${w.next}\u0001${w.detail}"

    private fun healthWords(n: CounterNode): SayWords {
        pumpFault?.let { return it }
        return try {
            val s = n.status()
            if (s.stopped) AppWords.stopped else s.health?.words?.say() ?: AppWords.starting
        } catch (e: CounterException) {
            e.sayWords()
        }
    }

    /** SL-41, SL-76, SL-77: what the library says of Shop Lightning, or null when it says nothing or cannot be read. */
    private fun shopLightning(n: CounterNode): ShopLightningStatus? =
        try {
            n.shopLightning().takeIf { it.needsSeed || it.deleteSeed || it.why != null }
        } catch (e: CounterException) {
            null
        }

    /** SL-77: the library has no channel and no output left to sweep, so the sealed seed is deleted, once. */
    private fun deleteShopSeed() {
        if (shopSeedDeleting) return
        shopSeedDeleting = true
        val file = File(dir, ShopLightningWords.FILE)
        if (!file.exists() || file.delete()) shopSeedRecord = null else shopSeedDeleting = false
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

    /** CR-164, CR-165: the line about the owner's settings, or null when there is none. */
    private fun settingsLine(n: CounterNode): SayWords? =
        try {
            n.linkState()?.settingsLine?.say()
        } catch (e: CounterException) {
            e.sayWords()
        }

    /** CR-240, CR-244: the change that waits for a manager and the words of a change that came to nothing; null when the library has none to say. */
    private fun changeState(n: CounterNode): ChangeState? =
        try {
            n.pendingChange(nowSecs())
        } catch (e: CounterException) {
            null
        } catch (e: LinkageError) {
            null
        }

    /** CR-260, CR-264: the switch that waits for a manager and the words of one that came to nothing; null when the library has none to say. */
    private fun switchState(n: CounterNode): SwitchState? =
        try {
            n.pendingSwitch(nowSecs())
        } catch (e: CounterException) {
            null
        } catch (e: LinkageError) {
            null
        }

    /** CR-261: the pairing record the library hands over for a switch begun and not finished is sealed and kept here, then the library is told. */
    private fun keepSwitchRecord(n: CounterNode, update: ByteArray?) {
        if (update == null || switchKeeping) return
        switchKeeping = true
        async({ ConfirmSwitch.finish(update, { keptRecord(it) }, { n.switchRecordKept(nowSecs()) }) }) { switchKeeping = false }
    }

    /** Seal and save the pairing record; what the app holds in memory is the new one from then on. */
    private fun keptRecord(bytes: ByteArray) {
        SealedFile(dir, KeystoreSealer(KEY_ALIAS), PAIRING_FILE).save(bytes)
        record = bytes
    }

    /** CR-107: the own node's health line, or the library's words for why there is none. */
    private fun ownNodeWords(n: CounterNode): SayWords =
        try {
            n.status().ownNode?.words?.say() ?: AppWords.starting
        } catch (e: CounterException) {
            e.sayWords()
        }

    // ---- settings -------------------------------------------------------------------------------------------------------

    /** Staff, the own node, the network, Lightning, About, and the sounds; with no cashier listed, the cashier's name first. No PIN is asked here: the screens behind it ask as they do. */
    private fun showSettings() {
        if (lockedOut()) return
        val items = ArrayList<RowItem>()
        // CR-146: with no cashier listed the cashier is the one name of this phone.
        if (cashiers.isEmpty()) items.add(RowItem("Your name", onClick = { showYourName() }))
        // CR-240: the change from the owner phone that waits, behind the manager's PIN on its own page.
        node?.let { n -> changeState(n)?.pending?.let { p -> items.add(RowItem(AppWords.CHANGE_BAR, ConfirmChange.menuSub(p), onClick = { showConfirmChange() })) } }
        // CR-260: the new wallet to check, while a switch from the starter wallet waits.
        node?.let { n -> switchState(n)?.pending?.let { items.add(RowItem(SwitchWords.BAR, onClick = { showConfirmSwitch() })) } }
        items.add(RowItem("Staff", onClick = { showStaffEntry() }))
        items.add(RowItem("Your own node", onClick = { showOwnNode() }))
        items.add(RowItem("Network", onClick = { showNetwork() }))
        // CR-203: the choice of Tor is there only while the shop's settings have Tor on.
        if (torOnNow) items.add(RowItem("Tor", onClick = { showTor() }))
        items.add(RowItem("Lightning", onClick = { showLightning(null) }))
        // SL-76: the Lightning key, only on the Lightning counter while no seed is kept.
        node?.let { n -> if (shopLightning(n)?.needsSeed == true) items.add(RowItem(ShopLightningWords.BAR, onClick = { showShopLightningKey() })) }
        items.add(RowItem("About", onClick = { showAbout() }))
        items.add(RowItem("Sounds", SOUNDS_SENTENCE, onClick = { showSounds() }))
        show(ui.bar("Settings", back = { showHome() }), listOf(ui.rows(items)))
    }

    // ---- a change to confirm (CR-239 to CR-244) ---------------------------------------------------

    /**
     * The change the owner phone sent that loosens the shop's settings: each field with the value now and after, the four
     * words to compare with the owner phone, and the manager's PIN. The PIN is checked before the counter signs anything.
     */
    private fun showConfirmChange() {
        if (lockedOut()) return
        val n = node ?: return render()
        val pending = changeState(n)?.pending ?: return showSettings()
        val msg = Slot(act)
        val pinInput = pinField(act, "")
        val views = ArrayList<View>()
        views.add(ui.text(AppWords.CHANGE_HEADING, 20f, bold = true))
        views.add(ui.text(AppWords.CHANGE_TEXT, 15f, colour = ui.pal.secondary))
        views.add(ui.rows(ConfirmChange.rows(pending).map { RowItem(it.field, it.change) }))
        views.add(label(AppWords.CHANGE_WORDS_LABEL))
        views.add(ui.wordCards(pending.words))
        views.add(ui.labelled("Manager's PIN", pinInput))
        views.add(msg)
        val confirm = ui.primary(AppWords.CHANGE_CONFIRM) {
            async({ ConfirmChange.confirm(pinInput.text.toString(), pin) { n.confirmChange(nowSecs(), pending.version) } }) { r ->
                // CR-200: an answer that comes back after the app was hidden shows nothing.
                if (!PinSteps.applies(visible)) return@async
                val e = r.exceptionOrNull()
                if (e != null) return@async msg.show(note(if (e is IOException) AppWords.fileNotSaved(e.message) else AppWords.fault(CounterException("pin", e.javaClass.simpleName, 4))))
                when (val o = r.getOrNull()) {
                    is ConfirmChange.Outcome.Confirmed -> showMessage(o.words, "Back") { showSettings() }
                    is ConfirmChange.Outcome.PinRefused -> { Feedback.shake(pinInput); msg.show(note(o.words)) }
                    is ConfirmChange.Outcome.Refused -> msg.show(note(o.words))
                    null -> Unit
                }
            }
        }
        show(ui.bar(AppWords.CHANGE_BAR, back = { showSettings() }), views, listOf(confirm, ui.button(AppWords.CHANGE_NOT_NOW) { showSettings() }))
    }

    // ---- the new wallet to check (CR-259 to CR-262) -------------------------------------------------

    /**
     * The switch from the starter wallet: the new wallet's first address as the largest thing on the page, in groups of four,
     * the kind of wallet beneath it, and the line that says to compare it with the new wallet's own screen. "Same address" asks
     * for the manager's PIN and then keeps the new record; "Different" drops the switch.
     */
    private fun showConfirmSwitch() {
        if (lockedOut()) return
        val n = node ?: return render()
        val pending = switchState(n)?.pending ?: return showSettings()
        val msg = Slot(act)
        val pinInput = pinField(act, "")
        val views = ArrayList<View>()
        views.add(ui.compactWords(WordsModel.of(SwitchWords.said(SwitchWords.COMPARE))))
        views.add(ui.text(ConfirmSwitch.address(pending.address), 28f, bold = true, mono = true))
        views.add(ui.text(FirstAddress.kindLine(pending.wallet), 16f))
        views.add(ui.labelled(SwitchWords.PIN_LABEL, pinInput))
        views.add(msg)
        val same = ui.primary(SwitchWords.SAME) {
            async({
                ConfirmSwitch.same(pinInput.text.toString(), pin, { n.switchSame(nowSecs()) }, { keptRecord(it) }, { n.switchRecordKept(nowSecs()) })
            }) { r ->
                // CR-200: an answer that comes back after the app was hidden shows nothing.
                if (!PinSteps.applies(visible)) return@async
                val e = r.exceptionOrNull()
                if (e != null) return@async msg.show(note(AppWords.fault(CounterException("pin", e.javaClass.simpleName, 4))))
                when (val o = r.getOrNull()) {
                    is ConfirmSwitch.Outcome.Switched -> showMessage(o.words, "Back") { showSettings() }
                    is ConfirmSwitch.Outcome.PinRefused -> { Feedback.shake(pinInput); msg.show(note(o.words)) }
                    is ConfirmSwitch.Outcome.Refused -> msg.show(note(o.words))
                    is ConfirmSwitch.Outcome.NotKept -> msg.show(note(o.words))
                    is ConfirmSwitch.Outcome.Dropped, null -> Unit
                }
            }
        }
        val different = ui.quiet(SwitchWords.DIFFERENT) {
            async({ ConfirmSwitch.different { n.switchDifferent(nowSecs()) } }) { r ->
                if (!PinSteps.applies(visible)) return@async
                val e = r.exceptionOrNull()
                if (e != null) return@async msg.show(note(AppWords.fault(CounterException("switch", e.javaClass.simpleName, 4))))
                when (val o = r.getOrNull()) {
                    is ConfirmSwitch.Outcome.Dropped -> showMessage(o.words, "Back") { showSettings() }
                    is ConfirmSwitch.Outcome.Refused -> msg.show(note(o.words))
                    else -> Unit
                }
            }
        }
        show(ui.bar(SwitchWords.BAR, back = { showSettings() }), views, listOf(same, different))
    }

    /** CR-146: the cashier's one name, kept in the config file. */
    private fun showYourName() {
        val cfg = config ?: return render()
        val saved = Slot(act)
        val name = ui.field("").also { it.setText(cfg.cashier) }
        titled(
            "Your name",
            null,
            listOf(ui.labelled("Your name", name), saved),
            listOf(
                ui.primary("Save my name") {
                    val v = name.text.toString().trim()
                    if (!AppConfig.validName(v)) {
                        saved.show(note(AppWords.nameNotAccepted))
                    } else {
                        try {
                            val updated = cfg.withCashier(v)
                            configFile.save(updated)
                            config = updated
                            showSettings()
                        } catch (e: IOException) {
                            saved.show(note(AppWords.fileNotSaved(e.message)))
                        }
                    }
                },
                ui.button("Back") { showSettings() },
            ),
            back = { showSettings() },
        )
    }

    /** The sounds on or off, kept in the config file next to the cashier's name. The phone's own ringer setting still wins. */
    private fun showSounds() {
        val cfg = config ?: return render()
        val msg = Slot(act)
        val chips = ui.chips(listOf("On", "Off"), if (cfg.sounds) 0 else 1)
        chips.setOnCheckedChangeListener { _, _ ->
            val on = ui.chosen(chips) != 1
            val current = config ?: return@setOnCheckedChangeListener
            try {
                val updated = current.withSounds(on)
                configFile.save(updated)
                config = updated
                Feedback.soundOn = on
                msg.show()
            } catch (e: IOException) {
                msg.show(note(AppWords.fileNotSaved(e.message)))
            }
        }
        show(ui.bar("Sounds", back = { showSettings() }), listOf(chips, ui.text(SOUNDS_SENTENCE, 15f, colour = ui.pal.secondary), msg))
    }

    // ---- the own node's address (CR-100 to CR-102) ----------------------------------------------------

    /** The counter's own setting, behind the manager's PIN: save an address, or remove it. */
    private fun showOwnNode() {
        if (lockedOut()) return
        val cfg = config ?: return render()
        val rec = record ?: return render()
        val msg = Slot(act)
        val address = ui.field("", mono = true).also { it.setText(cfg.ownNode) }
        val pinInput = pinField(act, "")
        titled(
            "Your own node",
            AppWords.ownNodeAbout,
            listOf(ui.labelled("Tor address of your node", address), ui.labelled("Manager's PIN", pinInput), msg),
            listOf(
                ui.primary("Save") {
                    val text = address.text.toString().trim()
                    checkPin(pinInput.text.toString(), pinInput, { saveOwnNode(cfg, rec, text, msg) }, { msg.show(note(it)) })
                },
                ui.danger("Remove") {
                    checkPin(pinInput.text.toString(), pinInput, { keepOwnNode(cfg, "", AppWords.ownNodeRemoved, msg) }, { msg.show(note(it)) })
                },
                ui.button("Back") { showSettings() },
            ),
            back = { showSettings() },
        )
    }

    /** The library reads the address (a home-network address, no Tor and so on are refused with words) and the app keeps what it made. */
    private fun saveOwnNode(cfg: AppConfig, rec: ByteArray, text: String, msg: Slot) {
        async({
            val pairing = Shop.pairing(rec, nodeDir.path)
            OwnNodeSetting.check(text, pairing.network, pairing.settings.torOn)
        }) { r ->
            when (val e = r.exceptionOrNull()) {
                null -> keepOwnNode(cfg, r.getOrThrow(), AppWords.ownNodeSaved, msg)
                is CounterException -> msg.show(note(e.sayWords()))
                is LinkageError -> msg.show(note(AppWords.libraryMissing(e.message)))
                else -> throw e
            }
        }
    }

    private fun keepOwnNode(cfg: AppConfig, address: String, done: SayWords, msg: Slot) {
        try {
            configFile.save(cfg.withOwnNode(address))
        } catch (e: IOException) {
            return msg.show(note(AppWords.fileNotSaved(e.message)))
        }
        showMessage(done, "Start") { begin() }
    }

    // ---- the network: the address of Orbot (CR-87) -----------------------------------------------

    /**
     * The address Tor is reached at, behind the manager's PIN. Open from Settings, and from the screen that says Orbot did not
     * answer. With [choosing] it is the second step of choosing Use Orbot (CR-203): saving the address is the choice.
     */
    private fun showNetwork(choosing: Boolean = false) {
        if (node != null && lockedOut()) return
        val cfg = config ?: return render()
        val msg = Slot(act)
        val address = ui.field("host:port", mono = true).also { it.setText(cfg.proxy) }
        val pinInput = pinField(act, "")
        val back = { if (choosing) showTor() else if (node != null) showSettings() else render() }
        titled(
            "Network",
            AppWords.networkAbout,
            listOf(ui.labelled("Address of Orbot", address), ui.labelled("Manager's PIN", pinInput), msg),
            listOf(
                ui.primary(if (choosing) TorChoice.Orbot.label else "Save") {
                    val text = address.text.toString().trim()
                    val problem = NetworkCheck.problem(text)
                    if (problem != null) {
                        msg.show(note(problem))
                    } else {
                        checkPin(pinInput.text.toString(), pinInput, { keepNetwork(cfg, text, choosing, msg) }, { msg.show(note(it)) })
                    }
                },
                ui.button("Back") { back() },
            ),
            back = { back() },
        )
    }

    private fun keepNetwork(cfg: AppConfig, address: String, choosing: Boolean, msg: Slot) {
        try {
            val next = cfg.withProxy(address)
            configFile.save(if (choosing) next.with(TorChoice.Orbot) else next)
        } catch (e: IOException) {
            return msg.show(note(AppWords.fileNotSaved(e.message)))
        }
        showMessage(AppWords.networkSaved, "Start") { begin() }
    }

    // ---- Tor: the built-in one or Orbot (CR-203) ----------------------------------------------------

    /**
     * The page of the two choices, behind the manager's PIN: the words of the choice in use, and a button for each. Built-in Tor
     * is chosen here; Use Orbot goes on to the address of Orbot. Open from Settings, and from the screen that says Tor did not start.
     */
    private fun showTor() {
        if (node != null && lockedOut()) return
        val cfg = config ?: return render()
        val msg = Slot(act)
        val pinInput = pinField(act, "")
        val back = { if (node != null) showSettings() else render() }
        val actions = ArrayList<Button>()
        for (choice in TorChoice.values()) {
            val go = {
                if (choice == TorChoice.Orbot) {
                    showNetwork(choosing = true)
                } else {
                    checkPin(pinInput.text.toString(), pinInput, { chooseTor(cfg, choice, msg, back) }, { msg.show(note(it)) })
                }
            }
            actions.add(if (choice == cfg.tor) ui.primary(choice.label) { go() } else ui.button(choice.label) { go() })
        }
        actions.add(ui.button("Back") { back() })
        titled("Tor", cfg.tor.words, listOf(ui.labelled("Manager's PIN", pinInput), msg), actions, back = { back() })
    }

    /** The choice made: saved first, then the app starts again with it (CR-203). Tapping the choice in use goes back. */
    private fun chooseTor(cfg: AppConfig, choice: TorChoice, msg: Slot, same: () -> Unit) {
        if (choice == cfg.tor) return same()
        try {
            configFile.save(cfg.with(choice))
        } catch (e: IOException) {
            return msg.show(note(AppWords.fileNotSaved(e.message)))
        }
        begin()
    }

    // ---- Lightning through the shop's own node (CR-112 to CR-116) -------------------------------------

    /** The connection to the shop's node, behind the manager's PIN: scan or type it, test it, save it sealed, or remove it. */
    private fun showLightning(notice: SayWords?) {
        if (lockedOut()) return
        if (node == null) return render()
        val msg = Slot(act)
        (notice ?: lightningFault)?.let { msg.show(note(it)) }
        val code = ui.field("lndconnect:// code", lines = 3, mono = true).also { it.setText(draftLnCode) }
        val address = ui.field("host:port", mono = true).also { it.setText(draftLnAddress) }
        val rune = ui.field("Rune", mono = true).also { it.setText(draftLnRune) }
        val cert = ui.field("Certificate, or its SHA-256 fingerprint", lines = 4, mono = true).also { it.setText(draftLnCert) }
        val pinInput = pinField(act, "")
        fun keep() {
            draftLnCode = code.text.toString()
            draftLnAddress = address.text.toString()
            draftLnRune = rune.text.toString()
            draftLnCert = cert.text.toString()
        }
        val about = WordsModel.of(AppWords.lightningAbout)
        val views = ArrayList<View>()
        views.add(note(AppWords.lightningCare))
        views.add(ui.text(if (lightningRecord != null) "A connection is saved on this phone." else "No connection is saved.", 16f))
        views.add(label("LND"))
        views.add(code)
        views.add(
            ui.button("Scan the code") {
                keep()
                scan.begin(
                    show = { v -> showScanner(v) },
                    onBytes = { bytes -> draftLnCode = String(bytes, Charsets.UTF_8); showLightning(null) },
                    onFailed = { f -> showLightning(f.words ?: if (f.off) AppWords.cameraOff else AppWords.cameraNotStarted(f.problem)) },
                    onCancel = { showLightning(null) },
                )
            },
        )
        views.add(label("Core Lightning"))
        views.add(address)
        views.add(rune)
        views.add(cert)
        views.add(
            ui.button("Test and save the Core Lightning connection") {
                keep()
                testLightning(LightningSetup.cln(draftLnAddress, draftLnRune, draftLnCert), pinInput.text.toString(), pinInput, msg)
            },
        )
        views.add(ui.labelled("Manager's PIN", pinInput))
        views.add(msg)
        views.add(
            ui.danger("Remove the connection") {
                checkPin(pinInput.text.toString(), pinInput, { removeLightning(msg) }, { msg.show(note(it)) })
            },
        )
        val full = ArrayList<View>()
        full.add(ui.titleBlock(about, "Lightning"))
        full.addAll(views)
        full.add(ui.hint(about))
        show(
            ui.bar("Lightning", back = { forgetLightningDraft(); showSettings() }),
            full,
            listOf(
                ui.primary("Test and save the LND connection") {
                    keep()
                    testLightning(LightningSetup.lnd(draftLnCode), pinInput.text.toString(), pinInput, msg)
                },
                ui.button("Back") { forgetLightningDraft(); showSettings() },
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
    private fun testLightning(input: LnInput?, pinText: String, pinInput: View, msg: Slot) {
        if (input == null) return msg.show(note(AppWords.lightningFieldsNeeded))
        checkPin(pinText, pinInput, {
            val n = node
            val t = transport
            if (n == null || t == null) return@checkPin render()
            val id = runId
            msg.show(note(AppWords.lightningTesting))
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
        }, { msg.show(note(it)) })
    }

    private fun testDone(result: Result<Boolean>, msg: Slot) {
        when (val e = result.exceptionOrNull()) {
            null ->
                if (result.getOrThrow()) {
                    forgetLightningDraft()
                    showMessage(AppWords.lightningSaved, "Start") { begin() }
                } else {
                    msg.show(note(AppWords.lightningNotReached))
                }
            is CounterException -> msg.show(note(e.sayWords()))
            is LinkageError -> msg.show(note(AppWords.libraryMissing(e.message)))
            is IOException -> msg.show(note(AppWords.fileNotSaved(e.message)))
            is GeneralSecurityException -> msg.show(note(AppWords.storageUnavailable(e.message)))
            else -> throw e
        }
    }

    /** CR-115: the saved connection is deleted at once, and the library stops using it. */
    private fun removeLightning(msg: Slot) {
        val n = node ?: return render()
        val file = File(dir, LightningSetup.FILE)
        if (file.exists() && !file.delete()) return msg.show(note(AppWords.fileNotSaved("the file could not be deleted")))
        lightningRecord = null
        lightningFault = null
        forgetLightningDraft()
        try {
            n.lnRemove()
        } catch (e: CounterException) {
            return msg.show(note(e.sayWords()))
        }
        showMessage(AppWords.lightningRemoved, "Back") { showSettings() }
    }

    // ---- the Lightning key of Shop Lightning (SL-72, SL-76) -----------------------------------------

    /** The page that reads the stick's code: the camera, then the library checks the seed against the node ID the settings pin. */
    private fun showShopLightningKey() {
        if (lockedOut()) return
        val n = node ?: return render()
        val words = try {
            n.shopLightning().scan.say()
        } catch (e: CounterException) {
            return showMessage(e.sayWords(), "Back") { showSettings() }
        }
        show(
            ui.bar(ShopLightningWords.BAR, back = { showSettings() }),
            listOf(ui.titleBlock(WordsModel.of(words), ShopLightningWords.BAR)),
            listOf(
                ui.primary(ShopLightningWords.SCAN) {
                    scan.begin(
                        show = { v -> showScanner(v) },
                        onBytes = { bytes -> takeShopSeed(n, String(bytes, Charsets.UTF_8)) },
                        onFailed = { f -> showMessage(f.words ?: if (f.off) AppWords.cameraOff else AppWords.cameraNotStarted(f.problem), "Back") { showShopLightningKey() } },
                        onCancel = { showShopLightningKey() },
                    )
                },
                ui.button(ShopLightningWords.BACK) { showSettings() },
            ),
        )
    }

    /** SL-72: the library refuses a seed of another node and stores nothing; one that passes is sealed here and the node starts again with it. */
    private fun takeShopSeed(n: CounterNode, text: String) {
        val kept = try {
            n.shopLightningSeed(text)
        } catch (e: CounterException) {
            return showMessage(e.sayWords(), ShopLightningWords.BACK) { showShopLightningKey() }
        }
        try {
            SealedFile(dir, KeystoreSealer(ShopLightningWords.KEY_ALIAS), ShopLightningWords.FILE).save(kept)
        } catch (e: IOException) {
            return showMessage(AppWords.fileNotSaved(e.message), ShopLightningWords.BACK) { showShopLightningKey() }
        } catch (e: GeneralSecurityException) {
            return showMessage(AppWords.fileNotSaved(e.message), ShopLightningWords.BACK) { showShopLightningKey() }
        }
        shopSeedRecord = kept
        showMessage(ShopLightningWords.kept, ShopLightningWords.KEPT_BUTTON) { begin() }
    }

    /** The price as home and the keypad give it: the words, or null while there is no book or currency. */
    private fun priceWords(): SayWords? {
        val b = book ?: return null
        val m = money ?: return null
        return when (val s = b.status()) {
            is PriceStatus.Ready -> AppWords.priceReady(m.show(s.rate.rate), s.sources)
            is PriceStatus.Refused -> s.words
            PriceStatus.TorMissing -> b.torMissingWords
        }
    }

    /** One line for each price source: what it gave and when, or why it did not. Empty when there are none. */
    private fun priceNotes(): String {
        val b = book ?: return ""
        val m = money ?: return ""
        return b.notes().joinToString("\n") { n ->
            "${n.source}: " + (n.price?.let { "${m.show(it)} at ${Format.localTime(n.atSecs)}" } ?: n.words?.heading.orEmpty())
        }
    }

    // ---- a new sale ------------------------------------------------------------------------------------------------------

    /** CR-272, CR-273: what the starter line says, or null when the library cannot be asked. */
    private fun starterNow(n: CounterNode): StarterState? =
        try {
            n.starterState(nowSecs())
        } catch (e: CounterException) {
            null
        } catch (e: LinkageError) {
            null
        }

    /** CR-273: the words that stand in the place of a sale when the line says to offer no bitcoin; null when the sale may open. */
    private fun starterStop(): SayWords? = node?.let { StarterModel.stopWords(starterNow(it)) }

    private fun showNewSale() {
        if (lockedOut()) return
        val m = money ?: return render()
        starterStop()?.let { return showMessage(it, "Back") { showHome() } }
        val entry = AmountEntry(m)
        val display = ui.text(entry.show(), 60f, bold = true).also {
            it.gravity = android.view.Gravity.CENTER
            it.fontFeatureSettings = "tnum"
        }
        val msg = Slot(act)
        val centre = Centered(act)
        centre.addView(ui.text("Amount of the sale", 14f, colour = ui.pal.secondary, medium = true).also { it.gravity = android.view.Gravity.CENTER }, ui.margins())
        centre.addView(display, ui.margins(top = 6))
        priceWords()?.let { centre.addView(ui.text(it.what, 14f, colour = ui.pal.secondary).also { t -> t.gravity = android.view.Gravity.CENTER }, ui.margins(top = 6)) }
        centre.addView(msg, ui.margins(top = 12))
        val pad = ui.amountPad { face ->
            when (face) {
                "C" -> entry.clear()
                "⌫" -> entry.backspace()
                else -> entry.press(face[0])
            }
            display.text = entry.show()
            msg.show()
        }
        val open = ui.primary("Open the sale") {
            if (entry.minor <= 0) msg.show(note(AppWords.amountNeeded)) else chooseTip(entry.minor) { msg.show(note(it)) }
        }
        val request = ui.secondary(WordsOfRequests.REQUEST) {
            if (entry.minor <= 0) msg.show(note(AppWords.amountNeeded)) else showRequestForm(entry.minor)
        }
        show(ui.bar("New sale", back = { showHome() }), listOf(centre, pad), listOf(open, request))
    }

    /** CR-213: the optional note and the lifetime of a request to send, for the amount typed on New sale. */
    private fun showRequestForm(amountMinor: Long) {
        if (lockedOut()) return
        val m = money ?: return render()
        val views = ArrayList<View>()
        val msg = Slot(act)
        views.add(msg)
        views.add(label(WordsOfRequests.NOTE))
        val noteField = ui.field(WordsOfRequests.NOTE_HINT)
        views.add(noteField)
        val lifetime = ui.chips(WordsOfRequests.LIFETIMES.map { it.first })
        views.add(lifetime)
        val make = ui.primary(WordsOfRequests.MAKE) {
            val text = noteField.text.toString().trim()
            makeRequest(amountMinor, text.takeIf { it.isNotEmpty() }, WordsOfRequests.LIFETIMES[ui.chosen(lifetime)].second) { msg.show(note(it)) }
        }
        show(ui.bar(WordsOfRequests.REQUEST, sub = m.show(amountMinor), back = { showNewSale() }), views, listOf(make))
    }

    private fun makeRequest(amountMinor: Long, noteText: String?, days: Int, refused: (SayWords) -> Unit) {
        val n = node ?: return render()
        starterStop()?.let { return refused(it) }
        val (rate, words) = quote()
        if (rate == null) return refused(words ?: AppWords.stopped)
        val m = money ?: return render()
        try {
            val o = n.openRequest(nowSecs(), amountMinor, m.code, rate.rate, rate.sourceLabel, rate.sourceFetchedAt, atTheTill(), noteText, days)
            amounts[o.index] = amountMinor
            showSale(o.index, o.payment, null, made = true)
        } catch (e: CounterException) {
            refused(e.sayWords())
        }
    }

    // ---- sharing a request -------------------------------------------------------------------------------------------------------

    private fun shareUri(name: String): Uri =
        Uri.Builder().scheme("content").authority(ShareImageProvider.AUTHORITY).appendPath(ShareNames.FOLDER).appendPath(name).build()

    /** CR-214: every address handed to the share sheet is revoked and its image deleted when the page is left. */
    private val shared = ShareLedger(
        File(act.cacheDir, ShareNames.FOLDER),
        { name -> act.revokeUriPermission(shareUri(name), Intent.FLAG_GRANT_READ_URI_PERMISSION) },
        { ByteArray(ShareNames.BYTES).also { SecureRandom().nextBytes(it) } },
    )

    private fun dropSharedImage() = shared.end()

    /** CR-214: the label of the payment link: the request's note, or its amount when it has none. */
    private fun requestLabel(index: Long, info: SaleInfo, needsSats: Long): String =
        info.note ?: amounts[index]?.let { a -> money?.takeIf { !it.isSats }?.show(a) } ?: Format.sats(needsSats)

    /** CR-214: gives the share sheet the payment link; the app sends nothing itself. */
    private fun shareLink(index: Long, label: String, refused: (SayWords) -> Unit) {
        val n = node ?: return render()
        try {
            val send = Intent(Intent.ACTION_SEND).setType("text/plain").putExtra(Intent.EXTRA_TEXT, n.requestLink(index, label))
            act.startActivity(Intent.createChooser(send, null))
        } catch (e: CounterException) {
            refused(e.sayWords())
        }
    }

    /** CR-214: gives the share sheet the code as a PNG image, under a fresh name through [ShareImageProvider]. */
    private fun shareCode(payment: Payment, refused: (SayWords) -> Unit) {
        try {
            val side = 720
            val view = QrView(act, payment, side)
            view.measure(View.MeasureSpec.makeMeasureSpec(side, View.MeasureSpec.EXACTLY), View.MeasureSpec.makeMeasureSpec(side, View.MeasureSpec.EXACTLY))
            view.layout(0, 0, side, side)
            val bitmap = Bitmap.createBitmap(side, side, Bitmap.Config.ARGB_8888)
            view.draw(Canvas(bitmap))
            val name = shared.begin { f -> f.outputStream().use { bitmap.compress(Bitmap.CompressFormat.PNG, 100, it) } }
            val uri = shareUri(name)
            val send = Intent(Intent.ACTION_SEND).setType("image/png").putExtra(Intent.EXTRA_STREAM, uri)
                .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
            act.startActivity(Intent.createChooser(send, null))
        } catch (e: IOException) {
            refused(AppWords.fileNotSaved(e.message))
        }
    }

    /**
     * CR-188: with tips on, the customer chooses a tip after the amount and before the code; with tips off the sale
     * opens as it always did. The library offers the tips (none when the settings do not ask for one).
     */
    private fun chooseTip(amountMinor: Long, refused: (SayWords) -> Unit) {
        val n = node ?: return render()
        val choices = try {
            n.tipChoices(amountMinor)
        } catch (e: CounterException) {
            return refused(e.sayWords())
        }
        if (choices.isEmpty()) return openSale(amountMinor, 0, refused)
        showTip(amountMinor, choices)
    }

    /**
     * CR-188: the screen the customer sees: one large button for each suggested percent, "Other amount" and "No
     * tip", with big type and nothing else.
     */
    private fun showTip(amountMinor: Long, choices: List<TipChoice>) {
        if (lockedOut()) return
        val m = money ?: return render()
        val model = WordsModel.of(WordsOfTips.choose(m.show(amountMinor)))
        val failed = { words: SayWords -> showMessage(words, "Back") { showNewSale() } }
        val body = LinearLayout(act).apply { orientation = LinearLayout.VERTICAL }
        body.addView(ui.text(model.heading, 34f, bold = true).also { it.gravity = android.view.Gravity.CENTER }, ui.margins(top = 24))
        body.addView(ui.text(model.what, 20f, colour = ui.pal.secondary).also { it.gravity = android.view.Gravity.CENTER }, ui.margins(top = 12))
        body.addView(ui.text(model.next, 20f, medium = true).also { it.gravity = android.view.Gravity.CENTER }, ui.margins(top = 12))
        val actions = ArrayList<Button>()
        for (c in choices) {
            actions.add(big(ui.primary(WordsOfTips.button(c.percent, m.show(c.tipMinor))) { openSale(amountMinor, c.tipMinor, failed) }))
        }
        actions.add(big(ui.secondary(WordsOfTips.OTHER) { showTipKeypad(amountMinor) }))
        actions.add(big(ui.secondary(WordsOfTips.NONE) { openSale(amountMinor, 0, failed) }))
        show(ui.bar("Counter", logo = true), listOf(body), actions)
    }

    /** A button of the tip screen: taller and in larger type, for a customer's thumb. */
    private fun big(button: Button): Button = button.apply {
        textSize = 22f
        minHeight = ui.px(76)
        minimumHeight = ui.px(76)
    }

    /**
     * CR-188, CR-190: "Other amount": the keypad for the tip. A tip above the sale is refused by the library, and the
     * words show here with the keypad kept. A tip of nothing is no tip.
     */
    private fun showTipKeypad(amountMinor: Long) {
        if (lockedOut()) return
        val m = money ?: return render()
        val entry = AmountEntry(m)
        val display = ui.text(entry.show(), 60f, bold = true).also {
            it.gravity = android.view.Gravity.CENTER
            it.fontFeatureSettings = "tnum"
        }
        val msg = Slot(act)
        val centre = Centered(act)
        centre.addView(ui.text(WordsOfTips.OTHER, 14f, colour = ui.pal.secondary, medium = true).also { it.gravity = android.view.Gravity.CENTER }, ui.margins())
        centre.addView(display, ui.margins(top = 6))
        centre.addView(msg, ui.margins(top = 12))
        val pad = ui.amountPad { face ->
            when (face) {
                "C" -> entry.clear()
                "⌫" -> entry.backspace()
                else -> entry.press(face[0])
            }
            display.text = entry.show()
            msg.show()
        }
        val open = ui.primary("Open the sale") { openSale(amountMinor, entry.minor) { msg.show(note(it)) } }
        show(ui.bar(WordsOfTips.OTHER, back = { chooseTip(amountMinor) { showNewSale() } }), listOf(centre, pad), listOf(open))
    }

    /** The price a sale is quoted at: the newest `combine` of the samples already collected (RT-41), or the words of why there is none. */
    private fun quote(): Pair<org.openbmt.counter.Rate?, SayWords?> {
        // CR-197: with Bitcoin Standard the library reads no price, so none is asked for and the call carries a blank one.
        if (money?.isSats == true) return org.openbmt.counter.Rate(0, "", 0) to null
        val b = book ?: return null to AppWords.stopped
        val r = b.rate()
        val e = r.exceptionOrNull()
        if (e is CounterException) return null to e.sayWords()
        return r.getOrNull() to null
    }

    /** Opens the sale for [amountMinor] and [tipMinor] together (CR-189); a tip of 0 is the sale of `open`. */
    private fun openSale(amountMinor: Long, tipMinor: Long, refused: (SayWords) -> Unit) {
        val n = node ?: return render()
        val m = money ?: return render()
        val (rate, words) = quote()
        if (rate == null) return refused(words ?: AppWords.stopped)
        try {
            val o = if (tipMinor > 0) {
                n.openWithTip(nowSecs(), amountMinor, tipMinor, m.code, rate.rate, rate.sourceLabel, rate.sourceFetchedAt, atTheTill())
            } else {
                n.open(nowSecs(), amountMinor, m.code, rate.rate, rate.sourceLabel, rate.sourceFetchedAt, atTheTill())
            }
            // What the screens say of the sale is the total the customer pays: the sale and the tip.
            amounts[o.index] = amountMinor + tipMinor
            showSale(o.index, o.payment, null)
        } catch (e: CounterException) {
            refused(e.sayWords())
        }
    }

    // ---- the sale ------------------------------------------------------------------------------------------------------------

    /**
     * One screen that changes with the state. Not paid yet, and part paid, show the amount, the code and the next
     * step on the page; every other state is the whole screen in the colour of its outcome. The screen is made
     * again when what it says changes; the words are set at once, the background fades over 200 ms, and a sale
     * plays the sound of green once and of red once, the first time its screen shows that colour.
     */
    private fun showSale(index: Long, first: Payment?, notice: SayWords?, made: Boolean = false) {
        val n = node ?: return render()
        val m = money ?: return render()
        locks.saleScreen(index)
        var payment: Payment? = first
        var flash: SayWords? = notice
        val maxPx = (act.resources.displayMetrics.heightPixels * 0.45f).toInt()
        // Space for the code: the width of the page less its gutters and the code card's own, and the share of the height.
        val side = minOf(ui.px(276), act.resources.displayMetrics.widthPixels - ui.px(2 * 20 + 2 * 16), maxPx)
        var lastSig: String? = null
        var lastBackground: Int? = null
        val flashWith = { reflash: SayWords? -> flash = reflash; lastSig = null; tick?.invoke(); Unit }
        tick = null
        showing = Showing.Sale(index)
        tick = {
            try {
                val s = n.judge(nowSecs(), index)
                // The library gives a code only while the sale is open: with none, the code and the copied request are
                // cleared, so a sale that ended or has no code never shows an earlier one (CR-112).
                payment = s.payment
                // TP-01, TP-05: the tag changes with the code, and is empty when no code shows (TP-04).
                tapSet(if (SaleModel.showsCode(s.state)) payment?.tag else null)
                // CR-214: a request that is still open says its own words: ready when just made, waiting after.
                val info = n.saleInfo(index)
                val request = info.requestDays > 0
                val says = WordsModel.of(
                    if (request && s.state == SaleState.NotPaidYet) {
                        if (made) AppWords.requestMade(Format.localTime(info.expiresAt), Format.sats(s.needsSats)) else AppWords.requestOpen(Format.localTime(info.expiresAt))
                    } else {
                        s.words.say()
                    },
                )
                val extra = ArrayList<WordsModel>()
                // CR-269: a sale the till cancelled goes on; the line says so and the cashier ends it as any sale is ended.
                val tillCancelled = try {
                    n.tillOfSale(index).takeIf { it.cancelled }?.words
                } catch (e: CounterException) {
                    null
                } catch (e: LinkageError) {
                    null
                }
                tillCancelled?.let { extra.add(WordsModel.of(it.say())) }
                s.check?.let { extra.add(WordsModel.of(it.say())) }
                for (f in s.flags) extra.add(WordsModel.of(f.say()))
                // CR-113: the line that says Lightning is not available for this sale, or that its code is on the way.
                val ln = s.lightning
                val unavailable = ln?.unavailable
                val failed = ln?.failed
                when {
                    unavailable != null -> extra.add(WordsModel.of(unavailable.say()))
                    ln != null && ln.waiting -> extra.add(WordsModel.of(AppWords.lightningWaiting))
                    failed != null -> extra.add(WordsModel.of(failed.say()))
                }
                // CR-219: with a swap provider set and none for this sale, the code stays on-chain and one small line says so.
                val notForThisSale = ln != null && ln.notForThisSale
                val sig = listOf(
                    s.state, says.heading, says.what, says.next, s.receivedSats, s.needsSats, payment?.copyUri, flash?.heading,
                    flash?.what, extra.joinToString("\u0002") { it.heading + it.what + it.next }, notForThisSale, info.reference, info.billNumber,
                ).joinToString("\u0001")
                if (sig != lastSig) {
                    lastSig = sig
                    val code = SaleModel.showsCode(s.state)
                    val b = SaleModel.buttons(s.state, request)
                    val amountBig = SaleModel.amountBig(m, amounts[index], s.needsSats)
                    val bar = ui.bar("Sale ${index + 1}", onBlack = !code)
                    val page: View
                    val pageBackground: Int
                    if (code) {
                        pageBackground = ui.pal.background
                        addToBar(bar, ui.stateChip(says), ui)
                        val body = ArrayList<View>()
                        flash?.let { body.add(note(it)) }
                        val amount = LinearLayout(act).apply { orientation = LinearLayout.VERTICAL }
                        amount.addView(ui.text(amountBig, 48f, bold = true).also { it.gravity = android.view.Gravity.CENTER; it.fontFeatureSettings = "tnum" }, ui.margins())
                        amount.addView(ui.text(SaleModel.amountSmall(m, amounts[index], s.needsSats), 16f, colour = ui.pal.secondary).also { it.gravity = android.view.Gravity.CENTER; it.fontFeatureSettings = "tnum" }, ui.margins(top = 2))
                        amount.addView(ui.text(SaleModel.receivedLine(s.receivedSats, s.needsSats), 14f, colour = ui.pal.secondary).also { it.gravity = android.view.Gravity.CENTER; it.fontFeatureSettings = "tnum" }, ui.margins(top = 2))
                        body.add(amount)
                        payment?.let { p ->
                            val qr = FrameLayout(act).apply { addView(QrView(act, p, side), FrameLayout.LayoutParams(side, side)) }
                            body.add(ui.codeCard(qr))
                        }
                        if (notForThisSale) {
                            body.add(ui.text(AppWords.lnNotForThisSale, 13f, colour = ui.pal.secondary).also { it.gravity = android.view.Gravity.CENTER })
                        }
                        val words = LinearLayout(act).apply {
                            orientation = LinearLayout.VERTICAL
                            gravity = android.view.Gravity.CENTER_HORIZONTAL
                        }
                        words.addView(ui.text(says.what, 16f, colour = ui.pal.secondary).also { it.gravity = android.view.Gravity.CENTER }, ui.margins())
                        words.addView(ui.text(says.next, 17f, medium = true).also { it.gravity = android.view.Gravity.CENTER }, ui.margins(top = 8))
                        words.addView(left(ui.whoChip(says)).also { (it as LinearLayout).gravity = android.view.Gravity.CENTER_HORIZONTAL }, ui.margins(top = 10))
                        body.add(words)
                        for (w in extra) body.add(ui.notice(w))
                        if (request) {
                            val label = requestLabel(index, info, s.needsSats)
                            body.add(ui.button(WordsOfRequests.SHARE_LINK) { shareLink(index, label) { w -> flashWith(w) } })
                            payment?.let { p -> body.add(ui.button(WordsOfRequests.SHARE_CODE) { shareCode(p) { w -> flashWith(w) } }) }
                        }
                        body.add(
                            ui.button("Copy the request") {
                                val u = payment?.copyUri
                                if (u != null) {
                                    act.getSystemService(ClipboardManager::class.java)?.setPrimaryClip(ClipData.newPlainText("payment request", u))
                                    flashWith(AppWords.requestCopied)
                                }
                            },
                        )
                        body.add(ui.button("Details and refunds") { showSaleDetails(index) })
                        val actions = ArrayList<Button>()
                        if (b.requote) actions.add(ui.primary("Requote") { requote(index) { w -> flashWith(w) } })
                        if (b.cancel) actions.add(ui.danger("Cancel the sale") { cancelSale(index) { w -> flashWith(w) } })
                        if (b.override) actions.add(ui.button(if (cashiers.isEmpty()) "Accept by hand (manager)" else "Accept by hand") { showOverride(index) })
                        actions.add(ui.button("Done") { showHome() })
                        page = ui.page(bar, body, actions)
                    } else {
                        val band = outcomeBand(says.colour)
                        pageBackground = band.band
                        val centre = Centered(act)
                        centre.addView(OutcomeMark(act, says.colour, band.band), LinearLayout.LayoutParams(ui.px(112), ui.px(112)))
                        centre.addView(
                            ui.text(says.heading, SaleModel.outcomeHeadingSp(says.heading), bold = true, colour = Palette.WHITE).also { it.gravity = android.view.Gravity.CENTER },
                            ui.margins(top = 16),
                        )
                        centre.addView(
                            ui.text(SaleModel.outcomeLine(m, amounts[index], s.needsSats), 20f, colour = band.sub, medium = true).also {
                                it.gravity = android.view.Gravity.CENTER
                                it.fontFeatureSettings = "tnum"
                            },
                            ui.margins(top = 8),
                        )
                        // CR-254: the sale's number and the till's reference, large enough to copy into the till's payment note.
                        if (SaleModel.isPaid(s.state)) {
                            for (line in BillModel.paidLines(index, info)) {
                                centre.addView(ui.text(line.label, 14f, colour = band.sub, medium = true).also { it.gravity = android.view.Gravity.CENTER }, ui.margins(top = 14))
                                centre.addView(
                                    ui.text(line.value, 34f, bold = true, colour = Palette.WHITE).also {
                                        it.gravity = android.view.Gravity.CENTER
                                        it.setTextIsSelectable(true)
                                        it.fontFeatureSettings = "tnum"
                                    },
                                    ui.margins(top = 2),
                                )
                            }
                        }
                        centre.addView(ui.text(says.what, 17f, colour = Palette.WHITE).also { it.gravity = android.view.Gravity.CENTER }, ui.margins(top = 28))
                        centre.addView(ui.text(says.next, 17f, bold = true, colour = Palette.WHITE).also { it.gravity = android.view.Gravity.CENTER }, ui.margins(top = 10))
                        val more = ArrayList<WordsModel>(extra)
                        flash?.let { more.add(0, WordsModel.of(it)) }
                        for (w in more) {
                            val card = LinearLayout(act).apply {
                                orientation = LinearLayout.VERTICAL
                                background = ui.roundedFill(0x29000000, 16f)
                                setPadding(ui.px(16), ui.px(14), ui.px(16), ui.px(14))
                            }
                            card.addView(ui.text(w.heading, 15f, bold = true, colour = Palette.WHITE))
                            card.addView(ui.text(w.what, 15f, colour = Palette.WHITE), ui.margins(top = 4))
                            centre.addView(card, ui.margins(top = 16))
                        }
                        val actions = ArrayList<Button>()
                        actions.add(ui.onBand("Done", SaleModel.isPaid(s.state) && says.colour == Colour.Green, band.band) { showHome() })
                        if (b.override) actions.add(ui.onBand(if (cashiers.isEmpty()) "Accept by hand (manager)" else "Accept by hand", false, band.band) { showOverride(index) })
                        actions.add(ui.onBand("Details and refunds", false, band.band) { showSaleDetails(index) })
                        page = ui.page(bar, listOf(centre), actions)
                        ui.paintBand(page, bar, band, act.window)
                    }
                    act.show(page)
                    val from = lastBackground
                    if (from != null && from != pageBackground) Feedback.crossfade(page, from, pageBackground)
                    lastBackground = pageBackground
                    // The first time this sale's screen is green or red, its sound and haptic play, once.
                    if (outcomePlays.first(index, says.colour)) page.post { Feedback.outcome(page, says.colour) }
                }
            } catch (e: CounterException) {
                tapSet(null)
                act.show(ui.page(ui.bar("Sale ${index + 1}"), listOf(note(e.sayWords()))))
                lastSig = null
            }
        }
        tick?.invoke()
    }

    private fun requote(index: Long, refused: (SayWords) -> Unit) {
        val n = node ?: return render()
        val (rate, words) = quote()
        if (rate == null) return refused(words ?: AppWords.stopped)
        try {
            val o = n.requote(nowSecs(), index, rate.rate, rate.sourceLabel, rate.sourceFetchedAt)
            showSale(index, o.payment, null)
        } catch (e: CounterException) {
            refused(e.sayWords())
        }
    }

    private fun cancelSale(index: Long, refused: (SayWords) -> Unit) {
        val n = node ?: return render()
        try {
            n.cancel(nowSecs(), index, atTheTill())
            tick?.invoke()
        } catch (e: CounterException) {
            refused(e.sayWords())
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
            val pinInput = pinField(act, "")
            if (cashier) views.add(ui.labelled("Manager's PIN", pinInput))
            return ActorBlock(group, pinInput, who)
        }
        val group = ui.choice(listOf("Cashier", "Manager (needs the PIN)"))
        views.add(group)
        val pinInput = pinField(act, "")
        views.add(ui.labelled("Manager's PIN", pinInput))
        return ActorBlock(group, pinInput, null)
    }

    /** The person who acts: the signed-in person, or the cashier by name, or the manager when the PIN is right (CR-21, CR-148). */
    private fun resolveActor(block: ActorBlock, done: (Actor) -> Unit, failed: (SayWords) -> Unit) {
        val cfg = config ?: return render()
        val who = block.signedIn
        if (who != null) {
            if (ui.chosen(block.group) != 1) return done(who)
            return checkPin(block.pin.text.toString(), block.pin, { done(Actor(ActorRole.Manager, cfg.manager)) }, failed)
        }
        if (ui.chosen(block.group) != 1) {
            if (cfg.cashier.isEmpty()) return failed(AppWords.nameNeeded)
            return done(Actor(ActorRole.Cashier, cfg.cashier))
        }
        checkPin(block.pin.text.toString(), block.pin, { done(Actor(ActorRole.Manager, cfg.manager)) }, failed)
    }

    /** Checks [text] as the manager's PIN; a PIN that is wrong or held shakes [field]. */
    private fun checkPin(text: String, field: View?, ok: () -> Unit, failed: (SayWords) -> Unit) {
        async({ pin.check(text) }) { r ->
            // CR-200: an answer that comes back after the app was hidden unlocks nothing and shows nothing; the try was
            // counted before it was compared (CR-85), and the person types the PIN again.
            if (!PinSteps.applies(visible)) return@async
            val e = r.exceptionOrNull()
            if (e != null) {
                failed(if (e is IOException) AppWords.fileNotSaved(e.message) else AppWords.fault(CounterException("pin", e.javaClass.simpleName, 4)))
            } else {
                when (val v = r.getOrNull()) {
                    PinResult.Ok -> ok()
                    is PinResult.Wrong -> {
                        field?.let { Feedback.shake(it) }
                        failed(AppWords.pinWrong(v.waitSecs))
                    }
                    is PinResult.Locked -> {
                        field?.let { Feedback.shake(it) }
                        failed(AppWords.pinLocked(v.waitSecs))
                    }
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
        val back = { showSale(index, null, null) }
        if (atTheTill() != null) {
            // CR-148: a cashier's override is the signed-in cashier's, with no second PIN; the manager's still needs the PIN.
            val body = ArrayList<View>()
            val actor = actorBlock(body)
            body.add(msg)
            return titled(
                "Accept by hand",
                AppWords.overrideAbout,
                body,
                listOf(
                    ui.primary("Accept this sale") {
                        resolveActor(actor, { who ->
                            try {
                                n.overrideSale(nowSecs(), index, who)
                                showSale(index, null, null)
                            } catch (e: CounterException) {
                                msg.show(note(e.sayWords()))
                            }
                        }, { msg.show(note(it)) })
                    },
                    ui.button("Back") { back() },
                ),
                back = { back() },
                shown = Showing.Sale(index),
            )
        }
        val pinInput = pinField(act, "")
        titled(
            "Accept by hand",
            AppWords.managerNeeded,
            listOf(ui.labelled("Manager's PIN", pinInput), msg),
            listOf(
                ui.primary("Accept this sale") {
                    checkPin(pinInput.text.toString(), pinInput, {
                        try {
                            n.overrideSale(nowSecs(), index, Actor(ActorRole.Manager, cfg.manager))
                            showSale(index, null, null)
                        } catch (e: CounterException) {
                            msg.show(note(e.sayWords()))
                        }
                    }, { msg.show(note(it)) })
                },
                ui.button("Back") { back() },
            ),
            back = { back() },
            shown = Showing.Sale(index),
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
            val rows = ArrayList<RowItem>()
            for (i in sales) {
                val s = n.judge(nowSecs(), i)
                val amount = amounts[i]?.let { a -> money?.takeIf { !it.isSats }?.let { it.show(a) + ", " } }.orEmpty()
                rows.add(
                    RowItem(
                        "Sale ${i + 1}: $amount${Format.sats(s.needsSats)}",
                        s.words.heading,
                        WordsModel.colourOf(s.words.tone),
                        onClick = { showSaleDetails(i) },
                    ),
                )
            }
            if (rows.isNotEmpty()) views.add(ui.rows(rows))
        } catch (e: CounterException) {
            views.add(note(e.sayWords()))
        }
        show(ui.bar("Sales", back = { showHome() }), views, emptyList(), Showing.Sales)
    }

    private fun showSaleDetails(index: Long, notice: SayWords? = null) {
        val n = node ?: return render()
        val m = money ?: return render()
        locks.saleScreen(index)
        val views = ArrayList<View>()
        notice?.let { views.add(note(it)) }
        views.add(label("Sale ${index + 1}"))
        val actions = ArrayList<Button>()
        try {
            val s = n.judge(nowSecs(), index)
            views.add(
                ui.facts(
                    listOf(
                        "Sale ${index + 1}" to SaleModel.amountLine(m, amounts[index], s.needsSats),
                        "Received" to "${Format.sats(s.receivedSats)} of ${Format.sats(s.needsSats)}",
                    ),
                ),
            )
            views.add(note(s.words.say()))
            s.check?.let { views.add(note(it.say())) }
            for (f in s.flags) views.add(note(f.say()))
            // CR-216: the note of the sale, or a way to add one.
            val info = n.saleInfo(index)
            val saleNote = info.note
            if (saleNote != null) {
                views.add(ui.facts(listOf(WordsOfRequests.NOTE to saleNote)))
            } else {
                actions.add(ui.button(WordsOfRequests.ADD_NOTE) { showAddNote(index) })
            }
            // CR-245: the till's reference of the sale, or a way to add it once.
            val saleReference = info.reference
            if (saleReference != null) {
                views.add(ui.facts(listOf(WordsOfBills.REFERENCE to saleReference)))
            } else {
                actions.add(ui.button(WordsOfBills.ADD_REFERENCE) { showSaleReference(index) })
            }
            info.billNumber?.let { b -> actions.add(ui.button("${WordsOfBills.SALE_BILL} $b") { showBill(b, null) }) }
            views.add(label("Refunds"))
            val rows = n.refunds(index)
            if (rows.isEmpty()) views.add(ui.text("No refund was asked for.", 16f))
            for (r in rows) {
                val by = "${if (r.byRole == ActorRole.Manager) "manager" else "cashier"} ${Format.cleanForeign(r.byName)}"
                views.add(ui.text("Asked ${Format.sats(r.askedSats)}${r.valueMinor?.takeIf { !m.isSats }?.let { ", " + m.show(it) } ?: ""} by $by at ${Format.localTime(r.at)}", 14f))
                // CR-192: how much of the refund is the tip.
                if (r.tipMinor > 0) views.add(ui.text(WordsOfTips.refundLine(m.show(r.tipMinor)), 14f))
                views.add(note(r.words.say()))
                val number = r.number
                if (number != null && (r.state == RefundState.Requested || r.state == RefundState.WaitingForDepth)) {
                    views.add(ui.danger("Withdraw this request") { showWithdraw(index, number) })
                }
            }
            // CR-217: a sale paid over Lightning is refunded in cash only.
            if (info.cashRefundOnly) {
                views.add(note(AppWords.lightningRefundCash))
            } else {
                actions.add(ui.button("Ask for a refund") { showRefundRequest(index) })
            }
            actions.add(ui.button("Refund in cash from the till") { showCashRefund(index) })
        } catch (e: CounterException) {
            views.add(note(e.sayWords()))
        }
        actions.add(ui.button("Back to the sale") { showSale(index, null, null) })
        actions.add(ui.button("Back to sales") { showSales() })
        show(ui.bar("Details and refunds", back = { showSale(index, null, null) }), views, actions, Showing.Sale(index))
    }

    /** CR-216: asks for the note of a sale that has none, and adds it once (CR-206). */
    private fun showAddNote(index: Long) {
        val n = node ?: return render()
        locks.saleScreen(index)
        val views = ArrayList<View>()
        val msg = Slot(act)
        views.add(msg)
        views.add(label(WordsOfRequests.NOTE))
        val field = ui.field(WordsOfRequests.NOTE_HINT)
        views.add(field)
        val save = ui.primary(WordsOfRequests.ADD_NOTE) {
            try {
                n.addNote(nowSecs(), index, field.text.toString().trim())
                showSaleDetails(index)
            } catch (e: CounterException) {
                msg.show(note(e.sayWords()))
            }
        }
        show(ui.bar(WordsOfRequests.ADD_NOTE, back = { showSaleDetails(index) }), views, listOf(save, ui.button("Back") { showSaleDetails(index) }), Showing.Sale(index))
    }

    private fun showRefundRequest(index: Long) {
        val n = node ?: return render()
        val m = money ?: return render()
        locks.saleScreen(index)
        val views = ArrayList<View>()
        val msg = Slot(act)
        views.add(msg)
        views.add(label("Amount to refund"))
        val kind = ui.chips(if (m.isSats) listOf("In sats") else listOf("In ${m.code}", "In sats"))
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
        val ask = ui.primary("Ask for the refund") {
            val text = amount.text.toString()
            val dest = destination.text.toString().trim()
            val why = reason.text.toString().trim()
            val inSats = m.isSats || ui.chosen(kind) == 1
            val sats = Format.toAmount(text)
            val minor = m.parse(text)
            when {
                (inSats && (sats == null || sats == 0L)) || (!inSats && (minor == null || minor == 0L)) ->
                    msg.show(note(AppWords.notANumber))
                dest.isEmpty() -> msg.show(note(AppWords.destinationNeeded))
                why.isEmpty() -> msg.show(note(AppWords.reasonNeeded))
                else -> resolveActor(actor, { who ->
                    val asked: RefundAsk? = if (inSats) {
                        sats?.let { RefundAsk.Sats(it) }
                    } else {
                        val (rate, words) = quote()
                        if (rate == null) {
                            msg.show(note(words ?: AppWords.stopped))
                            null
                        } else {
                            minor?.let { RefundAsk.Value(it, rate.rate, rate.sourceLabel, rate.sourceFetchedAt) }
                        }
                    }
                    if (asked != null) {
                        try {
                            n.requestRefund(nowSecs(), index, asked, dest, who, why)
                            showSaleDetails(index, AppWords.refundAsked)
                        } catch (e: CounterException) {
                            msg.show(note(e.sayWords()))
                        }
                    }
                }, { msg.show(note(it)) })
            }
        }
        show(ui.bar("Ask for a refund", back = { showSaleDetails(index) }), views, listOf(ask, ui.button("Back") { showSaleDetails(index) }), Showing.Sale(index))
    }

    private fun showCashRefund(index: Long) {
        val n = node ?: return render()
        val m = money ?: return render()
        locks.saleScreen(index)
        val views = ArrayList<View>()
        val msg = Slot(act)
        views.add(msg)
        views.add(label("Cash to give back, in ${if (m.isSats) "sats" else m.code}"))
        val amount = amountField(act, "Amount")
        views.add(amount)
        views.add(label("Reason"))
        val reason = ui.field("A short reason")
        views.add(reason)
        val actor = actorBlock(views)
        val save = ui.primary("Save the cash refund") {
            val minor = m.parse(amount.text.toString())
            val why = reason.text.toString().trim()
            when {
                minor == null || minor == 0L -> msg.show(note(AppWords.notANumber))
                why.isEmpty() -> msg.show(note(AppWords.reasonNeeded))
                else -> resolveActor(actor, { who ->
                    try {
                        n.refundInCash(nowSecs(), index, who, minor, why)
                        showSaleDetails(index, AppWords.cashRefunded)
                    } catch (e: CounterException) {
                        msg.show(note(e.sayWords()))
                    }
                }, { msg.show(note(it)) })
            }
        }
        show(ui.bar("Refund in cash from the till", back = { showSaleDetails(index) }), views, listOf(save, ui.button("Back") { showSaleDetails(index) }), Showing.Sale(index))
    }

    private fun showWithdraw(index: Long, number: Long) {
        val n = node ?: return render()
        locks.saleScreen(index)
        val views = ArrayList<View>()
        val msg = Slot(act)
        views.add(msg)
        views.add(ui.text("Withdraw refund request $number of sale ${index + 1}?", 18f, bold = true))
        val actor = actorBlock(views)
        val withdraw = ui.danger("Withdraw it") {
            resolveActor(actor, { who ->
                try {
                    n.withdrawRefund(nowSecs(), index, number, who)
                    showSaleDetails(index, AppWords.refundWithdrawn)
                } catch (e: CounterException) {
                    msg.show(note(e.sayWords()))
                }
            }, { msg.show(note(it)) })
        }
        show(ui.bar("Withdraw this request", back = { showSaleDetails(index) }), views, listOf(withdraw, ui.button("Back") { showSaleDetails(index) }), Showing.Sale(index))
    }

    // ---- who is at the till (CR-147 to CR-150) ------------------------------------------------------------------------------

    /** "Who is at the till?": the cashiers' names and the manager's. */
    private fun showWho(notice: SayWords?) {
        val cfg = config ?: return render()
        val rows = ArrayList<RowItem>()
        for (name in cashiers) rows.add(RowItem(name, onClick = { showSignIn(Actor(ActorRole.Cashier, name)) }))
        rows.add(RowItem("${cfg.manager} (manager)", onClick = { showSignIn(Actor(ActorRole.Manager, cfg.manager)) }))
        titled("Counter", notice ?: AppWords.whoAtTill, listOf(ui.rows(rows)), logo = true)
    }

    /** The PIN of a person at the till is digits only (a cashier's 4 to 8, the manager's 6 to 12), so it is typed on the keypad. */
    private fun showSignIn(person: Actor) {
        val msg = Slot(act)
        val typed = StringBuilder()
        lateinit var keys: org.openbmt.app.Keypad
        keys = ui.keypad(
            { d ->
                if (typed.length < MAX_PIN) typed.append(d)
                keys.entered = typed.length
            },
            {
                if (typed.isNotEmpty()) typed.setLength(typed.length - 1)
                keys.entered = typed.length
            },
        )
        val name = ui.text(person.name, 28f, bold = true).also { it.gravity = android.view.Gravity.CENTER }
        show(
            ui.bar(person.name, back = { showWho(null) }),
            listOf(name, keys, msg),
            listOf(ui.primary("Sign in") { signIn(person, typed.toString(), msg, keys) }, ui.button("Back") { showWho(null) }),
        )
    }

    /** CR-150: the PIN check of CR-85 for that person, counted and stored before it is compared; a held PIN is not looked at. */
    private fun signIn(person: Actor, text: String, msg: Slot, field: View) {
        async({ if (person.role == ActorRole.Manager) pin.check(text) else staff.check(person.name, text) }) { r ->
            // CR-200: a sign-in that finished after the app was hidden signs nobody in.
            if (!PinSteps.applies(visible)) return@async
            val e = r.exceptionOrNull()
            if (e is IOException) return@async msg.show(note(AppWords.fileNotSaved(e.message)))
            if (e != null) throw e
            when (val v = r.getOrThrow()) {
                PinResult.Ok -> {
                    locks.signIn(person)
                    val next = afterSignIn
                    afterSignIn = null
                    if (next != null) open(next) else showHome()
                }
                is PinResult.Wrong -> {
                    Feedback.shake(field)
                    msg.show(note(AppWords.signInWrong(v.waitSecs)))
                }
                is PinResult.Locked -> {
                    Feedback.shake(field)
                    msg.show(note(AppWords.signInHeld(v.waitSecs)))
                }
            }
        }
    }

    // ---- staff: the cashiers (CR-142) ------------------------------------------------------------------------------------------------

    /** The manager's PIN opens Staff. */
    private fun showStaffEntry() {
        if (lockedOut()) return
        val msg = Slot(act)
        val pinInput = pinField(act, "")
        titled(
            "Staff",
            AppWords.staffPinNeeded,
            listOf(ui.labelled("Manager's PIN", pinInput), msg),
            listOf(
                ui.primary("Open") { checkPin(pinInput.text.toString(), pinInput, { showStaff(emptyList()) }, { msg.show(note(it)) }) },
                ui.button("Back") { showSettings() },
            ),
            back = { showSettings() },
        )
    }

    private fun showStaff(notices: List<SayWords>) {
        if (lockedOut()) return
        val cfg = config ?: return render()
        if (node == null) return render()
        val model = WordsModel.of(AppWords.staffAbout)
        val views = ArrayList<View>()
        views.add(ui.titleBlock(model, "Staff"))
        for (w in notices) views.add(note(w))
        views.add(label("Cashiers"))
        if (cashiers.isEmpty()) views.add(ui.text("No cashier is listed, so nobody has to sign in.", 16f))
        for (name in cashiers) {
            views.add(ui.text(name, 18f, bold = true))
            views.add(ui.button("New PIN for $name") { showCashierPin(name) })
            views.add(ui.danger("Remove $name") { showRemoveCashier(name) })
        }
        views.add(label("Add a cashier"))
        val nameField = ui.field("Name of the cashier")
        val pins = ui.pinPair("PIN, 4 to 8 digits (the cashier types it)", "PIN again")
        val msg = Slot(act)
        views.add(nameField)
        views.add(pins.view)
        views.add(msg)
        views.add(ui.hint(model))
        show(
            ui.bar("Staff", back = { showSettings() }),
            views,
            listOf(
                ui.primary("Add the cashier") {
                    addCashier(cfg, nameField.text.toString().trim(), pins.first.text.toString(), pins.second.text.toString(), msg)
                },
                ui.button("Back") { showSettings() },
            ),
        )
    }

    /** CR-142: the file first, then the record for the owner; the list is changed even when the record cannot be written, and the app says so. */
    private fun addCashier(cfg: AppConfig, name: String, pin1: String, pin2: String, msg: Slot) {
        (StaffCheck.nameProblem(name, cashiers, cfg.manager) ?: StaffCheck.pinProblem(pin1, pin2))?.let { return msg.show(note(it)) }
        msg.show(note(AppWords.starting))
        async({
            staff.add(name, pin1)
            staff.names()
        }) { r ->
            val e = r.exceptionOrNull()
            if (e is IOException) return@async msg.show(note(AppWords.fileNotSaved(e.message)))
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
        titled(
            "Remove $name",
            null,
            listOf(ui.text("Remove $name?", 22f, bold = true), msg),
            listOf(ui.danger("Remove") { removeCashier(name, msg) }, ui.button("Back") { showStaff(emptyList()) }),
            back = { showStaff(emptyList()) },
        )
    }

    private fun removeCashier(name: String, msg: Slot) {
        async({
            staff.remove(name)
            staff.names()
        }) { r ->
            val e = r.exceptionOrNull()
            if (e is IOException) return@async msg.show(note(AppWords.fileNotSaved(e.message)))
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
        val pins = ui.pinPair("New PIN, 4 to 8 digits (the cashier types it)", "New PIN again")
        titled(
            "New PIN for $name",
            null,
            listOf(ui.text("New PIN for $name", 22f, bold = true), pins.view, msg),
            listOf(
                ui.primary("Save the new PIN") { saveCashierPin(name, pins.first.text.toString(), pins.second.text.toString(), msg, pins.view) },
                ui.button("Back") { showStaff(emptyList()) },
            ),
            back = { showStaff(emptyList()) },
        )
    }

    /** CR-142: a new PIN is not a record; it clears that cashier's count and hold. */
    private fun saveCashierPin(name: String, pin1: String, pin2: String, msg: Slot, fields: View) {
        StaffCheck.pinProblem(pin1, pin2)?.let {
            Feedback.shake(fields)
            return msg.show(note(it))
        }
        msg.show(note(AppWords.starting))
        async({ staff.setPin(name, pin1) }) { r ->
            val e = r.exceptionOrNull()
            if (e is IOException) return@async msg.show(note(AppWords.fileNotSaved(e.message)))
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

    /** A small tile inside the hero card: the label, the amount and what is under it. */
    private fun heroTile(line: TodayModel.Split?): View = LinearLayout(act).apply {
        orientation = LinearLayout.VERTICAL
        background = ui.roundedFill(ui.pal.background, 14f)
        setPadding(ui.px(12), ui.px(12), ui.px(12), ui.px(12))
        if (line != null) {
            addView(ui.text(line.label, 13f, colour = ui.pal.secondary))
            addView(ui.text(line.amount, 17f, bold = true).also { it.fontFeatureSettings = "tnum" }, ui.margins(top = 2))
            if (line.detail.isNotEmpty()) addView(ui.text(line.detail, 13f, colour = ui.pal.secondary).also { it.fontFeatureSettings = "tnum" }, ui.margins(top = 2))
        }
    }

    /** The day's totals from the counter's own records, from local midnight to now; the lines per person only after the manager's PIN. */
    private fun showToday(people: Boolean) {
        if (lockedOut()) return
        val n = node ?: return render()
        val m = money ?: return render()
        val now = nowSecs()
        val from = DayWindow.start(now, ZoneId.systemDefault())
        val views = ArrayList<View>()
        var foot = "From ${Format.localTime(from)} until now."
        try {
            val t = n.dayTotals(from, DayWindow.end(now), people)
            val lines = TodayModel.lines(t, m).map { TodayModel.split(it) }
            fun line(label: String): TodayModel.Split? = lines.firstOrNull { it.label == label }
            // The hero: what was paid, and how.
            val paid = line("Sales paid")
            val hero = ui.surfaceCard().apply { setPadding(ui.px(20), ui.px(20), ui.px(20), ui.px(20)) }
            if (paid != null) {
                hero.addView(ui.text(paid.label, 14f, colour = ui.pal.secondary, medium = true))
                hero.addView(ui.text(paid.amount, 44f, bold = true).also { it.fontFeatureSettings = "tnum" }, ui.margins(top = 4))
                if (paid.detail.isNotEmpty()) hero.addView(ui.text(paid.detail, 15f, colour = ui.pal.secondary).also { it.fontFeatureSettings = "tnum" }, ui.margins(top = 4))
            }
            val tiles = LinearLayout(act).apply { orientation = LinearLayout.HORIZONTAL }
            tiles.addView(heroTile(line("On the chain")), LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f).also { it.rightMargin = ui.px(5) })
            tiles.addView(heroTile(line("Over Lightning")), LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f).also { it.leftMargin = ui.px(5) })
            hero.addView(tiles, ui.margins(top = 14))
            views.add(hero)
            // The rows that need a look, then the quiet lines, in one card.
            val card = ui.surfaceCard()
            val red = ui.pal.stop.band
            val blue = ui.pal.info.band
            val info = ui.pal.info.ink
            fun more(words: SayWords, row: View): View {
                val box = LinearLayout(act).apply { orientation = LinearLayout.VERTICAL }
                box.addView(row, matchWide())
                val m2 = WordsModel.of(words)
                val detail = LinearLayout(act).apply {
                    orientation = LinearLayout.VERTICAL
                    visibility = View.GONE
                    setPadding(ui.px(38), 0, ui.px(16), ui.px(12))
                }
                detail.addView(ui.text(m2.what, 14f, colour = ui.pal.secondary))
                detail.addView(ui.text(m2.next, 14f, medium = true), ui.margins(top = 6))
                box.addView(detail, matchWide())
                row.isClickable = true
                row.setOnClickListener { detail.visibility = if (detail.visibility == View.VISIBLE) View.GONE else View.VISIBLE }
                return box
            }
            var first = true
            fun add(v: View) {
                if (!first) card.addView(ui.line())
                first = false
                card.addView(v, matchWide())
            }
            if (t.stopped > 0) {
                val s = line("Not to be handed over")
                add(more(AppWords.todayStopped, ui.needRow(red, AppWords.todayStopped.heading, null, null, s?.count.orEmpty(), null, {})))
            }
            if (t.waiting.count > 0) {
                val w = line("Sales still waiting")
                val link = LinearLayout(act).apply {
                    orientation = LinearLayout.HORIZONTAL
                    gravity = android.view.Gravity.CENTER_VERTICAL
                    addView(View(act).apply { background = org.openbmt.app.IconDrawable(org.openbmt.app.Icon.InfoCircle, info, 2.2f) }, LinearLayout.LayoutParams(ui.px(14), ui.px(14)).also { it.rightMargin = ui.px(5) })
                    addView(ui.text(AppWords.todayWaiting.heading, 13f, colour = info, medium = true))
                }
                val parts = w?.parts.orEmpty()
                val sub = parts.lastOrNull()?.takeIf { parts.size > 1 }
                val money2 = if (parts.size >= 3) parts[1] else null
                add(more(AppWords.todayWaiting, ui.needRow(blue, w?.label.orEmpty(), sub, link, w?.count.orEmpty(), money2, {})))
            }
            if (t.refunds.count > 0) {
                val r = line("Refunds asked for")
                add(ui.needRow(blue, r?.label.orEmpty(), null, null, r?.count.orEmpty(), r?.rest?.ifEmpty { null }, null))
            }
            val quiet = LinearLayout(act).apply {
                orientation = LinearLayout.VERTICAL
                setPadding(ui.px(16), ui.px(12), ui.px(16), ui.px(12))
            }
            for (label in listOf("Sales cancelled or expired", "Overrides", "Cash refunds", WordsOfTips.LINE)) {
                val l = line(label) ?: continue
                val row = LinearLayout(act).apply { orientation = LinearLayout.HORIZONTAL }
                row.addView(ui.text(l.label, 14f, colour = ui.pal.secondary), LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f))
                row.addView(ui.text(l.value, 14f, colour = ui.pal.secondary).also { it.fontFeatureSettings = "tnum"; it.gravity = android.view.Gravity.END })
                quiet.addView(row, ui.margins(top = if (quiet.childCount == 0) 0 else 8))
            }
            add(quiet)
            views.add(card)
            // CR-274: the day's payments from the starter wallet, when the counter has any.
            val starterDay = StarterModel.day(starterNow(n), from, DayWindow.end(now))
            if (starterDay.isNotEmpty()) {
                val pays = ui.surfaceCard()
                pays.addView(ui.label(WordsOfStarter.TODAY).also { it.setPadding(ui.px(16), ui.px(12), ui.px(16), ui.px(4)) }, matchWide())
                for (p in starterDay) {
                    pays.addView(ui.text(StarterModel.todayLine(p), 14f, colour = ui.pal.secondary).also { it.setPadding(ui.px(16), ui.px(8), ui.px(16), ui.px(8)) }, matchWide())
                }
                views.add(pays)
            }
            // The lines per person, behind the manager's PIN.
            if (people) {
                val per = ui.surfaceCard()
                per.addView(ui.label("Lines per person").also { it.setPadding(ui.px(16), ui.px(12), ui.px(16), ui.px(4)) }, matchWide())
                if (t.people.isEmpty() && t.unnamedSales == 0L && t.peopleLeftOut == 0L) {
                    per.addView(ui.text("Nobody is named in today's records.", 16f).also { it.setPadding(ui.px(16), ui.px(8), ui.px(16), ui.px(14)) }, matchWide())
                }
                for (p in t.people) {
                    val parts = TodayModel.personParts(p, m)
                    val row = LinearLayout(act).apply {
                        orientation = LinearLayout.HORIZONTAL
                        gravity = android.view.Gravity.CENTER_VERTICAL
                        setPadding(ui.px(16), ui.px(12), ui.px(16), ui.px(12))
                    }
                    val initial = ui.text(parts.name.take(1).uppercase(), 15f, bold = true).also {
                        it.gravity = android.view.Gravity.CENTER
                        it.background = ui.roundedFill(ui.pal.background, 18f)
                    }
                    row.addView(initial, LinearLayout.LayoutParams(ui.px(36), ui.px(36)).also { it.rightMargin = ui.px(12) })
                    val col = LinearLayout(act).apply { orientation = LinearLayout.VERTICAL }
                    val head = ui.text("${parts.name} (${parts.role})", 15f, medium = true)
                    col.addView(head)
                    col.addView(ui.text(parts.line, 13f, colour = ui.pal.secondary), ui.margins(top = 2))
                    // CR-191: the tips this person took.
                    parts.tips?.let { col.addView(ui.text(it, 13f, colour = ui.pal.secondary), ui.margins(top = 2)) }
                    row.addView(col, LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f))
                    per.addView(ui.line())
                    per.addView(row, matchWide())
                }
                for (extra in TodayModel.people(t, m).drop(t.people.size)) {
                    per.addView(ui.line())
                    per.addView(ui.text(extra, 14f, colour = ui.pal.secondary).also { it.setPadding(ui.px(16), ui.px(12), ui.px(16), ui.px(12)) }, matchWide())
                }
                views.add(per)
            } else {
                val words = AppWords.todayPeopleNeedPin
                val model = WordsModel.of(words)
                val msg = Slot(act)
                val pinInput = pinField(act, "")
                val locked = ui.surfaceCard()
                val head = LinearLayout(act).apply {
                    orientation = LinearLayout.HORIZONTAL
                    gravity = android.view.Gravity.CENTER_VERTICAL
                    setPadding(ui.px(16), ui.px(14), ui.px(12), ui.px(14))
                    isClickable = true
                    background = ui.rippleOver(null, 0f)
                }
                val badge = FrameLayout(act).apply { background = ui.roundedFill(ui.pal.background, 12f) }
                badge.addView(ui.glyph(Glyph.Lock, ui.pal.text, 20), FrameLayout.LayoutParams(ui.px(20), ui.px(20), android.view.Gravity.CENTER))
                head.addView(badge, LinearLayout.LayoutParams(ui.px(40), ui.px(40)).also { it.rightMargin = ui.px(14) })
                val col = LinearLayout(act).apply { orientation = LinearLayout.VERTICAL }
                col.addView(ui.text(model.heading, 16f, medium = true))
                col.addView(ui.text(model.what, 13f, colour = ui.pal.secondary), ui.margins(top = 2))
                head.addView(col, LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f))
                locked.addView(head, matchWide())
                val form = LinearLayout(act).apply {
                    orientation = LinearLayout.VERTICAL
                    visibility = View.GONE
                    setPadding(ui.px(16), 0, ui.px(16), ui.px(16))
                }
                form.addView(ui.text(model.next, 14f, medium = true))
                form.addView(ui.labelled("Manager's PIN", pinInput), ui.margins(top = 12))
                form.addView(msg, ui.margins(top = 8))
                form.addView(
                    ui.primary("Show the lines") {
                        checkPin(pinInput.text.toString(), pinInput, { showToday(true) }, { msg.show(note(it)) })
                    },
                    ui.margins(top = 12),
                )
                locked.addView(form, matchWide())
                head.setOnClickListener { form.visibility = if (form.visibility == View.VISIBLE) View.GONE else View.VISIBLE }
                views.add(locked)
            }
            val about = AppWords.todayAbout
            foot = "$foot ${about.what} ${about.next}"
        } catch (e: CounterException) {
            views.add(note(e.sayWords()))
        }
        views.add(ui.text(foot, 13f, colour = ui.pal.secondary))
        show(ui.bar("Today", back = { showHome() }), views)
    }

    // ---- about ----------------------------------------------------------------------------------------------------------------------

    private fun showAbout() {
        if (lockedOut()) return
        show(
            ui.bar("About", back = { showSettings() }),
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
                    "When the shop's settings turn Tor on, every connection goes through Tor: the counter runs Tor itself, or uses Orbot if the manager chooses it. If Tor is not connected the counter sends nothing and says so.",
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
                        "A home-network address is refused, and the node needs Tor, so Tor must be on in the shop's settings.",
                    16f,
                ),
            ),
            listOf(ui.button("Back") { showSettings() }),
        )
    }

    // ---- bills, tabs and the till's reference (CR-245 to CR-254) -----------------------------------------------------------------

    /**
     * The keypad page of an amount: [heading] says what it is for, [extra] are more fields, [action] is the main
     * button, and [go] is given the amount and a place to show a refusal. [start] fills the keypad (a share of a bill).
     */
    private fun showAmountPad(
        title: String,
        sub: String?,
        heading: String,
        start: Long,
        extra: List<View>,
        action: String,
        back: () -> Unit,
        go: (Long, (SayWords) -> Unit) -> Unit,
    ) {
        val m = money ?: return render()
        val entry = AmountEntry(m)
        entry.set(start)
        val display = ui.text(entry.show(), 60f, bold = true).also {
            it.gravity = android.view.Gravity.CENTER
            it.fontFeatureSettings = "tnum"
        }
        val msg = Slot(act)
        val centre = Centered(act)
        centre.addView(ui.text(heading, 14f, colour = ui.pal.secondary, medium = true).also { it.gravity = android.view.Gravity.CENTER }, ui.margins())
        centre.addView(display, ui.margins(top = 6))
        for (v in extra) centre.addView(v, ui.margins(top = 8))
        centre.addView(msg, ui.margins(top = 12))
        val pad = ui.amountPad { face ->
            when (face) {
                "C" -> entry.clear()
                "\u232B" -> entry.backspace()
                else -> entry.press(face[0])
            }
            display.text = entry.show()
            msg.show()
        }
        val confirm = ui.primary(action) {
            if (entry.minor <= 0) msg.show(note(AppWords.amountNeeded)) else go(entry.minor) { msg.show(note(it)) }
        }
        show(ui.bar(title, sub = sub, back = back), listOf(centre, pad), listOf(confirm, ui.button("Back") { back() }))
    }

    /** CR-246: the bills and tabs the counter holds, open ones and those ended in the last 7 days, and the buttons that open one. */
    private fun showBills(notice: SayWords? = null) {
        if (lockedOut()) return
        val n = node ?: return render()
        val m = money ?: return render()
        val views = ArrayList<View>()
        notice?.let { views.add(note(it)) }
        try {
            val bills = n.bills(nowSecs())
            if (bills.isEmpty()) views.add(ui.text(WordsOfBills.NONE, 16f))
            val rows = bills.reversed().map { b ->
                RowItem(BillModel.listLine(m, b), null, if (b.state == BillState.Open) null else Colour.Neutral, onClick = { showBill(b.number, null) })
            }
            if (rows.isNotEmpty()) views.add(ui.rows(rows))
        } catch (e: CounterException) {
            views.add(note(e.sayWords()))
        }
        val actions = listOf(
            ui.primary(WordsOfBills.NEW_BILL) { showNewBill(BillKind.Bill) },
            ui.button(WordsOfBills.NEW_TAB) { showNewBill(BillKind.Tab) },
        )
        show(ui.bar(WordsOfBills.BILLS, back = { showHome() }), views, actions)
    }

    /** CR-246, CR-245: open a bill or a tab for an amount, with the till's reference when there is one. */
    private fun showNewBill(kind: BillKind) {
        val n = node ?: return render()
        val reference = ui.field(WordsOfBills.REFERENCE_HINT)
        showAmountPad(
            if (kind == BillKind.Tab) WordsOfBills.NEW_TAB else WordsOfBills.NEW_BILL, null, WordsOfBills.AMOUNT, 0,
            listOf(reference), if (kind == BillKind.Tab) WordsOfBills.NEW_TAB else WordsOfBills.NEW_BILL, { showBills() },
        ) { minor, refused ->
            try {
                val number = n.openBill(nowSecs(), kind, minor, BillModel.referenceFrom(reference.text.toString()))
                showBill(number, null)
            } catch (e: CounterException) {
                refused(e.sayWords())
            }
        }
    }

    /** CR-246 to CR-252: one bill with what it has received, its parts and the buttons that fit its state. */
    private fun showBill(number: Long, notice: SayWords?) {
        if (lockedOut()) return
        val n = node ?: return render()
        val m = money ?: return render()
        val views = ArrayList<View>()
        notice?.let { views.add(note(it)) }
        val actions = ArrayList<Button>()
        try {
            val bill = n.bills(nowSecs()).firstOrNull { it.number == number } ?: return showBills()
            val b = BillModel.buttons(bill)
            views.add(note(bill.words.say()))
            val facts = ArrayList<Pair<String, String>>()
            bill.reference?.let { facts.add(WordsOfBills.REFERENCE to it) }
            for (line in BillModel.totals(m, bill)) facts.add(line.label to line.value)
            views.add(ui.facts(facts))
            if (bill.parts.isNotEmpty()) {
                views.add(
                    ui.rows(
                        bill.parts.map { part ->
                            val sale = part.sale
                            RowItem(BillModel.partLine(m, part), null, if (part.counts) Colour.Green else null, onClick = if (sale != null) ({ showSale(sale, null, null) }) else null)
                        },
                    ),
                )
            }
            if (b.bitcoinPart) actions.add(ui.primary(WordsOfBills.PART_BITCOIN) { showBillPart(number, 0) })
            if (b.otherPart) actions.add(ui.button(WordsOfBills.PART_OTHER) { showOtherPart(number) })
            if (b.join) actions.add(ui.button(WordsOfBills.JOIN) { showJoinSale(number) })
            if (b.split) actions.add(ui.button(WordsOfBills.SPLIT) { showSplit(number) })
            if (b.addReference) actions.add(ui.button(WordsOfBills.ADD_REFERENCE) { showBillReference(number) })
            if (b.changeTab) actions.add(ui.button(WordsOfBills.CHANGE_TAB) { showChangeTab(number) })
            if (b.finishTab) {
                actions.add(
                    ui.button(WordsOfBills.FINISH_TAB) {
                        try {
                            n.finishTab(nowSecs(), number)
                            showBill(number, null)
                        } catch (e: CounterException) {
                            showBill(number, e.sayWords())
                        }
                    },
                )
            }
            if (b.closeShort) actions.add(ui.button(WordsOfBills.CLOSE_SHORT) { showCloseShort(number) })
            if (b.cancel) {
                actions.add(
                    ui.danger(WordsOfBills.CANCEL_BILL) {
                        try {
                            n.cancelBill(nowSecs(), number)
                            showBills()
                        } catch (e: CounterException) {
                            showBill(number, e.sayWords())
                        }
                    },
                )
            }
        } catch (e: CounterException) {
            views.add(note(e.sayWords()))
        }
        actions.add(ui.button("Back") { showBills() })
        show(ui.bar("${WordsOfBills.BILLS} $number", back = { showBills() }), views, actions)
    }

    /** CR-248: a bitcoin part: a sale for [start] (or the amount typed) is opened for the bill and shown for the customer to pay. */
    private fun showBillPart(number: Long, start: Long) {
        val n = node ?: return render()
        val reference = ui.field(WordsOfBills.REFERENCE_HINT)
        showAmountPad(WordsOfBills.PART_BITCOIN, "$number", WordsOfBills.AMOUNT, start, listOf(reference), WordsOfBills.PART_BITCOIN, { showBill(number, null) }) { minor, refused ->
            val (rate, words) = quote()
            if (rate == null) return@showAmountPad refused(words ?: AppWords.stopped)
            try {
                val o = n.openBillPart(nowSecs(), number, minor, 0, rate.rate, rate.sourceLabel, rate.sourceFetchedAt, atTheTill(), BillModel.referenceFrom(reference.text.toString()))
                amounts[o.index] = minor
                showSale(o.index, o.payment, null)
            } catch (e: CounterException) {
                refused(e.sayWords())
            }
        }
    }

    /** CR-247: a part paid another way, with the amount and the way the cashier chooses. */
    private fun showOtherPart(number: Long) {
        val n = node ?: return render()
        val ways = ui.chips(WordsOfBills.WAYS)
        showAmountPad(WordsOfBills.PART_OTHER, "$number", WordsOfBills.AMOUNT, 0, listOf(ways), WordsOfBills.PART_OTHER, { showBill(number, null) }) { minor, refused ->
            try {
                n.addOtherPart(nowSecs(), number, minor, PartWay.values()[ui.chosen(ways)])
                showBill(number, null)
            } catch (e: CounterException) {
                refused(e.sayWords())
            }
        }
    }

    /** CR-248: join a sale that is open or paid, such as a paid request, to the bill; the sales that are parts of a bill are not offered. */
    private fun showJoinSale(number: Long) {
        val n = node ?: return render()
        val m = money ?: return render()
        val views = ArrayList<View>()
        val msg = Slot(act)
        views.add(msg)
        try {
            val sales = BillModel.joinable(n.status().sales.reversed().take(MAX_LISTED), n.bills(nowSecs()))
            if (sales.isEmpty()) views.add(ui.text(WordsOfBills.NONE, 16f))
            views.add(
                ui.rows(
                    sales.map { i ->
                        val s = n.judge(nowSecs(), i)
                        val amount = amounts[i]?.let { a -> money?.takeIf { !it.isSats }?.let { it.show(a) + ", " } }.orEmpty()
                        RowItem(
                            "Sale ${i + 1}: $amount${Format.sats(s.needsSats)}", s.words.heading, WordsModel.colourOf(s.words.tone),
                            onClick = {
                                try {
                                    n.joinSale(nowSecs(), number, i)
                                    showBill(number, null)
                                } catch (e: CounterException) {
                                    msg.show(note(e.sayWords()))
                                }
                            },
                        )
                    },
                ),
            )
        } catch (e: CounterException) {
            views.add(note(e.sayWords()))
        }
        show(ui.bar(WordsOfBills.JOIN_PICK, sub = m.code, back = { showBill(number, null) }), views, listOf(ui.button("Back") { showBill(number, null) }))
    }

    /** CR-250: what each of 2 to 20 people pays of what is left; a share is opened as a part when its customer is ready. */
    private fun showSplit(number: Long) {
        val n = node ?: return render()
        val m = money ?: return render()
        val views = ArrayList<View>()
        val msg = Slot(act)
        val shares = Slot(act)
        views.add(msg)
        views.add(label(WordsOfBills.PEOPLE))
        val people = ui.field("2 - ${BillModel.MAX_PEOPLE}")
        people.inputType = android.text.InputType.TYPE_CLASS_NUMBER
        views.add(people)
        views.add(shares)
        val split = ui.primary(WordsOfBills.SPLIT) {
            val count = people.text.toString().trim().toIntOrNull()
            if (count == null || !BillModel.peopleOk(count)) return@primary msg.show(note(AppWords.amountNeeded))
            try {
                val list = n.billSplit(number, count)
                msg.show()
                shares.show(
                    label(WordsOfBills.EACH),
                    ui.rows(list.mapIndexed { i, s -> RowItem(BillModel.splitLines(m, list)[i], null, null, onClick = { showBillPart(number, s) }) }),
                )
            } catch (e: CounterException) {
                msg.show(note(e.sayWords()))
            }
        }
        show(ui.bar(WordsOfBills.SPLIT, back = { showBill(number, null) }), views, listOf(split, ui.button("Back") { showBill(number, null) }))
    }

    /** CR-251: raise a tab, or lower it to no less than it has received. */
    private fun showChangeTab(number: Long) {
        val n = node ?: return render()
        showAmountPad(WordsOfBills.CHANGE_TAB, "$number", WordsOfBills.AMOUNT, 0, emptyList(), WordsOfBills.CHANGE_TAB, { showBill(number, null) }) { minor, refused ->
            try {
                n.setTabAmount(nowSecs(), number, minor)
                showBill(number, null)
            } catch (e: CounterException) {
                refused(e.sayWords())
            }
        }
    }

    /** CR-252: close a bill that has received less than its amount, with the manager's PIN; the library refuses a cashier. */
    private fun showCloseShort(number: Long) {
        val n = node ?: return render()
        val views = ArrayList<View>()
        val actor = actorBlock(views)
        val msg = Slot(act)
        views.add(msg)
        val close = ui.danger(WordsOfBills.CLOSE_SHORT) {
            resolveActor(actor, { who ->
                try {
                    n.closeShort(nowSecs(), number, who)
                    showBill(number, null)
                } catch (e: CounterException) {
                    msg.show(note(e.sayWords()))
                }
            }, { msg.show(note(it)) })
        }
        show(ui.bar(WordsOfBills.CLOSE_SHORT, sub = "$number", back = { showBill(number, null) }), views, listOf(close, ui.button("Back") { showBill(number, null) }))
    }

    /** CR-245: the till's reference of a bill that has none, added once. */
    private fun showBillReference(number: Long) {
        val n = node ?: return render()
        referencePage(number.toString(), { showBill(number, null) }) { text -> n.addBillReference(nowSecs(), number, text) }
    }

    /** CR-245: the till's reference of a sale that has none, added once. */
    private fun showSaleReference(index: Long) {
        val n = node ?: return render()
        locks.saleScreen(index)
        referencePage("${index + 1}", { showSaleDetails(index) }) { text -> n.addReference(nowSecs(), index, text) }
    }

    private fun referencePage(sub: String, back: () -> Unit, save: (String) -> Unit) {
        val views = ArrayList<View>()
        val msg = Slot(act)
        views.add(msg)
        views.add(label(WordsOfBills.REFERENCE))
        val field = ui.field(WordsOfBills.REFERENCE_HINT)
        views.add(field)
        val add = ui.primary(WordsOfBills.ADD_REFERENCE) {
            try {
                save(field.text.toString().trim())
                back()
            } catch (e: CounterException) {
                msg.show(note(e.sayWords()))
            }
        }
        show(ui.bar(WordsOfBills.ADD_REFERENCE, sub = sub, back = back), views, listOf(add, ui.button("Back") { back() }))
    }

    companion object {
        const val VERSION = "0.1.0"

        /** How long the app waits for the built-in Tor to have a circuit: a little over the minute the words promise (CR-204). */
        const val TOR_WAIT_MS = 90_000L
        const val KEY_ALIAS = "openbmt-counter-pairing-v1"
        const val PAIRING_FILE = "pairing.obks"
        const val TYPE_BYTES = "bytes"
        private const val MAX_LISTED = 50

        /** The longest PIN of a person at the till: the manager's 12 digits. */
        private const val MAX_PIN = 12

        /** What the sounds are for, as the row of Settings and the screen of Sounds say it. */
        const val SOUNDS_SENTENCE = "A sound when a sale says to hand over the goods, or not to."
    }
}
