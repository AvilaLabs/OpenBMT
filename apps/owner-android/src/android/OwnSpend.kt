package org.openbmt.owner.app

import android.view.View
import org.openbmt.app.AnimatedQrMade
import org.openbmt.app.AnimatedQrView
import org.openbmt.app.BbqrScan
import org.openbmt.app.PartsProgress
import org.openbmt.app.RowItem
import org.openbmt.app.SayWords
import org.openbmt.app.UrScan
import org.openbmt.app.Who
import org.openbmt.owner.OwnSpendBuilt
import org.openbmt.owner.OwnSpendFile
import org.openbmt.owner.OwnSpendItem
import org.openbmt.owner.OwnerException
import org.openbmt.owner.OwnerNode

// "Send from your wallet" for a shop with its own wallet (OA-115 to OA-118): the refunds and cash-outs that are
// queued, the compare screen, the file for the wallet by the person's pick of OA-70, what comes back, and the
// state of a payment that waits. The library freezes, files and judges (`docs/SESSION-RULES.md` AS-100 to AS-107);
// the picks are this file's and the words are `OwnSpendWords`.

/** What one payment's way to the wallet holds in memory: the session, the item and where the owner came from. */
private class OwnRun(val session: ByteArray, val number: Long, val back: () -> Unit) {
    var transport: Transport = Transport.Ur
}

private val MAX_OWN_RETURNED = 4 * 1024 * 1024

private fun Controller.words(w: SayWords): View = ui.words(w)

// ---- the list (OA-115) -------------------------------------------------------------------------------------------

/** The refunds and cash-outs that are queued, the offer to merge when it holds, and the payments already waiting. */
internal fun Controller.showOwnSpend(picked: List<Long> = emptyList(), message: SayWords? = null) {
    val n = node ?: return begin()
    val msg = Slot(act)
    message?.let { msg.show(words(it)) }
    val queue = attempt(msg) { n.queue() }
    val rows = if (queue == null) emptyList() else OwnSpendModel.rows(queue)
    val views = ArrayList<View>()
    if (rows.isEmpty()) {
        views.add(words(OwnSpendWords.empty))
    } else {
        views.add(
            ui.rows(
                rows.map { row ->
                    val mark = if (row.id in picked) "[x] " else "[ ] "
                    RowItem(mark + row.title, row.detail) { showOwnSpend(OwnSpendModel.toggle(picked, row.id)) }
                },
            ),
        )
    }
    val offer = try {
        n.mergeOffer()
    } catch (_: OwnerException) {
        null
    }
    val actions = ArrayList<android.widget.Button>()
    if (OwnSpendModel.canPrepare(rows, picked)) {
        actions.add(ui.primary("Prepare") { prepared(n, msg, picked) { attempt(msg) { n.ownSpendBuild(nowSecs(), ownFeeRate(n), picked) } } })
    }
    if (offer != null && OwnSpendModel.showMerge(offer, rows)) {
        // OC-19: the offer's own words give the cost now and at the usual fee, and what a merge shows.
        views.add(words(offer.words.say()))
        actions.add(ui.button("Prepare the merge") { prepared(n, msg, picked) { attempt(msg) { n.ownMerge(nowSecs()) } } })
    }
    // A payment that was prepared stays until it is taken or given up (OA-117).
    val open = try {
        n.ownSpendSessions()
    } catch (_: OwnerException) {
        emptyList()
    }
    for (s in open) {
        for ((i, item) in s.items.withIndex()) {
            val state = OwnSpendModel.state(item)
            views.add(
                ui.rows(
                    listOf(
                        RowItem(OwnSpendModel.itemTitle(item, i + 1, s.items.size), if (state == OwnSendState.Waiting) "Given to the wallet." else "Prepared.") {
                            showOwnFile(OwnRun(s.session, item.number) { showOwnSpend() })
                        },
                    ),
                ),
            )
        }
    }
    views.add(msg)
    actions.add(ui.button("Back") { showHome() })
    screen("Send from your wallet", views, actions, back = { showHome() }, words = OwnSpendWords.list, guard = true)
}

private fun Controller.ownFeeRate(n: OwnerNode): Long = try {
    val r = n.receiptsLine()
    if (r.kind == org.openbmt.owner.ReceiptsKind.Known && r.ratePerKvb > 0) r.ratePerKvb else 1_000
} catch (_: OwnerException) {
    1_000
}

/** Freezes the payments (op 159 or 164) and goes on to the first one's lines; a refusal is shown and nothing changes. */
private fun Controller.prepared(n: OwnerNode, msg: Slot, picked: List<Long>, build: () -> OwnSpendBuilt?) {
    val built = build() ?: return
    val session = built.session
    if (session == null) {
        // Nothing was frozen: what stayed in the queue says why, in the library's words.
        val why = built.skipped.firstOrNull()?.words?.say() ?: OwnSpendWords.empty
        return showOwnSpend(picked, why)
    }
    showOwnCompare(OwnRun(session, built.items.first().number) { showOwnSpend() }, built.items)
}

/**
 * The lines of OA-73 for every payment that was frozen, before any file leaves: the owner compares them with the
 * wallet's screen after the file is given. Nothing more is frozen here.
 */
private fun Controller.showOwnCompare(run: OwnRun, items: List<OwnSpendItem>) {
    val views = ArrayList<View>()
    views.add(words(OwnSpendWords.compare))
    for ((i, item) in items.withIndex()) {
        views.add(ui.text(OwnSpendModel.itemTitle(item, i + 1, items.size), 15f, bold = true))
        views.add(ui.text(OwnSpendModel.compareLines(item).joinToString("\n\n"), 14f, mono = true))
    }
    val actions = listOf(ui.primary("Next: choose how to send it") { pickOwnTransport(run) }, ui.button("Later") { run.back() })
    screen("Send from your wallet", views, actions, back = run.back, guard = true)
}

// ---- the file for the wallet (OA-115, OA-116) ----------------------------------------------------------------------------

private fun Controller.pickOwnTransport(run: OwnRun) {
    showTransportChoice({ t -> run.transport = t; showOwnFile(run, chosen = true) }, run.back)
}

/**
 * The payment's lines for the owner to compare with the wallet's screen, and its file by the transport the person
 * picked. Giving the file starts the watch of its transaction (AS-105); asking again gives the same file.
 */
private fun Controller.showOwnFile(run: OwnRun, chosen: Boolean = false, message: List<View> = emptyList()) {
    if (!chosen) return pickOwnTransport(run)
    val n = node ?: return begin()
    val msg = Slot(act)
    val back = { pickOwnTransport(run) }
    val file = attempt(msg) { n.ownSpendFile(run.session, run.number) }
        ?: return screen("Send from your wallet", listOf(msg), listOf(ui.button("Back") { run.back() }), back = run.back, guard = true)
    val views = ArrayList<View>()
    views.add(ui.text(OwnSpendModel.compareLines(asItem(file, run)).joinToString("\n\n"), 14f, mono = true))
    val height = codeHeight()
    val actions = ArrayList<android.widget.Button>()
    fun code(made: AnimatedQrMade) {
        made.view?.let { views.add(ui.codeCard(it)) }
        views.addAll(codeFallback(made))
    }
    when (run.transport) {
        Transport.Ur -> code(AnimatedQrView.make(act, Who.OWNER, HardwareModel.UR_PSBT, file.psbt, height, HardwareModel.UR_FRAGMENT_BYTES))
        Transport.Bbqr -> code(AnimatedQrView.makeBbqr(act, Who.OWNER, HardwareModel.BBQR_PSBT, file.psbt, height, HardwareModel.BBQR_PART_CHARS))
        Transport.File -> actions.add(
            ui.primary("Save the file") {
                saveBytes(
                    file.fileName, file.psbt, HardwareModel.MIME_BYTES,
                    { showOwnFile(run, chosen = true) },
                    { showOwnFile(run, chosen = true, message = listOf(ui.text(file.fileName, 14f, mono = true))) },
                    { w -> msg.show(words(w)) },
                )
            },
        )
    }
    views.addAll(message)
    // OA-117: a payment given to the wallet waits, and a wallet that sent it itself needs nothing more.
    views.add(words(OwnSpendModel.waitingWords(OwnSendState.Waiting)))
    views.add(msg)
    actions.add(ui.button("Next: bring it back") { showOwnReturn(run, null) })
    actions.add(
        ui.danger("Give it up") {
            if (attempt(msg) { n.ownSpendAbandon(run.session) } != null) {
                centre(OwnSpendWords.abandoned, listOf(ui.primary("Done") { showOwnSpend() }), title = "Send from your wallet")
            }
        },
    )
    actions.add(ui.button("Back") { back() })
    screen(
        "Send from your wallet", views, actions, back = back,
        words = OwnSpendWords.transport,
        guard = true,
    )
}

/** The lines of a payment are read from the file's summary; the item holds what the compare screen needs. */
private fun asItem(file: OwnSpendFile, run: OwnRun): OwnSpendItem =
    OwnSpendItem(run.number, file.txid, emptyList(), null, true, file.payments, file.change, file.fee)

// ---- bringing the file back (OA-116, OA-117) -----------------------------------------------------------------------------

/** The way back: the wallet's code by the camera, or a file. [message] is what the last try said, if it said anything. */
private fun Controller.showOwnReturn(run: OwnRun, message: SayWords?) {
    val n = node ?: return begin()
    val msg = Slot(act)
    message?.let { msg.show(words(it)) }
    val views = ArrayList<View>()
    views.add(
        ui.button("Open a file") {
            pickBytes(MAX_OWN_RETURNED + 1, { showOwnReturn(run, null) }, { bytes -> takeOwn(n, run, bytes) }) { w -> msg.show(words(w)) }
        },
    )
    views.add(msg)
    screen(
        "Send from your wallet", views,
        listOf(ui.primary("Scan the wallet's code") { scanOwn(n, run) }, ui.button("Back") { showOwnFile(run, chosen = true) }),
        back = { showOwnFile(run, chosen = true) }, words = OwnSpendWords.returnNeeded,
        guard = true,
    )
}

/** The wallet's code: a UR of `crypto-psbt` or `psbt`, a BBQr of type P or T, parts in any order. */
private fun Controller.scanOwn(n: OwnerNode, run: OwnRun) {
    val parts = PartsProgress(HardwareWords.partsMissing) { read, of -> HardwareModel.partsLine(read, of) }
    scan.begin(
        show = { v -> showScan(v) },
        onBytes = { bytes -> takeOwn(n, run, bytes) },
        onFailed = { f -> showOwnReturn(run, HardwareModel.scanFailure(f.off, f.problem, f.words)) },
        onCancel = { showOwnReturn(run, null) },
        ur = UrScan(HardwareModel.RETURN_UR_TYPES, Who.OWNER, parts) { _, payload -> takeOwn(n, run, payload) },
        bbqr = BbqrScan(HardwareModel.RETURN_BBQR_KINDS, Who.OWNER, parts) { _, payload -> takeOwn(n, run, payload) },
    )
}

/** Hands what came back to the library (op 162); a refusal shows its words and nothing else changes (OA-117). */
private fun Controller.takeOwn(n: OwnerNode, run: OwnRun, bytes: ByteArray) {
    val taken = try {
        n.ownSpendTake(nowSecs(), run.session, run.number, bytes)
    } catch (e: OwnerException) {
        return showOwnReturn(run, refusal(e))
    } catch (e: LinkageError) {
        return showOwnReturn(run, refusal(e))
    }
    val views = ArrayList<View>()
    views.add(words(OwnSpendWords.taken))
    for (s in taken.sends) {
        views.add(stack(ui.words(s.words.say()), line("Transaction ${s.txid}", muted = true, mono = true)))
    }
    screen("Send from your wallet", views, listOf(ui.primary("Done") { showOwnSpend() }), guard = true)
}
