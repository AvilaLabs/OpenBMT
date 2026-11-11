package org.openbmt.owner.app

import android.view.View
import java.io.IOException
import org.openbmt.app.ButtonKind
import org.openbmt.app.Feedback
import org.openbmt.app.SayWords
import org.openbmt.owner.BoxDelivered
import org.openbmt.owner.BoxFound
import org.openbmt.owner.BoxQueued
import org.openbmt.owner.OwnerLock
import org.openbmt.owner.OwnerNode
import org.openbmt.owner.PendingBox

// Connecting the shop's Box (OA-40 to OA-48): the owner types the Box's address
// (or scans it as plain text), the node fetches the Box's claim, the node queues
// it with the settings that add the Box and shows the Box's four words, the
// approval goes through the screens of `Approval.kt` as any other, and then the
// node hands the Box its welcome. Only when the Box has taken it is the link to
// it saved, in the locked setup. A Box that was not handed its welcome can be
// forgotten. Every judgment is the library's; this file shows its answers and
// carries the owner's choices. It signs nothing and builds no claim or welcome.

/** The address a pending Box was found at, by its key in hex, so that the hand-over need not ask again. In memory only. */
private val boxAddresses = HashMap<String, String>()

private fun keyHex(key: ByteArray): String = key.joinToString("") { "%02x".format(it) }

/** The first screen: what the steps are, the address, and the Boxes that wait. [message] is what the last try said. */
internal fun Controller.showConnectBox(message: SayWords? = null, prefill: String = "") {
    val n = node
    if (n == null) {
        begin()
        return
    }
    val msg = Slot(act)
    val views = ArrayList<View>()
    message?.let { views.add(ui.words(it)) }
    for (line in ConnectBoxModel.explanation) views.add(ui.text(line, 16f))
    val address = ui.field("host:port or name.onion:port").also { it.setText(prefill) }
    views.add(ui.labelled("The Box's address", address))
    views.add(ui.button("Scan the Box's address") { scanBox() })
    views.add(msg)
    views.addAll(pendingBoxViews(n, Slot(act)) { showConnectBox() })
    screen(
        "Connect your Box", views,
        listOf(ui.primary("Look for the Box") { lookForBox(n, address.text.toString(), msg) { a, f -> showBoxFound(n, a, f) } }, ui.button("Back") { showHome() }),
        back = { showHome() },
    )
}

/** The address read by the camera as plain text, which is asked for only when the button is pressed. */
private fun Controller.scanBox() {
    fun give(r: SourceResult) = when (r) {
        is SourceResult.Bytes -> showConnectBox(prefill = String(r.bytes, Charsets.UTF_8).take(300))
        is SourceResult.Failed -> showConnectBox(message = r.words)
    }
    scan.begin(
        show = { v -> showScan(v) },
        onBytes = { give(SourceResult.Bytes(it)) },
        onFailed = { f -> give(ScanResult.failed(f.off, f.problem, f.words)) },
        onCancel = { give(SourceResult.Failed(AppWords.counterScanCancelled)) },
    )
}

/**
 * Asks the Box at [text] for its claim. The call waits on the network for up to about a minute, so a page says
 * so and the call runs off the screen thread (OA-41). [onFound] gets the address as the library will read it
 * and what the Box offered; every refusal is the library's words, shown on the first screen again.
 */
private fun Controller.lookForBox(n: OwnerNode, text: String, msg: Slot, onFound: (String, BoxFound) -> Unit) {
    when (val a = ConnectBoxModel.address(text)) {
        is Form.Refused -> msg.show(ui.words(a.words))
        is Form.Ok -> {
            val address = a.value
            val link = ConnectBoxModel.linkFor(address, proxyNow())
            waiting(BoxWords.looking)
            async({ n.boxFetchClaim(link) }) { r ->
                val found = r.getOrNull()
                if (found != null) onFound(address, found) else showConnectBox(refusal(r.exceptionOrNull()!!), address)
            }
        }
    }
}

/**
 * The Box that answered: its key in full, and for a phone with no settings the price. Queuing is the owner's
 * press; the node queues the claim and the settings that add the Box together, or neither (OA-42).
 */
private fun Controller.showBoxFound(n: OwnerNode, address: String, found: BoxFound) {
    val msg = Slot(act)
    val views = ArrayList<View>()
    views.add(stack(label("The Box's key"), line(ConnectBoxModel.keyGroups(found.boxKey), muted = true, mono = true)))
    val price = if (found.needsRate) {
        views.add(ui.words(BoxWords.priceNeeded))
        numberInput("Dollars for one bitcoin").also { views.add(it.view) }.field
    } else {
        null
    }
    views.add(msg)
    val queue = ui.primary("Queue the Box for the Key app") {
        when (val f = if (price == null) Form.Ok(0L) else CounterModel.price(price.text.toString())) {
            is Form.Refused -> msg.show(ui.words(f.words))
            is Form.Ok -> attempt(msg) { n.boxQueue(found.claim, f.value) }?.let { showBoxQueued(n, address, it) }
        }
    }
    screen("Box found", views, listOf(queue, ui.button("Back") { showConnectBox(prefill = address) }), back = { showConnectBox(prefill = address) })
}

/** The Box's four words, large, and what comes next. The approval is the request and response screens of OA-20 and OA-21. */
private fun Controller.showBoxQueued(n: OwnerNode, address: String, q: BoxQueued) {
    boxAddresses[keyHex(q.boxKey)] = address
    val msg = Slot(act)
    val views = ArrayList<View>()
    views.add(wordCards(ConnectBoxModel.wordsLine(q.words)))
    views.add(ui.words(BoxWords.queued))
    views.add(msg)
    screen(
        "Box queued", views,
        listOf(
            ui.primary("Build the request for the Key app") { showSpend() },
            ui.button("Later") { showHome() },
            ui.danger("Forget this Box") { forgetBox(n, q.boxKey, msg) { showHome() } },
        ),
        words = BoxWords.compare,
    )
}

/**
 * The Boxes that have a claim queued or a welcome waiting, each with its four words and a button that forgets
 * it (OA-47); one whose approved welcome is ready also has the address it was found at and a button that hands
 * the welcome over (OA-43). Empty when there is none.
 */
internal fun Controller.pendingBoxViews(n: OwnerNode, msg: Slot, back: () -> Unit): List<View> {
    val rows = attempt(msg) { n.boxesPending() } ?: return listOf(msg)
    if (rows.isEmpty()) return emptyList()
    val views = ArrayList<View>()
    views.add(label("Boxes being added"))
    for (p in rows) {
        val box = ArrayList<View>()
        box.add(ui.text(ConnectBoxModel.boxLabel(p.boxKey) + "\n" + ConnectBoxModel.wordsLine(p.words), 14f, mono = true))
        if (p.welcomeReady) {
            val address = ui.field("host:port or name.onion:port").also { it.setText(boxAddresses[keyHex(p.boxKey)] ?: "") }
            box.add(address)
            box.add(ui.primary("Hand over the welcome to ${ConnectBoxModel.boxLabel(p.boxKey)}") { handOver(n, p, address.text.toString(), back) })
        } else {
            box.add(ui.text("Waiting for the Key app's approval.", 14f, colour = ui.pal.secondary))
        }
        box.add(ui.danger("Forget ${ConnectBoxModel.boxLabel(p.boxKey)}") { forgetBox(n, p.boxKey, msg, back) })
        views.add(stack(*box.toTypedArray(), gap = 10))
    }
    views.add(msg)
    return views
}

/**
 * Forgets a Box that was not handed its welcome: its claim and settings leave the queue in one step. The Box is
 * not told; its invitation runs out by itself. A Box that is in an open session is refused with the library's
 * words, and a Box that took its welcome has nothing left to forget (OA-47).
 */
private fun Controller.forgetBox(n: OwnerNode, key: ByteArray, msg: Slot, back: () -> Unit) {
    if (attempt(msg) { n.boxForget(key) } == null) return
    boxAddresses.remove(keyHex(key))
    showMessage(BoxWords.forgotten, "Back") { back() }
}

/**
 * Hands the Box its welcome. The call takes the welcome out of the queue only when the Box took it; a refusal
 * changes nothing and the screen says what to do (OA-43). Only then is the link saved (OA-44).
 */
private fun Controller.handOver(n: OwnerNode, p: PendingBox, text: String, back: () -> Unit) {
    val address = when (val a = ConnectBoxModel.address(text)) {
        is Form.Refused -> {
            centre(a.words, listOf(ui.button("Back") { back() }))
            return
        }
        is Form.Ok -> a.value
    }
    val link = ConnectBoxModel.linkFor(address, proxyNow())
    waiting(BoxWords.handing)
    async({ n.boxDeliver(link, p.boxKey) }) { r ->
        val delivered = r.getOrNull()
        if (delivered != null) {
            boxAddresses.remove(keyHex(p.boxKey))
            showSaveLink(address, delivered, null)
        } else {
            val words = refusal(r.exceptionOrNull()!!)
            centre(
                words,
                listOf(ui.primary("Try again") { showConnectBox(prefill = address) }, ui.button("Back") { back() }),
                extra = listOf(ui.words(BoxWords.handoverKept)),
            )
        }
    }
}

/** What saving the link did, for the screen that follows. */
private enum class LinkSaved { Saved, NotValid }

/**
 * The Box has taken its welcome. The link to it is saved in the locked setup (OA-44): the PIN is asked for, the
 * library opens the stored setup with it, the link lines are put in, and the library locks the text again under
 * the same PIN; the file is replaced only when all that worked. The text is never stored unlocked. A wrong PIN
 * counts as at the unlock screen (OA-63). Without the PIN, or when the link cannot be saved, the lines are shown
 * to add by hand; the Box is paired either way.
 */
private fun Controller.showSaveLink(address: String, d: BoxDelivered, message: SayWords?) {
    val lines = ConnectBoxModel.linkLines(address, d.boxKey, d.counters)
    fun notSaved(words: SayWords) = centre(
        words, listOf(ui.button("Back") { showHome() }),
        extra = listOf(line(lines.joinToString("\n"), muted = true, mono = true)),
    )
    val pin = secret("", false)
    val views = ArrayList<View>()
    views.add(ui.labelled("PIN or passphrase", pin))
    val save =
        ui.button("Save the link", ButtonKind.Primary) {
            val typed = pin.text.toString()
            pin.setText("")
            if (typed.isEmpty()) return@button
            var outcome = LinkSaved.NotValid
            var next: OwnerSetup? = null
            triedPin({ showSaveLink(address, d, it) }, {
                val stored = setupFile().load() ?: throw IOException("the setup is missing")
                val text = OwnerLock.unlock(stored, typed)
                val edited = ConnectBoxModel.withLink(String(text, Charsets.UTF_8), address, d.boxKey, d.counters)
                text.fill(0)
                val parsed = try {
                    OwnerSetup.parse(edited)
                } catch (_: SetupFormatException) {
                    null
                }
                if (parsed != null) {
                    val bytes = edited.toByteArray(Charsets.UTF_8)
                    setupFile().save(OwnerLock.lock(typed, bytes))
                    bytes.fill(0)
                    next = parsed
                    outcome = LinkSaved.Saved
                }
            }) {
                when (outcome) {
                    LinkSaved.Saved -> {
                        // The node starts again from the setup just saved; the PIN was just typed, so it is not asked for again.
                        setup = next
                        showMessage(BoxWords.paired, "Start the link") { begin() }
                    }
                    LinkSaved.NotValid -> notSaved(BoxWords.linkNotSaved)
                }
            }
        }
    screen("Save the link", views, listOf(save, ui.button("Not now") { notSaved(BoxWords.linkNotSaved) }), words = message ?: BoxWords.savePin)
    if (shake) {
        shake = false
        pin.post { Feedback.shake(pin) }
    }
}
