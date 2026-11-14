package org.openbmt.owner.app

import android.view.View
import android.widget.Button
import org.openbmt.app.RowItem
import org.openbmt.owner.OwnerException
import org.openbmt.owner.OwnerNode
import org.openbmt.owner.ShopLightningClass
import org.openbmt.owner.ShopLightningEditArgs
import org.openbmt.owner.ShopLightningHeld
import org.openbmt.owner.ShopLightningState

// Shop Lightning on the owner phone (SL-04, SL-45, SL-76 to SL-78): a row under Settings and this phone that opens a
// page. Off, it says what it is and offers Turn on, whose five steps pick the Lightning counter, scan the node's ID
// from the stick (SK-35), take the provider, take the cap and the most for one opening, and show the change with the
// approval its class needs (AP-13). On, it shows the facts of the newest report and the settings, and offers a change
// of the cap or the fee, of the provider, and Turn off. The node judges everything: the app turns what was typed or
// scanned into an edit, asks the node what it would do, shows the answer, and only then calls `queueSettings` with the
// bytes the node gave. This file signs nothing and writes no text of its own: every text is of [ShopLightningWords]
// and every refusal is the library's.

/** The row under Settings and this phone. */
internal fun Controller.shopLightningRow() = RowItem(ShopLightningWords.rowTitle, ShopLightningModel.rowSub(stateOrNull())) { showShopLightning() }

private fun Controller.stateOrNull(): ShopLightningState? = try {
    node?.shopLightningReport()
} catch (_: OwnerException) {
    null
} catch (_: LinkageError) {
    null
}

/** The node and its Shop Lightning state, or the first screen again, or the words of why not. */
private fun Controller.withState(back: () -> Unit, block: (OwnerNode, ShopLightningState) -> Unit) {
    val n = node
    if (n == null) {
        begin()
        return
    }
    val state = try {
        n.shopLightningReport()
    } catch (e: OwnerException) {
        screen(ShopLightningWords.pageTitle, listOf(ui.words(e.sayWords())), listOf(ui.button(ShopLightningWords.back) { back() }), back = { back() })
        return
    } catch (e: LinkageError) {
        screen(ShopLightningWords.pageTitle, listOf(ui.words(AppWords.libraryMissing(e.message))), listOf(ui.button(ShopLightningWords.back) { back() }), back = { back() })
        return
    }
    block(n, state)
}

internal fun Controller.showShopLightning(): Unit = withState({ showSettingsAndPhone() }) { _, state ->
    when (ShopLightningModel.page(state)) {
        ShopLightningModel.Page.OwnWallet -> screen(
            ShopLightningWords.pageTitle,
            listOf(ui.text(ShopLightningWords.whatItIs), ui.text(ShopLightningWords.ownWallet)),
            listOf(ui.button(ShopLightningWords.back) { showSettingsAndPhone() }),
            back = { showSettingsAndPhone() },
        )
        ShopLightningModel.Page.Off -> {
            val body = ArrayList<View>()
            body.add(ui.text(ShopLightningWords.whatItIs))
            if (!ShopLightningModel.canTurnOn(state)) body.add(ui.text(ShopLightningWords.noBox))
            val turnOn = ui.primary(ShopLightningWords.turnOn) {
                shopLightningDraft = ShopLightningModel.Draft()
                showSlCounter()
            }
            turnOn.isEnabled = ShopLightningModel.canTurnOn(state)
            screen(ShopLightningWords.pageTitle, body, listOf(turnOn, ui.button(ShopLightningWords.back) { showSettingsAndPhone() }), back = { showSettingsAndPhone() })
        }
        ShopLightningModel.Page.On -> showSlOn(state)
    }
}

// ---- on: the facts and the changes (SL-45, SL-78) ----------------------------------------------------------

private fun Controller.showSlOn(state: ShopLightningState) {
    val held = state.held ?: return showSettingsAndPhone()
    val body = ArrayList<View>()
    body.add(ui.text(ShopLightningWords.onTitle))
    for (line in ShopLightningModel.lines(held, state.report)) body.add(ui.labelled(line.label, ui.text(line.value)))
    body.add(
        ui.rows(
            listOf(
                RowItem(ShopLightningWords.changeLimits) { showSlLimits(held) },
                RowItem(ShopLightningWords.changeProvider) { showSlProvider(held) },
            ),
        ),
    )
    screen(
        ShopLightningWords.pageTitle,
        body,
        listOf(ui.danger(ShopLightningWords.turnOff) { showSlTurnOff() }, ui.button(ShopLightningWords.back) { showSettingsAndPhone() }),
        back = { showSettingsAndPhone() },
    )
}

private fun Controller.showSlLimits(held: ShopLightningHeld) {
    val msg = Slot(act)
    val cap = ui.field(ShopLightningWords.capLabel).also { it.inputType = android.text.InputType.TYPE_CLASS_NUMBER; it.setText(held.capSats.toString()) }
    val fee = ui.field(ShopLightningWords.feeLabel).also { it.inputType = android.text.InputType.TYPE_CLASS_NUMBER; it.setText(held.maxOpeningFeeSats.toString()) }
    val save = ui.primary(ShopLightningWords.save) {
        when (val r = ShopLightningModel.limits(cap.text.toString(), fee.text.toString(), held)) {
            is Form.Refused -> msg.show(ui.words(r.words))
            is Form.Ok -> showSlReview(r.value, { showSlLimits(held) })
        }
    }
    screen(
        ShopLightningWords.changeLimits,
        listOf(ui.text(ShopLightningWords.capWhat), ui.labelled(ShopLightningWords.capLabel, cap), ui.text(ShopLightningWords.feeWhat), ui.labelled(ShopLightningWords.feeLabel, fee), msg),
        listOf(save, ui.button(ShopLightningWords.back) { showShopLightning() }),
        back = { showShopLightning() },
    )
}

private fun Controller.showSlProvider(held: ShopLightningHeld) {
    val msg = Slot(act)
    val key = ui.field(ShopLightningWords.providerKeyLabel, lines = 2, mono = true).also { it.setText(ShopLightningModel.hex(held.providerNodeId)) }
    val addr = ui.field(ShopLightningWords.providerAddrLabel).also { it.setText(held.providerAddr) }
    val save = ui.primary(ShopLightningWords.save) {
        when (val r = ShopLightningModel.provider(key.text.toString(), addr.text.toString())) {
            is Form.Refused -> msg.show(ui.words(r.words))
            is Form.Ok -> showSlReview(r.value, { showSlProvider(held) })
        }
    }
    screen(
        ShopLightningWords.changeProvider,
        listOf(ui.text(ShopLightningWords.providerWhat), ui.labelled(ShopLightningWords.providerKeyLabel, key), ui.labelled(ShopLightningWords.providerAddrLabel, addr), msg),
        listOf(save, ui.button(ShopLightningWords.back) { showShopLightning() }),
        back = { showShopLightning() },
    )
}

/** SL-77: turning off closes the channels; the page says so before anything is queued. */
private fun Controller.showSlTurnOff() {
    screen(
        ShopLightningWords.turnOff,
        listOf(ui.text(ShopLightningWords.turnOffWhat)),
        listOf(
            ui.danger(ShopLightningWords.turnOffConfirm) { showSlReview(ShopLightningModel.turnOff(), { showSlTurnOff() }) },
            ui.button(ShopLightningWords.turnOffKeep) { showShopLightning() },
        ),
        back = { showShopLightning() },
    )
}

// ---- the turn-on steps (SL-76) ------------------------------------------------------------------------------

/** Step 1: the Lightning counter, picked from the shop's counters (SL-03, SL-70). */
private fun Controller.showSlCounter(message: org.openbmt.app.SayWords? = null) = withState({ showShopLightning() }) { _, state ->
    val body = ArrayList<View>()
    message?.let { body.add(ui.words(it)) }
    body.add(ui.text(ShopLightningModel.stepLine(ShopLightningModel.Step.Counter)))
    body.add(ui.text(ShopLightningWords.counterWhat))
    val rows = state.counters.map { c ->
        RowItem(OwnerModel.counterLabel(c)) {
            shopLightningDraft.counter = c
            showSlNode()
        }
    }
    body.add(if (rows.isEmpty()) ui.text(ShopLightningWords.noCounters) else ui.rows(rows))
    screen(ShopLightningWords.counterTitle, body, listOf(ui.button(ShopLightningWords.back) { showShopLightning() }), back = { showShopLightning() })
}

/** Step 2: the node's ID, scanned from the stick's code (SK-35); never from a counter (SL-76). */
private fun Controller.showSlNode(message: org.openbmt.app.SayWords? = null) {
    val body = ArrayList<View>()
    message?.let { body.add(ui.words(it)) }
    body.add(ui.text(ShopLightningModel.stepLine(ShopLightningModel.Step.Node)))
    body.add(ui.text(ShopLightningWords.nodeWhat))
    if (shopLightningDraft.nodeId.isNotEmpty()) {
        body.add(ui.labelled(ShopLightningWords.nodeLabel, ui.text(ShopLightningModel.short(shopLightningDraft.nodeId))))
        body.add(ui.text(ShopLightningWords.nodeScanned))
    }
    val actions = ArrayList<Button>()
    if (shopLightningDraft.nodeId.isNotEmpty()) actions.add(ui.primary(ShopLightningWords.next) { showSlProviderStep() })
    actions.add(if (shopLightningDraft.nodeId.isEmpty()) ui.primary(ShopLightningWords.scanNode) { scanSlNode() } else ui.button(ShopLightningWords.scanNode) { scanSlNode() })
    actions.add(ui.button(ShopLightningWords.back) { showSlCounter() })
    screen(ShopLightningWords.nodeTitle, body, actions, back = { showSlCounter() })
}

/** The stick's code, read by the camera, which is asked for only when the button is pressed. */
private fun Controller.scanSlNode() {
    fun give(r: SourceResult) = when (r) {
        is SourceResult.Bytes -> when (val id = ShopLightningModel.nodeId(r.bytes)) {
            is Form.Ok -> {
                shopLightningDraft.nodeId = id.value
                showSlNode()
            }
            is Form.Refused -> showSlNode(id.words)
        }
        is SourceResult.Failed -> showSlNode(r.words)
    }
    scan.begin(
        show = { v -> showScan(v) },
        onBytes = { give(SourceResult.Bytes(it)) },
        onFailed = { f -> give(ScanResult.failed(f.off, f.problem, f.words)) },
        onCancel = { give(SourceResult.Failed(ShopLightningWords.nodeScanCancelledWords)) },
    )
}

/** Step 3: the provider's node ID and its IP address and port (SL-28, SL-32). */
private fun Controller.showSlProviderStep(message: org.openbmt.app.SayWords? = null) {
    val msg = Slot(act)
    message?.let { msg.show(ui.words(it)) }
    val key = ui.field(ShopLightningWords.providerKeyLabel, lines = 2, mono = true).also { it.setText(shopLightningDraft.providerKey) }
    val addr = ui.field(ShopLightningWords.providerAddrLabel).also { it.setText(shopLightningDraft.providerAddr) }
    val next = ui.primary(ShopLightningWords.next) {
        shopLightningDraft.providerKey = key.text.toString()
        shopLightningDraft.providerAddr = addr.text.toString()
        if (shopLightningDraft.providerKey.isBlank() || shopLightningDraft.providerAddr.isBlank()) msg.show(ui.words(ShopLightningWords.providerMissingWords)) else showSlLimitsStep()
    }
    screen(
        ShopLightningWords.providerTitle,
        listOf(ui.text(ShopLightningModel.stepLine(ShopLightningModel.Step.Provider)), ui.text(ShopLightningWords.providerWhat), ui.labelled(ShopLightningWords.providerKeyLabel, key), ui.labelled(ShopLightningWords.providerAddrLabel, addr), msg),
        listOf(next, ui.button(ShopLightningWords.back) { showSlNode() }),
        back = { showSlNode() },
    )
}

/** Step 4: the cap and the most for one opening (SL-34, SL-29). */
private fun Controller.showSlLimitsStep() {
    val msg = Slot(act)
    val cap = ui.field(ShopLightningWords.capLabel).also { it.inputType = android.text.InputType.TYPE_CLASS_NUMBER; it.setText(shopLightningDraft.cap) }
    val fee = ui.field(ShopLightningWords.feeLabel).also { it.inputType = android.text.InputType.TYPE_CLASS_NUMBER; it.setText(shopLightningDraft.fee) }
    val next = ui.primary(ShopLightningWords.next) {
        shopLightningDraft.cap = cap.text.toString()
        shopLightningDraft.fee = fee.text.toString()
        when (val r = ShopLightningModel.turnOn(shopLightningDraft)) {
            is Form.Refused -> msg.show(ui.words(r.words))
            is Form.Ok -> showSlReview(r.value, { showSlLimitsStep() })
        }
    }
    screen(
        ShopLightningWords.limitsTitle,
        listOf(ui.text(ShopLightningModel.stepLine(ShopLightningModel.Step.Limits)), ui.text(ShopLightningWords.capWhat), ui.labelled(ShopLightningWords.capLabel, cap), ui.text(ShopLightningWords.feeWhat), ui.labelled(ShopLightningWords.feeLabel, fee), msg),
        listOf(next, ui.button(ShopLightningWords.back) { showSlProviderStep() }),
        back = { showSlProviderStep() },
    )
}

// ---- the change and its approval (AP-13) -------------------------------------------------------------------------

/**
 * Step 5, and the last step of every change: the node previews the edit and the screen shows what the class of the
 * change needs. Free changes are signed by this phone alone; the others wait for the Key app, whose request is built
 * on the Spend page as for any change of the settings. The library's refusal is shown as its own words.
 */
private fun Controller.showSlReview(edit: ShopLightningEditArgs, back: () -> Unit) {
    val n = node
    if (n == null) {
        begin()
        return
    }
    val p = try {
        n.shopLightningPreview(edit)
    } catch (e: OwnerException) {
        screen(ShopLightningWords.approveTitle, listOf(ui.words(e.sayWords())), listOf(ui.button(ShopLightningWords.back) { back() }), back = { back() })
        return
    } catch (e: LinkageError) {
        screen(ShopLightningWords.approveTitle, listOf(ui.words(AppWords.libraryMissing(e.message))), listOf(ui.button(ShopLightningWords.back) { back() }), back = { back() })
        return
    }
    val msg = Slot(act)
    val body = ArrayList<View>()
    body.add(ui.text(ShopLightningModel.stepLine(ShopLightningModel.Step.Approve)))
    p.after?.let { after -> for (line in ShopLightningModel.lines(after, null).dropLast(1)) body.add(ui.labelled(line.label, ui.text(line.value))) }
    val needs = when (p.changeClass) {
        ShopLightningClass.Free -> ShopLightningWords.approveFree
        ShopLightningClass.Loosen -> ShopLightningWords.approveLoosen
        ShopLightningClass.Redirect -> ShopLightningWords.approveRedirect
    }
    val actions = ArrayList<Button>()
    if (p.nothingChanged) {
        body.add(ui.text(ShopLightningWords.nothingChanged))
    } else {
        body.add(ui.text(needs))
        actions.add(
            ui.primary(ShopLightningWords.queue) {
                val q = attempt(msg) { n.queueSettings(p.settings) } ?: return@primary
                shopLightningDraft = ShopLightningModel.Draft()
                if (q.tightening) {
                    screen(ShopLightningWords.approveTitle, listOf(ui.text(ShopLightningWords.queuedDone)), listOf(ui.primary(ShopLightningWords.back) { showShopLightning() }), back = { showShopLightning() })
                } else {
                    screen(
                        ShopLightningWords.approveTitle,
                        listOf(ui.text(ShopLightningWords.queuedKeyApp)),
                        listOf(ui.primary(ShopLightningWords.buildRequest) { showSpend() }, ui.button(ShopLightningWords.later) { showShopLightning() }),
                        back = { showShopLightning() },
                    )
                }
            },
        )
    }
    body.add(msg)
    actions.add(ui.button(ShopLightningWords.back) { back() })
    screen(ShopLightningWords.approveTitle, body, actions, back = { back() })
}
