package org.openbmt.owner.app

import android.view.View
import org.openbmt.app.AnimatedQrView
import org.openbmt.app.SayWords
import org.openbmt.app.UrScan
import org.openbmt.app.UrWords
import org.openbmt.app.Who

// Adding a counter to a shop with its own wallet (OA-108 to OA-111): the offer is drawn as a code, the counter's
// request is scanned, the four words are compared, "They match" makes the bundle, and the bundle is drawn until the
// owner says the counter shows Paired. The wallet's file and the owner root's secret are kept sealed by the store
// (OA-105); the pairing holds only what is on the screen. Every judgment is the library's; this file shows its
// answers and carries the owner's choices. Every sentence is in OwnPairWords or the library's own words.

/** The page the first run is at once the wallet is checked: the pairing step that [OwnPairing] is at. */
internal fun Controller.showOwnCounter(r: FirstRun, message: SayWords? = null) {
    val p = ownPairing ?: OwnPairing({ ownStore.args() }).also { fresh ->
        ownPairing = fresh
        val said = fresh.start(nowSecs())
        if (said != null) return ownStopped(r, said)
    }
    val leave: () -> Unit = {
        ownPairing?.back()
        ownPairing = null
        r.back()
        showFirstRun()
    }
    val back: () -> Unit = { if (p.back()) showOwnCounter(r) else leave() }
    when (p.step) {
        OwnPairStep.Offer -> showOwnOffer(r, p, message, back)
        OwnPairStep.Compare -> showOwnCompare(r, p, message, back)
        OwnPairStep.Bundle -> showOwnBundle(r, p, message, back)
        OwnPairStep.Paired -> showOwnPaired(r, p)
        OwnPairStep.Stopped -> ownStopped(r, p.stopped ?: OwnPairWords.codeNotShown)
    }
}

/** The words of a pairing that did not start or was stopped, and the way to start again. */
private fun Controller.ownStopped(r: FirstRun, words: SayWords) {
    val again = ui.primary(OwnPairWords.endedButton) {
        ownPairing = null
        showOwnCounter(r)
    }
    val back: () -> Unit = {
        ownPairing = null
        r.back()
        showFirstRun()
    }
    stepScreen(OwnPairWords.title, listOf(ui.words(words)), listOf(again), back, tone = ui.pal.stop)
}

/** A code of the library's, drawn as the animated code of the shared view; the words when it cannot be shown. */
private fun Controller.codeCardOf(code: ByteArray): View {
    val shown = AnimatedQrView.make(
        act, ExchangeModel.WHO, ExchangeModel.TYPE_BYTES, code,
        ExchangeModel.codeHeight(act.resources.displayMetrics.heightPixels),
    )
    return shown.view?.let { ui.codeCard(it) } ?: ui.words(shown.words ?: UrWords.cannotShow(Who.OWNER))
}

private fun Controller.showOwnOffer(r: FirstRun, p: OwnPairing, message: SayWords?, back: () -> Unit) {
    val offer = p.offer
    val views = ArrayList<View>()
    views.add(heading(OwnPairWords.offerHeading))
    val ended = p.ended(nowSecs())
    if (offer != null && !ended) {
        views.add(codeCardOf(offer.made.code))
        views.add(para(OwnPairWords.offerText))
    } else {
        views.add(ui.words(endedWords()))
    }
    message?.let { views.add(ui.words(it)) }
    val msg = Slot(act)
    // A bundle that was made and not yet taken waits in the library (OA-111).
    val waiting = p.waiting()
    if (waiting.isNotEmpty()) {
        views.add(label(OwnPairWords.waitingLabel))
        for (w in waiting) views.add(ui.button(CounterModel.showCodeLabel(w)) { ownResume(r, p, w) })
    }
    views.add(msg)
    val action = if (offer != null && !ended) {
        ui.primary(OwnPairWords.offerScan) { scanOwnCounter(r, p) }
    } else {
        ui.primary(OwnPairWords.endedButton) { showOwnCounter(r, p.start(nowSecs())) }
    }
    stepScreen(OwnPairWords.title, views, listOf(action), back)
}

/** The 10 minutes are over (OA-108): the words of the design file. */
private fun endedWords(): SayWords = SayWords(
    org.openbmt.app.Tones.STOP, Who.OWNER, OwnPairWords.endedHeading, OwnPairWords.endedText, OwnPairWords.endedNext,
    null, null, null, null,
)

private fun Controller.ownResume(r: FirstRun, p: OwnPairing, w: org.openbmt.owner.WaitingCounter) {
    val said = p.resume(w, nowSecs())
    showOwnCounter(r, said)
}

/** The counter's request, read by the camera, which is asked for only when the button is pressed. */
private fun Controller.scanOwnCounter(r: FirstRun, p: OwnPairing) {
    fun give(result: SourceResult) = when (result) {
        is SourceResult.Bytes -> showOwnCounter(r, p.readRequest(result.bytes, nowSecs()))
        is SourceResult.Failed -> showOwnCounter(r, result.words)
    }
    scan.begin(
        show = { v -> showScan(v) },
        onBytes = { give(SourceResult.Failed(AppWords.counterNotACode)) },
        onFailed = { f -> give(ScanResult.failed(f.off, f.problem, f.words)) },
        onCancel = { give(SourceResult.Failed(AppWords.counterScanCancelled)) },
        ur = UrScan(CounterModel.REQUEST_TYPES, ExchangeModel.WHO) { _, payload -> give(OwnPairing.requestBytes(payload)) },
    )
}

private fun Controller.showOwnCompare(r: FirstRun, p: OwnPairing, message: SayWords?, back: () -> Unit) {
    val read = p.read ?: return showOwnCounter(r)
    val msg = Slot(act)
    val views = ArrayList<View>()
    views.add(heading(OwnPairWords.compareHeading))
    views.add(para(OwnPairWords.compare(p.name())))
    views.add(wordCards(CounterModel.wordsLine(read.sessionWords)))
    views.add(ui.words(AppWords.sharesNote(read.shares.size)))
    views.add(stack(*CounterModel.shareLines(read.shares).map { line(it, muted = true, mono = true) }.toTypedArray(), gap = 4))
    val price = if (read.needsRate) {
        val note = Slot(act)
        views.add(note)
        numberInput("Dollars for one bitcoin").also { views.add(it.view) }.field.also {
            fillPrice(note, it, AppWords.counterPriceFetching, AppWords::counterPriceFetched, AppWords.counterPriceNeeded)
        }
    } else {
        null
    }
    message?.let { views.add(ui.words(it)) }
    views.add(msg)
    val match = ui.primary(OwnPairWords.compareMatch) {
        when (val f = if (price == null) Form.Ok(0L) else CounterModel.price(price.text.toString())) {
            is Form.Refused -> msg.show(ui.words(f.words))
            is Form.Ok -> {
                val said = p.match(nowSecs(), f.value)
                showOwnCounter(r, said)
            }
        }
    }
    val differ = ui.danger(OwnPairWords.compareDiffer) {
        p.differ()
        showOwnCounter(r)
    }
    stepScreen(OwnPairWords.title, views, listOf(match, differ), back)
}

private fun Controller.showOwnBundle(r: FirstRun, p: OwnPairing, message: SayWords?, back: () -> Unit) {
    val bundle = p.bundle ?: return showOwnCounter(r)
    val msg = Slot(act)
    val views = ArrayList<View>()
    views.add(heading(OwnPairWords.bundle(p.name())))
    views.add(codeCardOf(bundle.code))
    views.add(para(OwnPairWords.bundleText))
    message?.let { views.add(ui.words(it)) }
    views.add(msg)
    val done = ui.primary(OwnPairWords.bundlePaired) {
        val said = p.paired()
        if (said == null) showOwnCounter(r) else msg.show(ui.words(said))
    }
    val again = ui.button(OwnPairWords.bundleAgain) { showOwnCounter(r) }
    stepScreen(OwnPairWords.title, views, listOf(done, again), back)
}

private fun Controller.showOwnPaired(r: FirstRun, p: OwnPairing) {
    val views = listOf(heading(OwnPairWords.paired(p.name())), para(OwnPairWords.pairedText), ui.text(OwnPairWords.pairedNext, 15f, bold = true))
    val done = ui.primary(FirstRunWords.tillStepsDone) {
        ownPairing = null
        r.next()
        showFirstRun()
    }
    val wallet = ui.button(OwnPairWords.walletButton) { showOwnWallet(r) }
    stepScreen(OwnPairWords.title, views, listOf(done, wallet), { }, tone = ui.pal.good)
}

/** The wallet page: the first address, the kind of wallet and how far the wallet must look ahead (OA-113). */
internal fun Controller.showOwnWallet(r: FirstRun) {
    val views = ArrayList<View>()
    val msg = Slot(act)
    views.add(msg)
    val back: () -> Unit = { showOwnCounter(r) }
    try {
        val file = ownStore.loadWallet()
        if (file == null) {
            msg.show(ui.words(OwnPairWords.noWallet))
        } else {
            val wallet = org.openbmt.owner.OwnerOwn.read(file)
            file.fill(0)
            views.add(ui.text(Connect.rows(wallet.firstAddress), 22f, bold = true, mono = true))
            views.add(line(ConnectWords.kind(wallet.multisig), muted = true))
            // The running node knows what was paid and what the counters handed out (OA-113).
            val running = node?.let { n -> { n.ownGap() } }
            val row = try {
                OwnWalletPage.gapNow(running, NativeOwnLibrary)
            } catch (_: org.openbmt.owner.OwnerException) {
                OwnWalletPage.gapNow(null, NativeOwnLibrary)
            }
            views.add(stack(label(row.title), line(row.line), *listOfNotNull(row.next?.let { line(it, muted = true) }).toTypedArray(), gap = 4))
        }
    } catch (e: org.openbmt.owner.OwnerException) {
        msg.show(ui.words(e.sayWords()))
    } catch (e: java.io.IOException) {
        msg.show(ui.words(OwnPairWords.walletNotSaved))
    } catch (e: LinkageError) {
        msg.show(ui.words(AppWords.libraryMissing(e.message)))
    }
    stepScreen(OwnPairWords.walletTitle, views, listOf(ui.button(KeyGuideWords.kgBack) { back() }), back)
}
