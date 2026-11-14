package org.openbmt.owner.app

import android.content.ClipData
import android.content.ClipboardManager
import android.view.View
import java.text.DateFormat
import java.util.Date
import java.util.TimeZone
import org.openbmt.app.RowItem
import org.openbmt.owner.OwnerNode
import org.openbmt.owner.RequestAsk
import org.openbmt.owner.WebState

// The web store screens (`docs/WEB-ORDER-RULES.md`): the row under Settings and this phone, pairing a store, and the
// store's page. Every call into the library is made here on the screen thread; the visits to the store are made on the
// thread of `Controller.kt`, which sends what the node says to send through [webFetcher]. Every text is a placeholder
// of `WebOrderWords` until the words are written.

/** What sends the calls of a visit; null in the real app, which then sends through [HttpsWebFetcher]. A test puts its own here. */
internal var webFetcher: WebFetcher? = null

/** What every web order call is given: the time, the phone's offset from UTC in minutes east, and no rate. */
internal fun Controller.webAsk(): RequestAsk {
    val now = System.currentTimeMillis()
    return RequestAsk(now / 1000, TimeZone.getDefault().getOffset(now) / 60_000)
}

internal fun Controller.webRow() = RowItem(WebOrderWords.rowTitle, WebOrderWords.rowSub) { showWebStore() }

private inline fun Controller.withWebNode(block: (OwnerNode) -> Unit) {
    val n = node
    if (n == null) begin() else block(n)
}

private fun Controller.backHome(): android.widget.Button = ui.button(WebOrderWords.back) { showSettingsAndPhone() }

private fun shown(seconds: Long?): String? =
    seconds?.let { DateFormat.getDateTimeInstance(DateFormat.MEDIUM, DateFormat.SHORT).format(Date(it * 1000)) }

/** The store's page: no store yet, a code that waits for the owner, or the store as it stands (WO-05, WO-21, WO-22). */
internal fun Controller.showWebStore(notice: org.openbmt.app.SayWords? = null): Unit = withWebNode { n ->
    val msg = Slot(act)
    val state = attempt(msg) { n.webState(webAsk()) }
    val views = ArrayList<View>()
    notice?.let { views.add(ui.words(it)) }
    if (state == null) {
        views.add(msg)
        return@withWebNode screen(WebOrderWords.pageTitle, views, listOf(backHome()), back = { showSettingsAndPhone() })
    }
    state.off?.let { views.add(ui.words(it.say())) }
    when (WebModel.page(state)) {
        WebModel.Page.NoStore -> {
            views.add(ui.text(WebOrderWords.whatItIs))
            views.add(ui.text(WebOrderWords.noStore))
            val start = ui.primary(WebOrderWords.start) { showWebPair() }.also { it.isEnabled = state.off == null }
            screen(WebOrderWords.pageTitle, views, listOf(start, backHome()), back = { showSettingsAndPhone() })
        }
        WebModel.Page.Waiting -> {
            views.add(ui.text(WebOrderWords.waiting))
            val go = ui.primary(WebOrderWords.continuePairing) { showWebCode(state) }
            screen(WebOrderWords.pageTitle, views, listOf(go, backHome()), back = { showSettingsAndPhone() })
        }
        WebModel.Page.Paired -> showWebPaired(n, state, views)
    }
}

private fun Controller.showWebPaired(n: OwnerNode, state: WebState, views: ArrayList<View>) {
    views.add(ui.labelled(WebOrderWords.shopLabel, ui.text(state.shopName)))
    views.add(ui.labelled(WebOrderWords.storeLabel, ui.text(state.url)))
    views.add(ui.labelled(WebOrderWords.fingerprintLabel, ui.text(state.fingerprint)))
    views.add(ui.labelled(WebOrderWords.lastVisitLabel, ui.text(shown(state.lastVisit) ?: WebOrderWords.neverVisited)))
    shown(state.retryAt)?.let { views.add(ui.labelled(WebOrderWords.nextVisitLabel, ui.text(it))) }
    state.failure?.let { views.add(ui.words(it.say())) }
    state.trouble?.let { views.add(ui.words(it.say())) }
    views.add(ui.labelled(WebOrderWords.sentLabel, ui.text(state.addressesSent.toString())))
    state.left?.let { views.add(ui.labelled(WebOrderWords.leftLabel, ui.text(it.toString()))) }
    views.add(ui.labelled(WebOrderWords.ordersLabel, ui.text(state.orders.toString())))
    views.add(ui.labelled(WebOrderWords.waitingLabel, ui.text(state.waiting.toString())))
    views.add(ui.labelled(WebOrderWords.toldLabel, ui.text(state.told.toString())))
    views.add(ui.labelled(WebOrderWords.noticesLabel, ui.text(state.noticesWaiting.toString())))
    if (state.refused.isNotEmpty()) {
        views.add(ui.text(WebOrderWords.refusedTitle))
        for (r in state.refused) {
            views.add(ui.labelled(WebOrderWords.refusedOrderLabel, ui.text("${r.order}  ${r.address}  ${r.sats}")))
            views.add(ui.words(r.words.say()))
        }
    }
    val visit = ui.primary(WebOrderWords.visitNow) { webVisitNow() }.also { it.isEnabled = WebModel.canVisit(state) }
    val unpair = ui.danger(WebOrderWords.unpair) { showWebUnpair() }
    screen(WebOrderWords.pageTitle, views, listOf(visit, unpair, backHome()), back = { showSettingsAndPhone() })
    // The page is made again when a visit changed what it shows, and not otherwise.
    tick = {
        val now = try { n.webState(webAsk()) } catch (_: org.openbmt.owner.OwnerException) { null }
        if (now != null && WebModel.shownKey(now) != WebModel.shownKey(state)) showWebStore()
    }
}

/** WO-05, WO-06: the store's address and the shop's name; the library makes the key and the code. */
internal fun Controller.showWebPair(): Unit = withWebNode { n ->
    val url = ui.field(WebOrderWords.addressHint)
    val name = ui.field(WebOrderWords.nameLabel)
    val msg = Slot(act)
    val make = ui.primary(WebOrderWords.makeCode) {
        if (!WebModel.fieldsReady(url.text.toString(), name.text.toString())) {
            msg.show(ui.text(WebOrderWords.fieldsMissing))
        } else {
            val pair = attempt(msg) { n.webPairStart(webAsk(), url.text.toString().trim(), name.text.toString().trim()) }
            if (pair != null) showWebStore()
        }
    }
    screen(
        WebOrderWords.pairTitle,
        listOf(ui.labelled(WebOrderWords.addressLabel, url), ui.labelled(WebOrderWords.nameLabel, name), msg),
        listOf(make, ui.button(WebOrderWords.back) { showWebStore() }),
        back = { showWebStore() },
    )
}

/** WO-05: the code to paste into the store's settings, and the fingerprint the owner compares with the store's. */
private fun Controller.showWebCode(state: WebState) {
    val code = state.code ?: return showWebStore()
    val msg = Slot(act)
    val views = ArrayList<View>()
    views.add(ui.text(WebOrderWords.codePaste))
    views.add(ui.labelled(WebOrderWords.codeTitle, ui.text(code).also { it.setTextIsSelectable(true) }))
    views.add(
        ui.button(WebOrderWords.codeCopy) {
            act.getSystemService(ClipboardManager::class.java)?.setPrimaryClip(ClipData.newPlainText("", code))
            msg.show(ui.text(WebOrderWords.codeCopied))
        },
    )
    views.add(ui.labelled(WebOrderWords.fingerprintLabel, ui.text(state.fingerprint)))
    views.add(ui.text(WebOrderWords.compare))
    views.add(msg)
    val match = ui.primary(WebOrderWords.match) {
        withWebNode { n -> if (attempt(msg) { n.webPairConfirm(webAsk()) } != null) { webVisitNow(); showWebStore() } }
    }
    val differ = ui.danger(WebOrderWords.differ) {
        withWebNode { n -> if (attempt(msg) { n.webUnpair(webAsk()) } != null) showWebStore() }
    }
    screen(WebOrderWords.codeTitle, views, listOf(match, differ), back = { showWebStore() })
}

/** WO-07: unpairing forgets the key and the secret; the addresses given stay watched. */
private fun Controller.showWebUnpair() {
    val msg = Slot(act)
    val go = ui.danger(WebOrderWords.unpairConfirm) {
        withWebNode { n -> if (attempt(msg) { n.webUnpair(webAsk()) } != null) showWebStore() }
    }
    screen(
        WebOrderWords.unpairTitle,
        listOf(ui.text(WebOrderWords.unpairWhat), ui.text(WebOrderWords.unpairKeep), msg),
        listOf(go, ui.button(WebOrderWords.back) { showWebStore() }),
        back = { showWebStore() },
    )
}
