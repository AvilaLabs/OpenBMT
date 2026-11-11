package org.openbmt.owner.app

import android.view.View
import java.io.File
import java.io.IOException
import org.openbmt.app.AnimatedQrMade
import org.openbmt.app.AnimatedQrView
import org.openbmt.app.BbqrScan
import org.openbmt.app.DurableFile
import org.openbmt.app.Format
import org.openbmt.app.PartsProgress
import org.openbmt.app.SayWords
import org.openbmt.app.UrScan
import org.openbmt.app.UrWords
import org.openbmt.app.Who
import org.openbmt.app.RowItem
import org.openbmt.owner.FirstAddress
import org.openbmt.owner.OwnerException
import org.openbmt.owner.OwnerNode
import org.openbmt.owner.Registration
import org.openbmt.owner.SignerBuilt
import org.openbmt.owner.SignerFile
import org.openbmt.owner.SignerTaken

// The screens for key 2 on a hardware signer (OA-70 to OA-74): showing the wallet to the signer, the first
// address, and the payments that go to the signer instead of the Key app. The library does every judgment
// (`docs/SESSION-RULES.md`, AS-90 to AS-99); the choices of the person and the words of `HardwareModel` are
// this file's. The phone signs with key 1 only, and the signer's answer is bytes the library reads.
//
// TODO(OA-60): the camera scan and the file pickers below leave the app for the system's screens. The time
// spent there is to count toward the 60 seconds before the PIN is asked for again; the review fixes that make
// the wait count are not in this tree yet, so the pickers and the scan keep the present rule of
// `Controller.visible` and `hidden`.

/** What one payment's way to the signer holds in memory: the spends' session, the item, and where the owner came from. */
internal class SignerRun(val session: ByteArray, val number: Long, val built: org.openbmt.owner.Built?, val back: () -> Unit) {
    var transport: Transport = Transport.Ur
}

private val MAX_RETURNED = 4 * 1024 * 1024

private fun Controller.words(w: SayWords): View = ui.words(w)

/** The screen thread's view of the first address record, read once from its file. */
internal fun Controller.firstAddressNow(n: OwnerNode): FirstAddress? {
    if (!firstAddressRead) {
        firstAddressRead = true
        firstAddress = try {
            val file = File(appDir, AppFiles.FIRST_ADDRESS_FILE)
            if (file.exists()) n.firstAddressRead(file.readBytes()) else null
        } catch (_: OwnerException) {
            // A record of another wallet or a file that is not exact counts as not done (AS-99).
            null
        } catch (_: IOException) {
            null
        }
    }
    return firstAddress
}

/** OA-72: the line of the wallet screen about the first address. */
internal fun Controller.firstAddressView(n: OwnerNode): View {
    val record = firstAddressNow(n)
    return ui.text(HardwareModel.firstAddressLine(record) { Format.localTime(it) }, 14f, bold = record?.checked != true)
}

/** What goes under a code that could not be made: the words that say so. */
private fun Controller.codeFallback(made: AnimatedQrMade): List<View> =
    if (made.view != null) emptyList() else listOf(words(made.words ?: UrWords.cannotShow(Who.OWNER)))

private fun Controller.codeHeight(): Int = ExchangeModel.codeHeight(act.resources.displayMetrics.heightPixels)

// ---- the transport ---------------------------------------------------------------------------------------------------------

/** OA-70: the three ways, each with the devices that use it as examples. The person picks; no choice is made for them. */
internal fun Controller.showTransportChoice(onPick: (Transport) -> Unit, back: () -> Unit) {
    val rows = HardwareModel.transports.map { choice -> RowItem(choice.label, choice.examples) { onPick(choice.transport) } }
    screen("The signer", listOf(ui.rows(rows)), listOf(ui.button("Back") { back() }), back = back, words = HardwareWords.chooseTransport)
}

// ---- registering the wallet on the signer (OA-71) -----------------------------------------------------------------------

/** The wallet screen's row: the transport, then the wallet in the form the signer takes, then the first address. */
internal fun Controller.showHardwareRegister(back: () -> Unit) {
    showTransportChoice({ transport -> showRegisterWallet(transport, back) }, back)
}

private fun Controller.showRegisterWallet(transport: Transport, back: () -> Unit, shown: View? = null) {
    val n = node ?: return begin()
    val msg = Slot(act)
    // The wallet's name for the signer is made by the library from the shop's name; the app holds none, so it gives none.
    val r = attempt(msg) { n.signerRegistration("") } ?: return screen("Register the wallet", listOf(msg), listOf(ui.button("Back") { back() }), back = back)
    val way = HardwareModel.textWay(transport, r)
    val views = ArrayList<View>()
    val label = if (transport == Transport.File) "Save the wallet file" else "Show the wallet code"
    val actions = ArrayList<android.widget.Button>()
    actions.add(
        ui.primary(label) {
            when (way) {
                RegistrationWay.TextFile -> saveBytes(
                    HardwareModel.textFileName(r), r.textFile.toByteArray(Charsets.UTF_8), HardwareModel.MIME_TEXT,
                    { showRegisterWallet(transport, back) },
                    { showRegisterWallet(transport, back, ui.text(HardwareModel.textFileName(r), 14f, mono = true)) },
                    { w -> msg.show(words(w)) },
                )
                else -> showWalletCode(transport, r, way, back)
            }
        },
    )
    actions.add(ui.button("Show the descriptor code") { showDescriptorCode(transport, r, back) })
    actions.add(ui.button("Check the first address") { showFirstAddress(transport, r, back) })
    actions.add(ui.button("Back") { back() })
    shown?.let { views.add(it) }
    views.add(msg)
    screen("Register the wallet", views, actions, back = back, words = HardwareWords.registerIntro)
}

/** The wallet's text as one plain code, `ur:bytes`, or a BBQr set of type U. */
private fun Controller.showWalletCode(transport: Transport, r: Registration, way: RegistrationWay, back: () -> Unit) {
    val text = r.textFile.toByteArray(Charsets.UTF_8)
    val height = codeHeight()
    val made = when (way) {
        RegistrationWay.PlainCode -> AnimatedQrView.makePlain(act, Who.OWNER, r.textFile, height)
        RegistrationWay.BbqrText ->
            AnimatedQrView.makeBbqr(act, Who.OWNER, HardwareModel.BBQR_TEXT, text, height, HardwareModel.BBQR_PART_CHARS)
        else -> AnimatedQrView.make(act, Who.OWNER, HardwareModel.UR_TEXT, text, height, HardwareModel.UR_FRAGMENT_BYTES)
    }
    codeScreen(
        "Show the wallet code", HardwareWords.registerIntro, made.view, codeFallback(made),
        listOf(ui.button("Back") { showRegisterWallet(transport, back) }), back = { showRegisterWallet(transport, back) },
    )
}

/** The descriptor of the receive addresses as `ur:crypto-output`. */
private fun Controller.showDescriptorCode(transport: Transport, r: Registration, back: () -> Unit) {
    val made = AnimatedQrView.make(act, Who.OWNER, HardwareModel.UR_OUTPUT, r.outputCbor, codeHeight(), HardwareModel.UR_FRAGMENT_BYTES)
    codeScreen(
        "Show the descriptor code", HardwareWords.registerIntro, made.view, codeFallback(made),
        listOf(ui.button("Back") { showRegisterWallet(transport, back) }), back = { showRegisterWallet(transport, back) },
    )
}

// ---- the first address (OA-72) -------------------------------------------------------------------------------------------------

private fun Controller.showFirstAddress(transport: Transport, r: Registration, back: () -> Unit) {
    val n = node ?: return begin()
    val msg = Slot(act)
    val views = ArrayList<View>()
    views.add(ui.text(HardwareModel.grouped(r.firstAddress), 22f, bold = true, mono = true))
    views.add(msg)
    val actions = listOf(
        ui.primary("They match") { recordFirstAddress(n, true, back, msg) },
        ui.button("Check later") { recordFirstAddress(n, false, back, msg) },
        ui.button("Back") { showRegisterWallet(transport, back) },
        ui.danger("They differ") {
            centre(
                HardwareWords.firstAddressDiffers, listOf(ui.button("Back") { showRegisterWallet(transport, back) }),
                title = "First address", back = { showRegisterWallet(transport, back) },
            )
        },
    )
    screen("First address", views, actions, back = { showRegisterWallet(transport, back) }, words = HardwareWords.firstAddressCompare)
}

/** Stores the choice as the library's record (op 125), then says what it means. */
private fun Controller.recordFirstAddress(n: OwnerNode, checked: Boolean, back: () -> Unit, msg: Slot) {
    val now = nowSecs()
    val record = attempt(msg) { n.firstAddressRecord(checked, now) } ?: return
    try {
        DurableFile.write(appDir, File(appDir, AppFiles.FIRST_ADDRESS_FILE), record)
    } catch (e: IOException) {
        return msg.show(words(AppWords.fileNotSaved(e.message)))
    }
    firstAddress = FirstAddress(checked, now)
    firstAddressRead = true
    val said = if (checked) HardwareWords.firstAddressChecked else HardwareWords.firstAddressSkipped
    centre(said, listOf(ui.primary("Done") { back() }), title = "First address")
}

// ---- a payment on its way to the signer (OA-70, OA-73) -----------------------------------------------------------------------

/** Which kind of session [id] is, asked of the library: its file for item 0 is given, or its refusal says. */
internal fun Controller.signerSessionOf(n: OwnerNode, id: ByteArray): HardwareModel.SignerSession = try {
    n.signerFile(id, 0)
    HardwareModel.signerSession(true, null)
} catch (e: OwnerException) {
    HardwareModel.signerSession(false, e.sayWords().code)
} catch (_: LinkageError) {
    HardwareModel.signerSession(false, null)
}

/**
 * "Continue on the signer" for a session of spends that is open: one that was exported goes to the way back; one
 * that was not goes to the transport choice and then the payment.
 */
internal fun Controller.continueOnSigner(id: ByteArray, state: HardwareModel.SignerSession, back: () -> Unit) {
    val run = SignerRun(id, 0, null, back)
    if (state == HardwareModel.SignerSession.Exported) {
        showSignerReturn(run, null)
    } else {
        showTransportChoice({ transport -> run.transport = transport; showSignerExport(run) }, back)
    }
}

/**
 * After the owner approved a spend on this phone: when the library made a session of spends for the signer, the
 * way to reach it, then the payment (`showSignerExport`) and its way back. The request for the Key app, which
 * holds what is not a spend, is shown last as it is for a wallet without a signer.
 */
internal fun Controller.showSignerSpend(built: SignerBuilt, back: () -> Unit) {
    val session = built.signerSession ?: return showBuilt(built.built, back)
    val run = SignerRun(session, 0, built.built, back)
    showTransportChoice({ transport -> run.transport = transport; showSignerExport(run) }, { showBuilt(built.built, back) })
}

/**
 * The payment's file for the signer, made and signed with key 1 by the library, with what the signer's screen
 * must show; it is given as moving codes or saved as a file. Asking again gives the same file (AS-91).
 */
private fun Controller.showSignerExport(run: SignerRun, message: List<View> = emptyList()) {
    val n = node ?: return begin()
    val msg = Slot(act)
    val file = attempt(msg) { n.signerExport(nowSecs(), run.session, run.number) }
        ?: return screen(
            "The signer", listOf(msg), listOf(ui.button("Back") { showTransportChoice({ t -> run.transport = t; showSignerExport(run) }, run.back) }),
            back = { showTransportChoice({ t -> run.transport = t; showSignerExport(run) }, run.back) },
        )
    val views = ArrayList<View>()
    views.add(ui.text(compareText(file), 14f, mono = true))
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
                    { showSignerExport(run) },
                    { showSignerExport(run, listOf(words(HardwareWords.fileSaved), ui.text(file.fileName, 14f, mono = true))) },
                    { w -> msg.show(words(w)) },
                )
            },
        )
    }
    views.addAll(message)
    views.add(msg)
    actions.add(ui.button("Next: bring it back") { showSignerReturn(run, null) })
    actions.add(ui.button("Back") { showTransportChoice({ t -> run.transport = t; showSignerExport(run) }, run.back) })
    screen("The signer", views, actions, back = { showTransportChoice({ t -> run.transport = t; showSignerExport(run) }, run.back) }, words = HardwareWords.signerReview)
}

/** OA-73: each payment, the change and the fee, one to a line, as the signer's screen shows them. */
private fun compareText(file: SignerFile): String = HardwareModel.compareLines(
    file.payments.map { it.address to it.sats },
    file.change?.let { it.address to it.sats },
    file.fee,
).joinToString("\n\n")

// ---- bringing the signed payment back (OA-70, OA-74) ------------------------------------------------------------------------

/** The way back: the signer's code by the camera, or a file. [message] is what the last try said, if it said anything. */
internal fun Controller.showSignerReturn(run: SignerRun, message: SayWords?) {
    val n = node ?: return begin()
    val msg = Slot(act)
    message?.let { msg.show(words(it)) }
    val views = ArrayList<View>()
    views.add(
        ui.button("Open a file") {
            // The library reads a binary or base64 PSBT, a hex or binary transaction and refuses what is over 4 MiB.
            pickBytes(MAX_RETURNED + 1, { showSignerReturn(run, null) }, { bytes -> takeReturned(n, run, bytes) }) { w ->
                msg.show(words(w))
            }
        },
    )
    views.add(msg)
    screen(
        "The signer", views,
        listOf(ui.primary("Scan the signer's code") { scanReturned(n, run) }, ui.button("Back") { showSignerExport(run) }),
        back = { showSignerExport(run) }, words = HardwareWords.returnedNeeded,
    )
}

/**
 * The signer's code, read by the camera: a UR of `crypto-psbt` or `psbt`, a BBQr of type P or T, parts in any
 * order, with the progress in words. A plain code goes on as bytes, as a file does. Every end of the scan that is
 * not bytes is words on the screen it began from.
 */
private fun Controller.scanReturned(n: OwnerNode, run: SignerRun) {
    val parts = PartsProgress(HardwareWords.partsMissing) { read, of -> HardwareModel.partsLine(read, of) }
    scan.begin(
        show = { v -> showScan(v) },
        onBytes = { bytes -> takeReturned(n, run, bytes) },
        onFailed = { f -> showSignerReturn(run, HardwareModel.scanFailure(f.off, f.problem, f.words)) },
        onCancel = { showSignerReturn(run, null) },
        ur = UrScan(HardwareModel.RETURN_UR_TYPES, Who.OWNER, parts) { _, payload -> takeReturned(n, run, payload) },
        bbqr = BbqrScan(HardwareModel.RETURN_BBQR_KINDS, Who.OWNER, parts) { _, payload -> takeReturned(n, run, payload) },
    )
}

/** Hands what came back to the library (op 124); a refusal shows its words and nothing else changes. */
private fun Controller.takeReturned(n: OwnerNode, run: SignerRun, bytes: ByteArray) {
    val taken = try {
        n.signerTake(nowSecs(), run.session, run.number, bytes)
    } catch (e: OwnerException) {
        return showSignerReturn(run, refusal(e))
    } catch (e: LinkageError) {
        return showSignerReturn(run, refusal(e))
    }
    showSignerTaken(n, run, taken)
}

/** What taking the payment did, in the library's words: sent, or queued for sending. */
private fun Controller.showSignerTaken(n: OwnerNode, run: SignerRun, taken: SignerTaken) {
    val views = ArrayList<View>()
    for (s in taken.sends) {
        views.add(stack(ui.words(s.words.say()), line("Transaction ${s.txid}", muted = true, mono = true)))
    }
    // A session with another payment still in it goes on with that one; otherwise the Key app's request follows.
    val more = try {
        n.sessions().any { it.contentEquals(run.session) }
    } catch (_: OwnerException) {
        false
    }
    val done = ui.primary("Done") {
        if (more) showSignerExport(SignerRun(run.session, run.number + 1, run.built, run.back).also { it.transport = run.transport })
        else run.built?.let { showBuilt(it, run.back) } ?: run.back()
    }
    screen("The signer", views, listOf(done))
}
