package org.openbmt.owner.app

import android.text.InputType
import android.view.View
import android.widget.Button
import java.io.IOException
import org.openbmt.app.Format
import org.openbmt.app.RowItem
import org.openbmt.app.Tones
import org.openbmt.owner.FloatPaid
import org.openbmt.owner.FloatWalletView
import org.openbmt.owner.OwnerException
import org.openbmt.owner.OwnerNode
import org.openbmt.owner.ReceiptsKind
import org.openbmt.owner.RefundRow

// The refund float (`docs/FLOAT-RULES.md`). The setup (FL-01, FL-03, FL-04) scans the float's words as a Compact SeedQR, turns them
// into the seed and seals it in Rust, wraps the data key with the Keystore as the starter wallet's seed is (`StarterKeys.kt`), and
// never holds the words as text. What the float holds comes from the node's watch of it (FL-05). Paying a refund (FL-20 to FL-23)
// and turning the float off (FL-35) sign in the library from the sealed seed, after the owner unlocks the key for that call, and
// hand the finished payment to the node's outbox, which takes it only for the float's own coins.

/** The library's float calls, with the coin view from the running node. */
private fun Controller.floatCore(): FloatCore = NodeFloat(node)

private const val MIN_CONFIRMATIONS = 1L

/** FL-05: how many blocks before the tip the node's watch of a new float starts. */
private const val WATCH_MARGIN = 144L

internal fun Controller.floatRow(): RowItem {
    val record = try {
        FloatStore(appDir).load()
    } catch (_: IOException) {
        null
    }
    val home = FloatModel.home(floatCore(), record)
    val sub = when {
        home.low == true -> FloatWords.statusLow
        home.heldSats != null -> Format.sats(home.heldSats)
        else -> FloatWords.unavailable
    }
    return RowItem(FloatWords.rowTitle, sub) { showFloat() }
}

internal fun Controller.showFloat(notice: String? = null) {
    val store = FloatStore(appDir)
    val record = try {
        store.load()
    } catch (_: IOException) {
        return floatReadFailed()
    }
    val view = floatCore().coinView()
    val home = FloatModel.home(floatCore(), record, view)
    val actions = FloatModel.actions(record, home, store.hasSealed(), emptyList())
    val body = ArrayList<View>()
    notice?.let { body.add(ui.text(it)) }
    body.add(ui.text(home.message))
    record?.let { body.add(ui.labelled(FloatWords.capLabel, ui.text(Format.sats(it.capSats)))) }
    home.heldSats?.let { body.add(ui.labelled(FloatWords.heldLabel, ui.text(Format.sats(it)))) }
    // FL-10: the node's view of the float's coins is not there yet, so the page says so and never guesses.
    if (record?.state == FloatState.Ready && home.heldSats == null) body.add(ui.text(FloatWords.statusNotWatched))
    val rows = ArrayList<RowItem>()
    rows.add(RowItem(FloatWords.setupTitle) { showFloatSetup() })
    rows.add(RowItem(FloatWords.capLabel) { showFloatCap() })
    if (actions.refill) rows.add(RowItem(FloatWords.refillAction) { showFloatRefill() })
    rows.add(RowItem(FloatWords.refundPay) { showFloatRefund() })
    rows.add(RowItem(FloatWords.sweepTitle) { showFloatSweep() })
    body.add(ui.rows(rows))
    screen(FloatWords.rowTitle, body, listOf(ui.button(FloatWords.back) { showHome() }), back = { showHome() })
}

private fun Controller.floatReadFailed() {
    screen(FloatWords.rowTitle, listOf(ui.text(FloatWords.storageUnavailable)),
        listOf(ui.button(FloatWords.back) { showHome() }), back = { showHome() })
}

/** SS-12: a screen that takes the float's words carries the same window flags as the starter wallet's, applied once it is on show. */
private fun Controller.floatWordsScreen(title: String, body: List<View>, actions: List<Button>, back: (() -> Unit)?) {
    tick = null
    val page = ui.page(ui.bar(title, null, back), body, actions)
    act.show(page)
    StarterWindow.protect(act, page)
}

private fun floatKeystore() = StarterKeystore(FloatStore.KEY_ALIAS)

/** FL-05: the height the node starts watching a new float from: a day of blocks before the tip, or 0 when the tip is not known. */
private fun Controller.watchFrom(): Long = try {
    ((node?.status()?.tipHeight ?: 0L) - WATCH_MARGIN).coerceAtLeast(0L)
} catch (_: OwnerException) {
    0L
} catch (_: LinkageError) {
    0L
}

private fun Controller.recentRate(n: OwnerNode): Long? = try {
    val r = n.receiptsLine()
    if (r.kind == ReceiptsKind.Known && r.ratePerKvb > 0) r.ratePerKvb else null
} catch (_: OwnerException) {
    null
}

/**
 * FL-03, FL-06: the float's key is gone from the phone (or never was): nothing can be signed. The page offers to scan the float's
 * words again, made on the stick at the index the phone recorded, when the phone has the float's record and public wallet.
 */
private fun Controller.floatKeyGone(notice: View? = null) {
    val store = FloatStore(appDir)
    val record = record(store)
    val wallet = try { store.loadWallet() } catch (_: IOException) { null }
    val body = ArrayList<View>()
    notice?.let { body.add(it) }
    body.add(ui.text(FloatWords.keyGone))
    val actions = ArrayList<Button>()
    if (wallet != null && FloatModel.canRestore(record, wallet)) {
        body.add(ui.text(FloatWords.restoreWhat))
        actions.add(ui.primary(FloatWords.restoreScan) { floatRestoreScan(store, wallet) })
    } else {
        body.add(ui.text(FloatWords.restoreNeedsFloat))
    }
    actions.add(ui.button(FloatWords.back) { showFloat() })
    floatWordsScreen(FloatWords.rowTitle, body, actions, back = { showFloat() })
}

/** FL-06: the camera reads one plain code, as for the setup; the scan page sets the window flags of SS-12 and the bytes go to the library. */
private fun Controller.floatRestoreScan(store: FloatStore, wallet: FloatWalletView) {
    scan.begin(
        show = { v ->
            showScan(v)
            StarterWindow.protect(act, act.window.decorView)
        },
        onBytes = { bytes -> floatRestoreSeal(store, wallet, bytes) },
        onFailed = { floatKeyGone(ui.text(FloatWords.scanFailed)) },
        onCancel = { floatKeyGone() },
    )
}

/**
 * FL-06: the library takes the scan only when its wallet is the recorded float's wallet, and seals it under a new data key. Then the
 * old Keystore key goes, the new sealed seed is kept, and the new data key is wrapped by a new Keystore key (after the owner proves
 * they are there) and kept. The record is not touched: the float is ready again as it was. Whatever fails leaves no key behind, as
 * the setup's undo does.
 */
private fun Controller.floatRestoreSeal(store: FloatStore, wallet: FloatWalletView, scanned: ByteArray) {
    val sealed = try {
        LibraryFloat.restore(scanned, wallet.description)
    } catch (e: OwnerException) {
        return floatKeyGone(ui.words(e.sayWords()))
    } catch (e: LinkageError) {
        return floatKeyGone(ui.words(AppWords.libraryMissing(e.message)))
    }
    val keystore = floatKeystore()
    fun undo() {
        store.deleteKey()
        keystore.delete()
    }
    keystore.delete()
    try {
        store.saveSealed(sealed.sealed)
    } catch (_: IOException) {
        sealed.wipe()
        undo()
        return floatKeyGone(ui.text(FloatWords.storageUnavailable))
    }
    starterWrapKey(sealed.dataKey, FloatWords.unlockWhy, floatKeystore(), { store.saveWrapped(it) }, FloatWords.rowTitle) { kept ->
        if (!kept) {
            undo()
            return@starterWrapKey floatKeyGone(ui.text(FloatWords.sealFailed))
        }
        showFloat(FloatWords.restoreDone)
    }
}

/**
 * FL-05: the phone just recorded a new refill index or change index, so the node moves its watch of the float to 20 past it at once,
 * without a restart, and reads the blocks since the float was set up for the new addresses.
 */
private fun Controller.floatWatchRaise(store: FloatStore) {
    record(store)?.let { NodeFloat(node).raiseWatch(it) }
}

/**
 * FL-20, FL-35: opens the float's data key for one call after the owner proves they are there, signs in the library from the
 * sealed seed and gives the signed payment to [signed]. A key the phone dropped says so (FL-03).
 */
private fun Controller.floatSign(
    msg: Slot,
    description: String,
    sign: (sealed: ByteArray, key: ByteArray) -> FloatPaid,
    signed: (FloatPaid) -> Unit,
) {
    val store = FloatStore(appDir)
    val sealed = try { store.loadSealed() } catch (_: IOException) { null } ?: return floatKeyGone()
    starterWithDataKey(
        false, description,
        use = { key ->
            val paid = try {
                sign(sealed, key)
            } catch (e: OwnerException) {
                return@starterWithDataKey msg.show(ui.words(e.sayWords()))
            } catch (e: LinkageError) {
                return@starterWithDataKey msg.show(ui.words(AppWords.libraryMissing(e.message)))
            }
            signed(paid)
        },
        gone = { floatKeyGone() },
        failed = { msg.show(ui.words(StarterWords.said(StarterWords.authFailed, Tones.STOP))) },
        keystore = floatKeystore(),
        loadWrapped = { store.loadWrapped() },
    )
}

/** FL-01/04: no typed words or substitute QR format is accepted: only the Compact SeedQR of the float's 12 words. */
private fun Controller.showFloatSetup(notice: View? = null) {
    val store = FloatStore(appDir)
    val record = try {
        store.load()
    } catch (_: IOException) {
        return floatReadFailed()
    }
    val actions = FloatModel.actions(record, FloatModel.home(floatCore(), record), store.hasSealed(), emptyList())
    val body = ArrayList<View>()
    notice?.let { body.add(it) }
    body.add(ui.text(FloatWords.childOnly))
    body.add(ui.text(FloatWords.childIndex))
    body.add(ui.text(FloatWords.nextIndex))
    if (record == null) body.add(ui.text(FloatWords.needCapFirst))
    val scanButton = ui.primary(FloatWords.scanChild) { if (record != null) floatScan(store, record) }
    scanButton.isEnabled = actions.setup
    floatWordsScreen(FloatWords.setupTitle, body, listOf(scanButton, ui.button(FloatWords.back) { showFloat() }), back = { showFloat() })
}

/** FL-01: the camera reads one plain code; its bytes go to the library and are not kept here. */
private fun Controller.floatScan(store: FloatStore, draft: FloatRecord) {
    scan.begin(
        show = { v ->
            showScan(v)
            StarterWindow.protect(act, act.window.decorView)
        },
        onBytes = { bytes -> floatSeal(store, draft, bytes) },
        onFailed = { showFloatSetup(ui.text(FloatWords.scanFailed)) },
        onCancel = { showFloatSetup() },
    )
}

/**
 * FL-03, FL-04: the library turns the scan into the seed and seals it. The order is fixed: the sealed seed is kept, the data
 * key is wrapped by the Keystore key (after the owner proves they are there) and kept, and only then the public wallet and the
 * record, as Ready at index 0. Whatever fails before that leaves no key and no Ready record behind.
 */
private fun Controller.floatSeal(store: FloatStore, draft: FloatRecord, scanned: ByteArray) {
    val sealed = try {
        LibraryFloat.seal(scanned)
    } catch (e: OwnerException) {
        return showFloatSetup(ui.words(e.sayWords()))
    } catch (e: LinkageError) {
        return showFloatSetup(ui.words(AppWords.libraryMissing(e.message)))
    }
    val keystore = floatKeystore()
    fun undo() {
        store.deleteKey()
        keystore.delete()
    }
    try {
        store.saveSealed(sealed.sealed)
    } catch (_: IOException) {
        sealed.wipe()
        undo()
        return showFloatSetup(ui.text(FloatWords.storageUnavailable))
    }
    starterWrapKey(sealed.dataKey, FloatWords.unlockWhy, floatKeystore(), { store.saveWrapped(it) }, FloatWords.rowTitle) { kept ->
        if (!kept) {
            undo()
            return@starterWrapKey showFloatSetup(ui.text(FloatWords.sealFailed))
        }
        try {
            store.finishSetup(draft, sealed.wallet, 0, watchFrom())
            // The node watches the float from its next start (FL-05).
            stopNode()
            startNode()
        } catch (_: IOException) {
            undo()
            showFloatSetup(ui.text(FloatWords.storageUnavailable))
        } catch (_: IllegalStateException) {
            undo()
            showFloatSetup(ui.text(FloatWords.sealFailed))
        }
    }
}

/** FL-10/11: changing the cap changes no balance and creates no refill. */
private fun Controller.showFloatCap() {
    val store = FloatStore(appDir)
    val record = try { store.load() } catch (_: IOException) { return floatReadFailed() }
    val cap = ui.field(FloatWords.capLabel).also {
        it.inputType = InputType.TYPE_CLASS_NUMBER
        record?.let { saved -> it.setText(saved.capSats.toString()) }
    }
    val note = Slot(act)
    val save = ui.primary(FloatWords.saveCap) {
        val sats = FloatModel.cap(cap.text.toString())
        if (sats == null) {
            note.show(ui.text(FloatWords.capInvalid))
        } else {
            try {
                if (record == null) store.save(FloatRecord(sats)) else store.updateCap(record, sats)
                showFloat(FloatWords.capSaved)
            } catch (_: IOException) {
                note.show(ui.text(FloatWords.storageUnavailable))
            }
        }
    }
    screen(FloatWords.capLabel, listOf(ui.labelled(FloatWords.capLabel, cap), note),
        listOf(save, ui.button(FloatWords.back) { showFloat() }), back = { showFloat() })
}

/**
 * FL-11/30/31: the refill is prepared by the library as a payment from the shop's savings to the float's next address, then sent
 * through "Send from your wallet" (OA-115 to OA-118). It needs the node's view of the float's coins, so with none it is off and no
 * balance is guessed.
 */
private fun Controller.showFloatRefill() {
    val store = FloatStore(appDir)
    val record = try { store.load() } catch (_: IOException) { return floatReadFailed() }
    val wallet = try { store.loadWallet() } catch (_: IOException) { null }
    val view = floatCore().coinView()
    val home = FloatModel.home(floatCore(), record, view)
    val body = ArrayList<View>()
    body.add(ui.text(FloatWords.refillLinksSavings))
    val checked = view as? FloatCoinView.Checked
    when {
        checked == null -> body.add(ui.text(FloatWords.unavailable))
        home.refillSats == null || home.refillSats == 0L -> body.add(ui.text(FloatWords.refillNoRoom))
        else -> body.add(ui.labelled(FloatWords.refillAction, ui.text(Format.sats(home.refillSats))))
    }
    val msg = Slot(act)
    body.add(msg)
    val go = ui.primary(FloatWords.refillAction) {
        val n = node ?: return@primary begin()
        if (record == null || wallet == null || checked == null) return@primary
        try {
            val queued = n.floatRefill(wallet.description, record.nextReceiveIndex, record.capSats, checked.confirmedSats, checked.unconfirmedIncomingSats, null)
            // The next address is used up as soon as the payment to it is queued.
            store.save(FloatModel.afterRefill(store.load() ?: record))
            floatWatchRaise(store)
            showOwnSpend(listOf(queued.id))
        } catch (e: OwnerException) {
            msg.show(ui.words(e.sayWords()))
        } catch (_: IOException) {
            msg.show(ui.text(FloatWords.storageUnavailable))
        } catch (e: LinkageError) {
            msg.show(ui.words(AppWords.libraryMissing(e.message)))
        }
    }
    go.isEnabled = wallet != null && FloatModel.canRefill(home)
    screen(FloatWords.refillAction, body, listOf(go, ui.button(FloatWords.back) { showFloat() }), back = { showFloat() })
}

/**
 * FL-20/23: the ready refunds the float can cover are listed as the starter wallet's list reads them. One that is larger than
 * what the float holds is not split; it stays in the queue for the shop's wallet.
 */
private fun Controller.showFloatRefund() {
    val store = FloatStore(appDir)
    val record = try { store.load() } catch (_: IOException) { return floatReadFailed() }
    val n = node ?: return begin()
    val home = FloatModel.home(floatCore(), record)
    val msg = Slot(act)
    val queue = attempt(msg) { n.queue() }
    val ready: List<RefundRow> = FloatModel.readyRefunds(queue?.refunds.orEmpty())
    val payable = ready.filter { FloatModel.canPay(home, store.hasSealed(), it) }
    val body = ArrayList<View>()
    if (ready.isEmpty()) {
        body.add(ui.text(FloatWords.refundListEmpty))
    } else {
        body.add(ui.text(FloatWords.refundReadyHeading))
        body.add(ui.rows(payable.map { r -> RowItem(Format.sats(r.sats), r.address) { floatPayConfirm(n, store, r) } }))
        if (payable.size < ready.size) body.add(ui.text(FloatWords.refundTooBig))
    }
    body.add(msg)
    screen(FloatWords.refundPay, body, listOf(ui.button(FloatWords.back) { showFloat() }), back = { showFloat() })
}

/** FL-22: the address in full, the amount and the fee rate before the owner proves they are there; nothing is signed yet. */
private fun Controller.floatPayConfirm(n: OwnerNode, store: FloatStore, r: RefundRow) {
    val rate = try {
        floatCore().feeRate(recentRate(n), null)
    } catch (e: OwnerException) {
        return centre(e.sayWords(), listOf(ui.button(FloatWords.back) { showFloatRefund() }), title = FloatWords.refundPay)
    } catch (e: LinkageError) {
        return centre(AppWords.libraryMissing(e.message), listOf(ui.button(FloatWords.back) { showFloatRefund() }), title = FloatWords.refundPay)
    }
    val msg = Slot(act)
    val views = listOf(ui.facts(FloatModel.beforeLines(r.address, r.sats, rate)), ui.text(StarterWords.payAuthorise, 16f), msg)
    val go = ui.primary(FloatWords.payConfirm) {
        floatSign(msg, FloatWords.payConfirm, { sealed, key -> n.floatPay(sealed, key, r.id, rate, MIN_CONFIRMATIONS) }) { paid ->
            floatPaySigned(n, store, paid, r)
        }
    }
    screen(FloatWords.refundPay, views, listOf(go, ui.button(FloatWords.back) { showFloatRefund() }), back = { showFloatRefund() })
}

/** What was signed, shown in full before it goes out; the node's outbox takes it only for the float's own coins and this ready refund. */
private fun Controller.floatPaySigned(n: OwnerNode, store: FloatStore, paid: FloatPaid, r: RefundRow) {
    val msg = Slot(act)
    val send = ui.primary(StarterWords.paySendButton) {
        try {
            record(store)?.let { rec -> paid.changeIndex?.let { store.changeUsed(rec, it) } }
            n.floatSend(paid.tx, listOf(r.id), false)
            // The node reads the float's coins to send; its watch moves only once the payment is in its outbox.
            floatWatchRaise(store)
            centre(
                StarterWords.said(FloatWords.payDoneHeading, FloatWords.payDoneWhat, FloatWords.payDoneNext, Tones.GOOD),
                listOf(ui.primary(StarterWords.doneButton) { showFloat() }),
                extra = listOf(ui.text(paid.txid, 14f, mono = true)), title = FloatWords.rowTitle,
            )
        } catch (e: OwnerException) {
            msg.show(ui.words(e.sayWords()))
        } catch (e: IOException) {
            msg.show(ui.words(AppWords.fileNotSaved(e.message)))
        }
    }
    screen(FloatWords.refundPay, listOf(ui.facts(FloatModel.afterLines(paid)), ui.words(StarterWords.signed), msg),
        listOf(send, ui.button(FloatWords.back) { showFloatRefund() }), back = { showFloatRefund() })
}

private fun record(store: FloatStore): FloatRecord? = try { store.load() } catch (_: IOException) { null }

/**
 * FL-35: turning the float off moves all of it to an address of the shop's own range, once, through the node's outbox with no
 * refund. Once the move has one confirmation the sealed seed, the wrapped key and the Keystore key are deleted.
 */
private fun Controller.showFloatSweep() {
    val store = FloatStore(appDir)
    val record = try { store.load() } catch (_: IOException) { return floatReadFailed() }
    if (record?.pendingSweepId != null) return showFloatSweepDelete()
    val n = node ?: return begin()
    val home = FloatModel.home(floatCore(), record)
    val actions = FloatModel.actions(record, home, store.hasSealed(), emptyList())
    val msg = Slot(act)
    val body = ArrayList<View>()
    body.add(ui.text(if (home.heldSats == null) FloatWords.sweepConfirmationsUnknown else FloatWords.sweepBefore))
    if (home.state == FloatState.Ready && home.heldSats == 0L) body.add(ui.text(FloatWords.sweepNothing))
    body.add(msg)
    val sweep = ui.primary(FloatWords.sweepConfirm) { if (record != null) floatSweepConfirm(n, store, record) }
    sweep.isEnabled = actions.sweep
    screen(FloatWords.sweepTitle, body, listOf(sweep, ui.button(FloatWords.back) { showFloat() }), back = { showFloat() })
}

private fun Controller.floatSweepConfirm(n: OwnerNode, store: FloatStore, record: FloatRecord) {
    val rate = try {
        floatCore().feeRate(recentRate(n), null)
    } catch (e: OwnerException) {
        return centre(e.sayWords(), listOf(ui.button(FloatWords.back) { showFloatSweep() }), title = FloatWords.sweepTitle)
    } catch (e: LinkageError) {
        return centre(AppWords.libraryMissing(e.message), listOf(ui.button(FloatWords.back) { showFloatSweep() }), title = FloatWords.sweepTitle)
    }
    val msg = Slot(act)
    val views = listOf(ui.facts(FloatModel.sweepBeforeLines(rate)), ui.text(StarterWords.payAuthorise, 16f), msg)
    val go = ui.primary(FloatWords.sweepConfirm) {
        floatSign(msg, FloatWords.sweepConfirm, { sealed, key -> n.floatSweep(sealed, key, nowSecs(), rate, MIN_CONFIRMATIONS) }) { paid ->
            floatSweepSigned(n, store, record, paid)
        }
    }
    screen(FloatWords.sweepTitle, views, listOf(go, ui.button(FloatWords.back) { showFloatSweep() }), back = { showFloatSweep() })
}

private fun Controller.floatSweepSigned(n: OwnerNode, store: FloatStore, record: FloatRecord, paid: FloatPaid) {
    val msg = Slot(act)
    val send = ui.primary(StarterWords.paySendButton) {
        try {
            // The move is kept first, so that its confirmations are asked of the node by its ID; then the node's outbox
            // takes it, and the app sends nothing.
            store.pendingSweep(record, paid.txid)
            n.floatSend(paid.tx, emptyList(), true)
            showFloatSweepDelete()
        } catch (e: OwnerException) {
            msg.show(ui.words(e.sayWords()))
        } catch (e: IOException) {
            msg.show(ui.words(AppWords.fileNotSaved(e.message)))
        }
    }
    screen(FloatWords.sweepTitle, listOf(ui.facts(FloatModel.afterLines(paid)), ui.words(StarterWords.signed), msg),
        listOf(send, ui.button(FloatWords.back) { showFloatSweep() }), back = { showFloatSweep() })
}

/** FL-35: after one confirmation of the move the key is deleted and the float is off; an unknown depth never deletes it. */
private fun Controller.showFloatSweepDelete() {
    val store = FloatStore(appDir)
    val record = try { store.load() } catch (_: IOException) { return floatReadFailed() }
    val txid = record?.pendingSweepId ?: return showFloat()
    val n = node ?: return begin()
    val confirmations = try {
        n.starterConfirmations(txid)
    } catch (_: OwnerException) {
        null
    }
    val ready = FloatModel.sweepMayDeleteWords(confirmations)
    val views = ArrayList<View>()
    views.add(ui.text(txid, 14f, mono = true))
    views.add(ui.text(if (confirmations == null) FloatWords.sweepDepthUnknown else FloatWords.sweepWaiting, 16f))
    val actions = ArrayList<android.widget.Button>()
    if (ready) {
        actions.add(
            ui.danger(StarterWords.deleteButton) {
                try {
                    store.deleteKey()
                    floatKeystore().delete()
                    store.completeSweep(record, confirmations, nowSecs())
                    centre(
                        StarterWords.said(FloatWords.sweepDeletedHeading, FloatWords.sweepDeletedWhat, FloatWords.sweepDeletedNext, Tones.GOOD),
                        listOf(ui.primary(StarterWords.doneButton) { showFloat() }), title = FloatWords.rowTitle,
                    )
                } catch (_: IOException) {
                    showFloat(FloatWords.storageUnavailable)
                }
            },
        )
    }
    if (confirmations == null) {
        // The node never took the move: it can be signed again, to an address of the shop's own range taken once already.
        actions.add(ui.primary(FloatWords.sweepConfirm) {
            try {
                store.clearPendingSweep(record)
                showFloatSweep()
            } catch (_: IOException) {
                showFloat(FloatWords.storageUnavailable)
            }
        })
    }
    actions.add(ui.button(FloatWords.back) { showFloat() })
    screen(FloatWords.sweepTitle, views, actions, back = { showFloat() })
}
