package org.openbmt.owner.app

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Intent
import android.net.Uri
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.view.View
import android.widget.TextView
import java.io.ByteArrayOutputStream
import java.io.File
import java.io.IOException
import java.security.GeneralSecurityException
import java.util.concurrent.LinkedBlockingQueue
import java.util.concurrent.TimeUnit
import org.openbmt.app.ExternalWait
import org.openbmt.app.KeystoreSealer
import org.openbmt.app.Relock
import org.openbmt.app.SayWords
import org.openbmt.app.ScanFlow
import org.openbmt.app.SealedFile
import org.openbmt.app.TorLease
import org.openbmt.app.TorRunner
import org.openbmt.app.TorStartException
import org.openbmt.app.Tries
import org.openbmt.app.TriesFile
import org.openbmt.app.Ui
import org.openbmt.app.UrScan
import org.openbmt.app.WaitOutcome
import org.openbmt.owner.OwnerCeremony
import org.openbmt.owner.OwnerException
import org.openbmt.owner.OwnerNode
import org.openbmt.owner.PosReply
import org.openbmt.owner.PosRequest

/**
 * The root of the app: what is saved, what runs, and the pages. The node runs
 * while the activity is visible: [visible] starts it and [hidden] stops it,
 * releasing its folder. Four threads touch it (OA-12): the screen thread
 * makes every call of the screens; one thread pumps the node (`pump(250)`);
 * one thread takes the turns of the link (`linkStep`, `deliver`) every few
 * seconds when the phone has a link; one thread reads the till (`posRequest`,
 * `posReply`) at most once a minute when the phone has a link. The start
 * itself runs on a worker thread before any of them begins. The screens are
 * in `Screens.kt`, `PosScreen.kt` and `Approval.kt`.
 */
class Controller(internal val act: MainActivity) {
    internal val ui = Ui(act)
    internal val main = Handler(Looper.getMainLooper())
    internal val scan = ScanFlow(act, ui)
    private val dir: File = act.noBackupFilesDir

    /** The app's own folder, for the files of the screens that are in their own file (`Ceremony.kt`). */
    internal val appDir: File get() = dir
    private val nodeDir = File(dir, AppFiles.NODE_DIR)
    private val configFile = ConfigFile(dir)

    // What is saved, as read at the last start.
    internal var setup: OwnerSetup? = null
    internal var config: AppConfig? = null

    // The count of wrong PINs, in the same folder as the setup (OA-63); when the app was last hidden, on the clock that
    // counts since the phone started (OA-62); and the countdown of the unlock screen while it is on show.
    internal val tries by lazy { TriesFile(dir) }
    internal var pausedAt: Long? = null
    internal var ticker: Runnable? = null

    /** Set when a PIN was refused: the next PIN screen shakes its field once (OA-91). */
    internal var shake = false
    private val relockTimer = Runnable { if (!visible) lockNow() }

    // What the owner has typed in the settings editor, in memory only (OA-34).
    internal var settingsDraft = SettingsDraft()

    // The key ceremony in progress: held in memory for the screens, and sealed on disk between steps (OA-26).
    internal var ceremony: CeremonyDraft? = null

    /** Whether the logo has clicked together on the new wallet's screen in this ceremony: it does so once (OA-91). */
    internal var ceremonyLogoShown = false

    // The first run (`FirstRun.kt`): its step and entries, the practice shop (in memory only, never saved) and the notice the
    // welcome shows once after the practice shop is deleted.
    internal var firstRun: FirstRun? = null
    internal var practice: Practice? = null
    internal var welcomeNotice: String? = null
    internal val shopFile by lazy { ShopFile(dir) }

    // A shop with its own wallet (`OwnCounter.kt`): the wallet's sealed file and the owner root's secret, and the pairing in
    // progress, which holds nothing but what is on the screen (OA-105, OA-108 to OA-111).
    internal val ownStore by lazy { OwnShopStore(dir, KeystoreSealer(AppFiles.OWN_KEY_ALIAS)) }
    internal var ownPairing: OwnPairing? = null

    // The starter wallet (`Starter*.kt`): what is kept, its Keystore key, the picture of words on show and the upgrade screen's turn.
    internal val starter by lazy { StarterRuntime(dir) }

    /** What the system's Back does on a screen of the first run: the same as the bar's arrow. Cleared by every screen that is shown. */
    internal var firstRunBack: (() -> Unit)? = null

    /** The system's Back: true when a screen of the first run took it. */
    fun systemBack(): Boolean {
        val b = firstRunBack ?: return false
        b()
        return true
    }

    /** A key ceremony was begun and not finished: the first screen resumes it instead of the welcome. */
    internal fun ceremonyPending(): Boolean = File(dir, AppFiles.CEREMONY_FILE).exists() && !File(dir, AppFiles.SETUP_FILE).exists()

    // What the first screen holds while a file picker is open. Never written anywhere (OA-01).
    private var draftText = ""

    // What runs.
    @Volatile
    private var runId = 0
    internal var visible = false
        private set
    internal var fault: SayWords? = null
    private var torBlocked: SayWords? = null
    internal var node: OwnerNode? = null
    internal var pumpFault: SayWords? = null
    internal var linkFault: SayWords? = null

    /** The tint of the network status on home as it was last drawn, so that a change of tone fades (OA-91). */
    internal var homeTint: Int? = null

    // The till (OA-16 to OA-19): its sealed token, how its last read went, and the screen's call for a read now.
    internal val posTokens = PosTokenQueue(PosTokenStore(dir, KeystoreSealer(AppFiles.POS_KEY_ALIAS))) { main.post(it) }
    internal val tillEpoch = TillEpoch()
    internal var posFault: SayWords? = null

    // The background watch (OA-80 to OA-89): its two sealed files and its on-marker.
    internal val watchStore = WatchStore(dir, KeystoreSealer(AppFiles.WATCH_KEY_ALIAS))
    private val posWake = LinkedBlockingQueue<Boolean>()

    /** Whether a till is connected, as the screen thread last saw it. The till thread reads nothing else of the screens. */
    @Volatile
    internal var posConnected = false
    private var pumpThread: Thread? = null
    private var linkThread: Thread? = null
    private var posThread: Thread? = null
    private val startLock = Any()

    // A hardware signer as key 2 (OA-70 to OA-74, `Hardware.kt`): whether the saved setup has one, read at each start,
    // and the record of the first address check, read once from its file.
    internal var key2Signer = false
    internal var firstAddress: org.openbmt.owner.FirstAddress? = null
    internal var firstAddressRead = false

    /** Set by the last ceremony screen: the register screen is the first one after the node starts (OA-71). */
    internal var registerNext = false

    // What the open screen redraws as the node reports.
    internal var tick: (() -> Unit)? = null
    private var lastTick = 0L

    // A picker that is open, and since when (the clock that counts since the phone started): the node keeps running
    // while the system's file screen is in front, for up to 60 seconds (OA-62).
    private var pickerSince: Long? = null
    private val pickerOpen: Boolean get() = pickerSince != null
    private var pickerDone: ((Uri) -> Unit)? = null
    private var pickerCancel: (() -> Unit)? = null

    internal fun nowSecs(): Long = System.currentTimeMillis() / 1000

    // ---- visible, hidden -------------------------------------------------------------------------

    fun visible() {
        visible = true
        starter.gate = true
        main.removeCallbacks(relockTimer)
        // The watcher lets go of the node's folder before the node starts (OA-84).
        WatchRunner.reclaim()
        if (pickerOpen || scan.asking) {
            if (!waitedTooLong()) return
            // OA-62: the wait counts toward the 60 seconds; the picker or dialog is given up and the setup is locked.
            endWait()
            lockNow()
            return begin()
        }
        // OA-62: after more than 60 seconds away the setup is locked again, and the start asks for the PIN.
        if (setup != null && Relock.needed(pausedAt, SystemClock.elapsedRealtime())) lockNow()
        begin()
    }

    /** OA-62: whether the owner has been in a system screen (picker, permission dialog) for more than 60 seconds. */
    private fun waitedTooLong(): Boolean =
        ExternalWait.decide(pickerSince ?: scan.askedAt, SystemClock.elapsedRealtime()) == WaitOutcome.Lock

    /** Gives up the picker or permission dialog that was waited on: its result is dropped when it comes. */
    private fun endWait() {
        pickerSince = null
        pickerDone = null
        pickerCancel = null
        scan.stop()
    }

    fun hidden() {
        val wasVisible = visible
        visible = false
        if (pickerOpen || scan.asking) return
        // SS-11: a picture of words is cleared and a setup or restore that is open is dropped when the app goes out of view.
        starterLeave()
        scan.stop()
        stopTicker()
        val toWatch = handOverContents()
        stopNode()
        // The node has let go of its folder: the watcher takes the wallet, if the setting is on (OA-84). The
        // watcher's own lease on the built-in Tor, taken when its service started, keeps Tor up at the same port.
        if (toWatch) WatchRunner.handOver()
        releaseTor()
        if (wasVisible) {
            pausedAt = SystemClock.elapsedRealtime()
            main.postDelayed(relockTimer, Relock.AFTER_MS + 1_000)
        }
    }

    /**
     * OA-90: a change between light and dark draws the screen again, in place. With the node up that is the home
     * screen; locked, the unlock screen (the PIN is still asked for); with no setup saved, the first screen. Nothing
     * is drawn while a picker or the camera is open, while the app is out of view, or while a key ceremony is in
     * progress, whose screens are not drawn from state; the next screen is drawn in the new colours.
     */
    fun redraw() {
        if (!visible || pickerOpen || scan.asking || ceremony != null) return
        when {
            node != null || setup != null -> render()
            File(dir, AppFiles.SETUP_FILE).exists() -> openStored()
            else -> showEntry()
        }
    }

    /**
     * With the setting on and the node running, writes what the watcher starts from (the counters it follows now,
     * with the watch key's secret) and says so. Nothing else of the node goes to the watcher (OA-81).
     */
    private fun handOverContents(): Boolean {
        val n = node ?: return false
        if (!watchStore.enabled()) return false
        val secret = try {
            watchStore.loadSecret()
        } catch (_: IOException) {
            null
        } catch (_: GeneralSecurityException) {
            null
        } ?: return false
        return try {
            watchStore.saveContents(n.watchExport(secret))
            true
        } catch (_: OwnerException) {
            false
        } catch (_: IOException) {
            false
        } catch (_: GeneralSecurityException) {
            false
        } finally {
            secret.fill(0)
        }
    }

    internal fun begin() {
        fault = null
        torBlocked = null
        ceremony = null
        stopNode()
        // A wipe that was begun and not finished is finished first (OA-64).
        if (tries.read(nowSecs()).wrong >= Tries.WIPE_AT) return wipe()
        val setupStatus = try {
            config = configFile.load()
            if (config == null && ownStore.hasWallet()) {
                // A shop with its own wallet has no setup text to carry the config: it starts from the defaults.
                configFile.save(AppConfig.fresh())
                config = configFile.load()
            }
            SetupStatus(File(dir, AppFiles.SETUP_FILE).exists(), config != null, ownStore.hasWallet())
        } catch (e: IOException) {
            fault = AppWords.fileNotRead(e.message)
            return render()
        }
        if (!setupStatus.complete) return showEntry()
        key2Signer = File(dir, AppFiles.KEY2_SIGNER_FILE).exists()
        firstAddressRead = false
        // A ceremony record left behind by a save that did not finish deleting it: the setup is saved, so it goes (OA-29).
        File(dir, AppFiles.CEREMONY_FILE).delete()
        // Unlocked within the last minute: the setup is in memory, and the node starts from it. Otherwise the stored
        // setup is read and the PIN is asked for before anything else (OA-62, `Lock.kt`).
        if (setup != null) return startNode()
        // A shop with its own wallet keeps no setup text: its node starts from the wallet and the owner root (OA-107).
        if (!File(dir, AppFiles.SETUP_FILE).exists()) return startNode()
        openStored()
    }

    internal fun render() {
        val status = SetupStatus(File(dir, AppFiles.SETUP_FILE).exists(), config != null, ownStore.hasWallet())
        when (val s = ScreenModel.pick(fault, status, torBlocked, node != null, ceremonyPending())) {
            is Screen.Fault -> showMessage(s.words, "Try again") { begin() }
            is Screen.Welcome -> showFirstRun()
            is Screen.FirstStart -> showFirstStart(null)
            is Screen.TorBlocked -> showMessage(s.words, if (s.words === AppWords.torFailed) "Try again" else "Check again") { begin() }
            is Screen.Starting -> waiting(AppWords.starting)
            is Screen.Home -> showHome()
        }
    }

    // ---- running the node ----------------------------------------------------------------------------

    // ---- the built-in Tor (OA-97 to OA-100) ---------------------------------------------------------------

    private val torLock = Any()
    private var torLease: TorLease? = null

    /** The lease on the built-in Tor, taken now if this controller has none or its run has ended. */
    private fun ensureTor(): TorLease = synchronized(torLock) {
        torLease?.takeIf { !it.dead } ?: run {
            torLease?.close()
            TorRunner.acquire(dir).also { torLease = it }
        }
    }

    /** Lets go of the built-in Tor; it shuts down when nothing else (the watcher) holds it (OA-97). */
    internal fun releaseTor() {
        val l = synchronized(torLock) { torLease.also { torLease = null } }
        l?.close()
    }

    /**
     * The `host:port` of the proxy everything is sent through now: Orbot's address, or the built-in Tor's port when
     * it has a circuit. Empty when the built-in Tor is not up, which no probe or fetch accepts (OA-05).
     */
    internal fun proxyNow(): String {
        val cfg = config ?: return AppConfig.DEFAULT_PROXY
        if (cfg.tor == TorChoice.Orbot) return cfg.proxy
        val port = synchronized(torLock) { torLease?.port() } ?: return ""
        return "127.0.0.1:$port"
    }

    /** True when the app's own Tor has to be up for the next connection. */
    private val builtIn: Boolean get() = config?.tor == TorChoice.BuiltIn

    /** Turns to the other choice of Tor (OA-99): saved first, and the app starts again with it. */
    internal fun chooseTor(choice: TorChoice, proxy: String, onRefused: (SayWords) -> Unit) {
        val old = config ?: return
        val next = AppConfig(proxy, choice)
        if (next.tor == old.tor && next.proxy == old.proxy) return begin()
        async({ configFile.save(next) }) { r ->
            val e = r.exceptionOrNull()
            when (e) {
                null -> {
                    config = next
                    // The watcher starts again from the new config, and the old run of Tor is let go of.
                    WatchService.stop(act)
                    if (choice == TorChoice.Orbot) releaseTor()
                    begin()
                }
                is IOException -> onRefused(AppWords.fileNotSaved(e.message))
                else -> throw e
            }
        }
    }

    internal fun startNode() {
        // A node is never started while the app is hidden: `hidden()` has already run (OA-62).
        if (!visible) return
        val id = ++runId
        val s = setup
        if (s == null && !ownStore.hasWallet()) return
        val cfg = config ?: return
        waiting(if (cfg.tor == TorChoice.BuiltIn) AppWords.torStarting else AppWords.starting)
        val thread = Thread {
            try {
                // No Tor, no node: nothing is sent around the proxy (OA-05, OA-100).
                val proxy: String
                if (cfg.tor == TorChoice.BuiltIn) {
                    val port = try {
                        ensureTor().awaitPort(TOR_WAIT_MS)
                    } catch (_: TorStartException) {
                        main.post {
                            if (id == runId) {
                                torBlocked = AppWords.torFailed
                                render()
                            }
                        }
                        return@Thread
                    }
                    proxy = "127.0.0.1:$port"
                } else {
                    val gate = TorGate(cfg.proxy)
                    if (!gate.available()) {
                        main.post {
                            if (id == runId) {
                                torBlocked = AppWords.orbotNotRunning
                                render()
                            }
                        }
                        return@Thread
                    }
                    proxy = gate.proxy
                }
                synchronized(startLock) {
                    if (id != runId) return@Thread
                    nodeDir.mkdirs()
                    // The secrets are handed over for this start and kept nowhere but in the sealed file (OA-01).
                    val start = if (s != null) {
                        s.startConfig(nodeDir.path, nowSecs(), proxy)
                    } else {
                        val shop = ownStore.args() ?: return@Thread
                        OwnStart.config(shop, OwnerCeremony.defaults(AppConfig.NETWORK), AppConfig.NETWORK, nowSecs(), proxy)
                    }
                    val n = OwnerNode.start(start)
                    if (id != runId) {
                        n.close()
                        return@Thread
                    }
                    main.post {
                        if (id == runId) adopt(id, n) else n.close()
                    }
                }
            } catch (e: OwnerException) {
                main.post { if (id == runId) { fault = e.sayWords(); render() } }
            } catch (e: LinkageError) {
                main.post { if (id == runId) { fault = AppWords.libraryMissing(e.message); render() } }
            }
        }
        thread.isDaemon = true
        thread.start()
    }

    private fun adopt(id: Int, n: OwnerNode) {
        node = n
        if (setup == null) starterWalletFileFollows(n)
        // The service is started while the app is on screen, which Android allows; it waits for the hand-over (OA-82).
        if (watchStore.enabled()) WatchService.start(act)
        pumpFault = null
        linkFault = null
        posFault = null
        posWake.clear()
        posConnected = try {
            n.posState().setup != null
        } catch (_: OwnerException) {
            false
        }
        val pump = Thread {
            while (runId == id) {
                try {
                    val p = n.pump(nowSecs(), 250)
                    main.post { if (id == runId) onPump(p.notes.isNotEmpty()) }
                    if (p.stopped) break
                } catch (e: OwnerException) {
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
        if (setup?.link != null) {
            val link = Thread {
                while (runId == id) {
                    try {
                        val now = nowSecs()
                        n.linkStep(now)
                        n.deliver(now)
                        main.post { if (id == runId) { linkFault = null; tick?.invoke() } }
                    } catch (e: OwnerException) {
                        main.post { if (id == runId) { linkFault = e.sayWords(); tick?.invoke() } }
                    }
                    try {
                        Thread.sleep(LINK_EVERY_MS)
                    } catch (_: InterruptedException) {
                        return@Thread
                    }
                }
            }
            link.isDaemon = true
            linkThread = link
            link.start()
        }
        if (setup?.link != null) {
            val link = object : PosLink {
                override fun request(now: Long): PosRequest = n.posRequest(now)

                override fun reply(now: Long, status: Long, body: ByteArray): PosReply = n.posReply(now, status, body)
            }
            val till = Thread {
                var lastStart: Long? = null
                while (runId == id) {
                    // Never more than one read a minute, whoever asks (OA-18).
                    val wait = PosSchedule.waitMs(lastStart, System.currentTimeMillis())
                    if (wait > 0 || !posConnected) {
                        try {
                            posWake.poll(if (wait > 0) wait else POS_IDLE_MS, TimeUnit.MILLISECONDS)
                        } catch (_: InterruptedException) {
                            return@Thread
                        }
                        continue
                    }
                    lastStart = System.currentTimeMillis()
                    val outcome = readTill(link, TorGate(proxyNow()))
                    main.post {
                        if (id == runId) {
                            posFault = (outcome as? PosOutcome.Failed)?.words
                            tick?.invoke()
                        }
                    }
                }
            }
            till.isDaemon = true
            posThread = till
            till.start()
        }
        if (registerNext && key2Signer) {
            registerNext = false
            showHardwareRegister { showHome() }
        } else {
            registerNext = false
            showHome()
        }
    }

    /** One read of the till, with the token read from its sealed file for this read only (OA-16). */
    private fun readTill(link: PosLink, gate: TorGate): PosOutcome {
        val token = try {
            posTokens.load()
        } catch (e: IOException) {
            return PosOutcome.Failed(AppWords.storageUnavailable(e.message))
        } catch (e: GeneralSecurityException) {
            return PosOutcome.Failed(AppWords.storageUnavailable(e.message))
        } ?: return PosOutcome.Failed(AppWords.posTokenMissing)
        val outcome = PosPoll.readOnce(link, HttpsPosFetcher(gate), token) { nowSecs() }
        // The words that Orbot is not running are not true of the built-in Tor.
        if (builtIn && outcome is PosOutcome.Failed && outcome.words === AppWords.orbotNotRunning) return PosOutcome.Failed(AppWords.torFailed)
        return outcome
    }

    /** The screen asks for a read now; it starts at once if the last one began more than a minute ago, and not before (OA-18). */
    internal fun readTillNow() {
        posWake.offer(true)
    }

    private fun onPump(reported: Boolean) {
        val now = System.currentTimeMillis()
        if (reported || now - lastTick >= 1000) {
            lastTick = now
            tick?.invoke()
        }
    }

    /** Stops the pump and the link thread, then the node, which lets go of its folder (OA-06). */
    internal fun stopNode() {
        runId++
        tick = null
        linkThread?.interrupt()
        posThread?.interrupt()
        linkThread?.let { try { it.join(1500) } catch (_: InterruptedException) { } }
        linkThread = null
        posThread?.let { try { it.join(1500) } catch (_: InterruptedException) { } }
        posThread = null
        pumpThread?.let { try { it.join(1500) } catch (_: InterruptedException) { } }
        pumpThread = null
        node?.let {
            try {
                it.stop()
            } catch (_: OwnerException) {
                // Already stopped.
            } catch (_: LinkageError) {
            }
        }
        node = null
    }

    // ---- pages -----------------------------------------------------------------------------------------------

    internal fun showMessage(words: SayWords, label: String = "Back", back: () -> Unit) {
        centre(words, listOf(ui.primary(label) { back() }))
    }

    internal fun <T> async(work: () -> T, done: (Result<T>) -> Unit) {
        val t = Thread {
            val r = runCatching(work)
            main.post { done(r) }
        }
        t.isDaemon = true
        t.start()
    }

    internal fun label(text: String): TextView = ui.label(text)

    /** The words of a call that refused or could not run, for the screen to show. */
    internal fun refusal(e: Throwable): SayWords = when (e) {
        is OwnerException -> e.sayWords()
        is LinkageError -> AppWords.libraryMissing(e.message)
        else -> throw e
    }

    internal fun copy(text: String) {
        act.getSystemService(ClipboardManager::class.java)?.setPrimaryClip(ClipData.newPlainText("request for the Key app", text))
    }

    // ---- first start ---------------------------------------------------------------------------------------------

    /**
     * The first screen: make a new shop wallet with the Key app, or use setup text. An unfinished ceremony is
     * resumed instead (OA-27).
     */
    internal fun showFirstStart(message: SayWords?) {
        if (File(dir, AppFiles.CEREMONY_FILE).exists() && !File(dir, AppFiles.SETUP_FILE).exists()) {
            return showCeremonyResume(message)
        }
        screen(
            "Owner", emptyList(),
            listOf(ui.primary("Make a new shop wallet") { startCeremony() }, ui.button("Use setup text") { showPasteSetup(null) }),
            words = message ?: AppWords.notSetUp, logo = true,
        )
    }

    /** The second choice: setup text of a key ceremony made elsewhere. The new setup uses the built-in Tor (OA-99). */
    private fun showPasteSetup(message: SayWords?) {
        val views = ArrayList<View>()
        val pasted = ui.field("Paste the setup text here", lines = 6, mono = true).also { it.setText(draftText) }
        views.add(ui.labelled("Setup text from the key ceremony", pasted))
        for (source in setupSources()) views.add(ui.button(source.label) {
            draftText = pasted.text.toString()
            source.begin({ text -> draftText = text; showPasteSetup(message) }, { w -> showPasteSetup(w) })
        })
        screen(
            "Use setup text", views,
            listOf(
                ui.primary("Save and start") {
                    draftText = pasted.text.toString()
                    saveSetup(draftText, { w -> showPasteSetup(w) })
                },
                ui.button("Back") { draftText = ""; showFirstStart(null) },
            ),
            back = { draftText = ""; showFirstStart(null) }, words = message ?: AppWords.setupTextNeeded,
        )
    }

    /**
     * Saves a setup text as the paste path and the ceremony both do: the text is checked for its shape, a PIN is
     * chosen and the library locks the text under it (OA-61), the locked setup is sealed and saved, then the config,
     * which is the last thing a finished setup adds. [onRefused] shows what could not be done, and [backWords] are
     * its words when the owner goes back from the PIN; [afterSaved] runs once both are saved.
     */
    internal fun saveSetup(
        text: String,
        onRefused: (SayWords) -> Unit,
        working: SayWords = AppWords.starting,
        afterSaved: () -> Unit = {},
        backWords: SayWords = AppWords.setupTextNeeded,
        keepsSignerMark: Boolean = false,
    ) {
        try {
            OwnerSetup.parse(text)
        } catch (e: SetupFormatException) {
            return onRefused(AppWords.setupNotReadable(e.message))
        }
        val bytes = text.toByteArray(Charsets.UTF_8)
        askNewPin(bytes, false, null, { bytes.fill(0); onRefused(backWords) }) { locked ->
            waiting(working)
            async({
                // The setup first, then the config: the config is the last thing a finished setup adds.
                SealedFile(dir, KeystoreSealer(AppFiles.KEY_ALIAS), AppFiles.SETUP_FILE).save(locked)
                // A setup that does not come from the ceremony says nothing of key 2: no signer is assumed (OA-72).
                if (!keepsSignerMark) {
                    File(dir, AppFiles.KEY2_SIGNER_FILE).delete()
                    File(dir, AppFiles.FIRST_ADDRESS_FILE).delete()
                }
                configFile.save(AppConfig.fresh())
                afterSaved()
            }) { r ->
                bytes.fill(0)
                when (val e = r.exceptionOrNull()) {
                    null -> {
                        // The text held secrets: nothing of it stays in the app's memory or on the clipboard (OA-01).
                        // It was just typed behind the PIN, so the start that follows does not ask for it again.
                        draftText = ""
                        ceremony = null
                        setup = try { OwnerSetup.parse(text) } catch (_: SetupFormatException) { null }
                        act.getSystemService(ClipboardManager::class.java)?.clearPrimaryClip()
                        showMessage(AppWords.setupSaved, "Start") { begin() }
                    }
                    is IOException -> onRefused(AppWords.fileNotSaved(e.message))
                    is GeneralSecurityException -> onRefused(AppWords.storageUnavailable(e.message))
                    else -> throw e
                }
            }
        }
    }

    /** The places the setup text can come from besides the field. */
    private fun setupSources(): List<TextSource> = listOf(
        object : TextSource {
            override val label = "Choose a setup file"

            override fun begin(onText: (String) -> Unit, onFailed: (SayWords) -> Unit) {
                val intent = Intent(Intent.ACTION_OPEN_DOCUMENT).addCategory(Intent.CATEGORY_OPENABLE).setType("*/*")
                openPicker(intent, { showPasteSetup(null) }) { uri ->
                    try {
                        onText(String(readBytes(uri, OwnerSetup.MAX_TEXT * 4 + 1), Charsets.UTF_8))
                    } catch (e: IOException) {
                        onFailed(AppWords.fileNotRead(e.message))
                    } catch (e: SecurityException) {
                        onFailed(AppWords.fileNotRead(e.message))
                    }
                }
            }
        },
    )

    // ---- files -----------------------------------------------------------------------------------------------------

    /** The sources of the Key app's response, in the order of [ExchangeModel.responseKinds]: the Key app's code, a file, the pasted text. */
    internal fun responseSources(pasted: () -> String, cancelled: () -> Unit): List<ResponseSource> = listOf(
        scanSource(),
        object : ResponseSource {
            override val label = "Choose the response file"

            override fun begin(onBytes: (ByteArray) -> Unit, onFailed: (SayWords) -> Unit) {
                val intent = Intent(Intent.ACTION_OPEN_DOCUMENT).addCategory(Intent.CATEGORY_OPENABLE).setType("*/*")
                openPicker(intent, cancelled) { uri ->
                    try {
                        val bytes = readBytes(uri, ApprovalText.MAX_BYTES + 1)
                        if (bytes.size > ApprovalText.MAX_BYTES) {
                            onFailed(AppWords.responseNotReadable("the file is longer than a response can be"))
                        } else {
                            onBytes(bytes)
                        }
                    } catch (e: IOException) {
                        onFailed(AppWords.fileNotRead(e.message))
                    } catch (e: SecurityException) {
                        onFailed(AppWords.fileNotRead(e.message))
                    }
                }
            }
        },
        PastedResponse(pasted),
    )

    /**
     * The Key app's code, read by the camera. The camera is asked for only when this button is pressed
     * (the permission dialog comes from [ScanFlow.begin]). The bytes of a complete `ur:bytes` code go on
     * as the other sources' bytes do; every other end of the scan is words ([ScanResult]).
     */
    private fun scanSource(): ResponseSource = object : ResponseSource {
        override val label = "Scan the Key app's code"
        override val leavesScreen = true

        override fun begin(onBytes: (ByteArray) -> Unit, onFailed: (SayWords) -> Unit) {
            fun give(r: SourceResult) = when (r) {
                is SourceResult.Bytes -> onBytes(r.bytes)
                is SourceResult.Failed -> onFailed(r.words)
            }
            scan.begin(
                show = { v -> showScan(v) },
                onBytes = { give(ScanResult.plain()) },
                onFailed = { f -> give(ScanResult.failed(f.off, f.problem, f.words)) },
                onCancel = { give(ScanResult.cancelled()) },
                ur = UrScan(ExchangeModel.RESPONSE_TYPES, ExchangeModel.WHO) { _, payload -> give(ScanResult.message(payload)) },
            )
        }
    }

    /** The answer of the system's permission dialog. */
    internal fun permission(code: Int, results: IntArray) {
        if (code == REQUEST_NOTIFY) return watchPermissionResult(results)
        if (scan.asking && waitedTooLong()) {
            // OA-62: an answer after more than 60 seconds is dropped, and the setup is locked.
            endWait()
            lockNow()
            return begin()
        }
        scan.permissionResult(code, results)
    }

    /** Asks the system for a file name and writes [bytes] to it; [done] gets the words of what happened. */
    internal fun saveFile(name: String, bytes: ByteArray, cancelled: () -> Unit, done: (SayWords) -> Unit) {
        saveBytes(name, bytes, "application/octet-stream", cancelled, { done(AppWords.requestSaved(name)) }, done)
    }

    /** As [saveFile], with the type of the file, and [saved] and [failed] apart: the caller says what a save means. */
    internal fun saveBytes(
        name: String,
        bytes: ByteArray,
        mime: String,
        cancelled: () -> Unit,
        saved: () -> Unit,
        failed: (SayWords) -> Unit,
    ) {
        val intent = Intent(Intent.ACTION_CREATE_DOCUMENT).addCategory(Intent.CATEGORY_OPENABLE)
            .setType(mime).putExtra(Intent.EXTRA_TITLE, name)
        openPicker(intent, cancelled) { uri ->
            try {
                val out = act.contentResolver.openOutputStream(uri, "wt") ?: throw IOException("the file could not be opened")
                out.use { it.write(bytes) }
                saved()
            } catch (e: IOException) {
                failed(AppWords.fileNotSaved(e.message))
            } catch (e: SecurityException) {
                failed(AppWords.fileNotSaved(e.message))
            }
        }
    }

    /** Asks the system for a file and gives at most [limit] bytes of it; a longer file gives [limit] bytes, for the caller to refuse. */
    internal fun pickBytes(limit: Int, cancelled: () -> Unit, onBytes: (ByteArray) -> Unit, failed: (SayWords) -> Unit) {
        val intent = Intent(Intent.ACTION_OPEN_DOCUMENT).addCategory(Intent.CATEGORY_OPENABLE).setType("*/*")
        openPicker(intent, cancelled) { uri ->
            try {
                onBytes(readBytes(uri, limit))
            } catch (e: IOException) {
                failed(AppWords.fileNotRead(e.message))
            } catch (e: SecurityException) {
                failed(AppWords.fileNotRead(e.message))
            }
        }
    }

    @Suppress("DEPRECATION")
    private fun openPicker(intent: Intent, cancelled: () -> Unit, done: (Uri) -> Unit) {
        pickerSince = SystemClock.elapsedRealtime()
        pickerDone = done
        pickerCancel = cancelled
        try {
            act.startActivityForResult(intent, REQUEST_PICK)
        } catch (e: RuntimeException) {
            pickerSince = null
            pickerDone = null
            pickerCancel = null
            showMessage(AppWords.fileNotRead(e.message)) { cancelled() }
        }
    }

    fun picked(code: Int, result: Int, data: Intent?) {
        if (code == REQUEST_CREDENTIAL) return credentialResult(result)
        if (code != REQUEST_PICK) return
        // A result with no wait noted was already dealt with by the resume (OA-62).
        val since = pickerSince ?: return
        val done = pickerDone
        val cancelled = pickerCancel
        pickerSince = null
        pickerDone = null
        pickerCancel = null
        if (ExternalWait.decide(since, SystemClock.elapsedRealtime()) == WaitOutcome.Lock) {
            // More than 60 seconds in the picker: the result, picked or cancelled, is dropped unread and the setup locks.
            lockNow()
            return begin()
        }
        val uri = data?.data
        if (result != android.app.Activity.RESULT_OK || uri == null || done == null) {
            if (cancelled != null) cancelled() else begin()
            return
        }
        done(uri)
    }

    // The phone's own PIN screen (SS-21, SS-42 on Android 10): the node keeps running while it is in front, as with a picker.
    private var credentialDone: ((Boolean) -> Unit)? = null

    internal fun startCredential(intent: Intent, done: (Boolean) -> Unit) {
        credentialDone = done
        pickerSince = SystemClock.elapsedRealtime()
        @Suppress("DEPRECATION")
        act.startActivityForResult(intent, REQUEST_CREDENTIAL)
    }

    private fun credentialResult(result: Int) {
        pickerSince = null
        val done = credentialDone
        credentialDone = null
        if (done != null) done(result == android.app.Activity.RESULT_OK) else begin()
    }

    /** At most [limit] bytes of the file at [uri]; a longer file gives [limit] bytes, so that the caller can refuse it. */
    private fun readBytes(uri: Uri, limit: Int): ByteArray {
        val input = act.contentResolver.openInputStream(uri) ?: throw IOException("the file could not be opened")
        input.use {
            val out = ByteArrayOutputStream()
            val buf = ByteArray(8192)
            while (out.size() < limit) {
                val n = it.read(buf, 0, minOf(buf.size, limit - out.size()))
                if (n < 0) break
                out.write(buf, 0, n)
            }
            return out.toByteArray()
        }
    }

    companion object {
        const val VERSION = "0.1.0"
        const val REQUEST_PICK = 1
        const val REQUEST_CREDENTIAL = 2

        /** The request code of the notification permission's dialog (OA-83). */
        const val REQUEST_NOTIFY = 8001
        const val LINK_EVERY_MS = 5_000L

        /** How long the app waits for the built-in Tor to have a circuit: a little over the minute the words promise (OA-100). */
        const val TOR_WAIT_MS = 90_000L

        /** How long the till thread waits before it looks again at whether a till is connected. */
        const val POS_IDLE_MS = 5_000L
    }
}

/** Where the setup text comes from, other than the text field of the first screen. */
interface TextSource {
    /** What the button for this source says. */
    val label: String

    fun begin(onText: (String) -> Unit, onFailed: (SayWords) -> Unit)
}
