package org.openbmt.owner.app

import android.text.Editable
import android.text.TextWatcher
import android.view.Gravity
import android.view.View
import android.widget.Button
import android.widget.EditText
import android.widget.LinearLayout
import android.widget.RadioGroup
import android.widget.TextView
import java.io.IOException
import java.util.Locale
import org.openbmt.app.AnimatedQrView
import org.openbmt.app.RowItem
import org.openbmt.app.Type
import org.openbmt.app.UrWords
import org.openbmt.app.Who
import org.openbmt.owner.ShopCurrency

// The first run of the owner app (docs/DESIGN.md section 2.1): the welcome, the practice shop, the shop, the
// till and where the money goes. Every sentence is in FirstRunWords. The practice shop lives in memory only: it
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
        FirstRunStep.Shop -> showShop(r)
        FirstRunStep.Till -> showTillChoice(r)
        FirstRunStep.Money -> showMoney(r)
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
    val tryIt = ui.button(FirstRunWords.welcomeTry) { startPractice() }
    views.add(stack(tryIt, line(FirstRunWords.welcomeTryNote, muted = true), gap = 6))
    screen("Owner", views, listOf(ui.primary(FirstRunWords.welcomeSetup) { run().next(); showFirstRun() }), logo = true)
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
    val currencies = ShopCurrency.values().toList()
    val money = ui.choice(currencies.map { it.label }, currencies.indexOf(r.currency))
    val msg = Slot(act)
    fun capture() {
        r.name = name.field.text.toString()
        currencies.getOrNull(ui.chosen(money))?.let { r.currency = it }
    }
    val body = listOf(heading(FirstRunWords.shopTitle), name.view, stack(label(FirstRunWords.shopCurrencyLabel), money), para(FirstRunWords.shopCurrencyNote), msg)
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
            shopFile.save(held.with(till = t, stepsShown = held.stepsShown && held.till == t))
            r.next()
            showFirstRun()
        } catch (e: IOException) {
            msg.show(ui.words(AppWords.fileNotSaved(e.message)))
        }
    }
    stepScreen(
        FirstRunWords.shopTitle, listOf(ui.tillPicture(), heading(FirstRunWords.tillTitle), para(FirstRunWords.tillText), search, list, msg),
        listOf(next), { r.back(); showFirstRun() },
    )
}

// ---- where the money goes ----------------------------------------------------------------------------------------------------

private fun Controller.showMoney(r: FirstRun) {
    val rows = listOf(
        RowItem(FirstRunWords.moneyOwn, FirstRunWords.moneyOwnNote) { r.choose(MoneyChoice.Own); showFirstRun() },
        RowItem(FirstRunWords.moneyMake, FirstRunWords.moneyMakeNote) { r.choose(MoneyChoice.Make); showFirstRun() },
        RowItem(FirstRunWords.moneyLater, FirstRunWords.moneyLaterNote) { r.choose(MoneyChoice.Later); showFirstRun() },
    )
    stepScreen(
        FirstRunWords.shopTitle, listOf(ui.moneyPicture(), heading(FirstRunWords.moneyTitle), para(FirstRunWords.moneyText), ui.rows(rows)),
        back = { r.back(); showFirstRun() },
    )
}

private fun Controller.showWhy(r: FirstRun) {
    stepScreen(
        FirstRunWords.shopTitle,
        listOf(ui.whyPicture(), heading(FirstRunWords.whyTitle), para(FirstRunWords.whyText1), para(FirstRunWords.whyText2), para(FirstRunWords.whyText3), ui.button(FirstRunWords.whyRealCases) { r.openRealCases(); showFirstRun() }),
        listOf(ui.primary(FirstRunWords.shopNext) { r.next(); showFirstRun() }), { r.back(); showFirstRun() },
    )
}

/** Four cases of money held by a bank or a payment company: words only, no link, no picture, no network. */
private fun Controller.showRealCases(r: FirstRun) {
    fun case(place: String, text: String): View = stack(line(place, muted = true), ui.text(text, 15f), gap = 4)
    val back: () -> Unit = { r.back(); showFirstRun() }
    stepScreen(
        FirstRunWords.shopTitle,
        listOf(
            heading(FirstRunWords.realTitle), para(FirstRunWords.realIntro),
            case(FirstRunWords.realCase1Place, FirstRunWords.realCase1Text),
            case(FirstRunWords.realCase2Place, FirstRunWords.realCase2Text),
            case(FirstRunWords.realCase3Place, FirstRunWords.realCase3Text),
            case(FirstRunWords.realCase4Place, FirstRunWords.realCase4Text),
            para(FirstRunWords.realOutro), line(FirstRunWords.realMore, muted = true),
        ),
        listOf(ui.button(KeyGuideWords.kgBack) { back() }), back,
    )
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
    stepScreen(page.title, views, listOf(ui.primary(FirstRunWords.tillStepsDone) { back() }), back)
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
