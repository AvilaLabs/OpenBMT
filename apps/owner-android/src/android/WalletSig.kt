package org.openbmt.owner.app

import android.content.ClipboardManager
import android.view.View
import android.widget.Button
import org.openbmt.app.AnimatedQrView
import org.openbmt.app.UrWords
import org.openbmt.app.Who
import org.openbmt.owner.OwnerException
import org.openbmt.owner.OwnerOwn

// Signing a Redirect change with the wallet (OA-127, AP-14). The screen follows the review of a change of a shop with
// its own wallet that decides where Lightning payments go: the message the wallet signs, as text and as a code, the
// address that must sign, and a field for the signature. The phone checks the signature with the library before
// anything is queued; the app holds no key and checks nothing itself.

/** The message and the address as the screen shows them, then the signature field, "Scan signature" and "Send change". */
internal fun Controller.showWalletSig(
    settings: ByteArray,
    message: String,
    address: String,
    path: String?,
    typed: String = "",
    words: org.openbmt.app.SayWords? = null,
    done: () -> Unit = { showHome() },
    back: () -> Unit,
) {
    val msg = Slot(act)
    words?.let { msg.show(ui.words(it)) }
    val sig = ui.field(WalletSigWords.fieldLabel, lines = 4, mono = true).also { it.setText(typed) }
    val paste = ui.button(WalletSigWords.paste) {
        val clip = act.getSystemService(ClipboardManager::class.java)?.primaryClip?.getItemAt(0)?.text?.toString()
        if (clip != null) sig.setText(clip.trim())
    }
    val views = ArrayList<View>()
    views.add(ui.text(WalletSigWords.what))
    views.add(ui.text(WalletSigWords.next))
    views.add(label(WalletSigWords.addressLabel))
    views.add(ui.text(address, 15f, mono = true))
    views.add(ui.button(WalletSigWords.copy) { copy(address) })
    views.add(label(WalletSigWords.messageLabel))
    views.add(ui.text(message, 15f, mono = true))
    views.add(ui.button(WalletSigWords.copy) { copy(message) })
    val made = AnimatedQrView.makePlain(act, Who.OWNER, WalletSigWords.qrText(message, path), ExchangeModel.codeHeight(act.resources.displayMetrics.heightPixels))
    views.add(made.view?.let { ui.codeCard(it) } ?: ui.words(made.words ?: UrWords.cannotShow(Who.OWNER)))
    views.add(ui.labelled(WalletSigWords.fieldLabel, sig))
    views.add(paste)
    views.add(msg)
    val send = ui.primary(WalletSigWords.send) {
        val shop = ownStore.args() ?: return@primary
        try {
            OwnerOwn.settingsAttach(shop, settings, sig.text.toString())
            settingsDraft = SettingsDraft()
            done()
        } catch (e: OwnerException) {
            // The screen stays, with the signature as typed, and nothing was queued.
            showWalletSig(settings, message, address, path, sig.text.toString(), if (e.rule == "AP-10") WalletSigWords.wrong(address) else e.sayWords(), done, back)
        } catch (e: LinkageError) {
            msg.show(ui.words(AppWords.libraryMissing(e.message)))
        }
    }
    val scanButton = ui.button(WalletSigWords.scan) { scanSignature(settings, message, address, path, done, back) }
    screen(WalletSigWords.heading, views, listOf<Button>(send, scanButton, ui.button("Back") { back() }), back = { back() })
}

/** The wallet's code, read by the camera, which is asked for only when the button is pressed. */
private fun Controller.scanSignature(settings: ByteArray, message: String, address: String, path: String?, done: () -> Unit, back: () -> Unit) {
    fun give(r: SourceResult) = when (r) {
        is SourceResult.Bytes -> showWalletSig(settings, message, address, path, String(r.bytes, Charsets.UTF_8).trim(), null, done, back)
        is SourceResult.Failed -> showWalletSig(settings, message, address, path, "", r.words, done, back)
    }
    scan.begin(
        show = { v -> showScan(v) },
        onBytes = { give(SourceResult.Bytes(it)) },
        onFailed = { f -> give(ScanResult.failed(f.off, f.problem, f.words)) },
        onCancel = { give(SourceResult.Failed(AppWords.counterScanCancelled)) },
    )
}

/**
 * A change of a shop with its own wallet that asks for no wallet signature: the library signs it with the owner root
 * and puts it in force, or holds it for a counter's confirmation, which is shown on the screen of the waiting change
 * (OA-119, OA-121). A refusal is shown in [msg] and nothing is sent.
 */
internal fun Controller.sendOwnChange(settings: ByteArray, msg: Slot, done: () -> Unit) {
    val shop = ownStore.args() ?: return
    val sent = attempt(msg) { OwnerOwn.settingsSend(shop, settings, System.currentTimeMillis() / 1000) } ?: return
    settingsDraft = SettingsDraft()
    if (sent.waits) showOwnWaitingChange() else { savedNote = WalletSigWords.saved; done() }
}

/**
 * The same send for a step that runs off the screen's thread (the first-run price, the watch key): the library puts the
 * change in force or holds it, the answer is kept in [Controller.ownSentLast], and [takeOwnSent] shows it on the screen.
 */
internal fun Controller.sendOwnQuietly(settings: ByteArray) {
    val shop = ownStore.args() ?: throw OwnerException("OA-127", "no wallet on this phone", 4)
    ownSentLast = OwnerOwn.settingsSend(shop, settings, System.currentTimeMillis() / 1000)
}

/** True when the last quiet send waits for a counter, and its screen is shown; otherwise the saved line is set for the next page. */
internal fun Controller.takeOwnSent(): Boolean {
    val s = ownSentLast ?: return false
    ownSentLast = null
    settingsDraft = SettingsDraft()
    if (s.waits) { showOwnWaitingChange(); return true }
    savedNote = WalletSigWords.saved
    return false
}
