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

private fun Controller.back(to: () -> Unit = { showHome() }): View = ui.button("Back") { to() }

/** Takes the typed sign-in out of memory: the field is emptied and the clipboard it may have come from is cleared (OA-16). */
private fun Controller.forget(field: android.widget.EditText) {
    field.setText("")
    act.getSystemService(ClipboardManager::class.java)?.clearPrimaryClip()
}

internal fun Controller.showTill(message: SayWords? = null): Unit = withTillNode { n ->
    val views = ArrayList<View>()
    views.add(ui.text("Till", 22f, bold = true))
    message?.let { views.add(ui.words(it)) }
    if (setup?.link == null) {
        views.add(ui.words(AppWords.posNeedsLink))
        views.add(back())
        return@withTillNode page(views)
    }
    val msg = Slot(act)
    val state = attempt(msg) { n.posState() }
    if (state == null) {
        views.add(msg)
        views.add(back())
        return@withTillNode page(views)
    }
    posConnected = state.setup != null
    val till = state.setup
    if (till == null) {
        views.add(ui.words(AppWords.posNotConnected))
        views.addAll(connectForm(n, msg))
        views.add(msg)
        views.add(back())
        return@withTillNode page(views)
    }
    views.add(ui.text(PosLines.till(till), 14f))
    val body = Slot(act)
    views.add(body)
    views.add(ui.primary("Read the till now") { readTillNow() })
    views.add(label("Replace the sign-in"))
    val token = secretField(act, "New read-only sign-in")
    views.add(token)
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
    views.add(
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
        },
    )
    views.add(msg)
    views.add(back())
    page(views)
    tick = {
        try {
            val s = n.posState()
            val out = ArrayList<View>()
            posFault?.let { out.add(ui.words(it)) }
            if (!posTokens.exists()) out.add(ui.words(AppWords.posTokenMissing))
            out.add(ui.text(PosLines.lastRead(nowSecs(), s.lastPollAt), 15f))
            out.add(ui.text(PosLines.counts(s), 15f, bold = s.openAlerts.isNotEmpty()))
            for (a in s.openAlerts) {
                out.add(ui.words(a.words.say()))
                out.add(
                    ui.button("Acknowledge this alert") {
                        if (attempt(msg) { n.posAcknowledge(nowSecs(), a.key) } != null) showTill(AppWords.posAcknowledged)
                    },
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
private fun Controller.connectForm(n: OwnerNode, msg: Slot): List<View> {
    val views = ArrayList<View>()
    views.add(label("Vendor"))
    val vendor = ui.choice(listOf("Square", "Clover"))
    views.add(vendor)
    views.add(label("Location"))
    val location = ui.field("Square location id, or Clover merchant id")
    views.add(location)
    views.add(label("Names of the Bitcoin tender, one per line"))
    val tenders = ui.field("Bitcoin", lines = 3).also { it.setText("Bitcoin") }
    views.add(tenders)
    views.add(label("Currencies the shop sells in"))
    val currencies = ui.field("USD").also { it.setText("USD") }
    views.add(currencies)
    views.add(label("Read-only sign-in"))
    views.add(ui.text("Make it in the till's system with read access to orders and payments only. It is pasted once and not shown again.", 14f))
    val token = secretField(act, "Read-only sign-in")
    views.add(token)
    views.add(
        ui.button("Connect the till", ButtonKind.Primary) {
            val t = when (val f = PosForms.token(token.text.toString())) {
                is Form.Refused -> return@button msg.show(ui.words(f.words))
                is Form.Ok -> f.value
            }
            val args = PosForms.setup(ui.chosen(vendor), location.text.toString(), tenders.text.toString(), currencies.text.toString())
            // The library judges the settings first, so that a refused till seals nothing.
            if (attempt(msg) { n.posSetup(args) } == null) return@button
            forget(token)
            sealToken(t, n, rollback = true)
        },
    )
    return views
}

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
