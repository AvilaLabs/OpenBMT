package org.openbmt.owner.app

import android.view.View
import android.widget.Button
import org.openbmt.app.Format
import org.openbmt.app.RowItem
import org.openbmt.app.SayWords
import org.openbmt.app.WordsModel
import org.openbmt.owner.AlertKind
import org.openbmt.owner.CashAmount
import org.openbmt.owner.CashOutArgs
import org.openbmt.owner.ClearArgs
import org.openbmt.owner.OwnerException
import org.openbmt.owner.OwnerNode
import org.openbmt.owner.ReceiptsKind
import org.openbmt.owner.ShopCurrency
import org.openbmt.owner.Stage
import org.openbmt.owner.StageKind
import org.openbmt.owner.ViewKind

// The screens of the owner app. Every call into the library is made here on
// the screen thread (OA-12); every state and every refusal is shown with the
// library's words, and the app's own words are only for what it decides itself.
// The look of every screen is that of OA-91: a bar, a body that scrolls and a footer.

/** The node of the running screens, or the first screen again when there is none. */
private inline fun Controller.withNode(block: (OwnerNode) -> Unit) {
    val n = node
    if (n == null) begin() else block(n)
}

/** Runs [call] and shows its refusal in [slot]; returns what it returned, or null when it refused. */
internal fun <T> Controller.attempt(slot: Slot, call: () -> T): T? = try {
    call()
} catch (e: OwnerException) {
    slot.show(ui.words(e.sayWords()))
    null
} catch (e: LinkageError) {
    slot.show(ui.words(AppWords.libraryMissing(e.message)))
    null
}

private fun Controller.backButton(label: String = "Back", to: () -> Unit): Button = ui.button(label) { to() }

/** A page that holds only the words of a call that refused, with the way back to home. */
private fun Controller.refusedPage(msg: Slot) {
    screen("Owner", listOf(msg), listOf(backButton { showHome() }), back = { showHome() }, logo = true)
}

// ---- home ------------------------------------------------------------------------------------------

internal fun Controller.showHome(): Unit = withNode { n ->
    // SS-37: with the starter wallet at its limit or past its date, the app opens on the upgrade screen.
    if (starterGate(n)) return@withNode
    val content = Slot(act, 14)
    homeTint = null
    screen("Owner", listOf(content), listOf(ui.primary("Spend") { showSpend() }), logo = true)
    tick = tick@{
        try {
            val s = n.status()
            if (starterGate(n)) return@tick
            val pump = pumpFault
            val status: SayWords = when {
                pump != null -> pump
                s.stopped -> AppWords.stopped
                else -> s.health.words.say()
            }
            val extra = ArrayList<View>()
            s.halted?.let { extra.add(ui.words(it.words.say())) }
            if (s.view != ViewKind.Ready) extra.add(ui.words(s.viewWords.say()))
            if (s.view == ViewKind.DeepReorg) {
                extra.add(ui.button("Check again from the first block") { rescan() })
            }
            if (s.recoveryPending) {
                s.recoveryWords?.let { extra.add(ui.words(it.say())) }
                extra.add(ui.button("Finish the approval that was interrupted") { recover() })
            }
            if (key2Signer) extra.add(firstAddressView(n))
            val balance = OwnerLook.balance(Format.sats(s.balanceSats), s.checkedHeight, s.tipHeight)
            val hero = heroCard(WordsModel.of(status), balance, extra, homeTint)
            homeTint = pillTint(hero)

            val link = if (setup?.link != null) n.linkState() else null
            val inProgress = OwnerModel.sendsInProgress(n.sendStates())
            val views = ArrayList<View>()
            views.add(hero)
            // SS-36: the starter wallet's line, which cannot be dismissed.
            starterLine(n)?.let { views.add(starterLineView(it)) }
            // SS-43: a payment that came to the starter wallet after the move.
            starterLateView(n)?.let { views.add(it) }
            // Home has no count of open approvals, of what waits to be delivered or of the refund queue, and it adds no
            // library call to get one: the card shows a row when a count is given, and none is given today (OA-91).
            val counts = listOf<Pair<String, Int?>>("Open approvals" to null, "Waiting to be delivered" to null, "Refund queue" to null)
            val rows = OwnerLook.waitingRows(counts).map { r ->
                Triple(r.title, r.count, { open(r.title) })
            }
            waitingCard(rows)?.let { views.add(it) }
            val groups = listOf(
                GroupRow(OwnerIcon.Kind.Money, false, OwnerLook.MONEY, OwnerLook.moneySub(inProgress, OwnerModel.sendsLine(inProgress))) { showMoney() },
                GroupRow(OwnerIcon.Kind.Devices, false, OwnerLook.DEVICES, OwnerModel.linkLine(link, link?.counters?.size ?: 0)) { showDevices() },
                GroupRow(OwnerIcon.Kind.Bell, false, OwnerLook.ALERTS, OwnerModel.alertsLine(link?.alerts?.size ?: 0)) { showAlertsAndNotes() },
                GroupRow(null, true, OwnerLook.SETTINGS, OwnerLook.SETTINGS_SUB) { showSettingsAndPhone() },
            )
            views.add(groupCard(groups))
            linkFault?.let { views.add(ui.words(it)) }
            content.show(*views.toTypedArray())
        } catch (e: OwnerException) {
            content.show(ui.words(e.sayWords()))
        }
    }
    tick?.invoke()
}

/** The screen a row of the waiting card opens. */
private fun Controller.open(title: String) = when (title) {
    "Open approvals" -> showSessions()
    "Waiting to be delivered" -> showOutbox()
    else -> showQueue()
}

/** A page of the rows of one group of home: each row opens what its button opened. */
private fun Controller.groupPage(title: String, rows: List<RowItem>) {
    screen(title, listOf(ui.rows(rows)), back = { showHome() })
}

private fun Controller.showMoney() {
    val opens: List<() -> Unit> = listOf(
        { showReceipts() }, { showCashOut() }, { showOld() }, { showRefundNotes() }, { showSessions() }, { showQueue() }, { showOutbox() },
    )
    val rows = OwnerLook.moneyTitles.zip(opens).map { (title, go) -> RowItem(title) { go() } }
    groupPage(OwnerLook.MONEY, rows + RowItem(OwnerLook.BOOKS, OwnerLook.BOOKS_SUB, onClick = { showBooks() }) + starterRows() + floatRow())
}

internal fun Controller.showDevices() {
    val rows = ArrayList<RowItem>()
    rows.add(RowItem("Link and counters") { showLink() })
    rows.add(RowItem("Add a counter") { showAddCounter() })
    rows.add(RowItem("Connect your Box") { showConnectBox() })
    rows.add(RowItem("Till") { showTill() })
    if (key2Signer) rows.add(RowItem("Register the wallet on your signer") { showHardwareRegister { showHome() } })
    groupPage(OwnerLook.DEVICES, rows)
}

private fun Controller.showAlertsAndNotes() {
    groupPage(OwnerLook.ALERTS, listOf(RowItem("Alerts") { showAlerts() }, RowItem("Notes") { showNotes() }))
}

internal fun Controller.showSettingsAndPhone() {
    groupPage(
        OwnerLook.SETTINGS,
        listOfNotNull(
            RowItem("Settings") { showSettings() },
            RowItem("Change the PIN") { showChangePin(null) },
            RowItem("About") { showAbout() },
            RowItem(AppWords.watchSetting.heading) { showWatch() },
            RowItem(AppWords.torBuiltIn.heading) { showTor() },
            tillStepsRow { showSettingsAndPhone() },
        ),
    )
}

private fun Controller.rescan(): Unit = withNode { n ->
    val msg = Slot(act)
    if (attempt(msg) { n.rescan() } != null) showHome() else refusedPage(msg)
}

private fun Controller.recover(): Unit = withNode { n ->
    val msg = Slot(act)
    if (attempt(msg) { n.recoverKey1() } != null) showHome() else refusedPage(msg)
}

// ---- the receipts line and the merge offer -------------------------------------------------------------

internal fun Controller.showReceipts(): Unit = withNode { n ->
    val receipts = Slot(act)
    val offer = Slot(act)
    val msg = Slot(act)
    val views = ArrayList<View>()
    views.add(receipts)
    views.add(stack(label("The offer to merge coins"), offer))
    views.add(msg)
    screen("Receipts", views, listOf(backButton { showHome() }), back = { showHome() })
    tick = {
        try {
            val r = n.receiptsLine()
            val rv = ArrayList<View>()
            rv.add(ui.words(r.words.say()))
            if (r.kind == ReceiptsKind.Known) {
                rv.add(line("${r.payments} payments held. Spending all of them costs ${Format.sats(r.costSats)} at ${r.ratePerKvb} sats per 1000 virtual bytes.", muted = true))
            }
            receipts.show(*rv.toTypedArray())
            val m = n.mergeOffer()
            if (m == null) {
                offer.show(line("No merge is offered now."))
            } else {
                offer.show(
                    ui.words(m.words.say()),
                    line(
                        "Merge ${m.coins} of ${m.of} coins at ${m.ratePerKvb} sats per 1000 virtual bytes. It costs ${Format.sats(m.costSats)} now. " +
                            "Spending them as they are usually costs ${Format.sats(m.usualCostSats)}. The saving is ${Format.sats(m.savingSats)}.",
                        muted = true,
                    ),
                    ui.primary("Build the merge request") {
                        attempt(msg) { n.approveMerge(nowSecs()) }?.let { showBuilt(it) { showReceipts() } }
                    },
                )
            }
        } catch (e: OwnerException) {
            receipts.show(ui.words(e.sayWords()))
        }
    }
    tick?.invoke()
}

// ---- spend -----------------------------------------------------------------------------------------------

internal fun Controller.showSpend(): Unit = withNode { n ->
    // A shop with its own wallet sends from that wallet (OA-115): there is no Key app to ask.
    if (ownStore.hasWallet()) return@withNode showOwnSpend()
    val msg = Slot(act)
    val views = ArrayList<View>()
    views.add(
        para("This builds a request for the Key app from what is in the queue. Nothing is signed here, and nothing is sent until the Key app has approved."),
    )
    val input = numberInput("Fee rate, satoshis per 1000 virtual bytes", "Fee rate")
    val fee = input.field
    // The rate the receipts line works at is a starting point, if the node has one.
    try {
        val r = n.receiptsLine()
        if (r.kind == ReceiptsKind.Known && r.ratePerKvb > 0) fee.setText(r.ratePerKvb.toString())
    } catch (_: OwnerException) {
    }
    views.add(input.view)
    views.add(msg)
    val build = ui.primary("Build the request") {
        when (val f = Forms.feeRate(fee.text.toString())) {
            is Form.Refused -> msg.show(ui.words(f.words))
            // With key 2 on a hardware signer the spends go to the signer, and the Key app gets the rest (OA-70).
            is Form.Ok -> if (key2Signer) {
                attempt(msg) { n.signerBuild(nowSecs(), f.value) }?.let { showSignerSpend(it) { showSpend() } }
            } else {
                attempt(msg) { n.buildRequest(nowSecs(), f.value) }?.let { showBuilt(it) { showSpend() } }
            }
        }
    }
    screen("Spend", views, listOf(build, backButton { showHome() }), back = { showHome() })
}

// ---- open approvals ---------------------------------------------------------------------------------------

internal fun Controller.showSessions(message: SayWords? = null): Unit = withNode { n ->
    val views = ArrayList<View>()
    message?.let { views.add(ui.words(it)) }
    val msg = Slot(act)
    val ids = attempt(msg) { n.sessions() }
    if (ids == null) {
        views.add(msg)
    } else if (ids.isEmpty()) {
        views.add(ui.text("No approval is open.", 16f))
    } else {
        for (id in ids) {
            val name = ApprovalText.fileName("approval", id).removeSuffix(".obm")
            // A session of spends for the signer is carried on with the signer, not shown as a request for the Key app.
            val signer = if (key2Signer) signerSessionOf(n, id) else HardwareModel.SignerSession.NotSigner
            val show = if (signer == HardwareModel.SignerSession.NotSigner) {
                ui.button("Show its request again") {
                    attempt(msg) { n.sessionRequest(id) }?.let { showBuilt(org.openbmt.owner.Built(id, it, emptyList())) { showSessions() } }
                }
            } else {
                ui.button("Continue on the signer") { continueOnSigner(id, signer) { showSessions() } }
            }
            val abandon = ui.danger("Abandon it") {
                // An approval whose file went to the signer may still be signed and sent (AS-89): the owner is asked first.
                if (signer == HardwareModel.SignerSession.Exported) {
                    showAbandonExported(id)
                } else if (attempt(msg) { n.abandonSession(id) } != null) {
                    showSessions(AppWords.sessionAbandoned)
                }
            }
            views.add(stack(label(name), show, abandon, gap = 10))
        }
        views.add(msg)
    }
    screen("Open approvals", views, listOf(backButton { showHome() }), back = { showHome() })
}

/** The question before an exported approval is abandoned: the old transaction is still watched, and counts as paid if it is sent. */
private fun Controller.showAbandonExported(id: ByteArray): Unit = withNode { n ->
    val msg = Slot(act)
    val keep = ui.primary("Keep it") { showSessions() }
    val abandon = ui.danger("Abandon it") {
        if (attempt(msg) { n.abandonSession(id) } != null) showSessions(AppWords.abandonedExported)
    }
    screen("Abandon this approval?", listOf(para(AppWords.abandonExportedMore), msg), listOf(keep, abandon), back = { showSessions() }, words = AppWords.abandonExported)
}

// ---- the signed objects that wait -------------------------------------------------------------------------

internal fun Controller.showOutbox(): Unit = withNode { n ->
    val views = ArrayList<View>()
    views.add(para("Signed by both keys and kept on this phone until they are delivered or confirmed."))
    val msg = Slot(act)
    val rows = attempt(msg) { n.outbox() }
    // New addresses that the link keeps sending until the counter holds them.
    val sent = try {
        if (setup?.link != null) n.linkState().batches else emptyList()
    } catch (_: OwnerException) {
        emptyList()
    }
    if (rows == null) {
        views.add(msg)
    } else if (rows.isEmpty() && sent.isEmpty()) {
        views.add(ui.text("Nothing is waiting.", 16f))
    } else {
        val now = nowSecs()
        for (b in sent) {
            views.add(
                stack(
                    label("New addresses: ${OwnerModel.counterLabel(b.counter)}"),
                    ui.words(b.words.say()),
                    line(OwnerModel.batchWaitingLines(b, now).joinToString("\n"), muted = true, mono = true),
                    gap = 10,
                ),
            )
        }
        for (row in rows) {
            views.add(
                stack(
                    label(OwnerModel.outboxTitle(row)),
                    ui.words(row.words.say()),
                    line(OwnerModel.outboxLines(row, now).joinToString("\n"), muted = true, mono = true),
                    gap = 10,
                ),
            )
        }
        views.addAll(waitingCounterViews(n, Slot(act)) { showOutbox() })
        views.addAll(pendingBoxViews(n, Slot(act)) { showOutbox() })
    }
    screen("Waiting to be delivered", views, listOf(backButton { showHome() }), back = { showHome() })
}

// ---- the queue: refunds, cash-outs and the rest ---------------------------------------------------------------

/** Cancel and requeue where the stage allows; the library decides and its words are shown. */
private fun Controller.entryButtons(n: OwnerNode, id: Long, stage: Stage, msg: Slot, refresh: () -> Unit, into: MutableList<View>) {
    if (stage.kind == StageKind.Returned) {
        into.add(
            ui.button("Queue it again") {
                if (attempt(msg) { n.requeue(id) } != null) { refresh() }
            },
        )
    }
    if (stage.kind == StageKind.Queued || stage.kind == StageKind.Returned) {
        into.add(
            ui.danger("Cancel it") {
                if (attempt(msg) { n.cancel(id) } != null) { refresh() }
            },
        )
    }
}

internal fun Controller.showQueue(prefillCounter: ByteArray? = null, prefillSale: Long? = null): Unit = withNode { n ->
    val msg = Slot(act)
    val views = ArrayList<View>()
    val q = attempt(msg) { n.queue() }
    if (q == null) {
        views.add(msg)
        return@withNode screen("Refund queue", views, listOf(backButton { showHome() }), back = { showHome() })
    }
    if (q.refunds.isEmpty()) views.add(ui.text("No refund is queued.", 16f))
    for (r in q.refunds) {
        val part = ArrayList<View>()
        part.add(label("Refund ${r.id}: ${Format.sats(r.sats)} for sale ${OwnerModel.saleNumber(r.sale)} of ${OwnerModel.counterLabel(r.counter)}"))
        part.add(line("Asked by ${Format.cleanForeign(r.cashier)}. To ${r.address}", muted = true, mono = true))
        r.receivedSats?.let { part.add(line("The wallet's own view shows ${Format.sats(it)} received for this sale.", muted = true)) }
        part.add(ui.words(r.stage.words.say()))
        entryButtons(n, r.id, r.stage, msg, { showQueue() }, part)
        views.add(stack(*part.toTypedArray(), gap = 10))
    }
    if (q.cashOuts.isNotEmpty() || q.others.isNotEmpty()) views.add(label("Other entries"))
    for (c in q.cashOuts) {
        val amount = when (val a = c.amount) {
            is CashAmount.Sats -> Format.sats(a.sats)
            is CashAmount.All -> "everything"
        }
        val part = ArrayList<View>()
        part.add(ui.text("Cash-out ${c.id}: $amount to ${c.address}", 14f, bold = true))
        part.add(ui.words(c.stage.words.say()))
        entryButtons(n, c.id, c.stage, msg, { showQueue() }, part)
        views.add(stack(*part.toTypedArray(), gap = 10))
    }
    for (o in q.others) {
        val part = ArrayList<View>()
        part.add(ui.text(OwnerModel.entryTitle(o.kind, o.id), 14f, bold = true))
        part.add(ui.words(o.stage.words.say()))
        entryButtons(n, o.id, o.stage, msg, { showQueue() }, part)
        views.add(stack(*part.toTypedArray(), gap = 10))
    }
    views.add(msg)

    // Queue a refund. The sale's facts come from the node's own view of the chain.
    views.add(label("Queue a refund"))
    val counters = try {
        if (setup?.link != null) n.linkState().counters.map { it.counter } else emptyList()
    } catch (_: OwnerException) {
        emptyList()
    }
    val group = if (counters.isNotEmpty()) {
        val index = counters.indexOfFirst { it.contentEquals(prefillCounter) }.coerceAtLeast(0)
        ui.choice(counters.map { OwnerModel.counterLabel(it) }, index).also { views.add(it) }
    } else {
        null
    }
    val counterInput = textInput("Counter identity, 64 digits", mono = true)
    val counterField = counterInput.field.also {
        if (group == null && prefillCounter != null) it.setText(prefillCounter.joinToString("") { b -> "%02x".format(b) })
    }
    views.add(counterInput.view)
    val saleInput = numberInput("Sale number, as the counter shows it")
    val sale = saleInput.field.also { f -> prefillSale?.let { f.setText(OwnerModel.saleNumber(it).toString()) } }
    views.add(saleInput.view)
    val cashierInput = textInput("Name of the cashier who asked")
    val cashier = cashierInput.field
    views.add(cashierInput.view)
    val addressInput = textInput("Address the customer wants the money sent to", mono = true)
    val address = addressInput.field
    views.add(addressInput.view)
    val satsInput = numberInput("Amount in satoshis")
    val sats = satsInput.field
    views.add(satsInput.view)
    val result = Slot(act)
    views.add(result)
    val queue = ui.primary("Queue the refund") {
        val typed = counterField.text.toString()
        val counter = if (typed.isNotBlank()) {
            Forms.counter(typed) ?: byteArrayOf()
        } else {
            group?.let { g -> ui.chosen(g).takeIf { it >= 0 }?.let { counters[it] } }
        }
        when (val f = Forms.refund(counter, sale.text.toString(), cashier.text.toString(), address.text.toString(), sats.text.toString())) {
            is Form.Refused -> result.show(ui.words(f.words))
            is Form.Ok -> attempt(result) { n.queueRefund(nowSecs(), f.value) }?.let {
                // What the sale received and whether it is deep enough come from the node's own view of the chain.
                showQueueAfterRefund(it.receivedSats, it.reachedRefundDepth)
            }
        }
    }
    screen("Refund queue", views, listOf(queue, backButton { showHome() }), back = { showHome() })
}

private fun Controller.showQueueAfterRefund(receivedSats: Long, atDepth: Boolean) {
    centre(
        if (atDepth) AppWords.refundQueuedAtDepth else AppWords.refundQueuedWaiting,
        listOf(ui.button("Back to the queue") { showQueue() }),
        extra = listOf(line("The wallet's own view shows ${Format.sats(receivedSats)} received for this sale.")),
    )
}

// ---- cash out ------------------------------------------------------------------------------------------------

internal fun Controller.showCashOut(): Unit = withNode { n ->
    val msg = Slot(act)
    val views = ArrayList<View>()
    views.add(para("Moves the shop's money to an address of yours. It is queued here and paid only after the Key app approves it."))
    val addressInput = textInput("Address", mono = true)
    val address = addressInput.field
    views.add(addressInput.view)
    val which = ui.chips(listOf("An amount", "Everything the wallet can spend"))
    views.add(stack(label("How much"), which))
    val satsInput = numberInput("Amount in satoshis")
    val sats = satsInput.field
    views.add(satsInput.view)
    val nameInput = textInput("What the Key app shows for this address", "Label")
    val name = nameInput.field
    views.add(nameInput.view)
    views.add(msg)
    val q = attempt(msg) { n.queue() }
    if (q != null && q.cashOuts.isNotEmpty()) {
        views.add(label("Queued cash-outs"))
        for (c in q.cashOuts) {
            val amount = when (val a = c.amount) {
                is CashAmount.Sats -> Format.sats(a.sats)
                is CashAmount.All -> "everything"
            }
            val part = ArrayList<View>()
            part.add(ui.text("Cash-out ${c.id}: $amount to ${c.address}", 14f, bold = true))
            part.add(ui.words(c.stage.words.say()))
            entryButtons(n, c.id, c.stage, msg, { showCashOut() }, part)
            views.add(stack(*part.toTypedArray(), gap = 10))
        }
    }
    val queue = ui.primary("Queue the cash-out") {
        when (val f = Forms.cashOut(address.text.toString(), ui.chosen(which) == 1, sats.text.toString(), name.text.toString())) {
            is Form.Refused -> msg.show(ui.words(f.words))
            is Form.Ok -> {
                // The books record a cash-out at a price (OA-102): the sources are asked for it, except in Bitcoin Standard.
                // A phone with no settings has no currency to ask in: its cash-out is queued with no price.
                val currency = attempt(msg) { n.settings()?.currency ?: ShopCurrency.Sats }
                if (currency != null && currency != ShopCurrency.Sats) {
                    showCashOutPrice(n, f.value, currency)
                } else if (currency != null && attempt(msg) { n.cashOut(f.value) } != null) {
                    centre(AppWords.cashOutQueued, listOf(ui.button("Back") { showCashOut() }))
                }
            }
        }
    }
    screen("Cash out", views, listOf(queue, backButton { showHome() }), back = { showHome() })
}

/**
 * The price a cash-out is recorded at (OA-102): the sources are asked for the price of one bitcoin in the shop's
 * currency and it is shown with the cash-out to confirm. The owner can type another price; when the sources do not
 * answer, the owner types it.
 */
private fun Controller.showCashOutPrice(n: OwnerNode, args: CashOutArgs, currency: ShopCurrency) {
    val note = Slot(act)
    val msg = Slot(act)
    val input = numberInput(OwnerLook.capital(currency.plural) + " for one bitcoin")
    val field = input.field
    fillPriceIn(note, field, currency, AppWords.counterPriceFetching, { AppWords.cashOutPrice(it, currency) }, AppWords.cashOutPriceNeeded)
    val queue = ui.primary("Queue the cash-out") {
        when (val p = Forms.cashOutPrice(field.text.toString(), currency)) {
            is Form.Refused -> msg.show(ui.words(p.words))
            is Form.Ok -> {
                val priced = CashOutArgs(args.address, args.amount, args.label, p.value)
                if (attempt(msg) { n.cashOut(priced) } != null) {
                    centre(AppWords.cashOutQueued, listOf(ui.button("Back") { showCashOut() }))
                }
            }
        }
    }
    screen("Cash out", listOf(note, input.view, msg), listOf(queue, backButton { showCashOut() }), back = { showCashOut() })
}

// ---- old addresses ------------------------------------------------------------------------------------------------

internal fun Controller.showOld(): Unit = withNode { n ->
    val line = Slot(act)
    val msg = Slot(act)
    val views = ArrayList<View>()
    views.add(line)
    views.add(msg)
    val check = ui.primary("Check them now") {
        attempt(msg) { n.requestDeepCheck(nowSecs()) }?.let { msg.show(ui.words(it.words.say())) }
    }
    screen("Old addresses", views, listOf(check, backButton { showHome() }), back = { showHome() })
    tick = {
        try {
            val o = n.oldAddresses()
            val w = o.words
            if (w == null) {
                line.show(ui.text("No old address is set aside.", 16f))
            } else {
                line.show(ui.words(w.say()), ui.text("${o.count} addresses are set aside.", 15f))
            }
        } catch (e: OwnerException) {
            line.show(ui.words(e.sayWords()))
        }
    }
    tick?.invoke()
}

// ---- the link and the counters -----------------------------------------------------------------------------------------

internal fun Controller.showLink(): Unit = withNode { n ->
    val body = Slot(act)
    screen("Link and counters", listOf(body), listOf(backButton { showHome() }), back = { showHome() })
    if (setup?.link == null) {
        body.show(ui.words(AppWords.noLink))
        return@withNode
    }
    tick = {
        try {
            val l = n.linkState()
            val out = ArrayList<View>()
            out.add(ui.text(OwnerModel.linkLine(l, l.counters.size) + " The relay refused ${l.refused} messages since the start.", 15f))
            linkFault?.let { out.add(ui.words(it)) }
            val now = nowSecs()
            for (c in l.counters) {
                out.add(
                    stack(
                        label(OwnerModel.counterLabel(c.counter)),
                        ui.words(c.words.say()),
                        this.line(OwnerModel.counterLines(c, now).joinToString("\n"), muted = true, mono = true),
                        gap = 10,
                    ),
                )
            }
            body.show(*out.toTypedArray())
        } catch (e: OwnerException) {
            body.show(ui.words(e.sayWords()))
        }
    }
    tick?.invoke()
}

internal fun Controller.showAlerts(message: SayWords? = null): Unit = withNode { n ->
    val views = ArrayList<View>()
    message?.let { views.add(ui.words(it)) }
    val msg = Slot(act)
    if (setup?.link == null) {
        views.add(ui.words(AppWords.noLink))
    } else {
        val l = attempt(msg) { n.linkState() }
        if (l == null) {
            views.add(msg)
        } else {
            views.add(ui.text(OwnerModel.alertsLine(l.alerts.size), 16f, bold = l.alerts.isNotEmpty()))
            for (a in l.alerts) {
                val kind = when (a.kind) {
                    AlertKind.MissedCheckIn -> "Missed check-in"
                    AlertKind.Diverged -> "Records do not match"
                    AlertKind.BatchUndelivered -> "New addresses not taken"
                }
                views.add(
                    stack(
                        label("$kind: ${OwnerModel.counterLabel(a.counter)}"),
                        ui.words(a.words.say()),
                        ui.button("Clear this alert") {
                            if (attempt(msg) { n.clearAlert(ClearArgs(a.kind, a.counter, a.value)) } != null) showAlerts(AppWords.alertCleared)
                        },
                        gap = 10,
                    ),
                )
            }
            views.add(msg)
        }
    }
    screen("Alerts", views, listOf(backButton { showHome() }), back = { showHome() })
}

// ---- refund notes and notes ------------------------------------------------------------------------------------------------

internal fun Controller.showRefundNotes(message: SayWords? = null): Unit = withNode { n ->
    val views = ArrayList<View>()
    message?.let { views.add(ui.words(it)) }
    val msg = Slot(act)
    val notes = attempt(msg) { n.refundNotes() }
    if (notes == null) {
        views.add(msg)
    } else if (notes.isEmpty()) {
        views.add(ui.text("No note about a refund request.", 16f))
    } else {
        for (note in notes) {
            views.add(
                stack(
                    label("${OwnerModel.counterLabel(note.counter)}, sale ${OwnerModel.saleNumber(note.sale)}, request ${note.number}"),
                    ui.words(note.words.say()),
                    ui.button("Start a refund for this sale") { showQueue(note.counter, note.sale) },
                    ui.button("Dismiss this note") {
                        if (attempt(msg) { n.dismissRefundNote(note.index) } != null) showRefundNotes(AppWords.noteDismissed)
                    },
                    gap = 10,
                ),
            )
        }
        views.add(msg)
    }
    screen("Refund notes", views, listOf(backButton { showHome() }), back = { showHome() })
}

internal fun Controller.showNotes(): Unit = withNode { n ->
    val msg = Slot(act)
    val views = ArrayList<View>()
    views.add(para("The lines the node and the link logged since the last time this screen was opened."))
    val lines = attempt(msg) { n.notes() }
    if (lines == null) {
        views.add(msg)
    } else if (lines.isEmpty()) {
        views.add(ui.text("No new lines.", 16f))
    } else {
        views.add(line(lines.joinToString("\n") { Format.cleanForeign(it) }, muted = true, mono = true))
    }
    screen("Notes", views, listOf(backButton { showHome() }), back = { showHome() })
}

// ---- settings and about ---------------------------------------------------------------------------------------------------------

internal fun Controller.showSettings(): Unit = withNode { n ->
    val msg = Slot(act)
    val views = ArrayList<View>()
    var failed = false
    val held = try {
        n.settings()
    } catch (e: OwnerException) {
        msg.show(ui.words(e.sayWords()))
        failed = true
        null
    }
    var sub: String? = null
    if (failed) {
        views.add(msg)
    } else if (held == null) {
        views.add(ui.words(AppWords.noSettingsYet))
    } else {
        sub = OwnerLook.settingsSub(held.version)
        val (table, rest) = OwnerLook.instantTable(OwnerModel.settingsLines(held))
        views.add(instantCard(table))
        val folded = OwnerLook.settingsFolded(rest)
        if (folded.isNotEmpty()) views.add(foldedCard(folded))
    }
    views.add(line("What this phone holds as the shop's settings. This is a read-only view: changes go through Change settings.", muted = true))
    val change = ui.primary("Change settings") {
        settingsDraft = SettingsDraft()
        showChangeSettings()
    }
    screen("Settings", views, listOf(change), back = { showHome() }, sub = sub)
}

internal fun Controller.showAbout() {
    val sections = listOf(
        "Keep the app open" to
            "The wallet is followed while this app is on the screen, and it stops when you leave the app or the screen goes off, " +
            "unless Watch in the background is on. Then a service keeps following the wallet and tells the counters it is watching, " +
            "and Android shows a notification while it runs. It never starts by itself after the phone restarts.",
        "Keys" to
            "This phone holds Key 1 only. It never holds Key 2 or Key 3, and it never signs a spend by itself: " +
            "a spend leaves this app as a request for the Key app, and money moves only after the Key app has approved it.",
        "Screenshots" to "Screenshots and screen recordings of this app are blocked, because its screens show balances and addresses.",
        "Tor" to "Every connection goes through Tor: the app runs Tor itself, or uses Orbot if you choose it. If Tor is not connected the app sends nothing and says so.",
        "The till" to
            "The Till screen reads the shop's orders from Square or Clover with a read-only sign-in that you paste once. " +
            "The sign-in is sealed on this phone, is never shown again and can be removed. The orders are read about once a minute, " +
            "only while the app is on the screen, through Tor, and are matched against the counters' sales on this phone. " +
            "An alert you acknowledge is marked on this phone only. It has not been tried against a real till account yet.",
        "Not built yet" to
            "The link to the counters for a wallet made on this phone; " +
            "the books; archiving paid entries.",
    )
    val views = ArrayList<View>()
    views.add(ui.text("OpenBMT Owner ${Controller.VERSION}", 26f, bold = true))
    views.add(ui.text("Not for real funds. No independent security audit has been done. First build: it has not been run on a phone yet.", 16f, bold = true))
    for ((title, text) in sections) views.add(stack(label(title), ui.text(text, 16f), gap = 6))
    screen("About", views, listOf(backButton { showHome() }), back = { showHome() })
}
