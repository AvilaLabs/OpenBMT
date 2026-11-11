package org.openbmt.owner.app

import android.view.View
import org.openbmt.owner.Built
import org.openbmt.owner.OwnerException
import org.openbmt.owner.OwnerNode
import org.openbmt.owner.Taken

// The approval with the Key app: the request this phone builds, shown as text
// and saved as a file for the Key app; the Key app's response taken back from
// pasted text or a file, behind `ResponseSource`. Nothing here signs: the
// request is bytes the library made, and the response is bytes the library
// reads (OA-07, OA-09, OA-13).

/** Shows the request of [built], and the way to bring the Key app's response back. [back] is where the screen was opened from. */
internal fun Controller.showBuilt(built: Built, back: () -> Unit) {
    val n = node
    if (n == null) {
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
    views.add(
        ui.button("Copy the request as text") {
            copy(ApprovalText.encode(built.request))
            msg.show(ui.words(AppWords.requestCopied))
        },
    )
    views.add(
        ui.button("Save the request as a file") {
            saveFile(name, built.request, { showBuilt(built, back) }) { words ->
                msg.show(ui.words(words))
            }
        },
    )
    views.add(label("The Key app's response"))
    val pasted = ui.field("Paste the response text here", lines = 4)
    views.add(pasted)
    for (source in responseSources({ pasted.text.toString() }, { showBuilt(built, back) })) {
        views.add(
            ui.button(source.label) {
                source.begin({ bytes -> takeResponse(n, bytes, msg) }, { words -> msg.show(ui.words(words)) })
            },
        )
    }
    views.add(msg)
    views.add(ui.button("Back") { back() })
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
    views.add(ui.button("Done") { showHome() })
    page(views)
}
