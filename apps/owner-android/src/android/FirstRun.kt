package org.openbmt.owner.app

import android.graphics.drawable.GradientDrawable
import android.content.Intent
import android.text.Editable
import android.text.InputType
import android.text.TextWatcher
import android.view.Gravity
import android.view.View
import android.view.ViewGroup
import android.widget.Button
import android.widget.EditText
import android.widget.LinearLayout
import android.widget.RadioGroup
import android.widget.TextView
import java.io.IOException
import java.util.Locale
import org.openbmt.app.AnimatedQrView
import org.openbmt.app.RowItem
import org.openbmt.app.Space
import org.openbmt.app.Type
import org.openbmt.app.UrWords
import org.openbmt.app.Who
import org.openbmt.owner.OwnerException
import org.openbmt.owner.OwnerNode
import org.openbmt.owner.ShopCurrency

// The first run of the owner app (docs/DESIGN.md section 2.1): the welcome, the practice shop, the shop, the
// till, where the money goes, the hardware wallet's guide, the instant limit and the first sale. Every sentence is in FirstRunWords. The practice shop lives in memory only: it
// writes no file, sends nothing and calls nothing in the library. The shop's screens keep the name, the money and
// the till in the shop file of the app's folder, which holds no secret. Back on every screen returns to the
// step before, with what was entered.

/** The state of the first run, made when it is first needed from the shop file and the phone's country. */
private fun Controller.run(): FirstRun =
    firstRun ?: FirstRun(shopFile.load(), Locale.getDefault().country).also { firstRun = it }

/** The first screen of an app with nothing saved: a key ceremony left unfinished is resumed, else the first run. */
internal fun Controller.showEntry() {
    if (ceremonyPending()) return showFirstStart(null)
    showFirstRun()
}

/** The screen the first run is at: the practice shop when one is open, else the step. */
internal fun Controller.showFirstRun() {
    val p = practice
    if (p != null) return showPractice(p)
    val r = run()
    when (r.step) {
        FirstRunStep.Welcome -> showWelcome()
        FirstRunStep.Before -> showBefore(r)
        FirstRunStep.Shop -> showShop(r)
        FirstRunStep.Till -> showTillChoice(r)
        FirstRunStep.Money -> showMoney(r)
        FirstRunStep.Hardware -> showHardware(r)
        FirstRunStep.Helper -> showHelper(r)
        FirstRunStep.OwnAdvice -> showOwnAdvice(r)
        FirstRunStep.Connect -> showConnect(r)
        FirstRunStep.Starter -> showStarterStep(r)
        FirstRunStep.OwnCounter -> showOwnCounter(r)
        FirstRunStep.Instant -> showInstant(r)
        FirstRunStep.FirstSale -> showFirstSale(r)
        FirstRunStep.Why -> showWhy(r)
        FirstRunStep.RealCases -> showRealCases(r)
        FirstRunStep.Guide -> showKeyGuide(r)
        FirstRunStep.NotYet -> showNotYet(r)
    }
}

/** A page of the first run with a way back: the system's Back does the same as the bar's arrow. */
internal fun Controller.stepScreen(title: String, body: List<View>, actions: List<Button> = emptyList(), back: () -> Unit, tone: org.openbmt.app.Tone? = null) {
    screen(title, body, actions, back = back, tone = tone)
    firstRunBack = back
}

internal fun Controller.heading(text: String): TextView = ui.text(text, Type.HEADING, bold = true)

// ---- welcome -------------------------------------------------------------------------------------------------------

private fun Controller.showWelcome() {
    val r = run()
    r.toWelcome()
    val views = ArrayList<View>()
    welcomeNotice?.let { views.add(line(it, bold = true)); welcomeNotice = null }
    views.add(0, ui.welcomePicture())
    views.add(heading(FirstRunWords.welcomeTitle))
    views.add(para(FirstRunWords.welcomeText))
    views.add(line(FirstRunWords.welcomeLine, bold = true))
    views.add(line(FirstRunWords.welcomeNew, muted = true))
    val tryIt = ui.button(FirstRunWords.welcomeTry) { startPractice() }
    views.add(stack(tryIt, line(FirstRunWords.welcomeTryNote, muted = true), gap = 6))
    screen("Owner", views, listOf(ui.primary(FirstRunWords.welcomeSetup) { run().next(); showFirstRun() }), logo = true)
}

// ---- before starting -------------------------------------------------------------------------------------------------------

private fun Controller.showBefore(r: FirstRun) {
    stepScreen(
        FirstRunWords.shopTitle, listOf(heading(FirstRunWords.beforeTitle), para(FirstRunWords.beforeText), line(FirstRunWords.beforeResume, muted = true)),
        listOf(ui.primary(FirstRunWords.beforeGo) { r.next(); showFirstRun() }), { r.back(); showFirstRun() },
    )
}

// ---- the practice shop -----------------------------------------------------------------------------------------------

private fun Controller.startPractice() {
    practice = Practice(shopFile.load().currency ?: ShopCurrency.Usd) { nowSecs() }
    showFirstRun()
}

/** A page of the practice shop: the banner is the bar, tinted, so that every screen carries it. */
private fun Controller.practicePage(views: List<View>, actions: List<Button>, back: () -> Unit) {
    stepScreen(FirstRunWords.practiceBanner, views, actions, back, tone = ui.pal.warn)
}

private fun Controller.showPractice(p: Practice) {
    val back: () -> Unit = { if (p.back()) showFirstRun() else leavePractice() }
    fun go() { p.next(); showFirstRun() }
    when (p.step) {
        PracticeStep.Intro -> practicePage(
            listOf(ui.practicePicture(), heading(FirstRunWords.practiceIntroTitle), para(FirstRunWords.practiceIntroText)),
            listOf(ui.primary(FirstRunWords.practiceStart) { go() }), back,
        )
        PracticeStep.Amount -> showPracticeAmount(p, back)
        PracticeStep.Qr -> {
            val made = AnimatedQrView.makePlain(act, Who.OWNER, Practice.QR_TEXT, ExchangeModel.codeHeight(act.resources.displayMetrics.heightPixels))
            val code = made.view?.let { ui.codeCard(it) } ?: ui.words(made.words ?: UrWords.cannotShow(Who.OWNER))
            practicePage(
                listOf(heading(FirstRunWords.practiceQrTitle), code, para(FirstRunWords.practiceQrText)),
                listOf(ui.primary(FirstRunWords.practicePayButton) { go() }), back,
            )
        }
        PracticeStep.Paid -> practicePage(
            listOf(heading(FirstRunWords.practicePaidTitle), para(FirstRunWords.practicePaidText)),
            listOf(ui.primary(FirstRunWords.practiceBooksTitle) { go() }), back,
        )
        PracticeStep.Books -> practicePage(
            listOf(heading(FirstRunWords.practiceBooksTitle), para(FirstRunWords.practiceBooksText), ui.facts(listOf(p.booksRow() to ""))),
            listOf(ui.primary(FirstRunWords.practiceRefundButton) { go() }), back,
        )
        PracticeStep.Refund -> practicePage(
            listOf(heading(FirstRunWords.practiceRefundButton), para(FirstRunWords.practiceRefundText)),
            listOf(ui.primary(FirstRunWords.practiceRefundDone) { go() }), back,
        )
        PracticeStep.Done -> practicePage(
            listOf(heading(FirstRunWords.practiceDoneTitle), para(FirstRunWords.practiceDoneText)),
            listOf(
                ui.primary(FirstRunWords.practiceSetup) { practice = null; run().next(); showFirstRun() },
                ui.button(FirstRunWords.practiceAgain) { practice = p.again(); showFirstRun() },
                ui.danger(FirstRunWords.practiceDelete) {
                    practice = null
                    welcomeNotice = FirstRunWords.practiceDeleted
                    run().toWelcome()
                    showFirstRun()
                },
            ),
            back,
        )
    }
}

/** Leaves the practice shop for the welcome: nothing of it was kept. */
private fun Controller.leavePractice() {
    practice = null
    run().toWelcome()
    showFirstRun()
}

/** The amount: the figure large, a number pad under it, and Charge. */
private fun Controller.showPracticeAmount(p: Practice, back: () -> Unit) {
    val shown = ui.text(p.amountText(), Type.AMOUNT, bold = true).apply { gravity = Gravity.CENTER }
    val charge = ui.primary(FirstRunWords.practiceCharge) { p.next(); showFirstRun() }
    fun refresh() {
        shown.text = p.amountText()
        charge.isEnabled = p.canCharge
    }
    fun key(label: String, onTap: () -> Unit): Button = ui.button(label) { onTap(); refresh() }
    val pad = LinearLayout(act).apply { orientation = LinearLayout.VERTICAL }
    val labels = listOf(listOf("1", "2", "3"), listOf("4", "5", "6"), listOf("7", "8", "9"), listOf("", "0", "⌫"))
    for (row in labels) {
        val r = LinearLayout(act).apply { orientation = LinearLayout.HORIZONTAL }
        for (k in row) {
            val b: View = when (k) {
                "" -> View(act)
                "⌫" -> key(k) { p.delete() }
                else -> key(k) { p.digit(k.toInt()) }
            }
            r.addView(b, LinearLayout.LayoutParams(0, ui.px(64), 1f).also { it.setMargins(ui.px(4), ui.px(4), ui.px(4), ui.px(4)) })
        }
        pad.addView(r, LinearLayout.LayoutParams(-1, -2))
    }
    refresh()
    practicePage(listOf(heading(FirstRunWords.practiceAmountTitle), shown, pad), listOf(charge), back)
}

// ---- your shop -------------------------------------------------------------------------------------------------------------

private fun Controller.showShop(r: FirstRun) {
    val name = textInput(FirstRunWords.shopNameLabel, FirstRunWords.shopNameHint).also { it.field.setText(r.name) }
    val money = moneyPicker(r.moneyTabs)
    val msg = Slot(act)
    fun capture() {
        r.name = name.field.text.toString()
    }
    val body = listOf(heading(FirstRunWords.shopTitle), name.view, stack(label(FirstRunWords.shopCurrencyLabel), money), msg)
    val back = { capture(); r.back(); showFirstRun() }
    stepScreen(
        FirstRunWords.shopTitle, body,
        listOf(
            ui.primary(FirstRunWords.shopNext) {
                capture()
                try {
                    shopFile.save(shopFile.load().with(name = r.name, currency = r.currency))
                    r.next()
                    showFirstRun()
                } catch (e: IOException) {
                    msg.show(ui.words(AppWords.fileNotSaved(e.message)))
                }
            },
        ),
        back,
    )
}

// ---- how you take payments now ----------------------------------------------------------------------------------------------

private fun Controller.showTillChoice(r: FirstRun) {
    val search = ui.field(FirstRunWords.tillSearch).also { it.setText(r.query) }
    val list = Slot(act, 4)
    val next = ui.primary(FirstRunWords.tillNext) {}
    val msg = Slot(act)
    fun fill() {
        val tills = Till.search(r.query)
        val group: RadioGroup = ui.choice(tills.map { it.title }, tills.indexOf(r.till))
        group.setOnCheckedChangeListener { _, _ ->
            tills.getOrNull(ui.chosen(group))?.let { r.till = it }
            next.isEnabled = r.till != null
        }
        list.show(group)
        next.isEnabled = r.till != null
    }
    search.addTextChangedListener(
        object : TextWatcher {
            override fun beforeTextChanged(s: CharSequence?, start: Int, count: Int, after: Int) {}

            override fun onTextChanged(s: CharSequence?, start: Int, before: Int, count: Int) {}

            override fun afterTextChanged(s: Editable?) {
                r.query = s?.toString() ?: ""
                fill()
            }
        },
    )
    fill()
    next.setOnClickListener {
        val t = r.till ?: return@setOnClickListener
        try {
            // A new till is a new set of steps: they are shown again after the first counter.
            val held = shopFile.load()
            shopFile.save(held.with(till = t, stepsShown = held.stepsShown && held.till == t, cardPrinted = held.cardPrinted && held.till == t))
            r.next()
            showFirstRun()
        } catch (e: IOException) {
            msg.show(ui.words(AppWords.fileNotSaved(e.message)))
        }
    }
    stepScreen(
        FirstRunWords.shopTitle, listOf(ui.tillPicture(), heading(FirstRunWords.tillTitle), para(FirstRunWords.tillText), line(FirstRunWords.tillReads, muted = true), search, list, msg),
        listOf(next), { r.back(); showFirstRun() },
    )
}

// ---- where the money goes ----------------------------------------------------------------------------------------------------

private fun Controller.showMoney(r: FirstRun) {
    fun pick(c: MoneyChoice): () -> Unit = { r.choose(c); showFirstRun() }
    val main = listOf(
        RowItem(FirstRunWords.moneyPlan, FirstRunWords.moneyPlanToday + "\n" + FirstRunWords.moneyPlanLater, onClick = pick(MoneyChoice.Plan)),
        RowItem(FirstRunWords.moneyOwn, FirstRunWords.moneyOwnNote, onClick = pick(MoneyChoice.Own)),
    )
    val other = ui.rows(
        listOf(
            RowItem(FirstRunWords.moneyMake, FirstRunWords.moneyMakeNote, onClick = pick(MoneyChoice.Make)),
            RowItem(FirstRunWords.moneyDiy, FirstRunWords.moneyDiyNote, onClick = pick(MoneyChoice.Diy)),
        ),
    )
    val later = listOf(RowItem(FirstRunWords.moneyLater, FirstRunWords.moneyLaterNote, onClick = pick(MoneyChoice.Later)))
    stepScreen(
        FirstRunWords.shopTitle,
        listOf(ui.moneyPicture(), heading(FirstRunWords.moneyTitle), para(FirstRunWords.moneyText), ui.rows(main)) +
            fold(FirstRunWords.moneyOther, other) + ui.rows(later),
        back = { r.back(); showFirstRun() },
    )
}

/** A title that opens and closes [content], closed at first: the first layers of a screen stay short (DESIGN 2.1). */
private fun Controller.fold(title: String, content: View): List<View> {
    content.visibility = View.GONE
    val toggle = ui.text("\u25B8 $title", Type.SECONDARY, bold = true, colour = ui.pal.info.ink)
    toggle.minimumHeight = ui.px(Space.TOUCH)
    toggle.gravity = Gravity.CENTER_VERTICAL
    toggle.setOnClickListener {
        val open = content.visibility != View.VISIBLE
        content.visibility = if (open) View.VISIBLE else View.GONE
        toggle.text = (if (open) "\u25BE " else "\u25B8 ") + title
    }
    return listOf(toggle, content)
}

// ---- getting a hardware wallet ---------------------------------------------------------------------------------------------

private fun Controller.showHardware(r: FirstRun) {
    val views = ArrayList<View>()
    views.add(heading(FirstRunWords.hwTitle))
    views.add(para(FirstRunWords.hwText))
    for (rule in HelperPage.rules) views.add(ui.text("\u2022 $rule", 15f))
    views.add(line(FirstRunWords.hwNoBrand, muted = true))
    views.add(stack(ui.button(FirstRunWords.hwHelper) { r.openHelper(); showFirstRun() }, line(FirstRunWords.hwHelperNote, muted = true), gap = 6))
    views.add(para(FirstRunWords.hwArrives))
    // The same call as the start of the starter wallet's setup has always had: the next step is Starter.
    stepScreen(FirstRunWords.shopTitle, views, listOf(ui.primary(FirstRunWords.hwNext) { r.next(); showFirstRun() }), { r.back(); showFirstRun() })
}

/** The page for the helper: the rules and the steps, and the same as plain text through Android's share sheet. */
private fun Controller.showHelper(r: FirstRun) {
    val views = ArrayList<View>()
    views.add(heading(FirstRunWords.helperTitle))
    views.add(para(FirstRunWords.helperIntro))
    views.add(label(FirstRunWords.helperBuy))
    for (rule in HelperPage.rules) views.add(ui.text("\u2022 $rule", 15f))
    views.add(label(FirstRunWords.helperArrives))
    for (step in HelperPage.steps) views.add(ui.text(step, 15f))
    val share = ui.primary(FirstRunWords.hwHelper) {
        val send = Intent(Intent.ACTION_SEND).setType("text/plain").putExtra(Intent.EXTRA_TEXT, HelperPage.text())
        act.startActivity(Intent.createChooser(send, null))
    }
    stepScreen(FirstRunWords.helperTitle, views, listOf(share), { r.back(); showFirstRun() })
}

// ---- instant payments --------------------------------------------------------------------------------------------------------

/** The limit the shop's settings hold for one light-checked sale, in whole units of its money; null when the node holds none yet. */
private fun Controller.heldInstant(): Long? = try {
    node?.settings()?.lightPerSale?.displayUsd
} catch (_: OwnerException) {
    null
} catch (_: LinkageError) {
    null
}

private fun Controller.showInstant(r: FirstRun) {
    val limit = ui.field(FirstRunWords.instantLabel).also {
        it.inputType = InputType.TYPE_CLASS_NUMBER
        it.setText(r.instantTyped ?: InstantStep.initial(heldInstant()))
    }
    val typical = ui.field(FirstRunWords.instantTypical).also {
        it.inputType = InputType.TYPE_CLASS_NUMBER
        it.setText(r.typicalTyped)
    }
    val msg = Slot(act)
    fun capture() {
        r.instantTyped = limit.text.toString()
        r.typicalTyped = typical.text.toString()
    }
    val how = stack(para(FirstRunWords.instantHow1), para(FirstRunWords.instantHow2))
    val details = stack(para(FirstRunWords.instantDetails))
    val body = ArrayList<View>()
    body.add(heading(FirstRunWords.instantTitle))
    body.add(para(FirstRunWords.instantText))
    body.add(ui.labelled(FirstRunWords.instantLabel, limit))
    body.add(ui.labelled(FirstRunWords.instantTypical, typical))
    body.add(line(FirstRunWords.instantZero, muted = true))
    body.add(msg)
    body.addAll(fold(FirstRunWords.howThisWorks, how))
    body.addAll(fold(FirstRunWords.details, details))
    stepScreen(
        FirstRunWords.shopTitle, body,
        listOf(ui.primary(FirstRunWords.instantNext) { capture(); writeInstant(r, msg) }),
        { capture(); r.back(); showFirstRun() },
    )
}

/**
 * Writes the limit the way the settings screen does (SettingsEdit.kt): the node judges the one-field edit, and what it
 * would change is queued with the bytes it gave. The first run holds no settings of its own, so there is no other place
 * to write it. The limit is only lowered or kept (more than the shop's is refused with the words under the field), which is
 * a tightening that takes effect when delivered. Nothing is queued when the settings already hold the number or this step
 * queued it. While the starter wallet holds the shop at its limit or date (SS-37) nothing is written and the step goes on.
 * With no node or no settings yet the step stays, with the library's words.
 */
private fun Controller.writeInstant(r: FirstRun, msg: Slot) {
    val typed = r.instantTyped ?: ""
    if (InstantStep.parse(typed) == null) {
        msg.show(ui.words(AppWords.editValueNotRead(FieldKind.Dollars, FirstRunWords.instantLabel, shopFile.load().currency ?: ShopCurrency.Usd)))
        return
    }
    if (starterHolds(org.openbmt.owner.StarterChangeKind.Other)) {
        r.skipInstant()
        return showFirstRun()
    }
    val n = node
    val held = try {
        n?.settings()
    } catch (e: OwnerException) {
        return msg.show(ui.words(e.sayWords()))
    } catch (e: LinkageError) {
        return msg.show(ui.words(AppWords.libraryMissing(e.message)))
    }
    if (n == null || held == null) return msg.show(ui.words(AppWords.noSettingsYet))
    val result = try {
        r.saveInstant(typed, held.lightPerSale.displayUsd) { v ->
            val p = n.previewSettings(InstantStep.edit(v))
            if (!p.nothingChanged) n.queueSettings(p.settings)
        }
    } catch (e: OwnerException) {
        return msg.show(ui.words(e.sayWords()))
    } catch (e: LinkageError) {
        return msg.show(ui.words(AppWords.libraryMissing(e.message)))
    }
    when (result) {
        InstantResult.Saved -> showFirstRun()
        InstantResult.Higher -> msg.show(line(FirstRunWords.instantHigher, bold = true))
        else -> {}
    }
}

// ---- a first real sale ---------------------------------------------------------------------------------------------------------

private fun Controller.showFirstSale(r: FirstRun) {
    val sale = r.firstSale
    val back = { r.back(); showFirstRun() }
    val body = ArrayList<View>()
    val actions = ArrayList<Button>()
    if (!sale.paid) {
        body.add(heading(FirstRunWords.firstSaleTitle))
        body.add(para(FirstRunWords.firstSaleText))
        body.add(line(FirstRunWords.firstSaleWaiting, muted = true))
        actions.add(ui.secondary(FirstRunWords.firstSaleSkip) { r.next(); showFirstRun() })
    } else {
        body.add(heading(FirstRunWords.firstSalePaid))
        if (sale.seeing) {
            body.add(para(FirstRunWords.firstSaleSeeText))
            sale.txid?.let { body.add(ui.text(it, 13f, mono = true)) }
        } else {
            body.add(ui.button(FirstRunWords.firstSaleSee) { seeFirstSale(r) })
        }
        actions.add(ui.primary(FirstRunWords.firstSaleDone) { r.next(); showFirstRun() })
    }
    stepScreen(FirstRunWords.shopTitle, body, actions, back)
    if (!sale.paid) {
        // The link's turns call this every few seconds: the first paid sale of the books ends the waiting.
        tick = { if (node?.let { n -> paidSalesOf(n)?.let { c -> sale.observe(c) } } == true) showFirstRun() }
        tick?.invoke()
    }
}

/** The shop's paid sales as the books count them; null when the node cannot say. */
internal fun Controller.paidSalesOf(n: OwnerNode): Long? = try {
    FirstSale.paidSales(n.books(java.util.TimeZone.getDefault().getOffset(System.currentTimeMillis()) / 60_000))
} catch (_: OwnerException) {
    null
} catch (_: LinkageError) {
    null
}

/**
 * "See it yourself": the payment's ID, read from the full ledger, which the books keep behind their own warning (BK-19),
 * so the warning is shown first. No network is asked and no link is made.
 */
private fun Controller.seeFirstSale(r: FirstRun) {
    val n = node ?: return begin()
    val msg = Slot(act)
    val ask = BooksModel.LedgerAsk("", { n.booksLedger(it) })
    ask.ask()
    val leave = { ask.cancel(); showFirstRun() }
    val accept = ui.primary(ask.acceptLabel) {
        val bytes = attempt(msg) { n.booksLedger(true) } ?: return@primary
        r.firstSale.see(FirstSale.firstTxid(bytes))
        showFirstRun()
    }
    stepScreen(ask.title, listOf(para(ask.text), msg), listOf(accept, ui.button(ask.cancelLabel) { leave() }), leave)
}

// ---- the home checklist ---------------------------------------------------------------------------------------------------------

/**
 * The small checklist of the owner home screen (DESIGN 2.1): a first sale until one is paid, and the second copy of the
 * starter words when the owner skipped it. Null when nothing is left. The books are read at most every 15 seconds, and a
 * paid sale stays seen.
 */
internal fun Controller.homeChecklist(n: OwnerNode): View? {
    val now = System.currentTimeMillis()
    if (!checklistSaleSeen && now - checklistAt >= CHECKLIST_EVERY_MS) {
        checklistAt = now
        checklistSaleSeen = (paidSalesOf(n) ?: 0L) >= 1
    }
    val items = Checklist.items(checklistSaleSeen, secondCopySkipped(), Checklist.cardDue(shopFile.load()))
    if (items.isEmpty()) return null
    return stack(
        label(FirstRunWords.checklistTitle),
        ui.rows(items.map { i -> RowItem(i.text, i.note, onClick = if (i.cueCard) ({ showCueCard { showHome() } }) else null) }),
        gap = 6,
    )
}

private const val CHECKLIST_EVERY_MS = 15_000L

/** The owner has a starter wallet and did not write the second copy of its words: read from the starter record. */
private fun Controller.secondCopySkipped(): Boolean = Checklist.copyDue(starter.store.hasSealed(), starterPinned(), starterRecord())


/** The advice on an own wallet: a cold wallet is recommended; the owner may make one, or go on with the wallet they have. */
private fun Controller.showOwnAdvice(r: FirstRun) {
    stepScreen(
        FirstRunWords.shopTitle,
        listOf(heading(FirstRunWords.ownAdviceTitle), para(FirstRunWords.ownAdviceText), line(FirstRunWords.ownAdviceHot, muted = true), ui.secondary(FirstRunWords.ownAdviceMake) { r.makeInstead(); showFirstRun() }),
        listOf(ui.primary(FirstRunWords.ownAdviceGo) { r.next(); showFirstRun() }), { r.back(); showFirstRun() },
    )
}

private fun Controller.showWhy(r: FirstRun) {
    stepScreen(
        FirstRunWords.shopTitle,
        listOf(ui.whyPicture(), heading(FirstRunWords.whyTitle), para(FirstRunWords.whyText1), para(FirstRunWords.whyText2), para(FirstRunWords.whyText3), ui.button(FirstRunWords.whyRealCases) { r.openRealCases(); showFirstRun() }),
        listOf(ui.primary(FirstRunWords.shopNext) { r.next(); showFirstRun() }), { r.back(); showFirstRun() },
    )
}

/** One page of the real cases: a case, or the page on the owner's own bitcoin. Words only, no link, no picture, no network. */
private fun Controller.showRealCases(r: FirstRun) {
    val page = r.realPage
    val pal = ui.pal
    val back: () -> Unit = { r.back(); showFirstRun() }
    val next: () -> Unit = { r.realNext(); showFirstRun() }
    fun serif(text: String, size: Float, spacing: Float, medium: Boolean = false, colour: Int? = null): TextView =
        ui.text(text, size, medium = medium, serif = true, colour = colour).also { it.setLineSpacing(0f, spacing) }
    fun sans(text: String, size: Float, colour: Int, spacing: Float = 1f): TextView =
        ui.text(text, size, colour = colour).also { it.setLineSpacing(0f, spacing) }
    fun below(v: View, top: Int): View = v.also { it.layoutParams = ui.margins(top = top) }
    val column = LinearLayout(act).apply {
        orientation = LinearLayout.VERTICAL
        setPadding(ui.px(8), ui.px(0), ui.px(8), 0)
    }
    val actions: List<Button>
    if (page < FirstRun.REAL_PAGES - 1) {
        val cases = listOf(
            listOf(FirstRunWords.realCase1Place, FirstRunWords.realCase1Head, FirstRunWords.realCase1Text, FirstRunWords.realCase1Quote, FirstRunWords.realCase1Who),
            listOf(FirstRunWords.realCase2Place, FirstRunWords.realCase2Head, FirstRunWords.realCase2Text, FirstRunWords.realCase2Quote, FirstRunWords.realCase2Who),
            listOf(FirstRunWords.realCase3Place, FirstRunWords.realCase3Head, FirstRunWords.realCase3Text, FirstRunWords.realCase3Quote, FirstRunWords.realCase3Who),
            listOf(FirstRunWords.realCase4Place, FirstRunWords.realCase4Head, FirstRunWords.realCase4Text, FirstRunWords.realCase4Quote, FirstRunWords.realCase4Who),
        )
        val (place, head, text, quote, who) = cases[page]
        val count = LinearLayout(act).apply { orientation = LinearLayout.HORIZONTAL; gravity = Gravity.CENTER_VERTICAL }
        count.addView(sans(FirstRunWords.count(page + 1, cases.size), 14f, pal.hint), LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f))
        for (i in cases.indices) {
            count.addView(
                View(act).apply {
                    background = GradientDrawable().apply { cornerRadius = ui.px(3).toFloat(); setColor(if (i == page) pal.text else pal.fieldLine) }
                },
                LinearLayout.LayoutParams(ui.px(if (i == page) 18 else 6), ui.px(6)).also { if (i > 0) it.leftMargin = ui.px(6) },
            )
        }
        column.addView(count, ui.margins(top = 0, bottom = 22))
        if (page == 0) column.addView(below(sans(FirstRunWords.realIntro, 15.5f, pal.secondary, 1.28f), 0).also { it.layoutParams = ui.margins(bottom = 26) })
        column.addView(sans(place, 14f, pal.hint), ui.margins(bottom = 10))
        column.addView(serif(head, 27f, 1.0f, medium = true), ui.margins(bottom = 16))
        column.addView(sans(text, 15.5f, pal.secondary, 1.3f))
        column.addView(serif(quote, 23f, 1.12f), ui.margins(top = 26, bottom = 6))
        column.addView(sans(who, 13.5f, pal.hint))
        val last = page == cases.size - 1
        actions = listOf(
            ui.secondary(KeyGuideWords.kgBack) { back() }.also { it.minHeight = ui.px(Space.PRIMARY); it.minimumHeight = ui.px(Space.PRIMARY) },
            ui.primary(if (last) FirstRunWords.realToBitcoin else FirstRunWords.realNext) { next() },
        )
    } else {
        column.addView(serif(FirstRunWords.realBitcoinHead, 27f, 1.0f, medium = true), ui.margins(top = 0, bottom = 16))
        column.addView(sans(FirstRunWords.realBitcoinText, 15.5f, pal.secondary, 1.3f))
        for ((title, text) in listOf(FirstRunWords.realKeysTitle to FirstRunWords.realKeysText, FirstRunWords.realCoinTitle to FirstRunWords.realCoinText)) {
            column.addView(serif(title, 20f, 1.05f), ui.margins(top = 28, bottom = 8))
            column.addView(sans(text, 15.5f, pal.secondary, 1.3f))
        }
        column.addView(sans(FirstRunWords.realMore, 13.5f, pal.hint, 1.2f), ui.margins(top = 28))
        actions = listOf(ui.primary(FirstRunWords.realDone) { r.realDone(); showFirstRun() })
    }
    tick = null
    val screen = ui.page(ui.bar(FirstRunWords.realBar, back = back, ground = pal.paper), listOf(column), actions, ground = pal.paper, across = actions.size > 1)
    act.show(SwipePages(act, screen, ui.px(80), onNext = { if (page < FirstRun.REAL_PAGES - 1) next() }, onPrevious = back))
    firstRunBack = back
}

/** This build has no path of its own for the three choices yet: the existing setup follows. */
private fun Controller.showNotYet(r: FirstRun) {
    stepScreen(
        FirstRunWords.shopTitle, listOf(para(FirstRunWords.moneyNotYet)),
        listOf(ui.primary(FirstRunWords.shopNext) { showFirstStart(null) }), { r.back(); showFirstRun() },
    )
}

// ---- the till steps ------------------------------------------------------------------------------------------------------------

/** The steps for [till]: after the first counter, and from Settings at any time. [back] is where Done and Back lead. */
internal fun Controller.showTillSteps(till: Till, back: () -> Unit) {
    val page = TillStepsPage.of(till)
    val views = ArrayList<View>()
    views.add(heading(page.title))
    for (l in page.lines) {
        views.add(
            when (l.kind) {
                TillStepsPage.Kind.Step -> ui.text(l.text, 16f)
                TillStepsPage.Kind.Small -> line(l.text, muted = true)
                TillStepsPage.Kind.Body -> para(l.text)
            },
        )
    }
    views.add(ui.secondary(FirstRunWords.cardOpen) { showCueCard { showTillSteps(till, back) } })
    stepScreen(page.title, views, listOf(ui.primary(FirstRunWords.tillStepsDone) { back() }), back)
}

/** The staff cue card for the chosen till, to print. [back] is where Back leads. */
internal fun Controller.showCueCard(back: () -> Unit) {
    val held = shopFile.load()
    val till = held.till ?: return back()
    val card = CueCard.of(till, held.name)
    val msg = Slot(act)
    val views = ArrayList<View>()
    views.add(heading(card.title))
    card.lines.forEachIndexed { i, l ->
        views.add(if (i == card.lines.lastIndex) line(l, muted = true) else ui.text("${i + 1}. $l", 16f))
    }
    views.add(para(FirstRunWords.cardScreenText))
    views.add(msg)
    val print = ui.primary(FirstRunWords.cardPrint) {
        val sent = printCueCard(
            card,
            onPrinted = {
                try {
                    shopFile.save(shopFile.load().with(cardPrinted = true))
                } catch (_: IOException) {
                    // The card is printed; at worst it is listed once more.
                }
            },
            onFailed = { msg.show(line(FirstRunWords.cardPrintFailed, bold = true)) },
        )
        if (!sent) msg.show(line(FirstRunWords.cardPrintFailed, bold = true))
    }
    stepScreen(FirstRunWords.cardScreenTitle, views, listOf(print), back)
}

/** What follows the first counter: the steps for the chosen till once, then home. */
internal fun Controller.afterFirstCounter() {
    val held = shopFile.load()
    val till = held.till
    if (till == null || held.stepsShown) return showHome()
    try {
        shopFile.save(held.with(stepsShown = true))
    } catch (_: IOException) {
        // The steps are shown now; at worst they are shown once more.
    }
    showTillSteps(till) { showHome() }
}

/** The Settings row for the till steps: named for the chosen till, or null when none was chosen. */
internal fun Controller.tillStepsRow(back: () -> Unit): RowItem? {
    val till = shopFile.load().till ?: return null
    val title = if (till.listed) FirstRunWords.with(FirstRunWords.tillStepsTitle, till.title) else FirstRunWords.tillTitle
    return RowItem(title) { showTillSteps(till, back) }
}
