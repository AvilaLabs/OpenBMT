package org.openbmt.owner.app

import android.text.InputType
import android.view.View
import java.io.IOException
import org.openbmt.app.Format
import org.openbmt.app.RowItem

internal fun Controller.floatRow() = RowItem(FloatWords.rowTitle, FloatWords.unavailable) { showFloat() }

/** Only public cap preferences are saved until the child-code reader is bound. */
internal fun Controller.showFloat(notice: String? = null) {
    val record = try {
        FloatStore(appDir).load()
    } catch (_: IOException) {
        return floatReadFailed()
    }
    val home = FloatModel.home(LibraryFloat, record)
    val body = ArrayList<View>()
    notice?.let { body.add(ui.text(it)) }
    body.add(ui.text(home.message))
    record?.let { body.add(ui.labelled(FloatWords.capLabel, ui.text(Format.sats(it.capSats)))) }
    home.heldSats?.let { body.add(ui.labelled(FloatWords.heldLabel, ui.text(Format.sats(it)))) }
    body.add(ui.rows(listOf(
        RowItem(FloatWords.setupTitle) { showFloatSetup() },
        RowItem(FloatWords.capLabel) { showFloatCap() },
        RowItem(FloatWords.refillAction) { showFloatRefill() },
        RowItem(FloatWords.refundPay) { showFloatRefund() },
        RowItem(FloatWords.sweepTitle) { showFloatSweep() },
    )))
    screen(FloatWords.rowTitle, body, listOf(ui.button(FloatWords.back) { showHome() }), back = { showHome() })
}

private fun Controller.floatReadFailed() {
    screen(FloatWords.rowTitle, listOf(ui.text(FloatWords.storageUnavailable)),
        listOf(ui.button(FloatWords.back) { showHome() }), back = { showHome() })
}

/** FL-01/04: no typed words or substitute QR format is accepted. */
private fun Controller.showFloatSetup() {
    screen(FloatWords.setupTitle, listOf(
        ui.text(FloatWords.childOnly),
        ui.text(FloatWords.childIndex),
        ui.text(FloatWords.nextIndex),
        ui.text(FloatWords.scanUnavailable),
    ), listOf(
        ui.primary(FloatWords.scanChild) {}.also { it.isEnabled = false },
        ui.button(FloatWords.back) { showFloat() },
    ), back = { showFloat() })
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

/** FL-11/30: never pass a guessed zero balance to floatRefill. */
private fun Controller.showFloatRefill() {
    screen(FloatWords.refillAction, listOf(
        ui.text(FloatWords.refillLinksSavings),
        ui.text(FloatWords.unavailable),
    ), listOf(
        ui.primary(FloatWords.refillAction) {}.also { it.isEnabled = false },
        ui.button(FloatWords.back) { showFloat() },
    ), back = { showFloat() })
}

/** FL-20/23: refunds remain in their existing queue while float submission is unavailable. */
private fun Controller.showFloatRefund() {
    screen(FloatWords.refundPay, listOf(ui.text(FloatWords.refundUnavailable)), listOf(
        ui.primary(FloatWords.payConfirm) {}.also { it.isEnabled = false },
        ui.button(FloatWords.back) { showFloat() },
    ), back = { showFloat() })
}

/** FL-35: no key deletion or shutdown can occur without a confirmed sweep. */
private fun Controller.showFloatSweep() {
    screen(FloatWords.sweepTitle, listOf(ui.text(FloatWords.sweepConfirmationsUnknown)), listOf(
        ui.primary(FloatWords.sweepConfirm) {}.also { it.isEnabled = false },
        ui.button(FloatWords.back) { showFloat() },
    ), back = { showFloat() })
}
