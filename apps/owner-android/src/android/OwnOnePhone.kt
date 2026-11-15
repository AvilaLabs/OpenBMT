package org.openbmt.owner.app

import android.content.ComponentName
import android.content.Intent
import android.view.View
import org.openbmt.app.OnePhone
import org.openbmt.app.OnePhoneCheck
import org.openbmt.app.SayWords
import org.openbmt.owner.OwnerException

// This phone as a counter too (OA-124 to OA-126): the same pairing as the camera's, with the offer, the request and the
// bundle passed to the counter app's pairing activity on this phone in place of the camera. Every judgment is the
// library's and the model's; this file starts the activity, shows the owner's choices and carries what comes back.
// Every sentence is in OwnPairWords or the library's own words.

/** "Use this phone as the counter too": the counter app is looked for, and only the same publisher's is used (OA-124). */
internal fun Controller.oneStart(r: OwnPairExit, p: OwnPairing) {
    when (OnePhoneCheck.peer(act, OnePhone.COUNTER_PACKAGE)) {
        OnePhone.Peer.Missing -> oneNoApp(r, p)
        OnePhone.Peer.OtherSigner -> oneOtherSigner(r, p)
        OnePhone.Peer.Same -> oneOffer(r, p)
    }
}

private fun Controller.oneNoApp(r: OwnPairExit, p: OwnPairing) {
    val again = ui.primary(OwnPairWords.oneTryAgain) { oneStart(r, p) }
    val back = { showOwnCounter(r) }
    stepScreen(OwnPairWords.title, listOf(heading(OwnPairWords.oneNoAppTitle), para(OwnPairWords.oneNoAppText)), listOf(again, ui.button(KeyGuideWords.kgBack) { back() }), back)
}

private fun Controller.oneOtherSigner(r: OwnPairExit, p: OwnPairing) {
    val back = { showOwnCounter(r) }
    stepScreen(OwnPairWords.title, listOf(heading(OwnPairWords.oneOtherSignerTitle), para(OwnPairWords.oneOtherSignerText)), listOf(ui.primary(KeyGuideWords.kgBack) { back() }), back)
}

/** The words of an attempt that stopped before the counter app was paired: a new try starts from a new offer (OA-125). */
private fun Controller.oneEnded(r: OwnPairExit) {
    val again = ui.primary(OwnPairWords.oneTryAgain) {
        ownPairing = null
        showOwnCounter(r)
    }
    val back = {
        ownPairing = null
        r.leave()
    }
    stepScreen(OwnPairWords.title, listOf(para(OwnPairWords.oneEndedText)), listOf(again, ui.button(KeyGuideWords.kgBack) { back() }), back, tone = ui.pal.stop)
}

/** The answer came with no pairing in memory (the app was started again meanwhile): the words, and a new try (OA-125). */
internal fun Controller.oneLost() {
    ownPairing = null
    oneEnded(OwnPairExit(leave = { showDevices() }, done = { showDevices() }))
}

private fun oneIntent(action: String, extra: String, bytes: ByteArray): Intent =
    Intent(action).setComponent(ComponentName(OnePhone.COUNTER_PACKAGE, OnePhone.PAIR_ACTIVITY)).putExtra(extra, bytes)

/** The offer is made as the offer screen makes it, not drawn, and goes to the counter app's activity (OA-124). */
private fun Controller.oneOffer(r: OwnPairExit, p: OwnPairing) {
    val offer = p.offer
    if (offer == null || p.ended(nowSecs())) return showOwnCounter(r)
    startOnePhone(
        Controller.REQUEST_ONE_OFFER, oneIntent(OnePhone.ACTION_OFFER, OnePhone.EXTRA_OFFER, offer.made.code),
        failed = { oneNoApp(r, p) },
    ) { result, data ->
        val request = if (data?.hasExtra(OnePhone.EXTRA_REQUEST) == true) data.getByteArrayExtra(OnePhone.EXTRA_REQUEST) else null
        when (val answer = OnePhoneModel.offerAnswer(result, request)) {
            is OnePhoneModel.Answer.Ended -> oneEnded(r)
            is OnePhoneModel.Answer.Request -> when (val q = answer.result) {
                // Judged as a request read by the camera is (OA-109); a refusal shows its words on the offer screen.
                is SourceResult.Bytes -> {
                    val said = p.readRequest(q.bytes, nowSecs())
                    if (said == null) oneConfirm(r, p) else showOwnCounter(r, said)
                }
                is SourceResult.Failed -> showOwnCounter(r, q.words)
            }
        }
    }
}

/** The four words are not shown (OA-125): the owner is asked whether to add this phone, and the yes is "They match". */
private fun Controller.oneConfirm(r: OwnPairExit, p: OwnPairing) {
    val read = p.read ?: return showOwnCounter(r)
    val msg = Slot(act)
    val views = ArrayList<View>()
    views.add(heading(OwnPairWords.oneConfirmTitle))
    views.add(para(OwnPairWords.oneConfirmText))
    val price = if (read.needsRate) {
        val note = Slot(act)
        views.add(note)
        numberInput("Dollars for one bitcoin").also { views.add(it.view) }.field.also {
            fillPrice(note, it, AppWords.counterPriceFetching, AppWords::counterPriceFetched, AppWords.counterPriceNeeded)
        }
    } else {
        null
    }
    views.add(msg)
    val add = ui.primary(OwnPairWords.oneConfirmAdd) {
        when (val f = if (price == null) Form.Ok(0L) else CounterModel.price(price.text.toString())) {
            is Form.Refused -> msg.show(ui.words(f.words))
            is Form.Ok -> {
                val said = p.match(nowSecs(), f.value)
                if (said == null) oneBundle(r, p) else showOwnCounter(r, said)
            }
        }
    }
    // The request is forgotten; the counter app's side of the ceremony is gone with its next offer or its stop.
    val cancel = {
        p.back()
        showOwnCounter(r)
    }
    stepScreen(OwnPairWords.title, views, listOf(add, ui.button(OwnPairWords.oneCancel) { cancel() }), cancel)
}

/** The bundle goes back to the counter app by the same activity; its answer says whether it is paired (OA-125). */
private fun Controller.oneBundle(r: OwnPairExit, p: OwnPairing) {
    val bundle = p.bundle ?: return showOwnCounter(r)
    startOnePhone(
        Controller.REQUEST_ONE_BUNDLE, oneIntent(OnePhone.ACTION_BUNDLE, OnePhone.EXTRA_BUNDLE, bundle.code),
        failed = { oneEnded(r) },
    ) { result, data ->
        val paired = if (data?.hasExtra(OnePhone.EXTRA_PAIRED) == true) data.getBooleanExtra(OnePhone.EXTRA_PAIRED, false) else null
        if (OnePhoneModel.paired(result, paired)) onePaired(r, p, bundle.counter) else oneEnded(r)
    }
}

/** The counter is marked paired as "The counter shows Paired" does, and the owner keeps that it is on this phone (OA-125). */
private fun Controller.onePaired(r: OwnPairExit, p: OwnPairing, counter: ByteArray) {
    val said = p.paired()
    if (said != null) return showOwnCounter(r, said)
    try {
        OnePhoneFile.write(act.noBackupFilesDir, counter)
    } catch (_: java.io.IOException) {
        // Without the note only the shortcut "Open the counter" is missing; the counter is paired.
    }
    // In the first run the node is not up yet, and turning on the watch needs it: the offer is made on the last step
    // of the first run instead (showReady).
    if (watchStore.enabled() || node == null) return showOwnCounter(r)
    val next = { showOwnCounter(r) }
    val on = ui.primary(OwnPairWords.oneWatchOn) { watchTurnOnThen(next) }
    val later = ui.button(OwnPairWords.oneWatchLater) { next() }
    stepScreen(OwnPairWords.title, listOf(heading(OwnPairWords.oneWatchTitle), para(OwnPairWords.oneWatchText)), listOf(on, later), { next() })
}

/** OA-126: whether the note names a counter the settings in force still list. */
internal fun Controller.oneCounterInForce(n: org.openbmt.owner.OwnerNode): Boolean {
    val noted = OnePhoneFile.read(act.noBackupFilesDir) ?: return false
    return try {
        OnePhoneModel.inForce(noted, n.settings()?.counters?.map { it.counter }.orEmpty())
    } catch (_: OwnerException) {
        false
    } catch (_: LinkageError) {
        false
    }
}

/** "Open the counter": the counter app's launcher activity by its explicit name (OA-126). */
internal fun Controller.openCounter() {
    val intent = Intent(Intent.ACTION_MAIN).addCategory(Intent.CATEGORY_LAUNCHER)
        .setComponent(ComponentName(OnePhone.COUNTER_PACKAGE, OnePhone.COUNTER_MAIN)).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
    try {
        act.startActivity(intent)
    } catch (_: RuntimeException) {
        // The counter app is gone: the row goes at the next draw of home, when the settings no longer list it or the app is missing.
    }
}
