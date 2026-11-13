package org.openbmt.owner.app

import android.view.View
import org.openbmt.app.SayWords
import org.openbmt.app.Tones
import org.openbmt.app.Who

// Connecting the Box of a shop with its own wallet (BX-17, BX-18): the invitation is shown, the owner types the Box's
// address, the node fetches the Box's claim and the library checks it, the four words are compared, the statement of
// what the shop holds while the phone is away is shown, and the welcome is sent. The link to the Box is kept sealed in
// the own store and the node starts with it; the counters paired before the Box scan the relay code, and the ones
// paired after it get the relay in their bundle. Every judgment is the library's; this file shows its answers and
// carries the owner's choices. Every sentence is in OwnPairWords, BoxWords or the library's own words.

/** "Connect your Box" for a shop with its own wallet: the same page the Devices row opens, ending at Devices. */
internal fun Controller.showOwnConnectBox() {
    ownBoxPairing = null
    showOwnBox()
}

private fun Controller.leaveOwnBox() {
    ownBoxPairing = null
    showDevices()
}

/** The node reads the link when it starts: it starts again when the kept link is not the one it started with (BX-18). */
internal fun Controller.restartIfLinkChanged(): Boolean {
    val kept = try {
        ownStore.loadLink()
    } catch (_: java.io.IOException) {
        null
    }
    if (!OwnBoxLink.restartNeeded(ownLinkId, kept)) return false
    begin()
    return true
}

private fun Controller.finishOwnBox() {
    ownBoxPairing = null
    if (!restartIfLinkChanged()) showDevices()
}

private fun Controller.showOwnBox(message: SayWords? = null) {
    if (node == null) {
        begin()
        return
    }
    val p = ownBoxPairing ?: OwnBoxPairing(ownStore).also { fresh ->
        ownBoxPairing = fresh
        val said = fresh.start(nowSecs())
        if (said != null) return ownBoxStopped(said)
    }
    val back: () -> Unit = { if (p.back()) showOwnBox() else leaveOwnBox() }
    if (p.step != OwnBoxStep.Sent && p.step != OwnBoxStep.Stopped && p.ended(nowSecs())) {
        ownBoxPairing = null
        return showOwnBox()
    }
    when (p.step) {
        OwnBoxStep.Invitation -> showOwnBoxInvitation(p, message, back)
        OwnBoxStep.Address -> showOwnBoxAddress(p, message, back)
        OwnBoxStep.Words -> showOwnBoxWords(p, message, back)
        OwnBoxStep.Rest -> showOwnBoxRest(p, message, back)
        OwnBoxStep.Sent -> showOwnBoxSent(p)
        OwnBoxStep.Stopped -> ownBoxStopped(p.stopped ?: OwnPairWords.codeNotShown)
    }
}

/** The words of a connection that did not start or was stopped, and the way to start again. */
private fun Controller.ownBoxStopped(words: SayWords) {
    val again = ui.primary(OwnPairWords.boxStartAgain) {
        ownBoxPairing = null
        showOwnBox()
    }
    stepScreen(OwnPairWords.boxTitle, listOf(ui.words(words)), listOf(again), { leaveOwnBox() }, tone = ui.pal.stop)
}

private fun Controller.showOwnBoxInvitation(p: OwnBoxPairing, message: SayWords?, back: () -> Unit) {
    val views = ArrayList<View>()
    views.add(heading(OwnPairWords.boxInviteHeading))
    views.add(ui.text(p.invitation?.text ?: "", 22f, bold = true, mono = true))
    views.add(para(OwnPairWords.boxInviteText))
    message?.let { views.add(ui.words(it)) }
    val next = ui.primary(OwnPairWords.boxInviteNext) {
        p.toAddress()
        showOwnBox()
    }
    stepScreen(OwnPairWords.boxTitle, views, listOf(next), back)
}

/** The address, typed or scanned as plain text; the node asks the Box for its claim, which waits on the network (BX-14). */
private fun Controller.showOwnBoxAddress(p: OwnBoxPairing, message: SayWords?, back: () -> Unit, prefill: String = "") {
    val n = node ?: return begin()
    val msg = Slot(act)
    val views = ArrayList<View>()
    message?.let { views.add(ui.words(it)) }
    val address = ui.field(OwnPairWords.boxAddressHint).also { it.setText(prefill) }
    views.add(ui.labelled(OwnPairWords.boxAddressLabel, address))
    views.add(ui.button(OwnPairWords.boxAddressScan) { scanOwnBox(p, back) })
    views.add(msg)
    val look = ui.primary(OwnPairWords.boxAddressLook) {
        when (val a = ConnectBoxModel.address(address.text.toString())) {
            is Form.Refused -> msg.show(ui.words(a.words))
            is Form.Ok -> {
                val link = ConnectBoxModel.linkFor(a.value, proxyNow())
                waiting(BoxWords.looking)
                async({ n.boxFetchClaim(link) }) { r ->
                    val found = r.getOrNull()
                    if (found == null) {
                        showOwnBoxAddress(p, refusal(r.exceptionOrNull()!!), back, a.value)
                    } else {
                        showOwnBox(p.found(a.value, found))
                    }
                }
            }
        }
    }
    stepScreen(OwnPairWords.boxTitle, views, listOf(look), back)
}

private fun Controller.scanOwnBox(p: OwnBoxPairing, back: () -> Unit) {
    fun give(r: SourceResult) = when (r) {
        is SourceResult.Bytes -> showOwnBoxAddress(p, null, back, String(r.bytes, Charsets.UTF_8).take(300))
        is SourceResult.Failed -> showOwnBoxAddress(p, r.words, back)
    }
    scan.begin(
        show = { v -> showScan(v) },
        onBytes = { give(SourceResult.Bytes(it)) },
        onFailed = { f -> give(ScanResult.failed(f.off, f.problem, f.words)) },
        onCancel = { give(SourceResult.Failed(AppWords.counterScanCancelled)) },
    )
}

private fun Controller.showOwnBoxWords(p: OwnBoxPairing, message: SayWords?, back: () -> Unit) {
    val checked = p.checked ?: return showOwnBox()
    val views = ArrayList<View>()
    views.add(heading(OwnPairWords.boxWordsHeading))
    views.add(para(OwnPairWords.boxWordsText))
    views.add(wordCards(ConnectBoxModel.wordsLine(checked.words)))
    views.add(stack(label(ConnectBoxModel.boxLabel(checked.boxKey)), line(ConnectBoxModel.keyGroups(checked.boxKey), muted = true, mono = true)))
    message?.let { views.add(ui.words(it)) }
    val match = ui.primary(OwnPairWords.boxWordsMatch) {
        p.match()
        showOwnBox()
    }
    val differ = ui.danger(OwnPairWords.boxWordsDiffer) {
        p.differ(OwnPairWords.boxDifferStopped)
        showOwnBox()
    }
    stepScreen(OwnPairWords.boxTitle, views, listOf(match, differ), back)
}

/** The statement of what the shop holds while the phone is away (BX-17), the price when the phone has no settings, and the send. */
private fun Controller.showOwnBoxRest(p: OwnBoxPairing, message: SayWords?, back: () -> Unit) {
    val msg = Slot(act)
    val views = ArrayList<View>()
    views.add(heading(OwnPairWords.boxRestHeading))
    views.add(para(OwnPairWords.boxRestText))
    val price = if (p.needsRate) {
        val note = Slot(act)
        views.add(note)
        numberInput("Dollars for one bitcoin").also { views.add(it.view) }.field.also {
            fillPrice(note, it, BoxWords.priceFetching, BoxWords::priceFetched, BoxWords.priceNeeded)
        }
    } else {
        null
    }
    message?.let { views.add(ui.words(it)) }
    views.add(msg)
    val send = ui.primary(OwnPairWords.boxSend) {
        when (val f = if (price == null) Form.Ok(0L) else CounterModel.price(price.text.toString())) {
            is Form.Refused -> msg.show(ui.words(f.words))
            is Form.Ok -> {
                waiting(BoxWords.handing)
                async({ p.send(f.value, proxyNow()) }) { r ->
                    val said = r.getOrNull()
                    if (r.isFailure) showOwnBox(refusal(r.exceptionOrNull()!!)) else showOwnBox(said)
                }
            }
        }
    }
    stepScreen(OwnPairWords.boxTitle, views, listOf(send), back)
}

/** The Box has its welcome and the link is kept; counters paired before it still need the relay code (CR-132). */
private fun Controller.showOwnBoxSent(p: OwnBoxPairing) {
    val views = listOf(heading(OwnPairWords.boxPairedHeading), para(OwnPairWords.boxPairedText), ui.text(OwnPairWords.boxPairedNext, 15f, bold = true))
    val next = ui.primary(OwnPairWords.boxRelayHeading) {
        if (p.needsRelay().isEmpty()) finishOwnBox() else showOwnBoxRelay(p)
    }
    stepScreen(OwnPairWords.boxTitle, views, listOf(next), { }, tone = ui.pal.good)
}

/**
 * The relay code, for the counters that were paired before the Box: each scans it at its own relay screen
 * (CR-132). One that has taken it is marked, and the list is what is left.
 */
private fun Controller.showOwnBoxRelay(p: OwnBoxPairing, message: SayWords? = null) {
    val waitingCounters = p.needsRelay()
    if (waitingCounters.isEmpty()) return finishOwnBox()
    val views = ArrayList<View>()
    views.add(heading(OwnPairWords.boxRelayHeading))
    val code = try {
        p.relayCode()
    } catch (e: org.openbmt.owner.OwnerException) {
        return showOwnBoxRelay(p, e.sayWords())
    } catch (e: LinkageError) {
        return showOwnBoxRelay(p, AppWords.libraryMissing(e.message))
    }
    if (code != null) views.add(codeCardOf(code))
    views.add(para(OwnPairWords.boxRelayText))
    message?.let { views.add(ui.words(it)) }
    views.add(label(OwnPairWords.boxRelayList))
    for (c in waitingCounters) {
        views.add(ui.button(OwnPairWords.boxRelayTaken + " " + ConnectBoxModel.shortKey(c)) {
            p.relayTaken(c)
            showOwnBoxRelay(p)
        })
    }
    stepScreen(OwnPairWords.boxTitle, views, listOf(ui.primary(OwnPairWords.boxRelayLater) { finishOwnBox() }), { finishOwnBox() })
}
