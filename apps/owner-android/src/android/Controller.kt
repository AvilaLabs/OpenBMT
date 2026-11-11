package org.openbmt.owner.app

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Intent
import android.net.Uri
import android.os.Handler
import android.os.Looper
import android.view.View
import android.widget.EditText
import android.widget.TextView
import java.io.ByteArrayOutputStream
import java.io.File
import java.io.IOException
import java.security.GeneralSecurityException
import java.util.concurrent.LinkedBlockingQueue
import java.util.concurrent.TimeUnit
import org.openbmt.app.KeystoreSealer
import org.openbmt.app.SayWords
import org.openbmt.app.ScanFlow
import org.openbmt.app.SealedFile
import org.openbmt.app.Ui
import org.openbmt.app.UrScan
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

    // The key ceremony in progress: held in memory for the screens, and sealed on disk between steps (OA-26).
    internal var ceremony: CeremonyDraft? = null

    // What the first screen holds while a file picker is open. Never written anywhere (OA-01).
    private var draftText = ""
    private var draftProxy = AppConfig.DEFAULT_PROXY

    // What runs.
    @Volatile
    private var runId = 0
    private var visible = false
    private var fault: SayWords? = null
    private var torBlocked: SayWords? = null
    internal var node: OwnerNode? = null
    internal var pumpFault: SayWords? = null
    internal var linkFault: SayWords? = null

    // The till (OA-16 to OA-19): its sealed token, how its last read went, and the screen's call for a read now.
    internal val posTokens = PosTokenStore(dir, KeystoreSealer(AppFiles.POS_KEY_ALIAS))
    internal var posFault: SayWords? = null
    private val posWake = LinkedBlockingQueue<Boolean>()

    /** Whether a till is connected, as the screen thread last saw it. The till thread reads nothing else of the screens. */
    @Volatile
    internal var posConnected = false
    private var pumpThread: Thread? = null
    private var linkThread: Thread? = null
    private var posThread: Thread? = null
    private val startLock = Any()

    // What the open screen redraws as the node reports.
    internal var tick: (() -> Unit)? = null
    private var lastTick = 0L

    // A picker that is open: the node keeps running while the system's file screen is in front.
    private var pickerOpen = false
    private var pickerDone: ((Uri) -> Unit)? = null
    private var pickerCancel: (() -> Unit)? = null

    internal fun nowSecs(): Long = System.currentTimeMillis() / 1000

    // ---- visible, hidden -------------------------------------------------------------------------

    fun visible() {
        visible = true
        if (pickerOpen || scan.asking) return
        begin()
    }

    fun hidden() {
        visible = false
        if (pickerOpen || scan.asking) return
        scan.stop()
        stopNode()
    }

    internal fun begin() {
        fault = null
        torBlocked = null
        ceremony = null
        stopNode()
        val setupStatus = try {
            config = configFile.load()
            SetupStatus(File(dir, AppFiles.SETUP_FILE).exists(), config != null)
        } catch (e: IOException) {
            fault = AppWords.fileNotRead(e.message)
            return render()
        }
        if (!setupStatus.complete) return showFirstStart(null)
        // A ceremony record left behind by a save that did not finish deleting it: the setup is saved, so it goes (OA-29).
        File(dir, AppFiles.CEREMONY_FILE).delete()
        try {
            val text = SealedFile(dir, KeystoreSealer(AppFiles.KEY_ALIAS), AppFiles.SETUP_FILE).load()
                ?: throw IOException("the setup is missing")
            setup = OwnerSetup.parse(String(text, Charsets.UTF_8))
        } catch (e: SetupFormatException) {
            fault = AppWords.setupNotReadable(e.message)
            return render()
        } catch (e: IOException) {
            fault = AppWords.storageUnavailable(e.message)
            return render()
        } catch (e: GeneralSecurityException) {
            fault = AppWords.storageUnavailable(e.message)
            return render()
        }
        startNode()
    }

    private fun render() {
        val status = SetupStatus(File(dir, AppFiles.SETUP_FILE).exists(), config != null)
        when (val s = ScreenModel.pick(fault, status, torBlocked, node != null)) {
            is Screen.Fault -> showMessage(s.words, "Try again") { begin() }
            is Screen.FirstStart -> showFirstStart(null)
            is Screen.TorBlocked -> showMessage(s.words, "Check again") { begin() }
            is Screen.Starting -> page(listOf(ui.words(AppWords.starting)))
            is Screen.Home -> showHome()
        }
    }

    // ---- running the node ----------------------------------------------------------------------------

    private fun startNode() {
        val id = ++runId
        page(listOf(ui.words(AppWords.starting)))
        val s = setup ?: return
        val cfg = config ?: return
        val thread = Thread {
            try {
                // No Orbot, no node: nothing is sent around the proxy (OA-05).
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
                synchronized(startLock) {
                    if (id != runId) return@Thread
                    nodeDir.mkdirs()
                    // The secrets are handed over for this start and kept nowhere but in the sealed file (OA-01).
                    val n = OwnerNode.start(s.startConfig(nodeDir.path, nowSecs(), gate.proxy))
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
            val gate = TorGate(config?.proxy ?: AppConfig.DEFAULT_PROXY)
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
                    val outcome = readTill(link, gate)
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
        showHome()
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
        return PosPoll.readOnce(link, HttpsPosFetcher(gate), token) { nowSecs() }
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
    private fun stopNode() {
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

    internal fun page(views: List<View>) {
        tick = null
        act.show(ui.page(views))
    }

    internal fun showMessage(words: SayWords, label: String = "Back", back: () -> Unit) {
        page(listOf(ui.words(words), ui.button(label) { back() }))
    }

    internal fun <T> async(work: () -> T, done: (Result<T>) -> Unit) {
        val t = Thread {
            val r = runCatching(work)
            main.post { done(r) }
        }
        t.isDaemon = true
        t.start()
    }

    internal fun label(text: String): TextView = ui.text(text, 14f, bold = true)

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
        val views = ArrayList<View>()
        views.add(ui.words(message ?: AppWords.notSetUp))
        views.add(ui.button("Make a new shop wallet") { startCeremony() })
        views.add(ui.button("I have setup text") { showPasteSetup(null) })
        page(views)
    }

    /** The second choice: setup text of a key ceremony made elsewhere, with the address of Orbot. */
    private fun showPasteSetup(message: SayWords?) {
        val views = ArrayList<View>()
        views.add(ui.words(message ?: AppWords.setupTextNeeded))
        views.add(label("Setup text from the key ceremony"))
        val pasted = ui.field("Paste the setup text here", lines = 6).also { it.setText(draftText) }
        views.add(pasted)
        for (source in setupSources()) views.add(ui.button(source.label) {
            draftText = pasted.text.toString()
            source.begin({ text -> draftText = text; showPasteSetup(message) }, { w -> showPasteSetup(w) })
        })
        val proxy = orbotViews(views, draftProxy)
        views.add(ui.button("Save and start") {
            draftProxy = proxy.text.toString()
            draftText = pasted.text.toString()
            saveSetup(draftText, draftProxy.trim(), { w -> showPasteSetup(w) })
        })
        views.add(ui.button("Back") { draftText = ""; showFirstStart(null) })
        page(views)
    }

    /** The words that everything goes through Tor, and the field for Orbot's address, as the paste screen and the ceremony's last screen show them. */
    internal fun orbotViews(views: MutableList<View>, initial: String): EditText {
        views.add(ui.words(AppWords.torAlwaysOn))
        views.add(label("Address of Orbot"))
        val proxy = ui.field("host:port").also { it.setText(initial) }
        views.add(proxy)
        return proxy
    }

    /**
     * Saves a setup text as the paste path and the ceremony both do: the text is checked for its shape, the setup
     * is sealed and saved, then the config, which is the last thing a finished setup adds. [onRefused] shows
     * what could not be done; [afterSaved] runs once both are saved.
     */
    internal fun saveSetup(
        text: String,
        proxy: String,
        onRefused: (SayWords) -> Unit,
        working: SayWords = AppWords.starting,
        afterSaved: () -> Unit = {},
    ) {
        try {
            OwnerSetup.parse(text)
        } catch (e: SetupFormatException) {
            return onRefused(AppWords.setupNotReadable(e.message))
        }
        if (!HostPort.valid(proxy)) return onRefused(AppWords.torAddressNotAccepted)
        page(listOf(ui.words(working)))
        async({
            // The setup first, then the config: the config is the last thing a finished setup adds.
            SealedFile(dir, KeystoreSealer(AppFiles.KEY_ALIAS), AppFiles.SETUP_FILE).save(text.toByteArray(Charsets.UTF_8))
            configFile.save(AppConfig(proxy))
            afterSaved()
        }) { r ->
            when (val e = r.exceptionOrNull()) {
                null -> {
                    // The text held secrets: nothing of it stays in the app's memory or on the clipboard (OA-01).
                    draftText = ""
                    ceremony = null
                    act.getSystemService(ClipboardManager::class.java)?.clearPrimaryClip()
                    showMessage(AppWords.setupSaved, "Start") { begin() }
                }
                is IOException -> onRefused(AppWords.fileNotSaved(e.message))
                is GeneralSecurityException -> onRefused(AppWords.storageUnavailable(e.message))
                else -> throw e
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
                show = { v ->
                    tick = null
                    act.show(v)
                },
                onBytes = { give(ScanResult.plain()) },
                onFailed = { f -> give(ScanResult.failed(f.off, f.problem, f.words)) },
                onCancel = { give(ScanResult.cancelled()) },
                ur = UrScan(ExchangeModel.RESPONSE_TYPES, ExchangeModel.WHO) { _, payload -> give(ScanResult.message(payload)) },
            )
        }
    }

    /** The answer of the system's permission dialog. */
    internal fun permission(code: Int, results: IntArray) = scan.permissionResult(code, results)

    /** Asks the system for a file name and writes [bytes] to it; [done] gets the words of what happened. */
    internal fun saveFile(name: String, bytes: ByteArray, cancelled: () -> Unit, done: (SayWords) -> Unit) {
        val intent = Intent(Intent.ACTION_CREATE_DOCUMENT).addCategory(Intent.CATEGORY_OPENABLE)
            .setType("application/octet-stream").putExtra(Intent.EXTRA_TITLE, name)
        openPicker(intent, cancelled) { uri ->
            try {
                val out = act.contentResolver.openOutputStream(uri, "wt") ?: throw IOException("the file could not be opened")
                out.use { it.write(bytes) }
                done(AppWords.requestSaved(name))
            } catch (e: IOException) {
                done(AppWords.fileNotSaved(e.message))
            } catch (e: SecurityException) {
                done(AppWords.fileNotSaved(e.message))
            }
        }
    }

    @Suppress("DEPRECATION")
    private fun openPicker(intent: Intent, cancelled: () -> Unit, done: (Uri) -> Unit) {
        pickerOpen = true
        pickerDone = done
        pickerCancel = cancelled
        try {
            act.startActivityForResult(intent, REQUEST_PICK)
        } catch (e: RuntimeException) {
            pickerOpen = false
            pickerDone = null
            pickerCancel = null
            showMessage(AppWords.fileNotRead(e.message)) { cancelled() }
        }
    }

    fun picked(code: Int, result: Int, data: Intent?) {
        if (code != REQUEST_PICK) return
        pickerOpen = false
        val done = pickerDone
        val cancelled = pickerCancel
        pickerDone = null
        pickerCancel = null
        val uri = data?.data
        if (result != android.app.Activity.RESULT_OK || uri == null || done == null) {
            if (cancelled != null) cancelled() else begin()
            return
        }
        done(uri)
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
        const val LINK_EVERY_MS = 5_000L

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
