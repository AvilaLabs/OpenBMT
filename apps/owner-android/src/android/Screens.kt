package org.openbmt.owner.app

import android.view.View
import org.openbmt.app.Format
import org.openbmt.app.SayWords
import org.openbmt.owner.AlertKind
import org.openbmt.owner.CashAmount
import org.openbmt.owner.ClearArgs
import org.openbmt.owner.OwnerException
import org.openbmt.owner.OwnerNode
import org.openbmt.owner.ReceiptsKind
import org.openbmt.owner.Stage
import org.openbmt.owner.StageKind
import org.openbmt.owner.ViewKind

// The screens of the owner app. Every call into the library is made here on
// the screen thread (OA-12); every state and every refusal is shown with the
// library's words, and the app's own words are only for what it decides itself.

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

private fun Controller.backButton(label: String = "Back", to: () -> Unit): View = ui.button(label) { to() }

// ---- home ------------------------------------------------------------------------------------------

internal fun Controller.showHome(): Unit = withNode { n ->
    val health = Slot(act)
    val lines = Slot(act)
    val views = ArrayList<View>()
    views.add(label("Is Bitcoin ready?"))
    views.add(health)
    views.add(label("The receipts line"))
    views.add(lines)
    views.add(ui.button("Receipts and merge") { showReceipts() })
    views.add(ui.button("Spend") { showSpend() })
    views.add(ui.button("Open approvals") { showSessions() })
    views.add(ui.button("Waiting to be delivered") { showOutbox() })
    views.add(ui.button("Refund queue") { showQueue() })
    views.add(ui.button("Cash out") { showCashOut() })
    views.add(ui.button("Old addresses") { showOld() })
    views.add(ui.button("Add a counter") { showAddCounter() })
    views.add(ui.button("Link and counters") { showLink() })
    views.add(ui.button("Till") { showTill() })
    views.add(ui.button("Alerts") { showAlerts() })
    views.add(ui.button("Refund notes") { showRefundNotes() })
    views.add(ui.button("Notes") { showNotes() })
    views.add(ui.button("Settings") { showSettings() })
    views.add(ui.button("Change the PIN") { showChangePin(null) })
    views.add(ui.button("About") { showAbout() })
    page(views)
    tick = {
        try {
            val s = n.status()
            val statusViews = ArrayList<View>()
            val pump = pumpFault
            if (pump != null) {
                statusViews.add(ui.words(pump))
            } else if (s.stopped) {
                statusViews.add(ui.words(AppWords.stopped))
            } else {
                statusViews.add(ui.words(s.health.words.say()))
            }
            s.halted?.let { statusViews.add(ui.words(it.words.say())) }
            if (s.view != ViewKind.Ready) statusViews.add(ui.words(s.viewWords.say()))
            if (s.view == ViewKind.DeepReorg) {
                statusViews.add(ui.button("Check again from the first block") { rescan() })
            }
            if (s.recoveryPending) {
                s.recoveryWords?.let { statusViews.add(ui.words(it.say())) }
                statusViews.add(ui.button("Finish the approval that was interrupted") { recover() })
            }
            statusViews.add(ui.text("The wallet can spend ${Format.sats(s.balanceSats)}. Checked to block ${s.checkedHeight} of ${s.tipHeight}.", 15f))
            health.show(*statusViews.toTypedArray())

            val receipts = n.receiptsLine()
            val link = if (setup?.link != null) n.linkState() else null
            val inProgress = OwnerModel.sendsInProgress(n.sendStates())
            val out = ArrayList<View>()
            out.add(ui.words(receipts.words.say()))
            out.add(ui.text(OwnerModel.alertsLine(link?.alerts?.size ?: 0), 15f, bold = (link?.alerts?.isNotEmpty() == true)))
            out.add(ui.text(OwnerModel.linkLine(link, link?.counters?.size ?: 0), 15f))
            linkFault?.let { out.add(ui.words(it)) }
            out.add(ui.text(OwnerModel.sendsLine(inProgress), 15f))
            lines.show(*out.toTypedArray())
        } catch (e: OwnerException) {
            health.show(ui.words(e.sayWords()))
        }
    }
    tick?.invoke()
}

private fun Controller.rescan(): Unit = withNode { n ->
    val msg = Slot(act)
    if (attempt(msg) { n.rescan() } != null) showHome() else page(listOf(msg, ui.button("Back") { showHome() }))
}

private fun Controller.recover(): Unit = withNode { n ->
    val msg = Slot(act)
    if (attempt(msg) { n.recoverKey1() } != null) showHome() else page(listOf(msg, ui.button("Back") { showHome() }))
}

// ---- the receipts line and the merge offer -------------------------------------------------------------

internal fun Controller.showReceipts(): Unit = withNode { n ->
    val receipts = Slot(act)
    val offer = Slot(act)
    val msg = Slot(act)
    val views = ArrayList<View>()
    views.add(ui.text("Receipts", 22f, bold = true))
    views.add(receipts)
    views.add(label("The offer to merge coins"))
    views.add(offer)
    views.add(msg)
    views.add(backButton { showHome() })
    page(views)
    tick = {
        try {
            val r = n.receiptsLine()
            val rv = ArrayList<View>()
            rv.add(ui.words(r.words.say()))
            if (r.kind == ReceiptsKind.Known) {
                rv.add(ui.text("${r.payments} payments held. Spending all of them costs ${Format.sats(r.costSats)} at ${r.ratePerKvb} sats per 1000 virtual bytes.", 15f))
            }
            receipts.show(*rv.toTypedArray())
            val m = n.mergeOffer()
            if (m == null) {
                offer.show(ui.text("No merge is offered now.", 15f))
            } else {
                offer.show(
                    ui.words(m.words.say()),
                    ui.text(
                        "Merge ${m.coins} of ${m.of} coins at ${m.ratePerKvb} sats per 1000 virtual bytes. It costs ${Format.sats(m.costSats)} now. " +
                            "Spending them as they are usually costs ${Format.sats(m.usualCostSats)}. The saving is ${Format.sats(m.savingSats)}.",
                        15f,
                    ),
                    ui.button("Build the merge request") {
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
    val msg = Slot(act)
    val views = ArrayList<View>()
    views.add(ui.text("Spend", 22f, bold = true))
    views.add(
        ui.text(
            "This builds a request for the Key app from what is in the queue. Nothing is signed here, and nothing is sent until the Key app has approved.",
            15f,
        ),
    )
    views.add(label("Fee rate, satoshis per 1000 virtual bytes"))
    val fee = numberField(act, "Fee rate")
    // The rate the receipts line works at is a starting point, if the node has one.
    try {
        val r = n.receiptsLine()
        if (r.kind == ReceiptsKind.Known && r.ratePerKvb > 0) fee.setText(r.ratePerKvb.toString())
    } catch (_: OwnerException) {
    }
    views.add(fee)
    views.add(ui.button("Build the request") {
        when (val f = Forms.feeRate(fee.text.toString())) {
            is Form.Refused -> msg.show(ui.words(f.words))
            is Form.Ok -> attempt(msg) { n.buildRequest(nowSecs(), f.value) }?.let { showBuilt(it) { showSpend() } }
        }
    })
    views.add(msg)
    views.add(backButton { showHome() })
    page(views)
}

// ---- open approvals ---------------------------------------------------------------------------------------

internal fun Controller.showSessions(message: SayWords? = null): Unit = withNode { n ->
    val views = ArrayList<View>()
    views.add(ui.text("Open approvals", 22f, bold = true))
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
            views.add(label(name))
            views.add(
                ui.button("Show its request again") {
                    attempt(msg) { n.sessionRequest(id) }?.let { showBuilt(org.openbmt.owner.Built(id, it, emptyList())) { showSessions() } }
                },
            )
            views.add(
                ui.button("Abandon it") {
                    if (attempt(msg) { n.abandonSession(id) } != null) showSessions(AppWords.sessionAbandoned)
                },
            )
        }
        views.add(msg)
    }
    views.add(backButton { showHome() })
    page(views)
}

// ---- the signed objects that wait -------------------------------------------------------------------------

internal fun Controller.showOutbox(): Unit = withNode { n ->
    val views = ArrayList<View>()
    views.add(ui.text("Waiting to be delivered", 22f, bold = true))
    views.add(ui.text("Signed by both keys and kept on this phone until they are delivered or confirmed.", 15f))
    val msg = Slot(act)
    val rows = attempt(msg) { n.outbox() }
    if (rows == null) {
        views.add(msg)
    } else if (rows.isEmpty()) {
        views.add(ui.text("Nothing is waiting.", 16f))
    } else {
        val now = nowSecs()
        for (row in rows) {
            views.add(label(OwnerModel.outboxTitle(row)))
            views.add(ui.words(row.words.say()))
            views.add(ui.text(OwnerModel.outboxLines(row, now).joinToString("\n"), 13f, mono = true))
        }
        views.addAll(waitingCounterViews(n, Slot(act)) { showOutbox() })
    }
    views.add(backButton { showHome() })
    page(views)
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
            ui.button("Cancel it") {
                if (attempt(msg) { n.cancel(id) } != null) { refresh() }
            },
        )
    }
}

internal fun Controller.showQueue(prefillCounter: ByteArray? = null, prefillSale: Long? = null): Unit = withNode { n ->
    val msg = Slot(act)
    val views = ArrayList<View>()
    views.add(ui.text("Refund queue", 22f, bold = true))
    val q = attempt(msg) { n.queue() }
    if (q == null) {
        views.add(msg)
        views.add(backButton { showHome() })
        return@withNode page(views)
    }
    if (q.refunds.isEmpty()) views.add(ui.text("No refund is queued.", 16f))
    for (r in q.refunds) {
        views.add(label("Refund ${r.id}: ${Format.sats(r.sats)} for sale ${OwnerModel.saleNumber(r.sale)} of ${OwnerModel.counterLabel(r.counter)}"))
        views.add(ui.text("Asked by ${Format.cleanForeign(r.cashier)}. To ${r.address}", 13f, mono = true))
        r.receivedSats?.let { views.add(ui.text("The wallet's own view shows ${Format.sats(it)} received for this sale.", 14f)) }
        views.add(ui.words(r.stage.words.say()))
        entryButtons(n, r.id, r.stage, msg, { showQueue() }, views)
    }
    if (q.cashOuts.isNotEmpty() || q.others.isNotEmpty()) views.add(label("Other entries"))
    for (c in q.cashOuts) {
        val amount = when (val a = c.amount) {
            is CashAmount.Sats -> Format.sats(a.sats)
            is CashAmount.All -> "everything"
        }
        views.add(ui.text("Cash-out ${c.id}: $amount to ${c.address}", 14f, bold = true))
        views.add(ui.words(c.stage.words.say()))
        entryButtons(n, c.id, c.stage, msg, { showQueue() }, views)
    }
    for (o in q.others) {
        views.add(ui.text(OwnerModel.entryTitle(o.kind, o.id), 14f, bold = true))
        views.add(ui.words(o.stage.words.say()))
        entryButtons(n, o.id, o.stage, msg, { showQueue() }, views)
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
    val counterField = ui.field("Counter identity, 64 digits").also {
        if (group == null && prefillCounter != null) it.setText(prefillCounter.joinToString("") { b -> "%02x".format(b) })
    }
    views.add(counterField)
    val sale = numberField(act, "Sale number, as the counter shows it").also { f -> prefillSale?.let { f.setText(OwnerModel.saleNumber(it).toString()) } }
    views.add(sale)
    val cashier = ui.field("Name of the cashier who asked")
    views.add(cashier)
    val address = ui.field("Address the customer wants the money sent to")
    views.add(address)
    val sats = numberField(act, "Amount in satoshis")
    views.add(sats)
    val result = Slot(act)
    views.add(
        ui.button("Queue the refund") {
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
        },
    )
    views.add(result)
    views.add(backButton { showHome() })
    page(views)
}

private fun Controller.showQueueAfterRefund(receivedSats: Long, atDepth: Boolean) {
    page(
        listOf(
            ui.words(if (atDepth) AppWords.refundQueuedAtDepth else AppWords.refundQueuedWaiting),
            ui.text("The wallet's own view shows ${Format.sats(receivedSats)} received for this sale.", 15f),
            ui.button("Back to the queue") { showQueue() },
        ),
    )
}

// ---- cash out ------------------------------------------------------------------------------------------------

internal fun Controller.showCashOut(): Unit = withNode { n ->
    val msg = Slot(act)
    val views = ArrayList<View>()
    views.add(ui.text("Cash out", 22f, bold = true))
    views.add(
        ui.text(
            "Moves the shop's money to an address of yours. It is queued here and paid only after the Key app approves it.",
            15f,
        ),
    )
    views.add(label("Address"))
    val address = ui.field("Address")
    views.add(address)
    views.add(label("How much"))
    val which = ui.choice(listOf("An amount", "Everything the wallet can spend"))
    views.add(which)
    val sats = numberField(act, "Amount in satoshis")
    views.add(sats)
    views.add(label("What the Key app shows for this address"))
    val name = ui.field("Label")
    views.add(name)
    views.add(
        ui.button("Queue the cash-out") {
            when (val f = Forms.cashOut(address.text.toString(), ui.chosen(which) == 1, sats.text.toString(), name.text.toString())) {
                is Form.Refused -> msg.show(ui.words(f.words))
                is Form.Ok -> if (attempt(msg) { n.cashOut(f.value) } != null) {
                    page(listOf(ui.words(AppWords.cashOutQueued), ui.button("Back") { showCashOut() }))
                }
            }
        },
    )
    views.add(msg)
    val q = attempt(msg) { n.queue() }
    if (q != null && q.cashOuts.isNotEmpty()) {
        views.add(label("Queued cash-outs"))
        for (c in q.cashOuts) {
            val amount = when (val a = c.amount) {
                is CashAmount.Sats -> Format.sats(a.sats)
                is CashAmount.All -> "everything"
            }
            views.add(ui.text("Cash-out ${c.id}: $amount to ${c.address}", 14f, bold = true))
            views.add(ui.words(c.stage.words.say()))
            entryButtons(n, c.id, c.stage, msg, { showCashOut() }, views)
        }
    }
    views.add(backButton { showHome() })
    page(views)
}

// ---- old addresses ------------------------------------------------------------------------------------------------

internal fun Controller.showOld(): Unit = withNode { n ->
    val line = Slot(act)
    val msg = Slot(act)
    val views = ArrayList<View>()
    views.add(ui.text("Old addresses", 22f, bold = true))
    views.add(line)
    views.add(
        ui.button("Check them now") {
            attempt(msg) { n.requestDeepCheck(nowSecs()) }?.let { msg.show(ui.words(it.words.say())) }
        },
    )
    views.add(msg)
    views.add(backButton { showHome() })
    page(views)
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
    val views = ArrayList<View>()
    views.add(ui.text("Link and counters", 22f, bold = true))
    views.add(body)
    views.add(backButton { showHome() })
    page(views)
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
                out.add(label(OwnerModel.counterLabel(c.counter)))
                out.add(ui.words(c.words.say()))
                out.add(ui.text(OwnerModel.counterLines(c, now).joinToString("\n"), 13f, mono = true))
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
    views.add(ui.text("Alerts", 22f, bold = true))
    message?.let { views.add(ui.words(it)) }
    val msg = Slot(act)
    if (setup?.link == null) {
        views.add(ui.words(AppWords.noLink))
    } else {
        val l = attempt(msg) { n.linkState() }
        if (l == null) {
            views.add(msg)
        } else {
            views.add(ui.text(OwnerModel.alertsLine(l.alerts.size), 16f))
            for (a in l.alerts) {
                val kind = if (a.kind == AlertKind.MissedCheckIn) "Missed check-in" else "Records do not match"
                views.add(label("$kind: ${OwnerModel.counterLabel(a.counter)}"))
                views.add(ui.words(a.words.say()))
                views.add(
                    ui.button("Clear this alert") {
                        if (attempt(msg) { n.clearAlert(ClearArgs(a.kind, a.counter, a.value)) } != null) showAlerts(AppWords.alertCleared)
                    },
                )
            }
            views.add(msg)
        }
    }
    views.add(backButton { showHome() })
    page(views)
}

// ---- refund notes and notes ------------------------------------------------------------------------------------------------

internal fun Controller.showRefundNotes(message: SayWords? = null): Unit = withNode { n ->
    val views = ArrayList<View>()
    views.add(ui.text("Refund notes", 22f, bold = true))
    message?.let { views.add(ui.words(it)) }
    val msg = Slot(act)
    val notes = attempt(msg) { n.refundNotes() }
    if (notes == null) {
        views.add(msg)
    } else if (notes.isEmpty()) {
        views.add(ui.text("No note about a refund request.", 16f))
    } else {
        for (note in notes) {
            views.add(label("${OwnerModel.counterLabel(note.counter)}, sale ${OwnerModel.saleNumber(note.sale)}, request ${note.number}"))
            views.add(ui.words(note.words.say()))
            views.add(ui.button("Start a refund for this sale") { showQueue(note.counter, note.sale) })
            views.add(
                ui.button("Dismiss this note") {
                    if (attempt(msg) { n.dismissRefundNote(note.index) } != null) showRefundNotes(AppWords.noteDismissed)
                },
            )
        }
        views.add(msg)
    }
    views.add(backButton { showHome() })
    page(views)
}

internal fun Controller.showNotes(): Unit = withNode { n ->
    val msg = Slot(act)
    val views = ArrayList<View>()
    views.add(ui.text("Notes", 22f, bold = true))
    views.add(ui.text("The lines the node and the link logged since the last time this screen was opened.", 15f))
    val lines = attempt(msg) { n.notes() }
    if (lines == null) {
        views.add(msg)
    } else if (lines.isEmpty()) {
        views.add(ui.text("No new lines.", 16f))
    } else {
        views.add(ui.text(lines.joinToString("\n") { Format.cleanForeign(it) }, 13f, mono = true))
    }
    views.add(backButton { showHome() })
    page(views)
}

// ---- settings and about ---------------------------------------------------------------------------------------------------------

internal fun Controller.showSettings(): Unit = withNode { n ->
    val msg = Slot(act)
    val views = ArrayList<View>()
    views.add(ui.text("Settings", 22f, bold = true))
    views.add(ui.text("What this phone holds as the shop's settings. This is a read-only view: changes go through Change settings.", 15f))
    var failed = false
    val held = try {
        n.settings()
    } catch (e: OwnerException) {
        msg.show(ui.words(e.sayWords()))
        failed = true
        null
    }
    if (failed) {
        views.add(msg)
    } else if (held == null) {
        views.add(ui.words(AppWords.noSettingsYet))
    } else {
        for ((name, value) in OwnerModel.settingsLines(held)) {
            views.add(ui.text("$name\n$value", 15f))
        }
    }
    views.add(
        ui.button("Change settings") {
            settingsDraft = SettingsDraft()
            showChangeSettings()
        },
    )
    views.add(backButton { showHome() })
    page(views)
}

internal fun Controller.showAbout() {
    page(
        listOf(
            ui.text("OpenBMT Owner ${Controller.VERSION}", 22f, bold = true),
            ui.text("Not for real funds. No independent security audit has been done. First build: it has not been run on a phone yet.", 16f, bold = true),
            label("Keep the app open"),
            ui.text(
                "The wallet is followed only while this app is on the screen, and it stops when you leave the app or the screen goes off. " +
                    "There is no service in the background and no notification in this version.",
                16f,
            ),
            label("Keys"),
            ui.text(
                "This phone holds Key 1 only. It never holds Key 2 or Key 3, and it never signs a spend by itself: " +
                    "a spend leaves this app as a request for the Key app, and money moves only after the Key app has approved it.",
                16f,
            ),
            label("Screenshots"),
            ui.text("Screenshots and screen recordings of this app are blocked, because its screens show balances and addresses.", 16f),
            label("Tor"),
            ui.text("Every connection goes through Orbot on this phone. If Orbot is not running the app sends nothing and says so.", 16f),
            label("The till"),
            ui.text(
                "The Till screen reads the shop's orders from Square or Clover with a read-only sign-in that you paste once. " +
                    "The sign-in is sealed on this phone, is never shown again and can be removed. The orders are read about once a minute, " +
                    "only while the app is on the screen, through Orbot, and are matched against the counters' sales on this phone. " +
                    "An alert you acknowledge is marked on this phone only. It has not been tried against a real till account yet.",
                16f,
            ),
            label("Not built yet"),
            ui.text(
                "The link to the counters for a wallet made on this phone; " +
                    "the books; archiving paid entries.",
                16f,
            ),
            ui.button("Back") { showHome() },
        ),
    )
}
