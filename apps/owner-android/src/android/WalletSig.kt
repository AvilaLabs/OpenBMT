package org.openbmt.owner.app

import android.content.ClipboardManager
import android.view.View
import android.widget.Button
import org.openbmt.app.AnimatedQrView
import org.openbmt.app.UrWords
import org.openbmt.app.Who
import org.openbmt.owner.OwnSettingsPreview
import org.openbmt.owner.OwnerException
import org.openbmt.owner.OwnerOwn

// Signing a Redirect change with the wallet (OA-127, AP-14). The screen follows the review of a change of a shop with
// its own wallet that decides where Lightning payments go: the message the wallet signs, as text and as a code, the
// address that must sign, and a field for the signature. The phone checks the signature with the library before
// anything is queued; the app holds no key and checks nothing itself.

/** The message and the address as the screen shows them, then the signature field, "Scan signature" and "Send change". */
internal fun Controller.showWalletSig(ask: OwnSettingsPreview, typed: String = "", message: org.openbmt.app.SayWords? = null, back: () -> Unit) {
    val msg = Slot(act)
    message?.let { msg.show(ui.words(it)) }
    val sig = ui.field(WalletSigWords.fieldLabel, lines = 4, mono = true).also { it.setText(typed) }
    val paste = ui.button(WalletSigWords.paste) {
        val clip = act.getSystemService(ClipboardManager::class.java)?.primaryClip?.getItemAt(0)?.text?.toString()
        if (clip != null) sig.setText(clip.trim())
    }
    val views = ArrayList<View>()
    views.add(ui.text(WalletSigWords.what))
    views.add(ui.text(WalletSigWords.next))
    views.add(label(WalletSigWords.addressLabel))
    views.add(ui.text(ask.address, 15f, mono = true))
    views.add(ui.button(WalletSigWords.copy) { copy(ask.address) })
    views.add(label(WalletSigWords.messageLabel))
    views.add(ui.text(ask.message, 15f, mono = true))
    views.add(ui.button(WalletSigWords.copy) { copy(ask.message) })
    val made = AnimatedQrView.makePlain(act, Who.OWNER, ask.message, ExchangeModel.codeHeight(act.resources.displayMetrics.heightPixels))
    views.add(made.view?.let { ui.codeCard(it) } ?: ui.words(made.words ?: UrWords.cannotShow(Who.OWNER)))
    views.add(ui.labelled(WalletSigWords.fieldLabel, sig))
    views.add(paste)
    views.add(msg)
    val send = ui.primary(WalletSigWords.send) {
        val shop = ownStore.args() ?: return@primary
        try {
            OwnerOwn.settingsAttach(shop, ask.preview.settings, sig.text.toString())
            settingsDraft = SettingsDraft()
            showHome()
        } catch (e: OwnerException) {
            // The screen stays, with the signature as typed, and nothing was queued.
            showWalletSig(ask, sig.text.toString(), if (e.rule == "AP-10") WalletSigWords.wrong(ask.address) else e.sayWords(), back)
        } catch (e: LinkageError) {
            msg.show(ui.words(AppWords.libraryMissing(e.message)))
        }
    }
    val scanButton = ui.button(WalletSigWords.scan) { scanSignature(ask, back) }
    screen(WalletSigWords.heading, views, listOf<Button>(send, scanButton, ui.button("Back") { back() }), back = { back() })
}

/** The wallet's code, read by the camera, which is asked for only when the button is pressed. */
private fun Controller.scanSignature(ask: OwnSettingsPreview, back: () -> Unit) {
    fun give(r: SourceResult) = when (r) {
        is SourceResult.Bytes -> showWalletSig(ask, String(r.bytes, Charsets.UTF_8).trim(), null, back)
        is SourceResult.Failed -> showWalletSig(ask, "", r.words, back)
    }
    scan.begin(
        show = { v -> showScan(v) },
        onBytes = { give(SourceResult.Bytes(it)) },
        onFailed = { f -> give(ScanResult.failed(f.off, f.problem, f.words)) },
        onCancel = { give(SourceResult.Failed(AppWords.counterScanCancelled)) },
    )
}
