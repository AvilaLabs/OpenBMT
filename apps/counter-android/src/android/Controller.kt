package org.openbmt.counter.app

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Intent
import android.net.Uri
import android.os.Handler
import android.os.Looper
import android.text.Editable
import android.text.TextWatcher
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
import org.openbmt.app.Format
import org.openbmt.app.KeystoreSealer
import org.openbmt.app.ScanFlow
import org.openbmt.app.SayWords
import org.openbmt.app.SealedFile
import org.openbmt.app.Ui
import org.openbmt.counter.Actor
import org.openbmt.counter.ActorRole
import org.openbmt.counter.CounterException
import org.openbmt.counter.CounterNode
import org.openbmt.counter.Payment
import org.openbmt.counter.RateSource
import org.openbmt.counter.Rates
import org.openbmt.counter.RefundAsk
import org.openbmt.counter.RefundState
import org.openbmt.counter.SaleState
import org.openbmt.counter.Shop

/**
 * The screens and what they do. The node runs while the activity is visible:
 * [visible] starts it and [hidden] stops it, releasing its folder. One
 * background thread pumps the node (`pump(250)`) and posts what it reports to
 * the screen thread; every other call into the library is made on the screen
 * thread, except the start, which runs on a worker thread before the pump
 * begins. The price is fetched by one worker thread for each source on the
 * schedule of RT-40 to RT-45, and the bytes go to the library.
 */
class Controller(private val act: MainActivity) {
    private val ui = Ui(act)
    private val scan = ScanFlow(act, ui)
    private val main = Handler(Looper.getMainLooper())
    private val dir: File = act.noBackupFilesDir
    private val nodeDir = File(dir, "node")
    private val pin = PinStore(dir, { nowSecs() })
    private val configFile = ConfigFile(dir)

    // What is saved, as read at the last start.
    private var pairing: Pairing? = null
    private var config: AppConfig? = null

    // What the first screen holds while a file picker is open.
    private var draftText = ""
    private var draftName = ""
    private var draftProxy = AppConfig.DEFAULT_PROXY

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
    private val workers = ArrayList<Thread>()
    private val startLock = Any()

    // What the open screen redraws as the node reports.
    private var tick: (() -> Unit)? = null
    private var lastTick = 0L

    // What this run knows of its sales: the amount in the shop's currency of each, by index.
    private val amounts = HashMap<Long, Long>()

    private var pickerOpen = false
    private var pickerDone: ((Uri) -> Unit)? = null

    private fun nowSecs(): Long = System.currentTimeMillis() / 1000

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
        if (!setup.complete) return showFirstStart(null)
        try {
            val text = SealedFile(dir, KeystoreSealer(KEY_ALIAS), PAIRING_FILE).load()
                ?: throw IOException("the pairing is missing")
            pairing = Pairing.parse(String(text, Charsets.UTF_8))
        } catch (e: PairingFormatException) {
            fault = AppWords.pairingNotReadable(e.message)
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
        val setup = SetupStatus(File(dir, PAIRING_FILE).exists(), pin.isSet(), config != null)
        when (val s = ScreenModel.pick(fault, setup, torBlocked, node != null)) {
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
        val p = pairing ?: return
        val cfg = config ?: return
        val thread = Thread {
            try {
                val sources = Rates.sources()
                val settings = Shop.settings(p.settings)
                val gate = TorGate(settings.torOn, cfg.proxy)
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
                    val n = CounterNode.start(p.startConfig(nodeDir.path, nowSecs(), gate.proxy))
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
        showHome()
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
        for (w in workers) w.interrupt()
        workers.clear()
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
        page(listOf(ui.words(words), ui.button(label) { back() }))
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
        views.add(label("Pairing text from the owner's phone"))
        val pasted = ui.field("Paste the pairing text here", lines = 6).also { it.setText(draftText) }
        views.add(pasted)
        for (source in sources()) views.add(ui.button(source.label) {
            source.begin({ text -> draftText = text; showFirstStart(message) }, { w -> showFirstStart(w) })
        })
        val tor = Slot(act)
        views.add(tor)
        fun readTor() {
            val text = pasted.text.toString()
            draftText = text
            val words = try {
                if (text.isBlank()) null else if (Shop.settings(Pairing.parse(text).settings).torOn) AppWords.torIsOn else AppWords.torIsOff
            } catch (_: PairingFormatException) {
                null
            } catch (_: CounterException) {
                null
            } catch (_: LinkageError) {
                null
            }
            if (words == null) tor.show() else tor.show(ui.words(words))
        }
        pasted.addTextChangedListener(object : TextWatcher {
            override fun beforeTextChanged(s: CharSequence?, start: Int, count: Int, after: Int) {}

            override fun onTextChanged(s: CharSequence?, start: Int, before: Int, count: Int) {}

            override fun afterTextChanged(s: Editable?) = readTor()
        })
        readTor()
        views.add(label("Manager's name"))
        val name = ui.field("Name of the manager").also { it.setText(draftName) }
        views.add(name)
        views.add(label("Manager's PIN, 6 to 12 digits"))
        val pin1 = pinField(act, "PIN")
        val pin2 = pinField(act, "PIN again")
        views.add(pin1)
        views.add(pin2)
        views.add(label("Address of Orbot (used only when the shop's settings turn Tor on)"))
        val proxy = ui.field("host:port").also { it.setText(draftProxy) }
        views.add(proxy)
        val save = ui.button("Save and start") {
            draftName = name.text.toString()
            draftProxy = proxy.text.toString()
            draftText = pasted.text.toString()
            saveFirstStart(draftText, draftName.trim(), pin1.text.toString(), pin2.text.toString(), draftProxy.trim())
        }
        views.add(save)
        page(views)
    }

    private fun saveFirstStart(text: String, name: String, pin1: String, pin2: String, proxy: String) {
        val p = try {
            Pairing.parse(text)
        } catch (e: PairingFormatException) {
            return showFirstStart(AppWords.pairingNotReadable(e.message))
        }
        try {
            Shop.settings(p.settings)
        } catch (e: CounterException) {
            return showFirstStart(e.sayWords())
        } catch (e: LinkageError) {
            return showFirstStart(AppWords.libraryMissing(e.message))
        }
        if (!AppConfig.validName(name)) return showFirstStart(AppWords.nameNotAccepted)
        if (!PinStore.validPin(pin1) || pin1 != pin2) return showFirstStart(AppWords.pinNotAccepted)
        if (!HostPort.valid(proxy)) return showFirstStart(AppWords.torAddressNotAccepted)
        page(listOf(ui.words(AppWords.starting)))
        async({
            // Pairing first, then the PIN, then the config: the config is the last thing a finished setup adds.
            SealedFile(dir, KeystoreSealer(KEY_ALIAS), PAIRING_FILE).save(text.toByteArray(Charsets.UTF_8))
            pin.set(pin1)
            configFile.save(AppConfig(proxy, name, ""))
        }) { r ->
            val e = r.exceptionOrNull()
            when (e) {
                null -> {
                    draftText = ""
                    draftName = ""
                    showMessage(AppWords.pairingSaved, "Start") { begin() }
                }
                is IOException -> showFirstStart(AppWords.fileNotSaved(e.message))
                is GeneralSecurityException -> showFirstStart(AppWords.storageUnavailable(e.message))
                else -> throw e
            }
        }
    }

    /** The places the pairing text can come from besides the field. */
    private fun sources(): List<PairingSource> = listOf(
        object : PairingSource {
            override val label = "Choose a pairing file"

            override fun begin(onText: (String) -> Unit, onFailed: (SayWords) -> Unit) {
                val intent = Intent(Intent.ACTION_OPEN_DOCUMENT).addCategory(Intent.CATEGORY_OPENABLE).setType("*/*")
                openPicker(intent) { uri ->
                    try {
                        onText(readText(uri))
                    } catch (e: IOException) {
                        onFailed(AppWords.fileNotRead(e.message))
                    } catch (e: SecurityException) {
                        onFailed(AppWords.fileNotRead(e.message))
                    }
                }
            }
        },
        object : PairingSource {
            override val label = "Scan the pairing code"

            override fun begin(onText: (String) -> Unit, onFailed: (SayWords) -> Unit) {
                scan.begin(
                    show = { v -> tick = null; act.show(v) },
                    onBytes = { bytes -> onText(String(bytes, Charsets.UTF_8)) },
                    onFailed = { f -> onFailed(if (f.off) AppWords.cameraOff else AppWords.cameraNotStarted(f.problem)) },
                    onCancel = { showFirstStart(null) },
                )
            }
        },
    )

    /** The answer of the system's permission dialog. */
    fun permission(code: Int, results: IntArray) = scan.permissionResult(code, results)

    @Suppress("DEPRECATION")
    private fun openPicker(intent: Intent, done: (Uri) -> Unit) {
        pickerOpen = true
        pickerDone = done
        try {
            act.startActivityForResult(intent, REQUEST_PICK)
        } catch (e: RuntimeException) {
            pickerOpen = false
            pickerDone = null
            showFirstStart(AppWords.fileNotRead(e.message))
        }
    }

    fun picked(code: Int, result: Int, data: Intent?) {
        if (code != REQUEST_PICK) return
        pickerOpen = false
        val done = pickerDone
        pickerDone = null
        val uri = data?.data
        if (result != android.app.Activity.RESULT_OK || uri == null || done == null) return showFirstStart(null)
        done(uri)
    }

    /** At most one character more than a pairing text may hold, so that the parser can refuse a file that is too long. */
    private fun readText(uri: Uri): String {
        val input = act.contentResolver.openInputStream(uri) ?: throw IOException("the file could not be opened")
        input.use {
            val out = java.io.ByteArrayOutputStream()
            val buf = ByteArray(8192)
            val limit = Pairing.MAX_TEXT * 4 + 1
            while (out.size() < limit) {
                val n = it.read(buf, 0, minOf(buf.size, limit - out.size()))
                if (n < 0) break
                out.write(buf, 0, n)
            }
            return String(out.toByteArray(), Charsets.UTF_8)
        }
    }

    // ---- home ----------------------------------------------------------------------------------------------------------

    private fun showHome() {
        val n = node ?: return render()
        val cfg = config ?: return render()
        val health = Slot(act)
        val priceSlot = Slot(act)
        val views = ArrayList<View>()
        views.add(label("Is Bitcoin ready?"))
        views.add(health)
        views.add(label("Price"))
        views.add(priceSlot)
        views.add(ui.button("New sale") { showNewSale() })
        views.add(ui.button("Sales") { showSales() })
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
        views.add(ui.button("About") { showAbout() })
        page(views)
        tick = {
            health.show(ui.words(healthWords(n)))
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
        views.add(ui.button("Open the sale") {
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
            val o = n.open(nowSecs(), amountMinor, m.code, rate.rate, rate.sourceLabel, rate.sourceFetchedAt)
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
        val views = ArrayList<View>()
        val msg = Slot(act)
        if (notice != null) msg.show(ui.words(notice))
        views.add(msg)
        val maxPx = (act.resources.displayMetrics.heightPixels * 0.6f).toInt()
        var qr: QrView? = first?.let { QrView(act, it, maxPx) }
        qr?.let { views.add(it) }
        val amount = ui.text("", 20f, bold = true)
        val received = ui.text("", 16f)
        val state = Slot(act)
        val buttons = Slot(act)
        views.add(amount)
        views.add(received)
        views.add(state)
        views.add(buttons)
        var uri: String? = first?.copyUri
        val slotForQr = LinearLayout(act).apply { orientation = LinearLayout.VERTICAL }
        if (qr == null) views.add(1, slotForQr)
        views.add(ui.button("Copy the request") {
            val u = uri
            if (u != null) {
                act.getSystemService(ClipboardManager::class.java)?.setPrimaryClip(ClipData.newPlainText("payment request", u))
                msg.show(ui.words(AppWords.requestCopied))
            }
        })
        views.add(ui.button("Details and refunds") { showSaleDetails(index) })
        views.add(ui.button("Done") { showHome() })
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
                amount.text = SaleModel.amountLine(m, amounts[index], s.needsSats)
                received.text = SaleModel.receivedLine(s.receivedSats, s.needsSats)
                state.show(ui.words(s.words.say()), *s.flags.map { f -> ui.words(f.say()) }.toTypedArray())
                if (shown != s.state) {
                    shown = s.state
                    val b = SaleModel.buttons(s.state)
                    val list = ArrayList<View>()
                    if (b.requote) list.add(ui.button("Requote") { requote(index, msg) })
                    if (b.cancel) list.add(ui.button("Cancel the sale") { cancelSale(index, msg) })
                    if (b.override) list.add(ui.button("Accept by hand (manager)") { showOverride(index) })
                    buttons.show(*list.toTypedArray())
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
            n.cancel(nowSecs(), index)
            tick?.invoke()
        } catch (e: CounterException) {
            msg.show(ui.words(e.sayWords()))
        }
    }

    // ---- who acts (CR-21, CR-67 to CR-69) ----------------------------------------------------------------------------------------

    /** The choice of who acts, with the PIN field a manager types into. */
    private class ActorBlock(val group: RadioGroup, val pin: EditText)

    private fun actorBlock(views: MutableList<View>): ActorBlock {
        views.add(label("Who is asking"))
        val group = ui.choice(listOf("Cashier", "Manager (needs the PIN)"))
        views.add(group)
        val pinInput = pinField(act, "Manager's PIN")
        views.add(pinInput)
        return ActorBlock(group, pinInput)
    }

    /** The person who acts: the cashier by name, or the manager when the PIN is right (CR-21). */
    private fun resolveActor(block: ActorBlock, done: (Actor) -> Unit, failed: (SayWords) -> Unit) {
        val cfg = config ?: return render()
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
        val msg = Slot(act)
        val pinInput = pinField(act, "Manager's PIN")
        page(
            listOf(
                ui.words(AppWords.managerNeeded),
                pinInput,
                msg,
                ui.button("Accept this sale") {
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
        val views = ArrayList<View>()
        notice?.let { views.add(ui.words(it)) }
        views.add(label("Sale ${index + 1}"))
        try {
            val s = n.judge(nowSecs(), index)
            views.add(ui.text(SaleModel.amountLine(m, amounts[index], s.needsSats), 18f, bold = true))
            views.add(ui.text(SaleModel.receivedLine(s.receivedSats, s.needsSats), 16f))
            views.add(ui.words(s.words.say()))
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
                    views.add(ui.button("Withdraw this request") { showWithdraw(index, number) })
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
        views.add(ui.button("Ask for the refund") {
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
        views.add(ui.button("Save the cash refund") {
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
        val views = ArrayList<View>()
        val msg = Slot(act)
        views.add(msg)
        views.add(ui.text("Withdraw refund request $number of sale ${index + 1}?", 18f, bold = true))
        val actor = actorBlock(views)
        views.add(ui.button("Withdraw it") {
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

    // ---- about ----------------------------------------------------------------------------------------------------------------------

    private fun showAbout() {
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
                    "Reading the pairing code with the camera (pairing is pasted text or a file); a lock screen of the app's own; " +
                        "a list of cashiers and managers with their own PINs (there is one manager PIN for this phone); " +
                        "end-of-day totals and receipts; changing the shop's settings after pairing.",
                    16f,
                ),
                label("Tor"),
                ui.text(
                    "When the shop's settings turn Tor on, every connection goes through Orbot on this phone. If Orbot is not running the counter sends nothing and says so.",
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
        const val REQUEST_PICK = 1
        private const val MAX_LISTED = 50
    }
}
