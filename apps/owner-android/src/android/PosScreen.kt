package org.openbmt.owner.app

import android.content.ClipboardManager
import android.view.View
import java.io.IOException
import java.security.GeneralSecurityException
import org.openbmt.app.ButtonKind
import org.openbmt.app.SayWords
import org.openbmt.owner.OwnerNode
import org.openbmt.owner.OwnerException

// The till screen (OA-16 to OA-19). Every call into the library is made here
// on the screen thread, except the two of a read, which the till thread makes
// (`Controller.kt`, OA-12). The sign-in is typed into a field once, sealed
// and wiped from the field; it is never put on the screen again.

private fun Controller.back(to: () -> Unit = { showHome() }): android.widget.Button = ui.button("Back") { to() }

/** Takes the typed sign-in out of memory: the field is emptied and the clipboard it may have come from is cleared (OA-16). */
private fun Controller.forget(field: android.widget.EditText) {
    field.setText("")
    act.getSystemService(ClipboardManager::class.java)?.clearPrimaryClip()
}

internal fun Controller.showTill(message: SayWords? = null): Unit = withTillNode { n ->
    val views = ArrayList<View>()
    message?.let { views.add(ui.words(it)) }
    if (setup?.link == null) {
        views.add(ui.words(AppWords.posNeedsLink))
        return@withTillNode screen("Till", views, listOf(back()), back = { showHome() })
    }
    val msg = Slot(act)
    val state = attempt(msg) { n.posState() }
    if (state == null) {
        views.add(msg)
        return@withTillNode screen("Till", views, listOf(back()), back = { showHome() })
    }
    posConnected = state.setup != null
    val till = state.setup
    if (till == null) {
        views.add(ui.words(AppWords.posNotConnected))
        val form = connectForm(n, msg)
        views.addAll(form.views)
        views.add(msg)
        return@withTillNode screen("Till", views, listOf(ui.button("Connect the till", ButtonKind.Primary) { form.connect() }, back()), back = { showHome() })
    }
    views.add(line(PosLines.till(till), muted = true))
    val body = Slot(act)
    views.add(body)
    val token = secretField(act, "")
    views.add(ui.labelled("Replace the sign-in", token))
    views.add(
        ui.button("Seal the new sign-in") {
            when (val t = PosForms.token(token.text.toString())) {
                is Form.Refused -> msg.show(ui.words(t.words))
                is Form.Ok -> {
                    forget(token)
                    sealToken(t.value, n, rollback = false)
                }
            }
        },
    )
    val takeOff =
        ui.danger("Take the till off") {
            if (attempt(msg) { n.posRemove() } != null) {
                posConnected = false
                posFault = null
                async({ posTokens.remove() }) { r ->
                    when (val e = r.exceptionOrNull()) {
                        null -> showTill(AppWords.posRemoved)
                        is IOException -> showTill(AppWords.storageUnavailable(e.message))
                        else -> throw e
                    }
                }
            }
        }
    views.add(msg)
    screen("Till", views, listOf(ui.primary("Read the till now") { readTillNow() }, back(), takeOff), back = { showHome() })
    tick = {
        try {
            val s = n.posState()
            val out = ArrayList<View>()
            posFault?.let { out.add(ui.words(it)) }
            if (!posTokens.exists()) out.add(ui.words(AppWords.posTokenMissing))
            out.add(ui.text(PosLines.lastRead(nowSecs(), s.lastPollAt), 15f))
            out.add(ui.text(PosLines.counts(s), 15f, bold = s.openAlerts.isNotEmpty()))
            for (a in s.openAlerts) {
                out.add(
                    stack(
                        ui.words(a.words.say()),
                        ui.button("Acknowledge this alert") {
                            if (attempt(msg) { n.posAcknowledge(nowSecs(), a.key) } != null) showTill(AppWords.posAcknowledged)
                        },
                        gap = 10,
                    ),
                )
            }
            body.show(*out.toTypedArray())
        } catch (e: OwnerException) {
            body.show(ui.words(e.sayWords()))
        }
    }
    tick?.invoke()
}

/** The node of the till screen, or the first screen again when there is none. */
private inline fun Controller.withTillNode(block: (OwnerNode) -> Unit) {
    val n = node
    if (n == null) begin() else block(n)
}

/** The fields that connect a till, and the button. The library judges what was typed; the sign-in is only checked for its shape. */
private fun Controller.connectForm(n: OwnerNode, msg: Slot): TillForm {
    val views = ArrayList<View>()
    val vendor = ui.chips(listOf("Square", "Clover"))
    views.add(stack(label("Vendor"), vendor))
    val location = ui.field("Square location id, or Clover merchant id")
    views.add(ui.labelled("Location", location))
    val tenders = ui.field("Bitcoin", lines = 3).also { it.setText("Bitcoin") }
    views.add(ui.labelled("Names of the Bitcoin tender, one per line", tenders))
    val currencies = ui.field("USD").also { it.setText("USD") }
    views.add(ui.labelled("Currencies the shop sells in", currencies))
    val token = secretField(act, "")
    views.add(stack(ui.labelled("Read-only sign-in", token), line("Make it in the till's system with read access to orders and payments only. It is pasted once and not shown again.", muted = true), gap = 8))
    val connect = {
        val t = when (val f = PosForms.token(token.text.toString())) {
            is Form.Refused -> msg.show(ui.words(f.words)).let { null }
            is Form.Ok -> f.value
        }
        if (t != null) {
            val args = PosForms.setup(ui.chosen(vendor), location.text.toString(), tenders.text.toString(), currencies.text.toString())
            // The library judges the settings first, so that a refused till seals nothing.
            if (attempt(msg) { n.posSetup(args) } != null) {
                forget(token)
                sealToken(t, n, rollback = true)
            }
        }
    }
    return TillForm(views, connect)
}

/** The fields that connect a till and what the footer's button does with them. */
private class TillForm(val views: List<View>, val connect: () -> Unit)

/** Seals the token in its own file, off the screen thread; if it cannot be kept, a till just connected is taken off again. */
private fun Controller.sealToken(token: String, n: OwnerNode, rollback: Boolean) {
    async({ posTokens.save(token) }) { r ->
        val failed: SayWords? = when (val e = r.exceptionOrNull()) {
            null -> null
            is IOException -> AppWords.fileNotSaved(e.message)
            is GeneralSecurityException -> AppWords.storageUnavailable(e.message)
            else -> throw e
        }
        if (failed == null) {
            posConnected = true
            posFault = null
            readTillNow()
            showTill(AppWords.posConnected)
        } else {
            if (rollback) {
                try {
                    n.posRemove()
                } catch (_: OwnerException) {
                }
                posConnected = false
            }
            showTill(failed)
        }
    }
}
