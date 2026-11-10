package org.openbmt.key.app

import android.accessibilityservice.AccessibilityServiceInfo
import android.content.Intent
import android.net.Uri
import android.provider.Settings
import android.text.Editable
import android.text.TextWatcher
import android.view.View
import android.view.accessibility.AccessibilityManager
import java.io.File
import java.io.IOException
import org.openbmt.key.CeilingsView
import org.openbmt.key.Decision
import org.openbmt.key.EntropySource
import org.openbmt.key.FeeCeilingsView
import org.openbmt.key.KeyApp
import org.openbmt.key.KeyException
import org.openbmt.key.KeyNetwork
import org.openbmt.key.KeySession
import org.openbmt.key.KeyWordCount
import org.openbmt.key.LineView
import org.openbmt.key.SetupState
import org.openbmt.key.WalletSetup
import org.openbmt.key.Words

/**
 * The screens and what they do. Every call into the library goes through
 * [attempt], which shows the library's own words for a refusal. The screens are
 * small on purpose: this is a first build, with no camera (so no QR), and
 * files, pasted text and typed numbers in place of scans.
 */
class Controller(private val act: MainActivity) {
    private val ui = Ui(act)
    private var store: PersistentStore? = null
    private var level: KeyLevel? = null
    private var app: KeyApp? = null
    private var session: KeySession? = null
    private var lines: List<LineView> = emptyList()
    private var pendingAnswer: ByteArray? = null

    /** The network of the next key or wallet; the person picks it on the screens that need it. */
    private var network = KeyNetwork.Mainnet

    /** Set while a system file picker is open: the screen and the session must survive it. */
    private var pickerOpen = false

    /** Set when the screen was cleared because the app went to the background. */
    private var cleared = false

    private val answerFile: File get() = File(act.noBackupFilesDir, "last-answer.bin")

    // ---- start, resume, pause ---------------------------------------------------

    fun start() {
        val refusals = refusals()
        if (refusals.isNotEmpty()) return render(ScreenModel.pick(refusals, null, null))
        close()
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

    fun resumed() {
        if (pickerOpen) return
        if (refusals().isNotEmpty()) {
            dropSession()
            refresh(null)
        } else if (cleared || app == null) {
            cleared = false
            if (app == null) start() else refresh(null)
        }
    }

    fun paused() {
        if (pickerOpen) return
        dropSession()
        pendingAnswer = null
        act.show(View(act))
        cleared = true
    }

    fun close() {
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
        var problem = fault
        val a = app
        if (problem == null && a != null && refusals().isEmpty()) {
            try {
                state = a.state()
            } catch (e: KeyException) {
                problem = e.words ?: AppWords.fault(e)
            }
        }
        render(ScreenModel.pick(refusals(), problem, state), state)
    }

    private fun render(screen: Screen, state: SetupState? = null) {
        when (screen) {
            is Screen.Refused -> act.show(ui.page(screen.words.map { ui.words(it) } + ui.button("Check again") { start() }))
            is Screen.Fault -> act.show(ui.page(listOf(ui.words(screen.words), ui.button("Try again") { start() })))
            is Screen.Setup -> showSetup()
            is Screen.ShowWords -> showKeyWords()
            is Screen.AcceptWallet -> showAcceptWallet(state ?: SetupState.HasIdentity(screen.holdsKey2, false))
            is Screen.Home -> showHome(state ?: SetupState.Ready(screen.holdsKey2))
        }
    }

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
        act.show(ui.page(listOf(ui.words(words), ui.button("Back") { back() })))
    }

    private fun banner(): List<View> =
        if (level == KeyLevel.Software) listOf(ui.words(AppWords.keyInSoftware)) else emptyList()

    private fun networkChoice(): android.widget.RadioGroup {
        val names = listOf("Main bitcoin network", "Test network", "Signet", "Regtest")
        val group = ui.choice(names, network.ordinal)
        group.setOnCheckedChangeListener { _, _ -> network = KeyNetwork.values()[ui.chosen(group).coerceAtLeast(0)] }
        return group
    }

    // ---- first start ----------------------------------------------------------------

    private fun showSetup() {
        val a = app ?: return
        val back = { refresh() }
        act.show(
            ui.page(
                banner() + listOf(
                    ui.words(AppWords.state(SetupState.Empty)),
                    ui.text("Network", 16f, bold = true),
                    networkChoice(),
                    ui.button("Let this device make the key") {
                        allowed { attempt(back) { a.createFromEntropy(EntropySource.Platform, network) }?.let { refresh() } }
                    },
                    ui.button("Make it with dice or coins") { showThrows() },
                    ui.button("Type in words I already have") { showTypedWords() },
                    ui.button("Approver only: no key 2 on this phone") {
                        allowed { attempt(back) { a.createApproverOnly() }?.let { refresh() } }
                    },
                ),
            ),
        )
    }

    private fun showThrows() {
        val a = app ?: return
        val back = { refresh() }
        val method = ui.choice(
            listOf("Dice, hashed (99 rolls, 24 words)", "Dice, skew-corrected", "Coin, skew-corrected"),
        )
        val count = ui.choice(listOf("24 words", "12 words"))
        val input = ui.field("Dice faces 1 to 6, or heads H and tails T", lines = 4)
        val progress = ui.text("", 14f)

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

        act.show(
            ui.page(
                banner() + listOf(
                    ui.text("Make the key yourself", 20f, bold = true),
                    ui.text("Nothing but your throws goes into the key: the same throws give the same words in another tool.", 14f),
                    method, count, input, progress,
                    ui.button("Make the key") {
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
            ),
        )
    }

    private fun showTypedWords() {
        val a = app ?: return
        val input = ui.field("12 or 24 words, with spaces between them", lines = 3)
        val hint = ui.text("", 14f)
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
        act.show(
            ui.page(
                banner() + listOf(
                    ui.text("Type in the words", 20f, bold = true),
                    ui.text("A key made from typed words is a new key here, with its own limits.", 14f),
                    ui.text("Network", 16f, bold = true),
                    networkChoice(),
                    input, hint,
                    ui.button("Use these words") {
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
            ),
        )
    }

    private fun showKeyWords() {
        val a = app ?: return
        val words = attempt({ refresh() }) { a.keyWords() } ?: return
        val shown = ui.text(words.withIndex().joinToString("\n") { (i, w) -> "${i + 1}. $w" }, 20f, mono = true)
        act.show(
            ui.page(
                banner() + listOf(
                    ui.words(AppWords.state(SetupState.HasIdentity(true, true))),
                    shown,
                    ui.button("I have written them down") {
                        shown.text = ""
                        refresh()
                    },
                ),
            ),
        )
    }

    // ---- the wallet ----------------------------------------------------------------------

    private fun showAcceptWallet(state: SetupState) {
        val a = app ?: return
        val details = ArrayList<String>()
        try {
            a.key2Public()?.let {
                details.add("Key 2 on the ${it.network.name.lowercase()} network")
                details.add("Fingerprint: ${it.masterFingerprint}")
                details.add("Path: ${it.originPath}")
                details.add("Account key: ${it.accountXpub}")
            }
            details.add("This app's identity as words: ${a.deviceRootWords().joinToString(" ")}")
        } catch (e: KeyException) {
            return showMessage(e.words ?: AppWords.fault(e)) { refresh() }
        }
        val description = ui.field("The wallet description from the owner's phone", lines = 5)
        val ownerRoot = ui.field("The owner's identity: 64 letters and digits")
        val key3 = ui.field("Key 3's account key (optional)")
        val batch = ui.field("Addresses in a batch (optional)")
        act.show(
            ui.page(
                banner() + listOf(
                    ui.words(AppWords.state(state)),
                    ui.text("Give these to the owner's phone", 16f, bold = true),
                    ui.text(details.joinToString("\n"), 13f, mono = true),
                    ui.text("Network", 16f, bold = true),
                    networkChoice(),
                    description, ownerRoot, key3, batch,
                    ui.button("Accept the wallet") {
                        allowed {
                            val root = Format.fromHex(ownerRoot.text.toString())
                            val count = batch.text.toString().trim().let { if (it.isEmpty()) null else Format.toAmount(it) }
                            when {
                                root == null || root.size != 32 -> showMessage(AppWords.identityNotReadable) { showAcceptWallet(state) }
                                batch.text.toString().isNotBlank() && count == null ->
                                    showMessage(AppWords.notANumber) { showAcceptWallet(state) }
                                else -> {
                                    val setup = WalletSetup(
                                        description.text.toString(), network, root,
                                        key3.text.toString().trim().ifEmpty { null }, count,
                                    )
                                    attempt({ showAcceptWallet(state) }) { a.acceptWallet(setup) }?.let { showWallet(true) }
                                }
                            }
                        }
                    },
                ),
            ),
        )
    }

    private fun showWallet(justAccepted: Boolean) {
        val a = app ?: return
        val back = { refresh() }
        val v = attempt(back) { a.walletView() } ?: return
        val text = StringBuilder()
        text.append("Check words: ${v.checksumWords.joinToString(" ")}\n\n")
        text.append("This app's identity: ${v.ownRootWords.joinToString(" ")}\n")
        text.append("The owner's identity: ${v.ownerRootWords.joinToString(" ")}\n\n")
        for (s in v.slots) text.append("Key ${s.index + 1}: ${s.fingerprint}\n${s.xpub}\n\n")
        text.append("First address:\n${v.firstReceiveAddress}")
        act.show(
            ui.page(
                banner() + listOf(
                    ui.text(
                        if (justAccepted) "Wallet accepted. Compare the check words with the owner's phone." else "The wallet",
                        20f, bold = true,
                    ),
                    ui.text(text.toString(), 13f, mono = true),
                    ui.button("Continue") { refresh() },
                ),
            ),
        )
    }

    private fun showKitCheck() {
        val a = app ?: return
        val input = ui.field("The wallet text from the printed kit", lines = 6)
        act.show(
            ui.page(
                listOf(
                    ui.text("Check the printed kit", 20f, bold = true),
                    ui.text("Paste the wallet text read from the paper. It is compared with this app's own copy.", 14f),
                    input,
                    ui.button("Check") {
                        attempt({ showKitCheck() }) { a.checkKit(input.text.toString()) }?.let {
                            showMessage(AppWords.kit(it)) { refresh() }
                        }
                    },
                    ui.button("Back") { refresh() },
                ),
            ),
        )
    }

    // ---- home ------------------------------------------------------------------------------

    private fun showHome(state: SetupState) {
        val views = ArrayList<View>(banner())
        views.add(ui.words(AppWords.state(state)))
        views.add(ui.button("Answer a request from the owner's phone") { pickRequest() })
        if (answerFile.isFile) views.add(ui.button("Save the last answer again") { saveAnswer(answerFile.readBytes()) })
        views.add(ui.button("The wallet and its check words") { showWallet(false) })
        views.add(ui.button("Check the printed kit") { showKitCheck() })
        views.add(ui.button("Limits and status") { showStatus() })
        views.add(ui.button("About this app") { showAbout() })
        act.show(ui.page(views))
    }

    private fun showAbout() {
        val key = when (level) {
            KeyLevel.StrongBox -> "The saved key is held in the phone's StrongBox chip."
            KeyLevel.Tee -> "The saved key is held in the phone's trusted hardware."
            KeyLevel.Software -> "The saved key is held by software alone. Tests only."
            null -> "Not known."
        }
        act.show(
            ui.page(
                listOf(
                    ui.text("About this app", 20f, bold = true),
                    ui.text("Built by Avila Labs only. No independent party has rebuilt it yet.", 15f),
                    ui.text("First build: no independent security audit has been done. Not for real funds.", 15f, bold = true),
                    ui.text(key, 15f),
                    ui.text("This app asks for no permission, and it has no network access.", 15f),
                    ui.button("Back") { refresh() },
                ),
            ),
        )
    }

    // ---- limits and status --------------------------------------------------------------------

    private fun showStatus() {
        val a = app ?: return
        val back = { refresh() }
        val s = attempt(back) { a.signerStatus() } ?: return
        val t = StringBuilder()
        t.append("To other addresses, most for\n  one payment: ${Format.sats(s.ceilingsOther.perTx)}\n")
        t.append("  a day: ${Format.sats(s.ceilingsOther.perDay)}\n  a week: ${Format.sats(s.ceilingsOther.perWeek)}\n")
        t.append("To saved addresses, most for\n  one payment: ${Format.sats(s.ceilingsAllowlisted.perTx)}\n")
        t.append("  a day: ${Format.sats(s.ceilingsAllowlisted.perDay)}\n  a week: ${Format.sats(s.ceilingsAllowlisted.perWeek)}\n")
        t.append("Fee limit: ${s.feeCeilings.satPerVb} sats for each byte, or ${Format.sats(s.feeCeilings.absoluteSats)} in all\n")
        t.append("Spent today: ${Format.sats(s.otherDay)} to other, ${Format.sats(s.allowlistedDay)} to saved\n")
        t.append("Spent this week: ${Format.sats(s.otherWeek)} to other, ${Format.sats(s.allowlistedWeek)} to saved\n")
        t.append("Payments recorded: ${s.spendLogCount}\n")
        t.append("Last receive address number seen: ${s.receiveMark}\nLast change address number seen: ${s.changeMark}\n")
        t.append("Latest time seen: ${Format.localTime(s.latestTimeSeen)}\n\nSaved addresses\n")
        if (s.allowlist.isEmpty()) t.append("  none\n")
        for (d in s.allowlist) {
            t.append("  ${Format.cleanForeign(d.label)}: ${d.address ?: d.scriptHex}\n")
            t.append("    ${d.activeAt?.let { "active from ${Format.localTime(it)}" } ?: "not active yet"}\n")
        }
        act.show(
            ui.page(
                listOf(
                    ui.text("Limits and status", 20f, bold = true),
                    ui.text(t.toString(), 13f, mono = true),
                    ui.button("Change the limits") { showLimitsForm(s.ceilingsOther, s.ceilingsAllowlisted, s.feeCeilings) },
                    ui.button("Back") { back() },
                ),
            ),
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
        val fields = names.zip(values).map { (n, v) -> ui.field(n).also { it.setText(v.toString()) } }
        val views = ArrayList<View>()
        views.add(ui.text("Change the limits", 20f, bold = true))
        views.add(ui.text("Raising a limit is refused when it would loosen one without the owner's phone's answer; the reason is shown.", 14f))
        for ((n, f) in names.zip(fields)) {
            views.add(ui.text(n, 13f))
            views.add(f)
        }
        views.add(
            ui.button("Save the limits") {
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
        )
        views.add(ui.button("Back") { showStatus() })
        act.show(ui.page(views))
    }

    // ---- the approval session -------------------------------------------------------------------

    private fun pickRequest() {
        pickerOpen = true
        val intent = Intent(Intent.ACTION_OPEN_DOCUMENT).addCategory(Intent.CATEGORY_OPENABLE).setType("*/*")
        startPicker(intent, REQUEST_OPEN)
    }

    private fun saveAnswer(bytes: ByteArray) {
        pendingAnswer = bytes
        pickerOpen = true
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
            pickerOpen = false
            showMessage(AppWords.fileNotRead(e.message)) { refresh() }
        }
    }

    fun picked(code: Int, result: Int, data: Intent?) {
        pickerOpen = false
        val uri = data?.data
        if (result != android.app.Activity.RESULT_OK || uri == null) return refresh()
        when (code) {
            REQUEST_OPEN -> allowed { openRequest(uri) }
            REQUEST_SAVE -> writeAnswer(uri)
        }
    }

    private fun openRequest(uri: Uri) {
        val a = app ?: return
        val bytes = try {
            readLimited(uri)
        } catch (e: IOException) {
            return showMessage(AppWords.fileNotRead(e.message)) { refresh() }
        } catch (e: SecurityException) {
            return showMessage(AppWords.fileNotRead(e.message)) { refresh() }
        }
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
        views.add(ui.text("Item ${index + 1} of ${lines.size}", 14f))
        views.add(ui.text(t.title, 22f, bold = true))
        if (t.rows.isNotEmpty()) views.add(ui.text(t.rows.joinToString("\n\n"), 16f))
        if (t.claims.isNotEmpty()) {
            views.add(ui.text("The owner's phone says this. This app cannot check it:", 14f, bold = true))
            views.add(ui.text(t.claims.joinToString("\n"), 14f))
        }
        for (q in t.quoted) views.add(ui.quoted("Text from elsewhere, not written by this app. ${q.from}:", "“${q.text}”"))
        t.failure?.let {
            views.add(ui.text("This item cannot be approved. The checks said:", 14f, bold = true, colour = 0xFFC62828.toInt()))
            views.add(ui.text(it, 14f))
        }
        val reason = ui.field("Why you decline (optional)")
        val approve = ui.button("Approve this item") {
            allowed { attempt({ showLine(index) }) { s.decide(line.item, Decision.Approve) }?.let { showLine(index + 1) } }
        }
        approve.isEnabled = t.canApprove
        views.add(approve)
        views.add(reason)
        views.add(
            ui.button("Decline this item") {
                allowed {
                    val why = reason.text.toString().trim()
                    attempt({ showLine(index) }) { s.decide(line.item, Decision.Decline(why)) }?.let { showLine(index + 1) }
                }
            },
        )
        views.add(
            ui.button("Stop and answer nothing") {
                dropSession()
                refresh()
            },
        )
        act.show(ui.page(views))
    }

    private fun finishSession(s: KeySession) {
        allowed {
            val bytes = attempt({ showLine(lines.size - 1) }) { s.finish() } ?: return@allowed
            // The answer holds signatures only. A copy is kept so that a lost file
            // can be saved again: the state was stored before the answer was made.
            try {
                answerFile.writeBytes(bytes)
            } catch (_: IOException) {
            }
            dropSession()
            pendingAnswer = bytes
            act.show(
                ui.page(
                    listOf(
                        ui.words(AppWords.answersReady),
                        ui.button("Save the answer file") { saveAnswer(bytes) },
                        ui.button("Done") {
                            pendingAnswer = null
                            refresh()
                        },
                    ),
                ),
            )
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
        const val REQUEST_OPEN = 1
        const val REQUEST_SAVE = 2
    }
}
