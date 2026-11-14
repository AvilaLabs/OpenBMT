package org.openbmt.owner.app

import android.content.Intent
import android.text.InputType
import android.view.View
import android.widget.LinearLayout
import android.widget.RadioGroup
import android.widget.TextView
import java.util.TimeZone
import org.openbmt.app.AnimatedQrView
import org.openbmt.app.Format
import org.openbmt.app.RowItem
import org.openbmt.app.SayWords
import org.openbmt.app.Who
import org.openbmt.owner.OwnerException
import org.openbmt.owner.OwnerNode
import org.openbmt.owner.RequestAsk
import org.openbmt.owner.RequestConfirm
import org.openbmt.owner.RequestEvery
import org.openbmt.owner.RequestPriceIn
import org.openbmt.owner.RequestRate
import org.openbmt.owner.RequestReturnAdded
import org.openbmt.owner.RequestShared
import org.openbmt.owner.ShopCurrency

// The owner's payment requests (`docs/REQUEST-RULES.md`, RQ-35): Home's row, the list, the page of one request, the
// form, sharing and the return steps. Every call goes through the owner library's request ops (216 to 226) on the
// screen thread (OA-12); every refusal is shown with the library's words, and every text of the app's own is a
// `RequestWords` name. The app sends nothing itself: the link is given to Android's share sheet as text (RQ-13).

private fun offsetMillis(): Int = TimeZone.getDefault().getOffset(System.currentTimeMillis())

private fun Controller.requestAsk(rate: RequestRate? = null): RequestAsk = RequestModel.ask(nowSecs(), offsetMillis(), rate)

/** The shop's name for the label of a link and the line above it; none when the phone holds none. */
private fun Controller.shopName(): String = try {
    shopFile.load().name
} catch (_: Exception) {
    ""
}

/** The shop's currency from the settings in force; Bitcoin Standard when the phone holds none, which the library refuses on its own. */
private fun Controller.currencyOf(n: OwnerNode, msg: Slot): ShopCurrency? =
    attempt(msg) { n.settings()?.currency ?: ShopCurrency.Sats }

// ---- Home (RQ-22, RQ-29, RQ-31, RQ-35) -----------------------------------------------------------------------

/**
 * What Home shows of the requests: the row to the list with how many are overdue, the returns that wait, and the words
 * for a book that cannot be read. Nothing for a shop that is still paired through the Key app (RQ-01). The repeats
 * that are due are made the first time Home is drawn after the node starts (RQ-25).
 */
internal fun Controller.requestsHomeViews(n: OwnerNode): List<View> {
    if (!RequestModel.offered(ownStore.hasWallet())) return emptyList()
    val ask = requestAsk()
    if (requestsChecked !== n) {
        requestsChecked = n
        try {
            n.requestRepeats(ask)
        } catch (_: OwnerException) {
        } catch (_: LinkageError) {
        }
    }
    val state = try {
        n.requestState(ask)
    } catch (e: OwnerException) {
        return listOf(ui.words(e.sayWords()))
    } catch (e: LinkageError) {
        return listOf(ui.words(AppWords.libraryMissing(e.message)))
    }
    state.off?.let { return listOf(ui.words(it.say())) }
    val views = ArrayList<View>()
    views.add(groupCard(listOf(GroupRow(OwnerIcon.Kind.Money, false, RequestWords.homeRowTitle, RequestModel.homeSub(state)) { showRequests() })))
    state.returns?.let {
        views.add(ui.words(it.say()))
        views.add(ui.button(RequestWords.payReturns) { showSpend() })
    }
    return views
}

// ---- the list (RQ-21) -------------------------------------------------------------------------------------------

internal fun Controller.showRequests(message: SayWords? = null) {
    val n = node ?: return begin()
    val msg = Slot(act)
    message?.let { msg.show(ui.words(it)) }
    val views = ArrayList<View>()
    views.add(msg)
    val list = attempt(msg) { n.requestList(requestAsk()) }
    if (list != null) {
        val sections = RequestModel.sections(list)
        if (sections.isEmpty()) {
            // RQ-35: the empty list's words are the library's.
            views.add(ui.text(list.emptyHeading, 16f, bold = true))
            views.add(para(list.emptyWhat))
        }
        for (s in sections) {
            views.add(label(RequestModel.partTitle(s.part)))
            views.add(ui.rows(s.rows.map { r -> RowItem(r.note, r.line) { showRequestPage(r.number) } }))
        }
    }
    screen(
        RequestWords.listTitle, views,
        listOf(ui.primary(RequestWords.homeButton) { showRequestForm() }, ui.button(RequestWords.back) { showHome() }),
        back = { showHome() },
    )
}

// ---- the page of one request (RQ-35) ----------------------------------------------------------------------------

internal fun Controller.showRequestPage(number: Long, message: SayWords? = null) {
    val n = node ?: return begin()
    val msg = Slot(act)
    message?.let { msg.show(ui.words(it)) }
    val list = attempt(msg) { n.requestList(requestAsk()) }
    val part = list?.let { RequestModel.find(it, number) }?.first
    val d = if (part == null) null else attempt(msg) { n.requestRead(requestAsk(), number) }
    if (d == null) {
        // A request the list does not hold, or a call that did not read: the words of the call, and the way back.
        return screen(RequestWords.listTitle, listOf(msg), listOf(ui.button(RequestWords.back) { showRequests() }), back = { showRequests() })
    }
    val a = RequestModel.actions(d, part == RequestPart.Overdue)
    val views = ArrayList<View>()
    views.add(msg)
    views.add(ui.text(d.row.state, 20f, bold = true))
    for (mark in d.row.marks) views.add(line(mark, muted = true))
    // RQ-37: more was returned than the request received.
    d.overReturned?.let { views.add(line(it, muted = true)) }
    if (d.repeat != null) views.add(line(RequestWords.repeatsLine, muted = true))
    views.add(ui.labelled(RequestWords.owedLabel, ui.text(RequestModel.amountIn(d, d.row.owed))))
    views.add(ui.labelled(RequestWords.paidLabel, ui.text(RequestModel.amountIn(d, d.paid))))
    if (d.address.isNotEmpty()) views.add(ui.labelled(RequestWords.requestAddressLabel, ui.text(d.address, mono = true)))
    if (d.quotes.isNotEmpty()) {
        views.add(label(RequestWords.quotesLabel))
        views.add(ui.rows(d.quotes.reversed().map { q -> RequestModel.quoteRow(d, q).let { (t, sub) -> RowItem(t, sub) } }))
    }
    if (d.payments.isNotEmpty()) {
        views.add(label(RequestWords.paymentsLabel))
        views.add(ui.rows(d.payments.map { p -> RequestModel.paymentRow(d, p).let { (t, sub) -> RowItem(t, sub) } }))
    }
    if (d.returns.isNotEmpty()) {
        views.add(label(RequestWords.returnsLabel))
        views.add(ui.rows(d.returns.map { r -> RequestModel.returnRow(r).let { (t, sub) -> RowItem(t, sub) } }))
    }
    if (a.returnMoney) views.add(ui.button(RequestWords.returnMoney) { showReturn(number) })
    if (a.countPaid) views.add(ui.button(RequestWords.countPaid) { confirmEnd(number, EndKind.CountPaid, d.countPaid) })
    if (a.close) views.add(ui.button(RequestWords.closeRequest) { confirmEnd(number, EndKind.Close, d.close) })
    if (a.cancel) views.add(ui.button(RequestWords.cancelRequest) { confirmEnd(number, EndKind.Cancel, d.cancel) })
    if (a.stopRepeating) {
        views.add(
            ui.button(RequestWords.stopRepeating) {
                if (attempt(msg) { n.requestStopRepeating(requestAsk(), number) } != null) showRequestPage(number)
            },
        )
    }
    val footer = ArrayList<android.widget.Button>()
    if (a.share) footer.add(ui.primary(RequestModel.shareLabel(part!!)) { shareRequest(n, number, a.sendAgain, msg) })
    footer.add(ui.button(RequestWords.back) { showRequests() })
    screen(d.row.note, views, footer, back = { showRequests() }, sub = d.row.line)
}

private enum class EndKind { Cancel, Close, CountPaid }

/** RQ-16, RQ-19, RQ-20: the library's question before a request ends or is counted as paid. The library refuses what the request cannot do. */
private fun Controller.confirmEnd(number: Long, kind: EndKind, question: RequestConfirm) {
    val n = node ?: return begin()
    val msg = Slot(act)
    val go = ui.danger(question.yes) {
        val ask = requestAsk()
        val done = attempt(msg) {
            when (kind) {
                EndKind.Cancel -> n.requestCancel(ask, number)
                EndKind.Close -> n.requestClose(ask, number)
                EndKind.CountPaid -> n.requestCountPaid(ask, number)
            }
        }
        if (done != null) showRequests()
    }
    screen(question.title, listOf(ui.text(question.text), msg), listOf(go, ui.button(question.no) { showRequestPage(number) }), back = { showRequestPage(number) })
}

// ---- the form (RQ-05 to RQ-09, RQ-24) ---------------------------------------------------------------------------

internal fun Controller.showRequestForm(draft: RequestDraft = RequestDraft(), message: SayWords? = null) {
    val n = node ?: return begin()
    val msg = Slot(act)
    message?.let { msg.show(ui.words(it)) }
    val currency = currencyOf(n, msg) ?: return screen(RequestWords.formTitle, listOf(msg), listOf(ui.button(RequestWords.back) { showRequests() }), back = { showRequests() })
    val pricings = RequestForm.pricings(currency)
    val views = ArrayList<View>()
    views.add(msg)
    val note = textInput(RequestWords.noteLabel, RequestWords.noteHint).also { it.field.setText(draft.note) }
    views.add(note.view)
    val priced: RadioGroup? =
        if (pricings.size > 1) ui.chips(listOf(RequestWords.pricedBitcoin, RequestWords.pricedCurrency), pricings.indexOf(draft.priceIn).coerceAtLeast(0)) else null
    priced?.let { views.add(stack(label(RequestWords.pricedLabel), it)) }
    val amount = textInput(amountLabelOf(draft.priceIn), mono = true).also {
        it.field.inputType = InputType.TYPE_CLASS_NUMBER or InputType.TYPE_NUMBER_FLAG_DECIMAL
        it.field.setText(draft.amount)
    }
    views.add(amount.view)
    fun pricing(): RequestPriceIn = priced?.let { pricings[ui.chosen(it).coerceAtLeast(0)] } ?: RequestPriceIn.Bitcoin
    priced?.setOnCheckedChangeListener { _, _ -> ((amount.view as LinearLayout).getChildAt(0) as TextView).text = amountLabelOf(pricing()) }
    val due = textInput(RequestWords.dueLabel, RequestWords.dueHint, mono = true).also { it.field.setText(draft.due) }
    views.add(due.view)
    val repeats = listOf<RequestEvery?>(null, RequestEvery.Week, RequestEvery.Month, RequestEvery.Year)
    val every = ui.chips(listOf(RequestWords.repeatNone, RequestWords.repeatWeek, RequestWords.repeatMonth, RequestWords.repeatYear), repeats.indexOf(draft.repeat).coerceAtLeast(0))
    views.add(stack(label(RequestWords.repeatLabel), every))
    val make = ui.primary(RequestWords.makeButton) {
        val typed = RequestDraft(note.field.text.toString(), amount.field.text.toString(), pricing(), due.field.text.toString(), repeats[ui.chosen(every).coerceAtLeast(0)])
        val today = RequestModel.today(nowSecs(), RequestModel.offsetMinutes(offsetMillis()))
        when (val f = RequestForm.check(typed, currency, today)) {
            is Form.Refused -> msg.show(ui.words(f.words))
            is Form.Ok -> {
                val v = f.value
                attempt(msg) { n.requestMake(requestAsk(), v.note, v.amount, v.priceIn, v.due, v.repeat) }
                    ?.let { showRequestPage(it.number, RequestWords.made) }
            }
        }
    }
    screen(RequestWords.formTitle, views, listOf(make, ui.button(RequestWords.back) { showRequests() }), back = { showRequests() })
}

private fun amountLabelOf(priceIn: RequestPriceIn): String =
    if (priceIn == RequestPriceIn.Bitcoin) RequestWords.amountBitcoinLabel else RequestWords.amountCurrencyLabel

// ---- sharing (RQ-12 to RQ-14, RQ-22) -------------------------------------------------------------------------------

/**
 * The rate for a call that needs one: none for a shop that prices in bitcoin; the one fetched a moment ago when there
 * is one; else the price sources are asked through Tor, off the screen thread. When they do not answer the call goes on
 * with no rate, and the library refuses a request in the currency with the words of a stale rate (RQ-12).
 */
private fun Controller.withRate(currency: ShopCurrency, need: Boolean, msg: Slot, then: (RequestRate?) -> Unit) {
    if (!need || currency == ShopCurrency.Sats) return then(null)
    RequestRates.usable(requestRate, nowSecs())?.let { return then(it) }
    msg.show(ui.words(RequestWords.rateFetching))
    val gate = TorGate(config?.proxy ?: AppConfig.DEFAULT_PROXY)
    async({ RequestRates.fetch(currency, LibraryPrice, HttpsRateFetcher(gate)) { nowSecs() } }) { r ->
        requestRate = r.getOrNull()
        msg.show()
        then(requestRate)
    }
}

/** RQ-12: the quote is written first; only then is anything shown to share. [openNow] opens the sheet at once, for "Send again" (RQ-22). */
private fun Controller.shareRequest(n: OwnerNode, number: Long, openNow: Boolean, msg: Slot) {
    val currency = currencyOf(n, msg) ?: return
    withRate(currency, true, msg) { rate ->
        val shared = attempt(msg) { n.requestShare(requestAsk(rate), number, shopName()) }
        if (shared != null) {
            showShare(number, shared, currency)
            if (openNow) openShareSheet(RequestShare.text(shared))
        }
    }
}

/** The quote that was written, and the link to give to the share sheet. */
private fun Controller.showShare(number: Long, shared: RequestShared, currency: ShopCurrency) {
    val views = ArrayList<View>()
    views.add(ui.labelled(RequestWords.quoteLabel, ui.text(RequestShare.owed(shared, currency), 20f, bold = true)))
    views.add(ui.labelled(RequestWords.quoteSatsLabel, ui.text(Format.sats(shared.sats))))
    RequestShare.until(shared)?.let { views.add(ui.labelled(RequestWords.quoteUntilLabel, ui.text(it))) }
    views.add(line(shared.line))
    views.add(para(RequestWords.sendsNothing))
    screen(
        RequestWords.shareTitle, views,
        listOf(
            ui.primary(RequestWords.shareLink) { openShareSheet(RequestShare.text(shared)) },
            ui.button(RequestWords.showCode) { showCode(number, shared, currency) },
            ui.button(RequestWords.back) { showRequestPage(number) },
        ),
        back = { showRequestPage(number) },
    )
}

/**
 * RQ-13: the same link as a code on the screen, for a customer who is there to scan it. It is the shared code view of
 * the other code screens, on the window that no screenshot or recording can take (OA-04). No image is made or given
 * to another app.
 */
private fun Controller.showCode(number: Long, shared: RequestShared, currency: ShopCurrency) {
    val made = AnimatedQrView.makePlain(act, Who.OWNER, shared.link, codeHeight())
    val below = ArrayList<View>()
    below.add(ui.text(RequestShare.owed(shared, currency), 20f, bold = true))
    below.addAll(codeFallback(made))
    codeScreen(
        RequestWords.codeTitle, RequestWords.codeWords, made.view, below,
        listOf(ui.button(RequestWords.back) { showShare(number, shared, currency) }),
        back = { showShare(number, shared, currency) },
    )
}

/** RQ-13: gives the text to Android's share sheet. The app sends nothing itself. */
private fun Controller.openShareSheet(text: String) {
    val send = Intent(Intent.ACTION_SEND).setType("text/plain").putExtra(Intent.EXTRA_TEXT, text)
    act.startActivity(Intent.createChooser(send, null))
}

// ---- returning money (RQ-26 to RQ-29) ----------------------------------------------------------------------------

internal fun Controller.showReturn(number: Long, draft: ReturnDraft = ReturnDraft(), message: SayWords? = null) {
    val n = node ?: return begin()
    val msg = Slot(act)
    message?.let { msg.show(ui.words(it)) }
    val back = { showRequestPage(number) }
    val currency = currencyOf(n, msg) ?: return screen(RequestWords.returnTitle, listOf(msg), listOf(ui.button(RequestWords.back) { back() }), back = { back() })
    val views = ArrayList<View>()
    views.add(msg)
    val inGroup: RadioGroup? =
        if (currency != ShopCurrency.Sats) ui.chips(listOf(RequestWords.returnInSats, RequestWords.returnInCurrency), if (draft.inCurrency) 1 else 0) else null
    inGroup?.let { views.add(stack(label(RequestWords.returnInLabel), it)) }
    val amount = textInput(if (draft.inCurrency) RequestWords.returnAmountCurrencyLabel else RequestWords.returnAmountSatsLabel, mono = true).also {
        it.field.inputType = InputType.TYPE_CLASS_NUMBER or InputType.TYPE_NUMBER_FLAG_DECIMAL
        it.field.setText(draft.amount)
    }
    views.add(amount.view)
    inGroup?.setOnCheckedChangeListener { group, _ ->
        ((amount.view as LinearLayout).getChildAt(0) as TextView).text =
            if (ui.chosen(group as RadioGroup) == 1) RequestWords.returnAmountCurrencyLabel else RequestWords.returnAmountSatsLabel
    }
    val address = textInput(RequestWords.addressLabel, lines = 2, mono = true).also { it.field.setText(draft.address) }
    views.add(address.view)
    fun typed() = ReturnDraft(inGroup?.let { ui.chosen(it) == 1 } ?: false, amount.field.text.toString(), address.field.text.toString())
    views.add(
        ui.button(RequestWords.askAddress) {
            val d = typed()
            when (val f = ReturnForm.amount(d, currency)) {
                is Form.Refused -> msg.show(ui.words(f.words))
                is Form.Ok -> withRate(currency, f.value.inCurrency, msg) { rate ->
                    attempt(msg) { n.requestReturnAsk(requestAsk(rate), number, f.value.inCurrency, f.value.amount, shopName()) }
                        ?.let { openShareSheet(it) }
                }
            }
        },
    )
    views.add(ui.button(RequestWords.scanAddress) { scanReturnAddress(number, typed()) })
    val add = ui.primary(RequestWords.addReturn) {
        val d = typed()
        when (val f = ReturnForm.check(d, currency)) {
            is Form.Refused -> msg.show(ui.words(f.words))
            is Form.Ok -> withRate(currency, f.value.inCurrency, msg) { rate ->
                attempt(msg) { n.requestAddReturn(requestAsk(rate), number, f.value.inCurrency, f.value.amount, f.value.address) }
                    ?.let { showReturnAdded(number, it) }
            }
        }
    }
    screen(RequestWords.returnTitle, views, listOf(add, ui.button(RequestWords.back) { back() }), back = { back() })
}

/** RQ-28: the camera reads the customer's address into the field; the library judges it when the return is added. */
private fun Controller.scanReturnAddress(number: Long, draft: ReturnDraft) {
    fun give(address: String, words: SayWords?) =
        showReturn(number, ReturnDraft(draft.inCurrency, draft.amount, address), words)
    scan.begin(
        show = { v -> showScan(v) },
        onBytes = { give(String(it, Charsets.UTF_8), null) },
        onFailed = { f -> give(draft.address, (ScanResult.failed(f.off, f.problem, f.words) as SourceResult.Failed).words) },
        onCancel = { give(draft.address, null) },
    )
}

/** RQ-28, RQ-29: the accepted address in groups of four, to compare with the customer, and the way to pay the returns. */
private fun Controller.showReturnAdded(number: Long, added: RequestReturnAdded) {
    val views = ArrayList<View>()
    views.add(ui.words(added.words.say()))
    views.add(big(added.groups))
    views.add(ui.text(RequestWords.checkTitle, 16f, bold = true))
    views.add(para(RequestWords.checkText))
    views.add(para(RequestWords.checkNext))
    screen(
        RequestWords.returnTitle, views,
        listOf(ui.primary(RequestWords.payReturns) { showSpend() }, ui.button(RequestWords.back) { showRequestPage(number) }),
        back = { showRequestPage(number) },
    )
}
