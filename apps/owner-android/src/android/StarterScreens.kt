package org.openbmt.owner.app

import android.view.View
import android.widget.Button
import android.widget.CheckBox
import android.widget.ImageView
import android.widget.LinearLayout
import java.io.File
import java.io.IOException
import org.openbmt.app.RowItem
import org.openbmt.app.SayWords
import org.openbmt.app.Tones
import org.openbmt.owner.OwnerException
import org.openbmt.owner.OwnerStarter
import org.openbmt.owner.ShopCurrency
import org.openbmt.owner.StarterSealed
import org.openbmt.owner.StarterWalletView

// The starter wallet's setup and restore (`docs/START-SIMPLE-RULES.md`): the plain words, the checks of the phone, the
// limit and the date, the words shown four at a time and checked back on the in-app keypad, the sealing under the
// Keystore key, and the restore when the phone has dropped that key. The words are in the Rust core and cross only as
// pictures (SS-11); no EditText, no clipboard and no log is used by a screen that shows or takes words, and each of
// them is built by [starterWordsScreen], which applies the flags of SS-12. What the app writes itself is a placeholder
// of `StarterWords`; what the core says is shown as it arrives. The use of the key is in `StarterUse.kt`.

/** What the setup holds between its screens, in memory only. */
internal class StarterDraft(val limitSats: Long, val moveBy: Long, val stopAbove: Boolean, val rolls: ByteArray) {
    var wallet: StarterWalletView? = null
    var positions: List<Long> = emptyList()

    fun wipe() = rolls.fill(0)
}

/** What the starter wallet keeps while the app runs. */
internal class StarterRuntime(base: File) {
    val store = StarterStore(base)
    val keystore by lazy { StarterKeystore() }

    /** The picture of four words on show, until it is cleared. */
    var image: StarterWordsImage? = null

    /** Whether the core holds a setup or a restore that is open (SS-10, SS-23). */
    var coreOpen = false
    var draft: StarterDraft? = null

    /** Set when the app comes into view: the upgrade screen is the first screen once (SS-37). */
    var gate = false

    /** The library's coins and confirmations; none yet (see `StarterUse.kt`). */
    var chain: StarterChain = NoChain
}

/** SS-11: clears the picture of words, and drops a setup or restore that is open. */
internal fun Controller.starterLeave() {
    starter.image?.clear()
    starter.image = null
    if (starter.coreOpen) {
        starter.coreOpen = false
        try {
            OwnerStarter.cancel()
        } catch (_: OwnerException) {
        } catch (_: LinkageError) {
        }
    }
    starter.draft?.wipe()
    starter.draft = null
}

/** The picture is cleared at once; the screen that follows is not one of words. */
private fun Controller.clearImage() {
    starter.image?.clear()
    starter.image = null
}

private fun Controller.words(w: SayWords): View = ui.words(w)

// ---- the entry ------------------------------------------------------------------------------------------------------

/** The row of the Money page. */
internal fun Controller.starterRow() = RowItem(StarterWords.rowTitle, StarterWords.rowSub) { showStarter() }

/** What the phone has of the starter wallet decides what the entry shows (SS-20, SS-23, SS-43). */
internal fun Controller.showStarter() {
    val store = starter.store
    val state = try {
        StarterState.of(store.hasSealed(), store.hasWrapped(), (store.hasSealed() && store.hasWrapped()) && starter.keystore.usable())
    } catch (e: IOException) {
        return centre(AppWords.fileNotRead(e.message), listOf(ui.button(StarterWords.backButton) { showHome() }), title = StarterWords.screenTitle)
    }
    when (state) {
        StarterKeyState.None -> if (store.hasWallet()) showStarterAfterMove() else showStarterPlain()
        StarterKeyState.Ready -> showStarterManage()
        StarterKeyState.KeyGone -> showStarterKeyGone()
    }
}

/** SS-24: without a screen lock the starter wallet cannot be made, and the screen says why. */
private fun Controller.noScreenLock() =
    centre(StarterWords.noLock, listOf(ui.button(StarterWords.backButton) { showHome() }), title = StarterWords.screenTitle, back = { showHome() })

// ---- the plain words (SS-13) ------------------------------------------------------------------------------------------

/** SS-13: before the words, one screen says what to do and what never to do with them. */
internal fun Controller.showStarterPlain() {
    val views = listOf(
        ui.text(StarterWords.plainPen, 17f),
        ui.text(StarterWords.plainNoPhoto, 17f),
        ui.text(StarterWords.plainNobodyAsks, 17f),
    )
    screen(
        StarterWords.plainTitle, views,
        listOf(ui.primary(StarterWords.plainContinue) { starterChecked(CheckStage.Making) { showStarterLimit() } }, ui.button(StarterWords.backButton) { showHome() }),
        back = { showHome() },
    )
}

// ---- the checks of the phone (SS-33, SS-34, SS-24) -------------------------------------------------------------------------

/**
 * Before the key is made, before its words are typed and before every payment: what the phone is checked for. Each thing
 * found is named with its explanation and the owner confirms again; a service that is not an accessibility tool refuses
 * the making and the typing of words. [next] is called when the step may go on.
 */
internal fun Controller.starterChecked(stage: CheckStage, back: () -> Unit = { showStarter() }, next: () -> Unit) {
    val result = StarterChecks.decide(starterEnvironment(), stage, today())
    when (result.verdict) {
        CheckVerdict.NoScreenLock -> return noScreenLock()
        CheckVerdict.Clear -> return next()
        else -> {}
    }
    val views = ArrayList<View>()
    for (f in result.findings) {
        views.add(ui.text(StarterChecks.name(f.kind) + (f.name?.let { ": $it" } ?: ""), 16f, bold = true))
        views.add(ui.text(StarterChecks.why(f.kind), 15f))
    }
    views.add(ui.text(StarterWords.overlayNote, 14f))
    val actions = ArrayList<Button>()
    if (result.verdict == CheckVerdict.Refused) {
        views.add(0, words(StarterWords.said(StarterWords.serviceNotToolRefused, Tones.STOP)))
    } else {
        views.add(0, ui.text(StarterWords.confirmAgain, 16f))
        actions.add(ui.primary(StarterWords.confirmButton) { if (StarterChecks.mayGoOn(result, true)) next() })
    }
    actions.add(ui.button(StarterWords.backButton) { back() })
    screen(StarterWords.checksTitle, views, actions, back = back)
}

// ---- the limit and the date (SS-35, SS-38, SS-03) ---------------------------------------------------------------------------

internal fun Controller.showStarterLimit(message: SayWords? = null) {
    val currency = try {
        node?.settings()?.currency ?: ShopCurrency.Usd
    } catch (_: OwnerException) {
        ShopCurrency.Usd
    }
    val bitcoinStandard = currency == ShopCurrency.Sats
    val note = Slot(act)
    val msg = Slot(act)
    message?.let { msg.show(words(it)) }
    val limit = ui.field(StarterWords.limitLabel).also { it.inputType = android.text.InputType.TYPE_CLASS_NUMBER }
    val price = ui.field(StarterWords.priceLabel).also { it.inputType = android.text.InputType.TYPE_CLASS_NUMBER }
    val date = ui.field(StarterWords.dateLabel).also { it.setText(StarterSetup.suggestedDate(nowSecs())) }
    val dice = ui.field(StarterWords.diceLabel)
    val stop = CheckBox(act).apply { text = StarterWords.stopLabel }
    val views = ArrayList<View>()
    views.add(ui.labelled(StarterWords.limitLabel, limit))
    if (!bitcoinStandard) {
        views.add(note)
        views.add(ui.labelled(StarterWords.priceLabel, price))
        fillPriceIn(note, price, currency, StarterWords.said(StarterWords.priceLabel), { StarterWords.said(StarterWords.priceLabel) }, StarterWords.said(StarterWords.priceLabel))
    }
    views.add(ui.labelled(StarterWords.dateLabel, date))
    views.add(stop)
    views.add(ui.labelled(StarterWords.diceLabel, dice))
    views.add(msg)
    val go = ui.primary(StarterWords.limitContinue) {
        val sats = StarterSetup.limitSats(limit.text.toString(), if (bitcoinStandard) null else price.text.toString().trim().replace(",", "").toLongOrNull())
        val by = StarterSetup.moveBy(date.text.toString(), nowSecs())
        val rolls = StarterSetup.rolls(dice.text.toString())
        when {
            sats == null -> msg.show(words(StarterWords.said(StarterWords.limitInvalid, Tones.STOP)))
            by == null -> msg.show(words(StarterWords.said(StarterWords.dateInvalid, Tones.STOP)))
            rolls == null -> msg.show(words(StarterWords.said(StarterWords.diceInvalid, Tones.STOP)))
            else -> {
                starter.draft?.wipe()
                starter.draft = StarterDraft(sats, by, stop.isChecked, rolls)
                makeStarter()
            }
        }
    }
    screen(StarterWords.limitTitle, views, listOf(go, ui.button(StarterWords.backButton) { showStarter() }), back = { showStarter() })
}

// ---- making the key (SS-01 to SS-04) -----------------------------------------------------------------------------------------

/** The core makes the words from the kernel's randomness; only the public wallet comes back. A refusal says why in the core's words. */
private fun Controller.makeStarter() {
    val draft = starter.draft ?: return showStarter()
    val wallet = try {
        starter.coreOpen = true
        OwnerStarter.make(draft.rolls)
    } catch (e: OwnerException) {
        starter.coreOpen = false
        return centre(e.sayWords(), listOf(ui.button(StarterWords.backButton) { showStarter() }), title = StarterWords.screenTitle)
    } catch (e: LinkageError) {
        starter.coreOpen = false
        return centre(AppWords.libraryMissing(e.message), listOf(ui.button(StarterWords.backButton) { showStarter() }), title = StarterWords.screenTitle)
    } finally {
        draft.wipe()
    }
    draft.wallet = wallet
    showStarterPage(0)
}

// ---- the words, four at a time (SS-10, SS-11, SS-12) -----------------------------------------------------------------------------

/**
 * A screen that shows or takes words: the picture or the keypad, with the flags of SS-12 applied once the screen is on
 * show. Every such screen is built here and nowhere else.
 */
private fun Controller.starterWordsScreen(title: String, body: List<View>, actions: List<Button>, back: (() -> Unit)?, image: StarterWordsImage? = null) {
    starter.image?.clear()
    starter.image = image
    tick = null
    val page = ui.page(ui.bar(title, null, back), body, actions)
    act.show(page)
    StarterWindow.secure(act, page)
}

/** One screen of four numbered words, drawn by the core, shown as a picture and cleared when the owner leaves it. */
internal fun Controller.showStarterPage(page: Int, message: SayWords? = null) {
    val image = ImageView(act)
    val holder = StarterWordsImage(act, image)
    try {
        val probe = OwnerStarter.page(page.toLong(), 1)
        val across = act.resources.displayMetrics.widthPixels - ui.px(48)
        val scale = (across.toLong() / probe.width.coerceAtLeast(1L)).coerceIn(1L, 16L)
        probe.wipe()
        holder.show(OwnerStarter.page(page.toLong(), scale))
    } catch (e: OwnerException) {
        return centre(e.sayWords(), listOf(ui.button(StarterWords.backButton) { starterCancel() }), title = StarterWords.screenTitle)
    } catch (e: LinkageError) {
        return centre(AppWords.libraryMissing(e.message), listOf(ui.button(StarterWords.backButton) { starterCancel() }), title = StarterWords.screenTitle)
    }
    val views = ArrayList<View>()
    message?.let { views.add(words(it)) }
    views.add(image)
    val next = StarterSetup.pageAfter(page)
    val forward = if (next != null) ui.primary(StarterWords.pageNext) { clearImage(); showStarterPage(next) }
    else ui.primary(StarterWords.pageWritten) { clearImage(); showStarterCheck() }
    val back = if (page > 0) ({ clearImage(); showStarterPage(page - 1) }) else ({ starterCancel() })
    starterWordsScreen(StarterWords.pageTitle, views, listOf(forward, ui.button(StarterWords.backButton) { back() }), back = back, image = holder)
}

/** Gives up the setup; its words are wiped by the core. */
private fun Controller.starterCancel() {
    starterLeave()
    showStarter()
}

/** SS-11: the keypad of letters. A key goes to [onKey] as its code and nothing is kept of it here. */
private fun Controller.starterKeypad(onKey: (Int) -> Unit): View {
    val column = LinearLayout(act).apply { orientation = LinearLayout.VERTICAL }
    fun key(label: String, code: Int): Button = ui.button(label) { onKey(code) }.also {
        it.minWidth = 0
        it.minimumWidth = 0
        it.setPadding(0, 0, 0, 0)
    }
    fun row(keys: List<Button>) {
        val r = LinearLayout(act).apply { orientation = LinearLayout.HORIZONTAL }
        for (k in keys) r.addView(k, LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1f).also { it.setMargins(ui.px(2), ui.px(2), ui.px(2), ui.px(2)) })
        column.addView(r, LinearLayout.LayoutParams(LinearLayout.LayoutParams.MATCH_PARENT, LinearLayout.LayoutParams.WRAP_CONTENT))
    }
    for (letters in StarterKeypad.rows) row(letters.map { c -> key(c.toString(), StarterKeypad.letterKey(c)) })
    row(listOf(key(StarterWords.keyBackspace, StarterKeypad.BACKSPACE), key(StarterWords.keyEnter, StarterKeypad.ENTER)))
    StarterWindow.sensitive(column)
    return column
}

/** The counts the core gave back for the last key, and nothing else: no letter is ever shown. */
private fun Controller.countsView(t: org.openbmt.owner.StarterTyped): View =
    ui.facts(listOf(StarterWords.checkTyped to t.typed.toString(), (if (t.accepted) StarterWords.checkAccepted else StarterWords.checkNotAccepted) to t.step.toString()))

/** SS-10: the positions the core picked from fresh kernel bytes are asked back on the keypad. */
internal fun Controller.showStarterCheck() {
    val draft = starter.draft ?: return showStarter()
    val positions = try {
        OwnerStarter.checkStart()
    } catch (e: OwnerException) {
        return centre(e.sayWords(), listOf(ui.button(StarterWords.backButton) { starterCancel() }), title = StarterWords.screenTitle)
    } catch (e: LinkageError) {
        return centre(AppWords.libraryMissing(e.message), listOf(ui.button(StarterWords.backButton) { starterCancel() }), title = StarterWords.screenTitle)
    }
    draft.positions = positions
    val ask = Slot(act)
    val counts = Slot(act)
    fun asking(step: Long) {
        val at = StarterKeypad.asked(positions, step)
        ask.show(ui.text(StarterWords.checkAsk + (at?.let { " $it" } ?: ""), 18f, bold = true))
    }
    asking(0)
    counts.show(ui.facts(listOf(StarterWords.checkTyped to "0")))
    val pad = starterKeypad { code ->
        val t = try {
            OwnerStarter.key(code)
        } catch (e: OwnerException) {
            counts.show(words(e.sayWords()))
            return@starterKeypad
        } catch (e: LinkageError) {
            counts.show(words(AppWords.libraryMissing(e.message)))
            return@starterKeypad
        }
        when {
            // A wrong answer shows the words again, from the first screen.
            t.failed -> showStarterPage(0, StarterWords.said(StarterWords.checkWrong, Tones.STOP))
            t.finished -> sealStarter()
            else -> {
                asking(t.step)
                counts.show(countsView(t))
            }
        }
    }
    starterWordsScreen(StarterWords.checkTitle, listOf(ask, counts, pad), listOf(ui.button(StarterWords.backButton) { starterCancel() }), back = { starterCancel() })
}

// ---- sealing (SS-20, SS-21) -----------------------------------------------------------------------------------------------------

/** The check is passed: the core seals the seed, the Keystore key wraps its data key after the owner proves they are there. */
private fun Controller.sealStarter() {
    val draft = starter.draft ?: return showStarter()
    val wallet = draft.wallet ?: return showStarter()
    val sealed = try {
        OwnerStarter.seal()
    } catch (e: OwnerException) {
        return centre(e.sayWords(), listOf(ui.button(StarterWords.backButton) { starterCancel() }), title = StarterWords.screenTitle)
    } catch (e: LinkageError) {
        return centre(AppWords.libraryMissing(e.message), listOf(ui.button(StarterWords.backButton) { starterCancel() }), title = StarterWords.screenTitle)
    }
    keepSealed(sealed, wallet, StarterRecord(draft.limitSats, draft.moveBy, draft.stopAbove, null)) { ok ->
        if (ok) {
            starterLeave()
            centre(StarterWords.sealed, listOf(ui.primary(StarterWords.doneButton) { showHome() }), title = StarterWords.screenTitle)
        } else {
            // The core keeps the setup until it is dropped, so the owner can try the sealing again.
            centre(StarterWords.notSaved, listOf(ui.primary(StarterWords.authSeal) { sealStarter() }, ui.button(StarterWords.backButton) { starterCancel() }), title = StarterWords.screenTitle)
        }
    }
}

/**
 * Keeps the sealed seed and the wrapped data key, then the public wallet and the record; the data key is overwritten
 * whatever happens. [done] says whether everything was kept; if not, no file of the key is left.
 */
private fun Controller.keepSealed(sealed: StarterSealed, wallet: StarterWalletView, record: StarterRecord?, done: (Boolean) -> Unit) {
    val store = starter.store
    try {
        store.saveSealed(sealed.sealed)
    } catch (_: IOException) {
        sealed.wipe()
        return done(false)
    }
    val dataKey = sealed.dataKey
    starterWrapKey(dataKey, StarterWords.authSeal) { kept ->
        sealed.wipe()
        val ok = kept && try {
            store.saveWallet(wallet)
            if (record != null) store.saveRecord(record)
            true
        } catch (_: IOException) {
            false
        }
        if (!ok) {
            store.deleteKey()
            starter.keystore.delete()
        }
        done(ok)
    }
}

// ---- when the phone drops the key (SS-23) -----------------------------------------------------------------------------------------

/**
 * The Keystore key is gone: the sealed seed cannot be opened. The core says so in its SS-20 words, which the app asks
 * for by giving it a data key that opens nothing; the restore is offered.
 */
internal fun Controller.showStarterKeyGone() {
    val sealed = starter.store.loadSealed()
    var said: SayWords = StarterWords.keyGoneFallback
    if (sealed != null) {
        try {
            OwnerStarter.open(sealed, ByteArray(32))
        } catch (e: OwnerException) {
            e.words?.let { said = it.say() }
        } catch (_: LinkageError) {
        }
    }
    centre(said, listOf(ui.primary(StarterWords.restoreButton) { starterChecked(CheckStage.Words) { showStarterRestore() } }, ui.button(StarterWords.backButton) { showHome() }), title = StarterWords.screenTitle, back = { showHome() })
}

/** The owner types the 12 words on the in-app keypad; the core checks them against the wallet's fingerprint and seals them again. */
internal fun Controller.showStarterRestore() {
    val wallet = try {
        starter.store.loadWallet()
    } catch (_: IOException) {
        null
    } ?: return centre(StarterWords.keyGoneFallback, listOf(ui.button(StarterWords.backButton) { showHome() }), title = StarterWords.screenTitle)
    try {
        starter.coreOpen = true
        OwnerStarter.restoreStart()
    } catch (e: OwnerException) {
        starter.coreOpen = false
        return centre(e.sayWords(), listOf(ui.button(StarterWords.backButton) { showStarter() }), title = StarterWords.screenTitle)
    } catch (e: LinkageError) {
        starter.coreOpen = false
        return centre(AppWords.libraryMissing(e.message), listOf(ui.button(StarterWords.backButton) { showStarter() }), title = StarterWords.screenTitle)
    }
    val counts = Slot(act)
    counts.show(ui.facts(listOf(StarterWords.checkTyped to "0")))
    val pad = starterKeypad { code ->
        val t = try {
            OwnerStarter.key(code)
        } catch (e: OwnerException) {
            counts.show(words(e.sayWords()))
            return@starterKeypad
        } catch (e: LinkageError) {
            counts.show(words(AppWords.libraryMissing(e.message)))
            return@starterKeypad
        }
        counts.show(countsView(t))
        if (t.finished) finishRestore(wallet)
    }
    val ask = ui.text(StarterWords.restoreAsk, 17f, bold = true)
    starterWordsScreen(StarterWords.restoreTitle, listOf(ask, counts, pad), listOf(ui.button(StarterWords.backButton) { starterCancel() }), back = { starterCancel() })
}

private fun Controller.finishRestore(wallet: StarterWalletView) {
    val sealed = try {
        OwnerStarter.restoreFinish(wallet.description)
    } catch (e: OwnerException) {
        // A refusal (not 12 words, or another wallet's) is said in the core's words and nothing was sealed.
        starterLeave()
        return centre(e.sayWords(), listOf(ui.primary(StarterWords.restoreButton) { starterChecked(CheckStage.Words) { showStarterRestore() } }, ui.button(StarterWords.backButton) { showHome() }), title = StarterWords.screenTitle)
    } catch (e: LinkageError) {
        starterLeave()
        return centre(AppWords.libraryMissing(e.message), listOf(ui.button(StarterWords.backButton) { showHome() }), title = StarterWords.screenTitle)
    }
    // A new Keystore key is made for the sealed seed (the old one is gone); the record of the limit is kept.
    keepSealed(sealed, wallet, null) { ok ->
        starterLeave()
        if (ok) centre(StarterWords.restored, listOf(ui.primary(StarterWords.doneButton) { showStarter() }), title = StarterWords.screenTitle)
        else centre(StarterWords.notSaved, listOf(ui.button(StarterWords.backButton) { showHome() }), title = StarterWords.screenTitle)
    }
}
