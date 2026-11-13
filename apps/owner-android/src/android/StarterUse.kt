package org.openbmt.owner.app

import android.view.View
import java.io.IOException
import org.openbmt.app.Format
import org.openbmt.app.RowItem
import org.openbmt.app.SayWords
import org.openbmt.app.Tones
import org.openbmt.owner.OwnerException
import org.openbmt.owner.OwnerNode
import org.openbmt.owner.OwnRepinKind
import org.openbmt.owner.OwnerStarter
import org.openbmt.owner.ReceiptsKind
import org.openbmt.owner.RefundRow
import org.openbmt.owner.StageKind
import org.openbmt.owner.StarterChangeKind
import org.openbmt.owner.StarterPaid

// Using the starter wallet (`docs/START-SIMPLE-RULES.md`): the home line (SS-36), the upgrade screen and "Not yet"
// (SS-37), what waits (SS-37), a refund paid from it (SS-30, SS-31), the one move to the new wallet (SS-42) and the
// deleting of the key (SS-43). The library decides the state of the limit and signs; this file shows what the owner is
// to see before they prove they are there, and carries their choices. The data key is opened only for one call and
// overwritten (`StarterKeys.kt`).

/** The least confirmations of a coin that a payment of the starter wallet spends. */
private const val MIN_CONFIRMATIONS = 1L

private fun Controller.words(w: SayWords): View = ui.words(w)

internal fun Controller.starterRecord(): StarterRecord? = try {
    starter.store.loadRecord()
} catch (_: IOException) {
    null
}

// ---- the home line and the upgrade screen (SS-36, SS-37) -----------------------------------------------------------------

/** Where the wallet stands, from what it holds now (confirmed coins and payments on their way, as the node's watch counts them); null when there is no starter key on the phone. */
internal fun Controller.starterLine(n: OwnerNode): HomeLine? {
    if (!starter.store.hasSealed()) return null
    val record = starterRecord() ?: return null
    val held = try {
        n.starterHeld()
    } catch (_: OwnerException) {
        return null
    }
    return try {
        StarterHome.line(LibraryStarter, record, held, nowSecs(), StarterCoreWords.line(LibraryStarter)) { StarterCoreWords.state(LibraryStarter, it) }
    } catch (_: OwnerException) {
        null
    } catch (_: LinkageError) {
        null
    }
}

/** SS-36: the line of the home screen. It has no way to be dismissed. */
internal fun Controller.starterLineView(line: HomeLine): View {
    val slot = Slot(act, 12)
    slot.show(words(line.words), ui.facts(line.facts))
    return slot
}

/**
 * SS-37: once each time the app comes into view, the upgrade screen is the first screen when the wallet is at its limit
 * or past its date and no "Not yet" holds. The flag stays until the node has checked some blocks, so that a wallet
 * that has not been counted yet does not pass for empty. True when the upgrade screen was shown.
 */
internal fun Controller.starterGate(n: OwnerNode): Boolean {
    if (!starter.gate) return false
    val line = starterLine(n)
    if (line == null) {
        starter.gate = false
        return false
    }
    if (line.opensUpgrade) {
        starter.gate = false
        showStarterUpgrade(line)
        return true
    }
    val synced = try {
        n.status().checkedHeight > 0
    } catch (_: OwnerException) {
        false
    }
    if (synced) starter.gate = false
    return false
}

internal fun Controller.showStarterUpgrade(line: HomeLine) {
    screen(
        StarterWords.upgradeTitle, listOf(ui.facts(line.facts)),
        listOf(ui.primary(StarterWords.upgradeNow) { showStarterMove() }, ui.button(StarterWords.notYet) { showStarterNotYet() }),
        words = line.words, logo = true,
    )
}

/** SS-37: "Not yet" needs the sentence typed exactly, and lasts at most the library's 7 days. */
internal fun Controller.showStarterNotYet() {
    val msg = Slot(act)
    val typed = ui.field(StarterWords.notYetSentence)
    val views = listOf(ui.text(StarterWords.notYetAsk, 16f), ui.text(StarterWords.notYetSentence, 16f, bold = true), typed, msg)
    val go = ui.primary(StarterWords.notYetConfirm) {
        val record = starterRecord()
        val until = try {
            StarterNotYet.grant(LibraryStarter, typed.text.toString(), StarterWords.notYetSentence, nowSecs())
        } catch (e: OwnerException) {
            return@primary msg.show(words(e.sayWords()))
        } catch (e: LinkageError) {
            return@primary msg.show(words(AppWords.libraryMissing(e.message)))
        }
        if (until == null || record == null) {
            msg.show(words(StarterWords.said(StarterWords.notYetDiffers, Tones.STOP)))
        } else try {
            starter.store.saveRecord(record.withNotYet(until))
            centre(StarterWords.said(StarterWords.notYetGranted, Tones.INFO), listOf(ui.primary(StarterWords.doneButton) { showHome() }), title = StarterWords.screenTitle)
        } catch (e: IOException) {
            msg.show(words(AppWords.fileNotSaved(e.message)))
        }
    }
    screen(StarterWords.upgradeTitle, views, listOf(go, ui.button(StarterWords.backButton) { starterBackToUpgrade() }), back = { starterBackToUpgrade() })
}

private fun Controller.starterBackToUpgrade() {
    val n = node ?: return begin()
    val line = starterLine(n) ?: return showHome()
    showStarterUpgrade(line)
}

/** SS-37: whether a change of the settings waits in the state the wallet is in; the library decides. */
internal fun Controller.starterHolds(change: StarterChangeKind): Boolean {
    val n = node ?: return false
    val line = starterLine(n) ?: return false
    return try {
        LibraryStarter.changeHeld(line.state, change)
    } catch (_: OwnerException) {
        false
    } catch (_: LinkageError) {
        false
    }
}

/** SS-37: the screen a change that waits shows, in the core's words. */
internal fun Controller.showStarterHeld(back: () -> Unit) =
    centre(StarterCoreWords.held(LibraryStarter), listOf(ui.button(StarterWords.backButton) { back() }), title = StarterWords.screenTitle, back = back)

// ---- the page of the starter wallet ----------------------------------------------------------------------------------------------

internal fun Controller.showStarterManage() {
    val n = node ?: return begin()
    val views = ArrayList<View>()
    starterLine(n)?.let { views.add(starterLineView(it)) }
    val rows = arrayListOf(
        RowItem(StarterWords.payTitle) { showStarterPay() },
        RowItem(StarterWords.moveTitle) { showStarterMove() },
    )
    if (starter.store.loadPendingTxid() != null) rows.add(RowItem(StarterWords.deleteTitle) { showStarterDelete() })
    views.add(ui.rows(rows))
    screen(StarterWords.screenTitle, views, listOf(ui.button(StarterWords.backButton) { showHome() }), back = { showHome() })
}

/** SS-43: after the key is deleted a payment that arrives later is shown with the offer to restore and move it (SS-23). */
internal fun Controller.showStarterAfterMove() {
    centre(
        StarterCoreWords.deleted(LibraryStarter),
        listOf(ui.primary(StarterWords.restoreButton) { starterChecked(CheckStage.Words) { showStarterRestore() } }, ui.button(StarterWords.backButton) { showHome() }),
        title = StarterWords.screenTitle, back = { showHome() },
    )
}

// ---- paying a refund (SS-30, SS-31) ---------------------------------------------------------------------------------------------------

private fun Controller.recentRate(n: OwnerNode): Long? = try {
    val r = n.receiptsLine()
    if (r.kind == ReceiptsKind.Known && r.ratePerKvb > 0) r.ratePerKvb else null
} catch (_: OwnerException) {
    null
}

internal fun Controller.showStarterPay() {
    val n = node ?: return begin()
    val msg = Slot(act)
    val queue = attempt(msg) { n.queue() }
    val refunds = queue?.refunds.orEmpty().filter { it.stage.kind == StageKind.Queued }
    val views = ArrayList<View>()
    if (refunds.isEmpty()) views.add(ui.text(StarterWords.payEmpty, 16f))
    else views.add(ui.rows(refunds.map { r -> RowItem(Format.sats(r.sats), r.address) { starterChecked(CheckStage.Paying, back = { showStarterPay() }) { starterPayConfirm(n, r) } } }))
    views.add(msg)
    screen(StarterWords.payTitle, views, listOf(ui.button(StarterWords.backButton) { showStarter() }), back = { showStarter() })
}

/** SS-31: every address in full and every amount before the owner proves they are there. */
private fun Controller.starterPayConfirm(n: OwnerNode, r: RefundRow) {
    val rate = try {
        OwnerStarter.feeRate(recentRate(n), null)
    } catch (e: OwnerException) {
        return centre(e.sayWords(), listOf(ui.button(StarterWords.backButton) { showStarterPay() }), title = StarterWords.payTitle)
    } catch (e: LinkageError) {
        return centre(AppWords.libraryMissing(e.message), listOf(ui.button(StarterWords.backButton) { showStarterPay() }), title = StarterWords.payTitle)
    }
    val msg = Slot(act)
    val views = listOf(ui.facts(StarterPayModel.beforeLines(r.address, r.sats, rate)), ui.text(StarterWords.payAuthorise, 16f), msg)
    val go = ui.primary(StarterWords.authPay) {
        starterSign(n, msg, false, StarterWords.authPay, { showStarterPay() }, listOf(r.id), null) { sealed, key, _ ->
            n.starterPay(sealed, key, r.id, rate, MIN_CONFIRMATIONS)
        }
    }
    screen(StarterWords.payTitle, views, listOf(go, ui.button(StarterWords.backButton) { showStarterPay() }), back = { showStarterPay() })
}

/**
 * Opens the data key for one call after the owner proves they are there, signs, and shows the signed payment. A key that
 * the phone dropped goes to the restore (SS-23). [credentialOnly]: the final move asks for the phone's PIN (SS-21).
 */
private fun Controller.starterSign(
    n: OwnerNode,
    msg: Slot,
    credentialOnly: Boolean,
    description: String,
    back: () -> Unit,
    refunds: List<Long>,
    newWallet: String?,
    sign: (sealed: ByteArray, key: ByteArray, record: StarterRecord) -> StarterPaid,
) {
    val sealed = starter.store.loadSealed()
    val record = starterRecord()
    if (sealed == null || record == null) return showStarter()
    starterWithDataKey(
        credentialOnly, description,
        use = { key ->
            val paid = try {
                sign(sealed, key, record)
            } catch (e: OwnerException) {
                return@starterWithDataKey msg.show(words(e.sayWords()))
            } catch (e: LinkageError) {
                return@starterWithDataKey msg.show(words(AppWords.libraryMissing(e.message)))
            }
            showStarterSigned(n, paid, record, credentialOnly, refunds, newWallet, back)
        },
        gone = { showStarterKeyGone() },
        failed = { msg.show(words(StarterWords.said(StarterWords.authFailed, Tones.STOP))) },
    )
}

/** What was signed, shown in full before it goes out: every address, every amount, the fee and the change. */
private fun Controller.showStarterSigned(
    n: OwnerNode,
    paid: StarterPaid,
    record: StarterRecord,
    isMove: Boolean,
    refunds: List<Long>,
    newWallet: String?,
    back: () -> Unit,
) {
    val msg = Slot(act)
    val send = ui.primary(StarterWords.paySendButton) {
        try {
            // The move is kept first, so that its confirmations are asked of the node by its ID and the new wallet is
            // there for the pinned wallet to change to; then the node's outbox takes it, and the app sends nothing.
            if (isMove) {
                starter.store.savePending(paid)
                newWallet?.let { starter.store.saveNewWallet(it) }
            }
            if (paid.changeIndex != null) starter.store.saveRecord(record.withNextChange())
            n.starterSend(paid.tx, refunds)
            centre(
                StarterWords.signed,
                listOf(ui.primary(StarterWords.doneButton) { if (isMove) showStarterDelete() else showStarter() }),
                extra = listOf(ui.text(paid.txid, 14f, mono = true)), title = StarterWords.screenTitle,
            )
        } catch (e: OwnerException) {
            msg.show(words(e.sayWords()))
        } catch (e: IOException) {
            msg.show(words(AppWords.fileNotSaved(e.message)))
        }
    }
    screen(StarterWords.payTitle, listOf(ui.facts(StarterPayModel.afterLines(paid)), words(StarterWords.signed), msg), listOf(send, ui.button(StarterWords.backButton) { back() }), back = back)
}

// ---- the move (SS-42) and the end of the key (SS-43) ----------------------------------------------------------------------------------------

/** SS-42: everything the starter wallet holds goes to the new wallet's next address, in one payment, with the phone's PIN. */
internal fun Controller.showStarterMove() {
    val n = node ?: return begin()
    starterChecked(CheckStage.Paying, back = { showStarter() }) {
        val msg = Slot(act)
        val wallet = ui.field(StarterWords.moveWalletLabel, lines = 3, mono = true)
        val views = listOf(ui.text(StarterWords.moveEverything, 16f), ui.labelled(StarterWords.moveWalletLabel, wallet), msg)
        val go = ui.primary(StarterWords.authMove) {
            val description = wallet.text.toString().trim()
            if (description.isEmpty()) return@primary msg.show(words(StarterWords.said(StarterWords.moveInvalid, Tones.STOP)))
            starterMoveConfirm(n, description)
        }
        screen(StarterWords.moveTitle, views, listOf(go, ui.button(StarterWords.backButton) { showStarter() }), back = { showStarter() })
    }
}

private fun Controller.starterMoveConfirm(n: OwnerNode, newWallet: String) {
    val rate = try {
        OwnerStarter.feeRate(recentRate(n), null)
    } catch (e: OwnerException) {
        return centre(e.sayWords(), listOf(ui.button(StarterWords.backButton) { showStarterMove() }), title = StarterWords.moveTitle)
    } catch (e: LinkageError) {
        return centre(AppWords.libraryMissing(e.message), listOf(ui.button(StarterWords.backButton) { showStarterMove() }), title = StarterWords.moveTitle)
    }
    val msg = Slot(act)
    val views = listOf(
        ui.facts(StarterPayModel.moveBeforeLines(newWallet, rate)), ui.text(StarterWords.moveCredentialOnly, 16f), msg,
    )
    val go = ui.primary(StarterWords.authMove) {
        starterSign(n, msg, true, StarterWords.authMove, { showStarterMove() }, emptyList(), newWallet) { sealed, key, _ ->
            n.starterMoveSwitched(sealed, key, newWallet, nowSecs(), rate, MIN_CONFIRMATIONS)
        }
    }
    screen(StarterWords.moveTitle, views, listOf(go, ui.button(StarterWords.backButton) { showStarterMove() }), back = { showStarterMove() })
}

/** SS-43: once the move has one confirmation the sealed seed, the wrapped key and the Keystore key are deleted. */
internal fun Controller.showStarterDelete() {
    val n = node ?: return begin()
    val txid = starter.store.loadPendingTxid() ?: return showStarter()
    val confirmations = try {
        n.starterConfirmations(txid)
    } catch (_: OwnerException) {
        null
    }
    val ready = try {
        StarterPayModel.mayDelete(LibraryStarter, confirmations)
    } catch (_: OwnerException) {
        false
    } catch (_: LinkageError) {
        false
    }
    val views = ArrayList<View>()
    views.add(ui.text(txid, 14f, mono = true))
    views.add(ui.text(if (confirmations == null) StarterWords.deleteUnknown else StarterWords.deleteWaiting, 16f))
    val actions = ArrayList<android.widget.Button>()
    if (ready) {
        actions.add(
            ui.danger(StarterWords.deleteButton) {
                starter.store.deleteKey()
                starter.keystore.delete()
                starterRepin(n)
                centre(StarterCoreWords.deleted(LibraryStarter), listOf(ui.primary(StarterWords.doneButton) { showHome() }), title = StarterWords.screenTitle)
            },
        )
    }
    actions.add(ui.button(StarterWords.backButton) { showStarter() })
    screen(StarterWords.deleteTitle, views, actions, back = { showStarter() })
}

/**
 * SS-41: once every counter has switched, the owner's pinned wallet becomes the new wallet, which the node watches from its
 * next start. It is asked after the move has its confirmation and the key is gone, because until then the starter wallet's
 * coins are read from the pinned wallet. A counter that has not switched leaves it as it is; the call is made again at the
 * next visit.
 */
private fun Controller.starterRepin(n: OwnerNode) {
    val newWallet = try {
        starter.store.loadNewWallet()
    } catch (_: IOException) {
        null
    } ?: return
    try {
        val done = n.ownRepin(newWallet)
        if (done.kind != OwnRepinKind.NotReady) ownStore.saveWallet(done.wallet)
    } catch (_: OwnerException) {
    } catch (_: IOException) {
    } catch (_: LinkageError) {
    }
}
