package org.openbmt.owner.app

import android.text.InputType
import android.view.View
import android.widget.Button
import java.io.IOException
import org.openbmt.app.Format
import org.openbmt.app.RowItem
import org.openbmt.app.SayWords
import org.openbmt.owner.OwnerException
import org.openbmt.owner.RefundRow

// The refund float (`docs/FLOAT-RULES.md`). The setup (FL-01, FL-03, FL-04) is wired to the library: the float's words are scanned
// as a Compact SeedQR, turned into the seed and sealed in Rust, wrapped with the Keystore as the starter wallet's seed is
// (`StarterKeys.kt`), and never held as text. What the float holds, paying a refund and turning it off wait for the library: the
// node does not watch the float's addresses and takes no payment of the float into its outbox, and `float_pay` and `float_sweep`
// still take the words as text. Their buttons stay off (`FloatModel.SIGNING_WIRED`).

internal fun Controller.floatRow(): RowItem {
    val record = try {
        FloatStore(appDir).load()
    } catch (_: IOException) {
        null
    }
    val home = FloatModel.home(LibraryFloat, record)
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
    val view = LibraryFloat.coinView()
    val home = FloatModel.home(LibraryFloat, record, view)
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

/** FL-01/04: no typed words or substitute QR format is accepted: only the Compact SeedQR of the float's 12 words. */
private fun Controller.showFloatSetup(notice: View? = null) {
    val store = FloatStore(appDir)
    val record = try {
        store.load()
    } catch (_: IOException) {
        return floatReadFailed()
    }
    val actions = FloatModel.actions(record, FloatModel.home(LibraryFloat, record), store.hasSealed(), emptyList())
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
            store.finishSetup(draft, sealed.wallet)
            showFloat(FloatWords.setupDone)
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
    val view = LibraryFloat.coinView()
    val home = FloatModel.home(LibraryFloat, record, view)
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

/** FL-20/23: the ready refunds are listed as the starter wallet's list reads them; paying waits for the library. */
private fun Controller.showFloatRefund() {
    val store = FloatStore(appDir)
    val record = try { store.load() } catch (_: IOException) { return floatReadFailed() }
    val home = FloatModel.home(LibraryFloat, record)
    val ready: List<RefundRow> = try {
        FloatModel.readyRefunds(node?.queue()?.refunds.orEmpty())
    } catch (_: OwnerException) {
        emptyList()
    }
    val body = ArrayList<View>()
    if (ready.isEmpty()) {
        body.add(ui.text(FloatWords.refundListEmpty))
    } else {
        body.add(ui.text(FloatWords.refundReadyHeading))
        body.add(ui.facts(ready.map { Format.sats(it.sats) to it.address }))
    }
    body.add(ui.text(FloatWords.refundNotWired))
    val actions = FloatModel.actions(record, home, store.hasSealed(), ready)
    val pay = ui.primary(FloatWords.payConfirm) {}
    pay.isEnabled = actions.pay
    screen(FloatWords.refundPay, body, listOf(pay, ui.button(FloatWords.back) { showFloat() }), back = { showFloat() })
}

/** FL-35: no key deletion or shutdown can occur without a confirmed sweep, so with no sweep to send the button stays off. */
private fun Controller.showFloatSweep() {
    val store = FloatStore(appDir)
    val record = try { store.load() } catch (_: IOException) { return floatReadFailed() }
    val home = FloatModel.home(LibraryFloat, record)
    val actions = FloatModel.actions(record, home, store.hasSealed(), emptyList())
    val sweep = ui.primary(FloatWords.sweepConfirm) {}
    sweep.isEnabled = actions.sweep
    screen(FloatWords.sweepTitle, listOf(ui.text(FloatWords.sweepConfirmationsUnknown), ui.text(FloatWords.sweepNotWired)),
        listOf(sweep, ui.button(FloatWords.back) { showFloat() }), back = { showFloat() })
}
