package org.openbmt.owner.app

import android.view.View
import org.openbmt.app.AnimatedQrView
import org.openbmt.app.SayWords
import org.openbmt.app.UrScan
import org.openbmt.owner.CounterQueued
import org.openbmt.owner.CounterScan
import org.openbmt.owner.OwnerException
import org.openbmt.owner.OwnerNode
import org.openbmt.owner.WaitingCounter

// Adding a counter (OA-22 to OA-25): the camera reads the counter's request, the
// node queues its pairing and shows the four words, the owner compares them and
// confirms, the node queues the settings and the first batch together, the
// approval goes through the screens of `Approval.kt` as any other, and then the
// code the counter takes is shown. Every judgment is the library's; this file
// shows its answers and carries the owner's choices. It signs nothing.

/** The first screen: what the steps are, and the button that opens the camera. [message] is what the last try said. */
internal fun Controller.showAddCounter(message: SayWords? = null) {
    if (node == null) {
        begin()
        return
    }
    val views = ArrayList<View>()
    views.add(ui.text("Add a counter", 22f, bold = true))
    message?.let { views.add(ui.words(it)) }
    for (line in CounterModel.explanation) views.add(ui.text(line, 16f))
    views.add(ui.button("Scan the counter's code") { scanCounter() })
    views.add(ui.button("Back") { showHome() })
    page(views)
}

/**
 * The counter's code, read by the camera, which is asked for only when the button is pressed. The bytes of a
 * complete `ur:bytes` code go to the node; every other end of the scan is words.
 */
private fun Controller.scanCounter() {
    fun give(r: SourceResult) = when (r) {
        is SourceResult.Bytes -> readCounter(r.bytes)
        is SourceResult.Failed -> showAddCounter(r.words)
    }
    scan.begin(
        show = { v ->
            tick = null
            act.show(v)
        },
        onBytes = { give(SourceResult.Failed(AppWords.counterNotACode)) },
        onFailed = { f -> give(ScanResult.failed(f.off, f.problem, f.words)) },
        onCancel = { give(SourceResult.Failed(AppWords.counterScanCancelled)) },
        ur = UrScan(CounterModel.REQUEST_TYPES, ExchangeModel.WHO) { _, payload -> give(CounterModel.requestResult(payload)) },
    )
}

/**
 * Gives the request to the node. Working out the four words takes a moment, so a page says so and is drawn
 * first; the call stays on the screen thread (OA-12).
 */
private fun Controller.readCounter(bytes: ByteArray) {
    val n = node
    if (n == null) {
        begin()
        return
    }
    page(listOf(ui.words(AppWords.counterReading)))
    main.post {
        try {
            showCounterCheck(n, n.counterScan(bytes))
        } catch (e: OwnerException) {
            showAddCounter(e.sayWords())
        } catch (e: LinkageError) {
            showAddCounter(AppWords.libraryMissing(e.message))
        }
    }
}

/**
 * The counter's name, the four words large, the shares the new settings give, and the two answers: the words
 * match, or they differ. A phone with no settings also asks for today's rate.
 */
private fun Controller.showCounterCheck(n: OwnerNode, s: CounterScan) {
    val msg = Slot(act)
    val views = ArrayList<View>()
    views.add(ui.text("Check the counter", 22f, bold = true))
    views.add(label("The counter calls itself"))
    views.add(ui.text(CounterModel.nameOf(s.name), 18f, bold = true))
    views.add(ui.text(CounterModel.wordsLine(s.sessionWords), 30f, bold = true))
    views.add(ui.words(AppWords.counterCompare))
    views.add(ui.words(AppWords.sharesNote(s.shares.size)))
    for (line in CounterModel.shareLines(s.shares)) views.add(ui.text(line, 14f, mono = true))
    views.add(ui.text(CounterModel.windowLine(s.windowSecs), 15f))
    val rate = if (s.needsRate) {
        views.add(ui.words(AppWords.counterRateNeeded))
        numberField(act, "Satoshis to a dollar").also { views.add(it) }
    } else {
        null
    }
    views.add(
        ui.button("The four words match") {
            when (val f = if (rate == null) Form.Ok(0L) else CounterModel.rate(rate.text.toString())) {
                is Form.Refused -> msg.show(ui.words(f.words))
                is Form.Ok -> attempt(msg) { n.counterConfirm(nowSecs(), s.pairing, f.value) }?.let { showCounterQueued(it) }
            }
        },
    )
    views.add(
        ui.button("The words differ") {
            if (attempt(msg) { n.cancel(s.pairing) } != null) {
                page(listOf(ui.words(AppWords.counterCancelled), ui.button("Back") { showHome() }))
            }
        },
    )
    views.add(msg)
    views.add(ui.button("Back") { showHome() })
    page(views)
}

/** What the node queued: the settings with their shares, and the first batch. The approval comes next. */
private fun Controller.showCounterQueued(q: CounterQueued) {
    val views = ArrayList<View>()
    views.add(ui.text("Counter queued", 22f, bold = true))
    views.add(ui.words(q.words.say()))
    views.add(label("Shares of the instant limit"))
    for (line in CounterModel.shareLines(q.shares)) views.add(ui.text(line, 14f, mono = true))
    views.add(ui.text(CounterModel.batchLine(q.batchCount), 15f))
    views.add(ui.text(CounterModel.nextLine, 15f))
    views.add(ui.button("Build the request for the Key app") { showSpend() })
    views.add(ui.button("Later") { showHome() })
    page(views)
}

/**
 * The code the counter takes, as `ur:bytes` parts of the shared `AnimatedQrView`. Leaving this screen keeps
 * the counter waiting; only the owner's word that the counter shows Paired marks it delivered (AS-73).
 */
internal fun Controller.showCounterCode(w: WaitingCounter, back: () -> Unit) {
    val n = node
    if (n == null) {
        begin()
        return
    }
    val made = try {
        n.counterBundle(w.counter)
    } catch (e: OwnerException) {
        page(listOf(ui.words(e.sayWords()), ui.button("Back") { back() }))
        return
    } catch (e: LinkageError) {
        page(listOf(ui.words(AppWords.libraryMissing(e.message)), ui.button("Back") { back() }))
        return
    }
    val shown = AnimatedQrView.make(
        act, ExchangeModel.WHO, ExchangeModel.TYPE_BYTES, made.code,
        ExchangeModel.codeHeight(act.resources.displayMetrics.heightPixels),
    )
    val view = shown.view
    val step = CounterModel.codeStep(view != null, made.withBatch, view?.animated == true)
    val msg = Slot(act)
    val views = ArrayList<View>()
    views.add(ui.text("Code for ${CounterModel.nameOf(w.name)}", 22f, bold = true))
    views.add(ui.words(step.words))
    if (view != null) views.add(view)
    if (step.done) {
        views.add(
            ui.button("The counter shows Paired") {
                if (attempt(msg) { n.counterDelivered(w.counter) } != null) {
                    page(listOf(ui.words(AppWords.counterDelivered), ui.button("Done") { showHome() }))
                }
            },
        )
    }
    views.add(ui.button("Show again") { showCounterCode(w, back) })
    views.add(msg)
    views.add(ui.button("Back") { back() })
    page(views)
}

/** The counters whose approved pairing waits to be handed over, each with a button to show its code. Empty when none waits. */
internal fun Controller.waitingCounterViews(n: OwnerNode, msg: Slot, back: () -> Unit): List<View> {
    val rows = attempt(msg) { n.countersWaiting() } ?: return listOf(msg)
    if (rows.isEmpty()) return emptyList()
    val views = ArrayList<View>()
    views.add(label("Counters waiting for their code"))
    for (w in rows) {
        views.add(ui.text(CounterModel.nameOf(w.name) + "\n" + OwnerModel.counterLabel(w.counter), 14f, mono = true))
        views.add(ui.button(CounterModel.showCodeLabel(w)) { showCounterCode(w, back) })
    }
    return views
}
