package org.openbmt.owner.app

import android.view.View
import org.openbmt.app.AnimatedQrView
import org.openbmt.app.ButtonKind
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
    for (s in built.skipped) {
        views.add(stack(ui.text("Left out of this request: entry ${s.id}", 14f, bold = true), ui.words(s.words.say())))
    }
    views.add(line(ApprovalText.preview(built.request), muted = true, mono = true))
    views.add(msg)
    val actions = ArrayList<android.widget.Button>()
    val name = ApprovalText.fileName("request", built.sessionId)
    for (action in ExchangeModel.requestActions) {
        actions.add(
            ui.button(action.label, if (action == RequestAction.ShowCode) ButtonKind.Primary else ButtonKind.Secondary) {
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
    actions.add(ui.button("Back") { back() })
    screen("Request for the Key app", views, actions, back = back, words = AppWords.requestReady)
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
    val actions = ArrayList<android.widget.Button>()
    if (step.done) actions.add(ui.primary("Done") { showResponse(built, back, ExchangeStep.Response(null)) })
    actions.add(ui.button("Back") { showBuilt(built, back) })
    codeScreen("Request for the Key app", step.words, view, actions = actions, back = { showBuilt(built, back) })
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
    val actions = ArrayList<android.widget.Button>()
    val pasted = ui.field("Paste the response text here", lines = 4, mono = true)
    val sources = responseSources({ pasted.text.toString() }, { showResponse(built, back, ExchangeStep.Response(null)) })
    for ((kind, source) in ExchangeModel.responseKinds.zip(sources)) {
        val button = ui.button(source.label, if (kind == ExchangeModel.responseKinds.first()) ButtonKind.Primary else ButtonKind.Secondary) {
            source.begin({ bytes -> takeResponse(n, bytes, msg) }) { words ->
                // A scan has taken the screen: show this one again with the words.
                if (source.leavesScreen) showResponse(built, back, ExchangeStep.Response(words)) else msg.show(ui.words(words))
            }
        }
        if (kind == ExchangeModel.responseKinds.first()) {
            actions.add(button)
        } else {
            if (kind == ResponseKind.Pasted) views.add(pasted)
            views.add(button)
        }
    }
    views.add(msg)
    actions.add(ui.button("Back to the request") { showBuilt(built, back) })
    screen("The Key app's response", views, actions, back = { showBuilt(built, back) })
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
    for (o in taken.outcomes) {
        views.add(stack(label("Entry ${o.item}"), ui.words(o.words.say())))
    }
    if (taken.sends.isNotEmpty()) views.add(label("Payments"))
    for (s in taken.sends) {
        views.add(stack(ui.words(s.words.say()), line("Transaction ${s.txid}", muted = true, mono = true)))
    }
    // A counter whose pairing this response approved waits for its code (OA-24).
    node?.let { n -> views.addAll(waitingCounterViews(n, Slot(act)) { showHome() }) }
    // A Box whose welcome this response approved waits to be handed over (OA-43).
    node?.let { n -> views.addAll(pendingBoxViews(n, Slot(act)) { showHome() }) }
    screen("The Key app's response was taken", views, listOf(ui.primary("Done") { showHome() }))
}
