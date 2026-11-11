package org.openbmt.owner.app

import android.view.View
import org.openbmt.app.AnimatedQrView
import org.openbmt.owner.Built
import org.openbmt.owner.OwnerException
import org.openbmt.owner.OwnerNode
import org.openbmt.owner.Taken

// The approval with the Key app: the request this phone builds, shown as a
// code on the screen for the Key app to scan (first), or saved as a file or
// copied as text; the Key app's response taken back from its code by the
// camera (first), a file or pasted text, behind `ResponseSource`. Nothing
// here signs: the request is bytes the library made, and the response is
// bytes the library reads, whichever source they come from (OA-07, OA-09,
// OA-13, OA-21). Three screens, `ExchangeStep`: the request, its code, the
// response.

/** Shows the request of [built] and the ways to give it to the Key app. [back] is where the screen was opened from. */
internal fun Controller.showBuilt(built: Built, back: () -> Unit) {
    if (node == null) {
        begin()
        return
    }
    val msg = Slot(act)
    val views = ArrayList<View>()
    views.add(ui.text("Request for the Key app", 22f, bold = true))
    views.add(ui.words(AppWords.requestReady))
    for (s in built.skipped) {
        views.add(ui.text("Left out of this request: entry ${s.id}", 14f, bold = true))
        views.add(ui.words(s.words.say()))
    }
    views.add(ui.text(ApprovalText.preview(built.request), 12f, mono = true))
    val name = ApprovalText.fileName("request", built.sessionId)
    for (action in ExchangeModel.requestActions) {
        views.add(
            ui.button(action.label) {
                when (action) {
                    RequestAction.ShowCode -> showBuiltCode(built, back)
                    RequestAction.SaveFile -> saveFile(name, built.request, { showBuilt(built, back) }) { words ->
                        msg.show(ui.words(words))
                    }
                    RequestAction.CopyText -> {
                        copy(ApprovalText.encode(built.request))
                        msg.show(ui.words(AppWords.requestCopied))
                    }
                    RequestAction.Response -> showResponse(built, back, ExchangeStep.Response(null))
                }
            },
        )
    }
    views.add(msg)
    views.add(ui.button("Back") { back() })
    page(views)
}

/**
 * The request as a code on the screen, `ur:bytes` parts for the Key app to scan: one still code when the
 * request fits one part, otherwise a code that changes, as large as the Key app draws its answer. The words
 * say what to do next; Done leads to the response. If no code can be made the words say so and to save the
 * request as a file or copy it as text instead.
 */
private fun Controller.showBuiltCode(built: Built, back: () -> Unit) {
    val made = AnimatedQrView.make(
        act, ExchangeModel.WHO, ExchangeModel.TYPE_BYTES, built.request,
        ExchangeModel.codeHeight(act.resources.displayMetrics.heightPixels),
    )
    val view = made.view
    val step = ExchangeModel.codeStep(view != null, view?.animated == true)
    val views = ArrayList<View>()
    views.add(ui.text("Request for the Key app", 22f, bold = true))
    views.add(ui.words(step.words))
    if (view != null) views.add(view)
    if (step.done) views.add(ui.button("Done") { showResponse(built, back, ExchangeStep.Response(null)) })
    views.add(ui.button("Back") { showBuilt(built, back) })
    page(views)
}

/** The response: the Key app's code first, then a file, then pasted text. [step] says what the last try said. */
private fun Controller.showResponse(built: Built, back: () -> Unit, step: ExchangeStep.Response) {
    val n = node
    if (n == null) {
        begin()
        return
    }
    val msg = Slot(act)
    step.message?.let { msg.show(ui.words(it)) }
    val views = ArrayList<View>()
    views.add(ui.text("The Key app's response", 22f, bold = true))
    val pasted = ui.field("Paste the response text here", lines = 4)
    val sources = responseSources({ pasted.text.toString() }, { showResponse(built, back, ExchangeStep.Response(null)) })
    for ((kind, source) in ExchangeModel.responseKinds.zip(sources)) {
        if (kind == ResponseKind.Pasted) views.add(pasted)
        views.add(
            ui.button(source.label) {
                source.begin({ bytes -> takeResponse(n, bytes, msg) }) { words ->
                    // A scan has taken the screen: show this one again with the words.
                    if (source.leavesScreen) showResponse(built, back, ExchangeStep.Response(words)) else msg.show(ui.words(words))
                }
            },
        )
    }
    views.add(msg)
    views.add(ui.button("Back to the request") { showBuilt(built, back) })
    page(views)
}

private fun Controller.takeResponse(n: OwnerNode, bytes: ByteArray, msg: Slot) {
    try {
        showTaken(n.take(nowSecs(), bytes))
    } catch (e: OwnerException) {
        msg.show(ui.words(e.sayWords()))
    } catch (e: LinkageError) {
        msg.show(ui.words(AppWords.libraryMissing(e.message)))
    }
}

/** What taking the response did, item by item, in the library's words. */
private fun Controller.showTaken(taken: Taken) {
    val views = ArrayList<View>()
    views.add(ui.text("The Key app's response was taken", 22f, bold = true))
    for (o in taken.outcomes) {
        views.add(label("Entry ${o.item}"))
        views.add(ui.words(o.words.say()))
    }
    if (taken.sends.isNotEmpty()) views.add(label("Payments"))
    for (s in taken.sends) {
        views.add(ui.words(s.words.say()))
        views.add(ui.text("Transaction ${s.txid}", 12f, mono = true))
    }
    // A counter whose pairing this response approved waits for its code (OA-24).
    node?.let { n -> views.addAll(waitingCounterViews(n, Slot(act)) { showHome() }) }
    // A Box whose welcome this response approved waits to be handed over (OA-43).
    node?.let { n -> views.addAll(pendingBoxViews(n, Slot(act)) { showHome() }) }
    views.add(ui.button("Done") { showHome() })
    page(views)
}
