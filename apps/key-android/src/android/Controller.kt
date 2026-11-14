package org.openbmt.key.app

import android.accessibilityservice.AccessibilityServiceInfo
import android.content.Intent
import android.net.Uri
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.provider.Settings
import android.text.Editable
import android.text.InputType
import android.text.TextWatcher
import android.view.View
import android.widget.Button
import android.widget.EditText
import android.view.accessibility.AccessibilityManager
import java.io.File
import java.io.IOException
import org.openbmt.app.AnimatedQrMade
import org.openbmt.app.AnimatedQrView
import org.openbmt.app.Colour
import org.openbmt.app.Feedback
import org.openbmt.app.HoldButton
import org.openbmt.app.ExternalWait
import org.openbmt.app.Format
import org.openbmt.app.KeyLevel
import org.openbmt.app.KeystoreSealer
import org.openbmt.app.LineStyle
import org.openbmt.app.PickedUri
import org.openbmt.app.accept
import org.openbmt.app.ReceiptLine
import org.openbmt.app.Relock
import org.openbmt.app.RowItem
import org.openbmt.app.ScanFlow
import org.openbmt.app.SealedFile
import org.openbmt.app.StatusItem
import org.openbmt.app.Tone
import org.openbmt.app.Tries
import org.openbmt.app.TriesFile
import org.openbmt.app.TriesRecord
import org.openbmt.app.Ui
import org.openbmt.app.UrCall
import org.openbmt.app.UrScan
import org.openbmt.app.UrText
import org.openbmt.app.UrWords
import org.openbmt.app.WaitOutcome
import org.openbmt.app.Who
import org.openbmt.app.WordsModel
import org.openbmt.key.CeilingsView
import org.openbmt.key.Decision
import org.openbmt.key.EntropySource
import org.openbmt.key.FeeCeilingsView
import org.openbmt.key.Key2TakenView
import org.openbmt.key.Key3View
import org.openbmt.key.KeyApp
import org.openbmt.key.KeyException
import org.openbmt.key.KeyNetwork
import org.openbmt.key.KeySession
import org.openbmt.key.KeyWordCount
import org.openbmt.key.LineView
import org.openbmt.key.LockStatus
import org.openbmt.key.SetupState
import org.openbmt.key.WalletView
import org.openbmt.key.Words

/**
 * The screens and what they do. Every call into the library goes through
 * [attempt], which shows the library's own words for a refusal. The screens are
 * small on purpose: this is a first build, and text comes in by paste, by a
 * file or by scanning a QR code (one code, or a code that changes, shown part by part).
 */
class Controller(private val act: MainActivity) {
    /** The Key app always draws in the dark scheme, whatever the phone's mode. */
    private val dark = DarkContext(act)
    private val ui = Ui(dark)
    private val scan = ScanFlow(act, ui)
    private var store: PersistentStore? = null
    private var level: KeyLevel? = null
    private var app: KeyApp? = null

    /** True from the resume to the pause: the work posted by a PIN step runs only then (SR-77). */
    internal var visible = false
        private set
    private var session: KeySession? = null
    private var lines: List<LineView> = emptyList()
    private var pendingAnswer: ByteArray? = null

    /** The network of the next key or wallet; the person picks it on the screens that need it. */
    private var network = KeyNetwork.Mainnet

    /**
     * Set while a system file picker is open (the screen and the session survive it for up to 60 seconds, SR-77):
     * when it was opened, on the clock that counts since the phone started.
     */
    private var pickerSince: Long? = null
    private val pickerOpen: Boolean get() = pickerSince != null

    /** Set when the screen was cleared because the app went to the background. */
    private var cleared = false

    private val answerFile: File get() = File(act.noBackupFilesDir, "last-answer.bin")

    /** The count of wrong PINs, in the same folder as the saved state (SR-75). */
    private val tries by lazy { TriesFile(act.noBackupFilesDir) }

    private val main = Handler(Looper.getMainLooper())

    /** The countdown of the unlock screen, while it is on show. */
    private var ticker: Runnable? = null

    /** When the app was paused, on the clock that counts since the phone started (SR-77); null when it was not. */
    private var pausedAt: Long? = null

    private fun clock(): Long = System.currentTimeMillis() / 1000

    // ---- start, resume, pause ---------------------------------------------------

    fun start() {
        val refusals = refusals()
        if (refusals.isNotEmpty()) return render(ScreenModel.pick(refusals, null, null))
        close()
        // A wipe that was begun and not finished is finished first (SR-76).
        if (tries.read(clock()).wrong >= Tries.WIPE_AT) return wipe()
        var fault: Words? = null
        try {
            val sealer = KeystoreSealer()
            level = sealer.level
            val s = PersistentStore(SealedFile(act.noBackupFilesDir, sealer))
            store = s
            app = KeyApp.open(s)
        } catch (e: KeyException) {
            fault = e.words ?: AppWords.fault(e)
        } catch (e: LinkageError) {
            fault = AppWords.libraryMissing(e.message)
        } catch (e: IOException) {
            fault = AppWords.keystoreUnavailable(e.message)
        } catch (e: java.security.GeneralSecurityException) {
            fault = AppWords.keystoreUnavailable(e.message)
        }
        refresh(fault)
    }

    /** SR-77: whether the person has been in a system screen (picker, permission dialog) for more than 60 seconds. */
    private fun waitedTooLong(): Boolean =
        ExternalWait.decide(pickerSince ?: scan.askedAt, SystemClock.elapsedRealtime()) == WaitOutcome.Lock

    fun resumed() {
        visible = true
        if (pickerOpen || scan.asking) {
            if (!waitedTooLong()) return
            // The wait counts toward the 60 seconds: the session is dropped and the app locks, as after any relock.
            pickerSince = null
            pendingAnswer = null
            return start()
        }
        if (refusals().isNotEmpty()) {
            dropSession()
            refresh(null)
        } else if (cleared || app == null) {
            cleared = false
            // SR-77: after more than 60 seconds away the app is locked again, whatever was on screen.
            if (app == null || Relock.needed(pausedAt, SystemClock.elapsedRealtime())) start() else refresh(null)
        }
    }

    /**
     * SR-83: a change between light and dark has nothing to draw: this app is always dark, so the screen on show
     * is already the one the state names.
     */
    fun redraw() {}

    fun paused() {
        visible = false
        if (pickerOpen || scan.asking) return
        scan.stop()
        stopTicker()
        dropSession()
        pendingAnswer = null
        act.show(View(act))
        cleared = true
        pausedAt = SystemClock.elapsedRealtime()
    }

    fun close() {
        scan.stop()
        stopTicker()
        dropSession()
        try {
            app?.close()
        } catch (_: KeyException) {
        } catch (_: LinkageError) {
        }
        app = null
    }

    private fun dropSession() {
        val s = session ?: return
        session = null
        try {
            s.close()
        } catch (_: KeyException) {
        }
    }

    /** What the system says; a setting that cannot be read counts as on. */
    private fun refusals(): List<Words> {
        fun read(f: () -> Int): Int = try { f() } catch (_: RuntimeException) { 1 }
        val r = act.contentResolver
        val manager = act.getSystemService(AccessibilityManager::class.java)
        return Guard.refusals(
            adbEnabled = read { Settings.Global.getInt(r, Settings.Global.ADB_ENABLED, 0) },
            wirelessAdbEnabled = read { Settings.Global.getInt(r, "adb_wifi_enabled", 0) },
            accessibilityEnabled = read { Settings.Secure.getInt(r, Settings.Secure.ACCESSIBILITY_ENABLED, 0) },
            accessibilityServices = read {
                manager?.getEnabledAccessibilityServiceList(AccessibilityServiceInfo.FEEDBACK_ALL_MASK)?.size ?: 1
            },
        )
    }

    /** Run [block] only while the system still allows it; otherwise show the refusal. */
    private fun allowed(block: () -> Unit) {
        if (refusals().isNotEmpty()) {
            dropSession()
            refresh(null)
        } else {
            block()
        }
    }

    // ---- choosing the screen ------------------------------------------------------

    /** Reads the library's state and shows the screen for it. */
    fun refresh(fault: Words? = null) {
        var state: SetupState? = null
        var key3: Key3View? = null
        var key2Taken: Key2TakenView? = null
        var lock: LockStatus? = null
        var problem = fault
        val a = app
        if (problem == null && a != null && refusals().isEmpty()) {
            try {
                state = a.state()
                if (state is SetupState.HasIdentity && !state.wordsWaiting) {
                    key3 = a.key3View()
                    if (!state.holdsKey2) key2Taken = a.key2TakenView()
                }
                if (state is SetupState.Empty || state is SetupState.Locked) lock = a.lockStatus()
            } catch (e: KeyException) {
                problem = e.words ?: AppWords.fault(e)
            }
        }
        render(ScreenModel.pick(refusals(), problem, state, key3, lock, key2Taken), state)
    }

    private fun render(screen: Screen, state: SetupState? = null) {
        when (screen) {
            is Screen.Refused -> showCenter(screen.words, ui.primary("Check again") { start() })
            is Screen.Fault -> showCenter(listOf(screen.words), ui.primary("Try again") { start() })
            is Screen.Unlock -> showUnlock(null)
            is Screen.SetPin -> showSetPin(screen.upgrade, null)
            is Screen.Wiped -> showWiped()
            is Screen.Setup -> showSetup()
            is Screen.ShowWords -> showKeyWords()
            is Screen.Key3 -> showKey3(null)
            is Screen.Key2 -> showKey2()
            is Screen.GiveAnswer -> showAnswer(screen.key3)
            is Screen.ComparePending -> showPending()
            is Screen.Home -> showHome(state ?: SetupState.Ready(screen.holdsKey2))
        }
    }

    // ---- the page and its parts -----------------------------------------------------

    private fun model(w: Words): WordsModel = WordsModel.of(w.say())

    /** A screen: the bar, the body that scrolls and the footer. [logo] is the bar of the screens that have no way back. */
    private fun put(
        title: String,
        body: List<View>,
        actions: List<Button> = emptyList(),
        back: (() -> Unit)? = null,
        logo: Boolean = false,
        tone: Tone? = null,
    ) {
        act.show(ui.page(ui.bar(title, back = back, logo = logo), body, actions, tone))
    }

    /** A screen that is one message: a result, a refusal or an error. A second message is shown as a notice under the first. */
    private fun showCenter(words: List<Words>, vararg actions: Button, title: String? = null, back: (() -> Unit)? = null) {
        val first = model(words[0])
        val body = ArrayList<View>()
        body.add(ui.resultPanel(first))
        for (w in words.drop(1)) body.add(ui.notice(model(w)))
        put(title ?: "Key", body, actions.toList(), back, logo = title == null, tone = ui.pal.of(first.colour))
    }

    /** A heading that is not the bar's, and the sentence under it, for a screen that has no words of the library. */
    private fun plain(heading: String, what: String?, barTitle: String): View {
        val model = WordsModel(Colour.Blue, heading, what ?: "", "", "", null, null, emptyList())
        if (what != null) return ui.titleBlock(model, barTitle)
        return ui.titleBlock(model, barTitle).also { v ->
            // No sentence: the heading alone, or nothing when it is the bar's own.
            (v as android.view.ViewGroup).removeViewAt(v.childCount - 1)
            if (heading == barTitle) v.visibility = View.GONE
        }
    }

    private fun setLabel(labelled: View, text: String) {
        ((labelled as android.view.ViewGroup).getChildAt(0) as android.widget.TextView).text = text
    }

    private fun card(view: View): View = android.widget.LinearLayout(dark).apply {
        background = android.graphics.drawable.GradientDrawable().apply {
            setColor(ui.pal.surface)
            cornerRadius = ui.px(16).toFloat()
        }
        setPadding(ui.px(16), ui.px(14), ui.px(16), ui.px(14))
        addView(view)
    }

    private fun centred(v: android.widget.TextView): android.widget.TextView = v.also { it.gravity = android.view.Gravity.CENTER }

    /** The library call [block], with the library's words on a refusal. [back] shows the screen to return to. */
    private fun <T> attempt(back: () -> Unit, block: () -> T): T? {
        try {
            return block()
        } catch (e: KeyException) {
            showMessage(e.words ?: AppWords.fault(e), back)
        } catch (e: LinkageError) {
            showMessage(AppWords.libraryMissing(e.message), back)
        }
        return null
    }

    private fun showMessage(words: Words, back: () -> Unit) {
        showCenter(listOf(words), ui.primary("Back") { back() })
    }

    /** The same, with the heading as the bar's title and a way back from the bar. */
    private fun showTitled(words: Words, title: String, back: () -> Unit) {
        showCenter(listOf(words), ui.primary("Back") { back() }, title = title, back = back)
    }

    private fun banner(): List<View> =
        if (level == KeyLevel.Software) listOf(ui.notice(model(AppWords.keyInSoftware))) else emptyList()

    private fun networkChoice(): android.widget.RadioGroup {
        val names = listOf("Main bitcoin network", "Test network", "Signet", "Regtest")
        val group = ui.chips(names, network.ordinal)
        group.setOnCheckedChangeListener { _, _ -> network = KeyNetwork.values()[ui.chosen(group).coerceAtLeast(0)] }
        return group
    }

    // ---- the PIN lock (SR-71 to SR-77) -------------------------------------------------

    /** The bar of the unlock screen: the heading of its own words. */
    private val UNLOCK_TITLE = AppWords.state(SetupState.Locked).say().heading

    private fun stopTicker() {
        ticker?.let { main.removeCallbacks(it) }
        ticker = null
    }

    /** A field for a PIN or a passphrase: its letters are hidden, and no keyboard may learn or suggest them. */
    private fun secret(hint: String, numeric: Boolean): EditText = ui.field(hint).also {
        it.inputType = if (numeric) {
            InputType.TYPE_CLASS_NUMBER or InputType.TYPE_NUMBER_VARIATION_PASSWORD
        } else {
            InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_FLAG_NO_SUGGESTIONS or InputType.TYPE_TEXT_VARIATION_PASSWORD
        }
        // A password type makes the font monospace; the system sans again.
        ui.style(it)
    }

    /** Set when the last try was a wrong PIN: the screen that shows it again shakes its field once. */
    private var wrongPin = false

    private fun shakeIfWrong(field: View) {
        if (!wrongPin) return
        wrongPin = false
        field.post { Feedback.shake(field) }
    }

    /**
     * The page is drawn first, then [block] runs a moment later on the main thread: the key from the PIN
     * takes about a second, and the person sees that the app is working and not stuck (SR-77). When the app has
     * been hidden meanwhile, [block] does not run and [skipped] does: nothing is tried or unlocked, and the return
     * to the app shows the unlock screen again (SR-77).
     */
    private fun slow(skipped: () -> Unit = {}, block: () -> Unit) {
        put("Key", listOf(ui.working(model(AppWords.working))), logo = true)
        main.postDelayed({ if (app != null) { if (ScreenModel.runsDelayed(visible)) block() else skipped() } }, 100)
    }

    /** SR-72: a PIN for a new or restored key, or for a key an older build saved without one. */
    private fun showSetPin(upgrade: Boolean, message: Words?) {
        val a = app ?: return
        val kind = ui.chips(listOf("PIN: 6 digits or more", "Passphrase: 8 characters or more"))
        val first = secret("", true)
        val again = secret("", true)
        val firstBox = ui.labelled("PIN", first)
        val againBox = ui.labelled("The same PIN again", again)
        kind.setOnCheckedChangeListener { _, _ ->
            val numeric = ui.chosen(kind) != 1
            setLabel(firstBox, if (numeric) "PIN" else "Passphrase")
            setLabel(againBox, if (numeric) "The same PIN again" else "The same passphrase again")
            for (f in listOf(first, again)) {
                f.setText("")
                f.inputType = if (numeric) {
                    InputType.TYPE_CLASS_NUMBER or InputType.TYPE_NUMBER_VARIATION_PASSWORD
                } else {
                    InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_FLAG_NO_SUGGESTIONS or InputType.TYPE_TEXT_VARIATION_PASSWORD
                }
                ui.style(f)
            }
        }
        val words = model(if (upgrade) AppWords.upgradePin else AppWords.choosePin)
        val views = ArrayList<View>(banner())
        message?.let { views.add(ui.notice(model(it))) }
        views.add(ui.lock())
        views.add(ui.titleBlock(words, "Key"))
        views.addAll(listOf(kind, firstBox, againBox))
        views.add(ui.hint(words))
        val set = ui.primary("Set the PIN") {
            allowed {
                val pin = first.text.toString()
                val same = pin == again.text.toString()
                first.setText("")
                again.setText("")
                if (!same) return@allowed showSetPin(upgrade, AppWords.pinsDiffer)
                slow {
                    try {
                        a.setPin(pin)
                        refresh()
                    } catch (e: KeyException) {
                        showSetPin(upgrade, e.words ?: AppWords.fault(e))
                    } catch (e: LinkageError) {
                        showMessage(AppWords.libraryMissing(e.message)) { showSetPin(upgrade, null) }
                    }
                }
            }
        }
        put("Key", views, listOf(set), logo = true)
    }

    /** What the screens say about the wrong tries so far: the tries left from the 5th, and the wait. */
    private fun triesLines(check: Tries.Check): List<View> {
        val out = ArrayList<View>()
        if (Tries.showsTriesLeft(check.record.wrong)) out.add(ui.text(Tries.triesLeftText(check.record.wrong), 16f, bold = true))
        return out
    }

    /** SR-75: the unlock screen, with the wait counted down and the tries left said from the 5th wrong one. */
    private fun showUnlock(message: Words?) {
        stopTicker()
        val a = app ?: return
        val now = clock()
        val rec = tries.read(now)
        val check = Tries.check(rec, now)
        if (check.wipe) return wipe()
        if (check.record != rec) {
            try {
                tries.write(check.record)
            } catch (_: IOException) {
            }
        }
        // The label says what the field takes, so the field's own hint stays empty (no copy of the label).
        val field = secret("", false)
        val countdown = ui.text("", 20f, bold = true)
        val unlock = ui.primary("Unlock") {
            allowed {
                val pin = field.text.toString()
                field.setText("")
                if (pin.isNotEmpty()) triedPin({ showUnlock(it) }, { a.unlock(pin) }) { refresh() }
            }
        }
        fun update() {
            val left = Tries.check(check.record, clock()).waitLeft
            countdown.text = if (left > 0) "Wait ${Tries.waitText(left)}" else ""
            countdown.visibility = if (left > 0) View.VISIBLE else View.GONE
            unlock.isEnabled = left <= 0
        }
        update()
        val words = WordsModel.of((message ?: AppWords.state(SetupState.Locked)).say())
        val views = ArrayList<View>()
        if (level == KeyLevel.Software) views.add(ui.notice(WordsModel.of(AppWords.keyInSoftware.say())))
        views.add(ui.lock())
        views.add(ui.titleBlock(words, UNLOCK_TITLE))
        views.addAll(triesLines(check))
        views.add(countdown)
        views.add(ui.labelled("PIN or passphrase", field))
        views.add(ui.hint(words))
        act.show(ui.page(ui.bar(UNLOCK_TITLE), views, listOf(unlock)))
        shakeIfWrong(field)
        if (check.waitLeft > 0) {
            val tick = object : Runnable {
                override fun run() {
                    update()
                    if (Tries.check(check.record, clock()).waitLeft > 0) main.postDelayed(this, 1000) else ticker = null
                }
            }
            ticker = tick
            main.postDelayed(tick, 1000)
        }
    }

    /**
     * One try of the PIN, counted as SR-75 says: one more wrong try is written before [op] runs, and 0 after it
     * succeeds. A refusal that is not a wrong PIN gives the count back; the 10th wrong try wipes. [back] shows
     * the screen the try came from, with the words to show on it. [done] runs when [op] worked.
     */
    private fun giveBack(record: TriesRecord) {
        try {
            tries.write(record)
        } catch (_: IOException) {
        }
    }

    private fun triedPin(back: (Words?) -> Unit, op: () -> Unit, done: () -> Unit) {
        val now = clock()
        val rec = tries.read(now)
        val check = Tries.check(rec, now)
        if (check.wipe) return wipe()
        if (check.waitLeft > 0) return back(AppWords.waitForTry)
        val before = Tries.before(check.record, now)
        try {
            tries.write(before)
        } catch (e: IOException) {
            return showMessage(AppWords.triesNotSaved(e.message)) { back(null) }
        }
        // A try that was not run is not a wrong try: the count is given back.
        slow(skipped = { giveBack(check.record) }) {
            try {
                op()
                try {
                    tries.write(Tries.afterRight())
                } catch (_: IOException) {
                    // The count stays one too high: the stricter way.
                }
                done()
            } catch (e: KeyException) {
                if (e.rule == RULE_PIN_WRONG) {
                    if (before.wrong >= Tries.WIPE_AT) {
                        wipe()
                    } else {
                        wrongPin = true
                        back(e.words ?: AppWords.fault(e))
                    }
                } else {
                    giveBack(check.record)
                    showMessage(e.words ?: AppWords.fault(e)) { back(null) }
                }
            } catch (e: LinkageError) {
                showMessage(AppWords.libraryMissing(e.message)) { back(null) }
            }
        }
    }

    /** SR-76: delete the saved state, the Keystore key and the count, in that order, and say so. */
    private fun wipe() {
        close()
        try {
            Wipe.run(act.noBackupFilesDir, tries) { KeyWipe.deleteKey() }
        } catch (e: IOException) {
            return showCenter(listOf(AppWords.wipeFailed(e.message)), ui.primary("Try again") { start() })
        }
        showWiped()
    }

    private fun showWiped() {
        showCenter(listOf(AppWords.wiped), ui.primary("Restore the key from its words") { start() })
    }

    /** Changing the PIN asks for the current one, and a wrong one counts like a wrong try at the unlock screen. */
    private fun showChangePin(message: Words?) {
        val a = app ?: return
        val now = clock()
        val check = Tries.check(tries.read(now), now)
        if (check.wipe) return wipe()
        val current = secret("", false)
        val first = secret("", false)
        val again = secret("", false)
        val views = ArrayList<View>(banner())
        views.add(ui.lock())
        views.add(plain("Change the PIN", null, "Change the PIN"))
        message?.let { views.add(ui.notice(model(it))) }
        views.addAll(triesLines(check))
        if (check.waitLeft > 0) views.add(ui.text("Wait ${Tries.waitText(check.waitLeft)}", 20f, bold = true))
        views.add(ui.labelled("Current PIN or passphrase", current))
        views.add(ui.labelled("New PIN or passphrase", first))
        views.add(ui.labelled("The new one again", again))
        val change = ui.primary("Change the PIN") {
            allowed {
                val old = current.text.toString()
                val new = first.text.toString()
                val same = new == again.text.toString()
                for (f in listOf(current, first, again)) f.setText("")
                if (!same) return@allowed showChangePin(AppWords.pinsDiffer)
                triedPin({ showChangePin(it) }, { a.changePin(old, new) }) {
                    showTitled(AppWords.pinChanged, AppWords.pinChanged.say().heading) { refresh() }
                }
            }
        }
        put("Change the PIN", views, listOf(change, ui.button("Back") { refresh() }), back = { refresh() })
        shakeIfWrong(current)
    }

    // ---- cash-out addresses (SR-78, SR-79) ------------------------------------------------

    /** The list of cash-out addresses, each with "Remove", and "Add an address". */
    private fun showCashOut() {
        val a = app ?: return
        val s = attempt({ refresh() }) { a.signerStatus() } ?: return
        val words = model(AppWords.cashOut)
        val views = ArrayList<View>(banner())
        views.add(ui.titleBlock(words, "Cash-out addresses"))
        val rows = s.allowlist.map { CashOut.row(it, s.effectiveTimeNow) }
        if (rows.isEmpty()) views.add(ui.text("No cash-out addresses yet.", 16f))
        for (row in rows) {
            val item = android.widget.LinearLayout(dark).apply { orientation = android.widget.LinearLayout.VERTICAL }
            item.addView(ui.text(row.label, 18f, bold = true))
            item.addView(ui.text(row.address, 13f, mono = true, colour = ui.pal.secondary), ui.margins(top = 4))
            item.addView(ui.text("Added ${row.enrolled}, ${row.status}", 13f), ui.margins(top = 4))
            row.removable?.let { address -> item.addView(ui.danger("Remove") { showRemoveAddress(row, address) }, ui.margins(top = 10)) }
            views.add(card(item))
        }
        views.add(ui.hint(words))
        put("Cash-out addresses", views, listOf(ui.primary("Add an address") { showScanDeposit() }, ui.button("Back") { refresh() }), back = { refresh() })
    }

    /** SR-78: the address only comes from the camera. There is no field for it. */
    private fun showScanDeposit() {
        allowed {
            val back = { showCashOut() }
            val words = model(AppWords.scanDeposit)
            val scanDeposit = ui.primary("Scan the deposit code") {
                allowed {
                    scan.begin(
                        show = { act.show(scanPage(ui, it)) },
                        onBytes = { bytes -> allowed { depositScanned(bytes) } },
                        onFailed = { showMessage(cameraWords(it), back) },
                        onCancel = { showScanDeposit() },
                    )
                }
            }
            put(
                "Add an address",
                listOf(ui.titleBlock(words, "Add an address"), ui.hint(words)),
                listOf(scanDeposit, ui.button("Back") { back() }),
                back = back,
            )
        }
    }

    private fun depositScanned(bytes: ByteArray) {
        val a = app ?: return
        when (val input = CashOut.readCode(bytes)) {
            is DepositInput.Refused -> showMessage(input.words) { showScanDeposit() }
            is DepositInput.Text -> {
                // The wallet's network is key 2's; a Key app without key 2 has no allowlist (the library says so).
                val network = attempt({ showCashOut() }) { a.key2Public()?.network } ?: return showCashOut()
                val code = attempt({ showScanDeposit() }) { KeyApp.parseDepositCode(store!!, input.text, network) } ?: return
                showNameAddress(input.text, code.address, "")
            }
        }
    }

    private fun showNameAddress(text: String, address: String, label: String) {
        val field = ui.field("A name for this address (1 to 64 characters)")
        field.setText(label)
        val words = model(AppWords.nameAddress)
        put(
            "Name this address",
            banner() + listOf(
                ui.titleBlock(words, "Name this address"),
                ui.text(CashOut.groupsOfFour(address), 16f, mono = true, colour = ui.pal.secondary),
                field,
                ui.hint(words),
            ),
            listOf(
                ui.primary("Next") { showConfirmAddress(text, address, field.text.toString().trim()) },
                ui.button("Back") { showCashOut() },
            ),
            back = { showCashOut() },
        )
    }

    /** SR-78: the address in groups of four, and that it is usable 24 hours from now by this phone's clock. */
    private fun showConfirmAddress(text: String, address: String, label: String) {
        val a = app ?: return
        val words = model(AppWords.checkAddress)
        val add = ui.primary("Add this address") {
            allowed {
                attempt({ showNameAddress(text, address, label) }) { a.enrollDestination(text, label) }
                    ?.let { showMessage(AppWords.addressAdded) { showCashOut() } }
            }
        }
        put(
            "Next",
            banner() + listOf(
                ui.titleBlock(words, "Next"),
                ui.text(Format.cleanForeign(label), 18f, bold = true),
                ui.text(CashOut.groupsOfFour(address), 22f, bold = true, mono = true),
                ui.hint(words),
            ),
            listOf(add, ui.button("Back") { showNameAddress(text, address, label) }),
            back = { showNameAddress(text, address, label) },
        )
    }

    private fun showRemoveAddress(row: CashOutRow, address: String) {
        val a = app ?: return
        val words = model(AppWords.removeAddress)
        val remove = ui.danger("Remove it") {
            allowed {
                attempt({ showCashOut() }) { a.removeDestination(address) }
                    ?.let { showMessage(AppWords.addressRemoved) { showCashOut() } }
            }
        }
        put(
            "Remove",
            banner() + listOf(
                ui.titleBlock(words, "Remove"),
                ui.text(row.label, 18f, bold = true),
                ui.text(row.address, 16f, mono = true, colour = ui.pal.secondary),
                ui.hint(words),
            ),
            listOf(remove, ui.button("Back") { showCashOut() }),
            back = { showCashOut() },
        )
    }

    // ---- first start ----------------------------------------------------------------

    private fun showSetup() {
        val a = app ?: return
        val back = { refresh() }
        val words = model(AppWords.state(SetupState.Empty))
        val views = ArrayList<View>(banner())
        views.add(ui.titleBlock(words, "Key"))
        views.add(ui.label("Network"))
        views.add(networkChoice())
        views.add(ui.hint(words))
        put(
            "Key",
            views,
            listOf(
                ui.primary("Let this device make the key") {
                    allowed { attempt(back) { a.createFromEntropy(EntropySource.Platform, network) }?.let { refresh() } }
                },
                ui.button("Make it with dice or coins") { showThrows() },
                ui.button("Type in existing words") { showTypedWords() },
                ui.button("Approver only: no key 2 on this phone") {
                    allowed { attempt(back) { a.createApproverOnly() }?.let { refresh() } }
                },
            ),
            logo = true,
        )
    }

    private fun showThrows() {
        val a = app ?: return
        val back = { refresh() }
        val method = ui.choice(
            listOf("Dice, hashed (99 rolls, 24 words)", "Dice, skew-corrected", "Coin, skew-corrected"),
        )
        val count = ui.chips(listOf("24 words", "12 words"))
        val input = ui.field("Dice faces 1 to 6, or heads H and tails T", lines = 4)
        val progress = ui.text("", 13f, colour = ui.pal.secondary)

        fun words() = if (ui.chosen(count) == 1) KeyWordCount.Twelve else KeyWordCount.TwentyFour

        fun update() {
            val text = input.text.toString()
            progress.text = try {
                when (ui.chosen(method)) {
                    0 -> "${Format.toRolls(text).length} rolls typed."
                    1 -> KeyApp.diceBits(store!!, Format.toRolls(text), words()).let {
                        "${it.have} bits so far, ${it.stillNeeded} still needed."
                    }
                    else -> Format.toFlips(text)?.let { flips ->
                        KeyApp.coinBits(store!!, flips, words()).let { "${it.have} bits so far, ${it.stillNeeded} still needed." }
                    } ?: "Only H and T can be typed here."
                }
            } catch (e: KeyException) {
                "Progress is not available."
            }
        }
        val watcher = object : TextWatcher {
            override fun beforeTextChanged(s: CharSequence?, st: Int, c: Int, a: Int) {}
            override fun onTextChanged(s: CharSequence?, st: Int, b: Int, c: Int) {}
            override fun afterTextChanged(s: Editable?) = update()
        }
        input.addTextChangedListener(watcher)
        method.setOnCheckedChangeListener { _, _ -> update() }
        count.setOnCheckedChangeListener { _, _ -> update() }

        put(
            "Make it with dice or coins",
            banner() + listOf(
                plain(
                    "Make the key yourself",
                    "Nothing but your throws goes into the key: the same throws give the same words in another tool.",
                    "Make it with dice or coins",
                ),
                method, count, input, progress,
            ),
            listOf(
                ui.primary("Make the key") {
                    allowed {
                        val text = input.text.toString()
                        val source: EntropySource? = when (ui.chosen(method)) {
                            0 -> EntropySource.DiceHashed(Format.toRolls(text))
                            1 -> EntropySource.DiceSkewCorrected(Format.toRolls(text), words())
                            else -> Format.toFlips(text)?.let { EntropySource.CoinFlips(it, words()) }
                        }
                        if (source == null) {
                            showMessage(AppWords.flipsNotReadable) { showThrows() }
                        } else {
                            attempt({ showThrows() }) { a.createFromEntropy(source, network) }?.let {
                                input.setText("")
                                refresh()
                            }
                        }
                    }
                },
                ui.button("Back") { back() },
            ),
            back = back,
        )
    }

    private fun showTypedWords() {
        val a = app ?: return
        val input = ui.field("12 or 24 words, with spaces between them", lines = 3)
        val hint = ui.text("", 13f, colour = ui.pal.secondary)
        input.addTextChangedListener(
            object : TextWatcher {
                override fun beforeTextChanged(s: CharSequence?, st: Int, c: Int, a: Int) {}
                override fun onTextChanged(s: CharSequence?, st: Int, b: Int, c: Int) {}
                override fun afterTextChanged(s: Editable?) {
                    val last = s.toString().trim().substringAfterLast(' ', "").ifEmpty { s.toString().trim() }
                    hint.text = if (last.isEmpty() || s.toString().endsWith(" ")) "" else try {
                        KeyApp.wordSuggestions(store!!, last.lowercase()).joinToString(", ").ifEmpty { "No word starts like that." }
                    } catch (e: KeyException) {
                        ""
                    }
                }
            },
        )
        put(
            "Type in existing words",
            banner() + listOf(
                plain("Type in the words", "A key made from typed words is a new key here, with its own limits.", "Type in existing words"),
                ui.label("Network"),
                networkChoice(),
                input, hint,
            ),
            listOf(
                ui.primary("Use these words") {
                    allowed {
                        val words = input.text.toString().trim().split(Regex("\\s+")).filter { it.isNotEmpty() }
                        attempt({ showTypedWords() }) { a.createFromEntropy(EntropySource.TypedWords(words), network) }
                            ?.let {
                                input.setText("")
                                refresh()
                            }
                    }
                },
                ui.button("Back") { refresh() },
            ),
            back = { refresh() },
        )
    }

    private fun showKeyWords() {
        val a = app ?: return
        val words = attempt({ refresh() }) { a.keyWords() } ?: return
        // Four words go through the cards; twelve or more stay a numbered list in one card.
        val shown = ui.text(ScreenParts.numbered(words), 20f, bold = true, mono = true)
        val say = model(AppWords.state(SetupState.HasIdentity(true, true)))
        put(
            "Key",
            banner() + listOf(
                ui.titleBlock(say, "Key"),
                if (ScreenParts.asCards(words.size)) ui.wordCards(words) else card(shown),
                ui.hint(say),
            ),
            listOf(
                ui.primary("Words written down") {
                    shown.text = ""
                    refresh()
                },
            ),
            logo = true,
        )
    }

    // ---- the wallet ----------------------------------------------------------------------

    // ---- the key ceremony ---------------------------------------------------------------

    /** A button that shows or hides [text] under a fold called Advanced. */
    private fun advanced(text: String): List<View> {
        val body = ui.text(text, 13f, mono = true)
        body.visibility = View.GONE
        val toggle = ui.button(AppWords.Text.ADVANCED) {
            body.visibility = if (body.visibility == View.GONE) View.VISIBLE else View.GONE
        }
        return listOf(toggle, body)
    }

    /**
     * Step 2: key 3 comes from another wallet, by a plain text code, a key code (a UR, SR-64) or by paste. [back] is where Back goes
     * when key 3 is already held and is being changed; with none, the screen has no Back.
     */
    private fun showKey3(back: (() -> Unit)?) {
        val paste = ui.field(AppWords.Text.PASTE_KEY3, lines = 3, mono = true)
        val words = model(AppWords.key3Needed)
        val views = ArrayList<View>(banner())
        views.add(ui.titleBlock(words, "Key"))
        views.addAll(advanced(AppWords.Text.ADVANCED_KEY3))
        views.add(paste)
        views.add(ui.hint(words))
        val actions = arrayListOf(
            ui.primary(AppWords.Text.SCAN_KEY3) { scanKey3(back) },
            ui.button("Use the pasted text") { allowed { takeKey3(CeremonyModel.readKey3Text(paste.text.toString()), back) } },
        )
        if (back != null) actions.add(ui.button("Back") { back() })
        put("Key", views, actions, logo = true)
    }

    private fun scanKey3(back: (() -> Unit)?) {
        allowed {
            scan.begin(
                show = { act.show(scanPage(ui, it)) },
                onBytes = { bytes -> allowed { takeKey3(CeremonyModel.readKey3Code(bytes), back) } },
                onFailed = { showMessage(cameraWords(it)) { showKey3(back) } },
                onCancel = { showKey3(back) },
                ur = UrScan(CeremonyModel.KEY3_TYPES, Who.KEY_HOLDER) { type, payload ->
                    allowed { takeKey3(CeremonyModel.readKey3Message(UrCall.native().keyText(type, payload)), back) }
                },
            )
        }
    }

    private fun takeKey3(input: Key3Input, back: (() -> Unit)?) {
        val a = app ?: return
        when (input) {
            is Key3Input.Refused -> showMessage(input.words) { showKey3(back) }
            is Key3Input.Declined -> showMessage(input.words) { scanKey3(back) }
            is Key3Input.Text -> attempt({ showKey3(back) }) { a.takeKey3(input.text) }?.let { showKey3Taken(it) }
        }
    }

    /** The fingerprint of key 3, large, to be checked against the other wallet's screen. */
    private fun showKey3Taken(v: Key3View) {
        val c = CeremonyModel.key3Check(v)
        val words = model(AppWords.key3Taken)
        val views = ArrayList<View>(banner())
        views.add(ui.titleBlock(words, "Scan key 3"))
        views.add(centred(ui.text(c.fingerprint, 40f, bold = true, mono = true)))
        views.add(centred(ui.text(c.network, 13f, colour = ui.pal.secondary)))
        views.addAll(advanced(c.advanced.joinToString("\n")))
        views.add(ui.hint(words))
        put(
            "Scan key 3",
            views,
            listOf(
                ui.primary("They match") { refresh() },
                ui.danger("They differ") { showMessage(AppWords.key3Mismatch) { showKey3(null) } },
            ),
        )
    }

    /**
     * Step 2, for a Key app with no key 2 of its own: key 2 comes from its hardware signer, by a key code or
     * as text, before any wallet exists (SR-84). The owner phone takes key 2 from the signer on its own.
     */
    private fun showKey2() {
        val words = model(AppWords.key2Needed)
        val views = ArrayList<View>(banner())
        views.add(ui.titleBlock(words, "Key"))
        views.add(ui.hint(words))
        put("Key", views, listOf(ui.primary(AppWords.Text.SCAN_KEY2) { scanKey2() }), logo = true)
    }

    private fun scanKey2() {
        allowed {
            scan.begin(
                show = { act.show(scanPage(ui, it)) },
                onBytes = { bytes -> allowed { takeKey2(CeremonyModel.readKey2Code(bytes)) } },
                onFailed = { showMessage(cameraWords(it)) { showKey2() } },
                onCancel = { showKey2() },
                ur = UrScan(CeremonyModel.KEY3_TYPES, Who.KEY_HOLDER) { type, payload ->
                    allowed { takeKey2(CeremonyModel.readKey3Message(UrCall.native().keyText(type, payload))) }
                },
            )
        }
    }

    private fun takeKey2(input: Key3Input) {
        val a = app ?: return
        when (input) {
            is Key3Input.Refused -> showMessage(input.words) { showKey2() }
            is Key3Input.Declined -> showMessage(input.words) { scanKey2() }
            is Key3Input.Text -> attempt({ showKey2() }) { a.takeKey2(input.text) }?.let { showKey2Taken(it) }
        }
    }

    /** The fingerprint of key 2, large, to be checked against the signer's own screen. */
    private fun showKey2Taken(v: Key2TakenView) {
        val c = CeremonyModel.key2Check(v)
        val words = model(AppWords.key2Taken)
        val views = ArrayList<View>(banner())
        views.add(ui.titleBlock(words, "Key"))
        views.add(centred(ui.text(c.fingerprint, 40f, bold = true, mono = true)))
        views.add(centred(ui.text(c.network, 13f, colour = ui.pal.secondary)))
        views.addAll(advanced(c.advanced.joinToString("\n")))
        views.add(ui.hint(words))
        put(
            "Key",
            views,
            listOf(
                ui.primary("They match") { refresh() },
                ui.danger("They differ") { showMessage(AppWords.key2Mismatch) { showKey2() } },
            ),
        )
    }

    /** A code in a card, or the words that say why it cannot be shown. */
    private fun codeBlock(made: AnimatedQrMade?): View {
        val view = made?.view
        return if (view != null) ui.codeCard(view) else ui.notice(WordsModel.of(made?.words ?: UrWords.cannotShow(Who.KEY_HOLDER)))
    }

    /** Step 5, first half: the public answer as a code for the owner phone to scan. */
    private fun showAnswer(key3: Key3View) {
        allowed {
            val a = app ?: return@allowed
            val bytes = attempt({ refresh() }) { a.publicAnswer() } ?: return@allowed
            val words = model(AppWords.publicAnswerReady)
            val views = ArrayList<View>(banner())
            views.add(ui.compactWords(words))
            views.add(centred(ui.text(AppWords.Text.ON_OWNER_PHONE, 16f, bold = true)))
            val made = if (CeremonyModel.looksLikeAnswer(bytes)) {
                val metrics = act.resources.displayMetrics
                AnimatedQrView.make(dark, Who.KEY_HOLDER, TYPE_BYTES, bytes, (metrics.heightPixels * 0.6).toInt())
            } else {
                null
            }
            views.add(codeBlock(made))
            views.add(centred(ui.text("Key 3: ${CeremonyModel.groupFingerprint(key3.masterFingerprint)}", 13f, mono = true, colour = ui.pal.secondary)))
            views.add(ui.hint(words))
            put(
                "Give the owner phone these keys",
                views,
                listOf(
                    ui.primary("Scan the owner phone's wallet") { showScanWallet(key3) },
                    ui.button("Use another key 3") { showKey3 { showAnswer(key3) } },
                ),
            )
        }
    }

    /** Step 5, second half: read the owner phone's wallet setup. */
    private fun showScanWallet(key3: Key3View) {
        val words = model(AppWords.scanOwnerWallet)
        put(
            "Scan the owner phone's wallet",
            banner() + listOf(ui.titleBlock(words, "Scan the owner phone's wallet"), ui.hint(words)),
            listOf(
                ui.primary("Scan the wallet code") { scanWallet(key3) },
                ui.button("Back") { showAnswer(key3) },
            ),
            back = { showAnswer(key3) },
        )
    }

    private fun scanWallet(key3: Key3View) {
        val again = { showScanWallet(key3) }
        allowed {
            scan.begin(
                show = { act.show(scanPage(ui, it)) },
                onBytes = { bytes -> allowed { acceptSetup(bytes, key3) } },
                onFailed = { showMessage(cameraWords(it), again) },
                onCancel = again,
                ur = UrScan(setOf(TYPE_BYTES), Who.KEY_HOLDER) { _, payload -> allowed { acceptSetup(payload, key3) } },
            )
        }
    }

    /** The library decides: a refusal shows its words, and the swapped key 3 gets plain words first. */
    private fun acceptSetup(bytes: ByteArray, key3: Key3View) {
        val a = app ?: return
        val view = try {
            a.acceptWalletSetup(bytes)
        } catch (e: KeyException) {
            return showCenter(
                CeremonyModel.setupRefusal(e),
                ui.button("Back") { showScanWallet(key3) },
                title = "Scan the wallet code",
                back = { showScanWallet(key3) },
            )
        } catch (e: LinkageError) {
            return showMessage(AppWords.libraryMissing(e.message)) { showScanWallet(key3) }
        }
        showWallet(view, true)
    }

    /**
     * The comparison: the four words and the three fingerprints, large. [fresh] is true right after the wallet
     * was accepted, when the person says whether they match; later it is only shown.
     */
    private fun showWallet(v: WalletView, fresh: Boolean) {
        val c = CeremonyModel.walletCheck(v)
        val words = model(AppWords.checkWallet)
        val barTitle = if (fresh) "Check the wallet" else "The wallet and its check words"
        val views = ArrayList<View>(banner())
        views.add(ui.titleBlock(words, barTitle))
        views.add(ui.wordCards(ScreenParts.words(c.checkWords)))
        views.add(ui.facts(c.fingerprints.map { ScreenParts.keyValue(it) }))
        views.add(ui.text(c.identities.joinToString("\n"), 13f, colour = ui.pal.secondary))
        views.add(ui.text("First address:\n${c.firstAddress}", 13f, mono = true, colour = ui.pal.secondary))
        views.add(ui.hint(words))
        if (fresh) {
            put(
                barTitle,
                views,
                listOf(
                    ui.primary("They match") { allowed { attempt({ showWallet(v, true) }) { app?.confirmWallet() }?.let { refresh() } } },
                    ui.danger("They differ") {
                        allowed { attempt({ showWallet(v, true) }) { app?.discardWallet() }?.let { showMessage(AppWords.walletMismatch) { refresh() } } }
                    },
                ),
            )
        } else {
            put(barTitle, views, listOf(ui.button("Back") { refresh() }), back = { refresh() })
        }
    }

    /** The wallet that was checked and is kept, not registered, after a restart or a leave: the comparison again. */
    private fun showPending() {
        val a = app ?: return
        val v = attempt({ refresh() }) { a.walletView() } ?: return
        showWallet(v, true)
    }

    private fun showWalletAgain() {
        val a = app ?: return
        val v = attempt({ refresh() }) { a.walletView() } ?: return
        showWallet(v, false)
    }

    private fun showKitCheck() {
        val a = app ?: return
        val input = ui.field("The wallet text from the printed kit", lines = 6, mono = true)
        put(
            "Check the printed kit",
            listOf(
                plain("Check the printed kit", "Paste the wallet text read from the paper. It is compared with this app's own copy.", "Check the printed kit"),
                input,
            ),
            listOf(
                ui.primary("Check") {
                    attempt({ showKitCheck() }) { a.checkKit(input.text.toString()) }?.let {
                        showMessage(AppWords.kit(it)) { refresh() }
                    }
                },
                scanButton("Scan the wallet code from the paper", input),
                ui.button("Back") { refresh() },
            ),
            back = { refresh() },
        )
    }

    // ---- home ------------------------------------------------------------------------------

    private fun showHome(state: SetupState) {
        val words = model(AppWords.state(state))
        val views = ArrayList<View>(banner())
        views.add(ui.status(listOf(StatusItem(words.colour, words.heading, "${words.what}\n${words.next}"))))
        val items = ArrayList<RowItem>()
        items.add(RowItem("Scan a request code from the owner's phone") { scanRequest() })
        if (answerFile.isFile) {
            items.add(RowItem("Save the last answer again") { saveAnswer(answerFile.readBytes()) })
            items.add(RowItem("Show the last answer as a code") { showAnswerCode(answerFile.readBytes()) { refresh() } })
        }
        items.add(RowItem("Pair a counter") { showPairOffer() })
        items.add(RowItem("Pair a Box") { showPairBox(null) })
        items.add(RowItem("Cash-out addresses") { showCashOut() })
        items.add(RowItem("Change the PIN") { showChangePin(null) })
        items.add(RowItem("The wallet and its check words") { showWalletAgain() })
        items.add(RowItem("Check the printed kit") { showKitCheck() })
        items.add(RowItem("Limits and status") { showStatus() })
        items.add(RowItem("About this app") { showAbout() })
        views.add(ui.rows(items))
        put("Key", views, listOf(ui.primary("Answer a request from the owner's phone") { pickRequest() }), logo = true)
    }

    /**
     * "Pair a counter": the offer as a `ur:bytes` code for the counter to scan. It is reached from the home
     * screen, which is shown only once the wallet is accepted (the library makes no offer before).
     */
    private fun showPairOffer() {
        allowed {
            val a = app ?: return@allowed
            val bytes = attempt({ refresh() }) { a.pairOfferBytes() } ?: return@allowed
            if (!PairOfferModel.looksLikeOffer(bytes)) {
                return@allowed showMessage(AppWords.fault(KeyException("pair-offer", "the library gave bytes that are not an offer", 4)), { refresh() })
            }
            val metrics = act.resources.displayMetrics
            val made = AnimatedQrView.make(dark, Who.KEY_HOLDER, TYPE_BYTES, bytes, (metrics.heightPixels * 0.6).toInt())
            val words = model(AppWords.pairOffer)
            val views = ArrayList<View>()
            views.add(ui.compactWords(words))
            views.add(centred(ui.text("Scan this with the counter", 15f, medium = true)))
            views.add(codeBlock(made))
            views.add(ui.hint(words))
            put("Pair a counter", views, listOf(ui.button("Back") { refresh() }), back = { refresh() })
        }
    }

    /**
     * "Pair a Box" (SR-81): the open invitation as six groups of five characters, large and in a fixed-width
     * type, with the time left counting down on this phone's clock. The library stores a new invitation before
     * it is returned; the screen carries FLAG_SECURE like every screen of the app. Leaving the screen or the app
     * drops it from view; the invitation stays open until it runs out, is cancelled or is used.
     */
    private fun showPairBox(message: Words?) {
        stopTicker()
        allowed {
            val a = app ?: return@allowed
            val held = attempt({ refresh() }) { listOf(a.openInvitation()) } ?: return@allowed
            val open = held[0]
            val views = ArrayList<View>(banner())
            if (open == null) {
                views.add(plain("Pair a Box", null, "Pair a Box"))
                message?.let { views.add(ui.notice(model(it))) }
                views.add(ui.text("A new invitation lasts 30 minutes and works for one Box.", 16f))
                put(
                    "Pair a Box",
                    views,
                    listOf(
                        ui.primary("Make an invitation") {
                            allowed { attempt({ showPairBox(null) }) { a.makeInvitation() }?.let { showPairBox(null) } }
                        },
                        ui.button("Back") { refresh() },
                    ),
                    back = { refresh() },
                )
                return@allowed
            }
            val spaced = BoxModel.spaced(open.text)
            if (spaced == null) {
                return@allowed showMessage(AppWords.fault(KeyException("invitation", "the library gave text that is not an invitation", 4)), { refresh() })
            }
            val words = model(AppWords.pairBox)
            views.add(ui.titleBlock(words, "Pair a Box"))
            val groups = BoxModel.groups(open.text) ?: emptyList()
            views.add(centred(ui.text(groups.take(3).joinToString("   "), 28f, bold = true, mono = true)))
            views.add(centred(ui.text(groups.drop(3).joinToString("   "), 28f, bold = true, mono = true)))
            val left = centred(ui.text("", 18f, bold = true))
            views.add(left)
            views.add(ui.hint(words))
            val cancel = ui.danger("Cancel this invitation") {
                stopTicker()
                allowed { attempt({ showPairBox(null) }) { a.cancelInvitation() }?.let { showPairBox(null) } }
            }
            val leave = {
                stopTicker()
                refresh()
            }
            fun update(): Boolean {
                val now = clock()
                if (BoxModel.runOut(open, now)) {
                    stopTicker()
                    showPairBox(AppWords.invitationRunOut)
                    return false
                }
                left.text = "Time left: ${BoxModel.timeLeft(BoxModel.secsLeft(open, now))}"
                return true
            }
            if (!update()) return@allowed
            put("Pair a Box", views, listOf(cancel, ui.button("Back") { leave() }), back = leave)
            val tick = object : Runnable {
                override fun run() {
                    if (update()) main.postDelayed(this, 1000) else ticker = null
                }
            }
            ticker = tick
            main.postDelayed(tick, 1000)
        }
    }

    private fun showAbout() {
        val key = when (level) {
            KeyLevel.StrongBox -> "The saved key is held in the phone's StrongBox chip."
            KeyLevel.Tee -> "The saved key is held in the phone's trusted hardware."
            KeyLevel.Software -> "The saved key is held by software alone. Tests only."
            null -> "Not known."
        }
        put(
            "About this app",
            listOf(
                plain("About this app", null, "About this app"),
                ui.text("Built by Avila Labs only. No independent party has rebuilt it yet.", 15f),
                ui.text("First build: no independent security audit has been done. Not for real funds.", 15f, bold = true),
                ui.text(key, 15f),
                ui.text("This app asks for no permission, and it has no network access.", 15f),
            ),
            listOf(ui.button("Back") { refresh() }),
            back = { refresh() },
        )
    }

    // ---- limits and status --------------------------------------------------------------------

    private fun showStatus() {
        val a = app ?: return
        val back = { refresh() }
        val s = attempt(back) { a.signerStatus() } ?: return
        fun ceilings(c: CeilingsView) = ui.facts(
            listOf("one payment" to Format.sats(c.perTx), "a day" to Format.sats(c.perDay), "a week" to Format.sats(c.perWeek)),
        )
        val views = ArrayList<View>()
        views.add(plain("Limits and status", null, "Limits and status"))
        views.add(ui.label("To other addresses, most for"))
        views.add(ceilings(s.ceilingsOther))
        views.add(ui.label("To saved addresses, most for"))
        views.add(ceilings(s.ceilingsAllowlisted))
        val t = StringBuilder()
        t.append("Fee limit: ${s.feeCeilings.satPerVb} sats for each byte, or ${Format.sats(s.feeCeilings.absoluteSats)} in all\n")
        t.append("Spent today: ${Format.sats(s.otherDay)} to other, ${Format.sats(s.allowlistedDay)} to saved\n")
        t.append("Spent this week: ${Format.sats(s.otherWeek)} to other, ${Format.sats(s.allowlistedWeek)} to saved\n")
        t.append("Payments recorded: ${s.spendLogCount}\n")
        t.append("Last receive address number seen: ${s.receiveMark}\nLast change address number seen: ${s.changeMark}\n")
        t.append("Latest time seen: ${Format.localTime(s.latestTimeSeen)}")
        views.add(ui.text(t.toString(), 13f))
        views.add(ui.label("Saved addresses"))
        val saved = StringBuilder()
        if (s.allowlist.isEmpty()) saved.append("none")
        for ((i, d) in s.allowlist.withIndex()) {
            if (i > 0) saved.append("\n")
            saved.append("${Format.cleanForeign(d.label)}: ${d.address ?: d.scriptHex}\n")
            saved.append("  ${d.activeAt?.let { "active from ${Format.localTime(it)}" } ?: "not active yet"}")
        }
        views.add(ui.text(saved.toString(), 13f, mono = true))
        put(
            "Limits and status",
            views,
            listOf(
                ui.button("Change the limits") { showLimitsForm(s.ceilingsOther, s.ceilingsAllowlisted, s.feeCeilings) },
                ui.button("Back") { back() },
            ),
            back = back,
        )
    }

    private fun showLimitsForm(other: CeilingsView, saved: CeilingsView, fee: FeeCeilingsView) {
        val a = app ?: return
        val names = listOf(
            "Other addresses, one payment (sats)", "Other addresses, a day", "Other addresses, a week",
            "Saved addresses, one payment", "Saved addresses, a day", "Saved addresses, a week",
            "Fee limit, sats for each byte", "Fee limit, sats in all",
        )
        val values = listOf(
            other.perTx, other.perDay, other.perWeek, saved.perTx, saved.perDay, saved.perWeek,
            fee.satPerVb, fee.absoluteSats,
        )
        val fields = names.zip(values).map { (_, v) -> ui.field("").also { it.setText(v.toString()) } }
        val views = ArrayList<View>()
        views.add(
            plain(
                "Change the limits",
                "Raising a limit is refused when it would loosen one without the owner's phone's answer; the reason is shown.",
                "Change the limits",
            ),
        )
        for ((n, f) in names.zip(fields)) views.add(ui.labelled(n, f))
        put(
            "Change the limits",
            views,
            listOf(
                ui.primary("Save the limits") {
                    allowed {
                        val n = fields.map { Format.toAmount(it.text.toString()) }
                        if (n.any { it == null }) {
                            showMessage(AppWords.notANumber) { showStatus() }
                        } else {
                            val v = n.map { it!! }
                            attempt({ showStatus() }) {
                                a.setCeilings(CeilingsView(v[0], v[1], v[2]), CeilingsView(v[3], v[4], v[5]))
                                a.setFeeCeilings(FeeCeilingsView(v[6], v[7]))
                            }?.let { showStatus() }
                        }
                    }
                },
                ui.button("Back") { showStatus() },
            ),
            back = { showStatus() },
        )
    }

    // ---- scanning one code ------------------------------------------------------------------------

    /** A button that scans one code into [field]; the page and what is typed on it stay as they are. */
    private fun scanButton(label: String, field: EditText): Button = ui.button(label) {
        val page = act.current
        val restore = { if (page != null) act.show(page) else refresh() }
        allowed {
            scan.begin(
                show = { act.show(scanPage(ui, it)) },
                onBytes = { bytes ->
                    allowed {
                        if (UrText.startsWithUr(bytes)) {
                            showMessage(AppWords.codeNotForField, restore)
                        } else {
                            field.setText(String(bytes, Charsets.UTF_8))
                            restore()
                        }
                    }
                },
                onFailed = { showMessage(cameraWords(it), restore) },
                onCancel = restore,
            )
        }
    }

    private fun cameraWords(f: org.openbmt.app.ScanFailure): Words {
        val say = f.words
        return when {
            say != null -> Words(say.tone, say.who, say.heading, say.what, say.next, null, null, null, say.detail)
            f.off -> AppWords.cameraOff
            else -> AppWords.cameraNotStarted(f.problem)
        }
    }

    /**
     * The request of the owner's phone: one plain code, or a UR, whole or in parts as a code that changes.
     * OpenBMT's own request is `ur:bytes`; `ur:crypto-psbt` (a plain PSBT from another wallet) is taken
     * as well, and the library decides whether it is a request it can answer (docs/UR-RULES.md, UR-20).
     */
    private fun scanRequest() {
        allowed {
            scan.begin(
                show = { act.show(scanPage(ui, it)) },
                onBytes = { bytes -> allowed { openRequestBytes(bytes) } },
                onFailed = { showMessage(cameraWords(it)) { refresh() } },
                onCancel = { refresh() },
                ur = UrScan(setOf(TYPE_BYTES, TYPE_CRYPTO_PSBT), Who.KEY_HOLDER) { _, payload ->
                    allowed { openRequestBytes(payload) }
                },
            )
        }
    }

    /** The answer of the system's permission dialog. */
    fun permission(code: Int, results: IntArray) {
        if (scan.asking && waitedTooLong()) {
            // SR-77: an answer after more than 60 seconds is dropped, and the app locks.
            pendingAnswer = null
            return start()
        }
        scan.permissionResult(code, results)
    }

    // ---- the approval session -------------------------------------------------------------------

    private fun pickRequest() {
        pickerSince = SystemClock.elapsedRealtime()
        val intent = Intent(Intent.ACTION_OPEN_DOCUMENT).addCategory(Intent.CATEGORY_OPENABLE).setType("*/*")
        startPicker(intent, REQUEST_OPEN)
    }

    private fun saveAnswer(bytes: ByteArray) {
        pendingAnswer = bytes
        pickerSince = SystemClock.elapsedRealtime()
        val intent = Intent(Intent.ACTION_CREATE_DOCUMENT)
            .addCategory(Intent.CATEGORY_OPENABLE)
            .setType("application/octet-stream")
            .putExtra(Intent.EXTRA_TITLE, "openbmt-key-answer.bin")
        startPicker(intent, REQUEST_SAVE)
    }

    @Suppress("DEPRECATION")
    private fun startPicker(intent: Intent, code: Int) {
        try {
            act.startActivityForResult(intent, code)
        } catch (e: RuntimeException) {
            pickerSince = null
            showMessage(AppWords.fileNotRead(e.message)) { refresh() }
        }
    }

    fun picked(code: Int, result: Int, data: Intent?) {
        // A result with no wait noted was already dealt with by the resume (SR-77).
        val since = pickerSince ?: return
        pickerSince = null
        if (ExternalWait.decide(since, SystemClock.elapsedRealtime()) == WaitOutcome.Lock) {
            // More than 60 seconds in the picker: the result, picked or cancelled, is dropped unread and the app locks.
            pendingAnswer = null
            return start()
        }
        val uri = data?.data
        if (result != android.app.Activity.RESULT_OK || uri == null) return refresh()
        if (!PickedUri.accept(uri, act)) {
            // Only a document from a documents provider is read or written: nothing is, and the file is said not to be read or saved.
            val bytes = pendingAnswer
            return if (code == REQUEST_SAVE && bytes != null) {
                showMessage(AppWords.fileNotSaved(null)) { saveAnswer(bytes) }
            } else {
                showMessage(AppWords.fileNotRead(null)) { refresh() }
            }
        }
        when (code) {
            REQUEST_OPEN -> allowed { openRequest(uri) }
            REQUEST_SAVE -> writeAnswer(uri)
        }
    }

    private fun openRequest(uri: Uri) {
        val bytes = try {
            readLimited(uri)
        } catch (e: IOException) {
            return showMessage(AppWords.fileNotRead(e.message)) { refresh() }
        } catch (e: SecurityException) {
            return showMessage(AppWords.fileNotRead(e.message)) { refresh() }
        }
        openRequestBytes(bytes)
    }

    /** The request as bytes, from a file or from a scanned code. */
    private fun openRequestBytes(bytes: ByteArray) {
        val a = app ?: return
        dropSession()
        answerFile.delete()
        val s = attempt({ refresh() }) { a.openSession(bytes) } ?: return
        session = s
        lines = attempt({ dropSession(); refresh() }) { s.lines() } ?: return
        showLine(0)
    }

    /** At most one byte more than the library accepts, so that it can refuse a file that is too long. */
    private fun readLimited(uri: Uri): ByteArray {
        val limit = 1024 * 1024 + 1
        val input = act.contentResolver.openInputStream(uri) ?: throw IOException("the file could not be opened")
        input.use {
            val out = java.io.ByteArrayOutputStream()
            val buf = ByteArray(8192)
            while (out.size() < limit) {
                val n = it.read(buf, 0, minOf(buf.size, limit - out.size()))
                if (n < 0) break
                out.write(buf, 0, n)
            }
            return out.toByteArray()
        }
    }

    private fun showLine(index: Int) {
        val s = session ?: return refresh()
        if (index >= lines.size) return finishSession(s)
        val line = lines[index]
        val t = LineModel.describe(line)
        val views = ArrayList<View>()
        views.add(ui.text(t.title, 26f, bold = true))
        t.prominent?.let { views.add(ui.wordCards(ScreenParts.words(it))) }
        t.instruction?.let { views.add(ui.text(it, 16f, bold = true)) }
        val receipt = ArrayList<ReceiptLine>()
        for (r in t.rows) receipt.add(ReceiptLine(r))
        if (t.claims.isNotEmpty()) {
            receipt.add(ReceiptLine("The owner's phone says this. This app cannot check it:", LineStyle.Strong))
            for (c in t.claims) receipt.add(ReceiptLine(c, LineStyle.Small))
        }
        if (receipt.isNotEmpty()) views.add(ui.receipt(receipt))
        // Identities in full, in monospace, wrapped as needed (AS-39).
        for (identity in t.identities) views.add(ui.text(identity, 13f, mono = true))
        for (q in t.quoted) views.add(ui.quoted("Text from elsewhere, not written by this app. ${q.from}:", "\u201c${q.text}\u201d"))
        val failureWords = t.failureWords
        if (failureWords != null) {
            views.add(ui.notice(model(failureWords)))
        } else {
            t.failure?.let {
                views.add(ui.text("This item cannot be approved. The checks said:", 14f, bold = true, colour = ui.pal.stop.ink))
                views.add(ui.text(it, 14f))
            }
        }
        val reason = ui.field("Why you decline (optional)")
        views.add(reason)
        // Approving is a hold, not a tap; an item that cannot be approved shows the hold without letting it complete.
        val approve = HoldButton(dark, "Approve this item") {
            if (t.canApprove) allowed { attempt({ showLine(index) }) { s.decide(line.item, Decision.Approve) }?.let { showLine(index + 1) } }
        }
        views.add(if (t.canApprove) approve else unavailable(approve))
        val decline = ui.danger("Decline this item") {
            allowed {
                val why = reason.text.toString().trim()
                attempt({ showLine(index) }) { s.decide(line.item, Decision.Decline(why)) }?.let { showLine(index + 1) }
            }
        }
        val stop = ui.danger("Stop and answer nothing") {
            dropSession()
            refresh()
        }
        put("Item ${index + 1} of ${lines.size}", views, listOf(decline, stop))
    }

    /** [v] shown faded and deaf to touch, and hidden from accessibility actions: the hold of an item that cannot be approved. */
    private fun unavailable(v: View): View = object : android.widget.FrameLayout(dark) {
        override fun onInterceptTouchEvent(ev: android.view.MotionEvent): Boolean = true
    }.also {
        it.addView(v)
        it.alpha = 0.4f
        it.isEnabled = false
        it.importantForAccessibility = View.IMPORTANT_FOR_ACCESSIBILITY_NO_HIDE_DESCENDANTS
    }

    /** Whether the library has locked the app again. */
    private fun locked(): Boolean =
        try {
            app?.lockStatus() == LockStatus.Locked
        } catch (_: KeyException) {
            false
        }

    private fun finishSession(s: KeySession) {
        allowed {
            // SR-92: a save the library refused (another screen saved first) ends the session and locks the app, so
            // the way back is the unlock screen, not the line.
            val bytes = attempt({ if (locked()) { dropSession(); refresh(null) } else showLine(lines.size - 1) }) { s.finish() }
                ?: return@allowed
            // The answer holds signatures only. A copy is kept so that a lost file
            // can be saved again: the state was stored before the answer was made.
            try {
                answerFile.writeBytes(bytes)
            } catch (_: IOException) {
            }
            dropSession()
            pendingAnswer = bytes
            showAnswerReady(bytes)
        }
    }

    /** The page after the last line: the answer can be saved as a file or shown as a code. */
    private fun showAnswerReady(bytes: ByteArray) {
        val words = model(AppWords.answersReady)
        put(
            "Key",
            listOf(ui.resultPanel(words)),
            listOf(
                ui.primary("Save the answer file") { saveAnswer(bytes) },
                ui.button("Show as code") { showAnswerCode(bytes) { showAnswerReady(bytes) } },
                ui.button("Done") {
                    pendingAnswer = null
                    refresh()
                },
            ),
            logo = true,
            tone = ui.pal.of(words.colour),
        )
    }

    /**
     * The answer as a code on the screen, for the owner's phone to scan: `ur:bytes` parts, one still code
     * when the answer fits one part, otherwise a code that changes. [back] is where Back goes. If no code
     * can be made the words say so and to save the file instead.
     */
    private fun showAnswerCode(bytes: ByteArray, back: () -> Unit) {
        allowed {
            val metrics = act.resources.displayMetrics
            val made = AnimatedQrView.make(dark, Who.KEY_HOLDER, TYPE_BYTES, bytes, (metrics.heightPixels * 0.6).toInt())
            val view = made.view
            val say = if (view == null) {
                made.words ?: UrWords.cannotShow(Who.KEY_HOLDER)
            } else if (view.animated) {
                UrWords.showingAnimated(Who.KEY_HOLDER)
            } else {
                UrWords.showingStill(Who.KEY_HOLDER)
            }
            val words = WordsModel.of(say)
            val views = ArrayList<View>()
            views.add(ui.compactWords(words))
            views.add(codeBlock(made))
            views.add(ui.hint(words))
            put("Show as code", views, listOf(ui.button("Back") { back() }), back = back)
        }
    }

    private fun writeAnswer(uri: Uri) {
        val bytes = pendingAnswer ?: return refresh()
        try {
            val out = act.contentResolver.openOutputStream(uri, "wt") ?: throw IOException("the file could not be opened")
            out.use {
                it.write(bytes)
                it.flush()
            }
            pendingAnswer = null
            refresh()
        } catch (e: IOException) {
            showMessage(AppWords.fileNotSaved(e.message)) { saveAnswer(bytes) }
        } catch (e: SecurityException) {
            showMessage(AppWords.fileNotSaved(e.message)) { saveAnswer(bytes) }
        }
    }

    private companion object {
        /** The library's rule string for a wrong PIN (SR-73). */
        const val RULE_PIN_WRONG = "pin-wrong"

        const val REQUEST_OPEN = 1
        const val REQUEST_SAVE = 2

        /** OpenBMT's own framed requests and answers travel as `ur:bytes`; a plain PSBT is `ur:crypto-psbt` (UR-20). */
        const val TYPE_BYTES = "bytes"
        const val TYPE_CRYPTO_PSBT = "crypto-psbt"
    }
}
