package org.openbmt.owner.app

import android.text.InputType
import android.view.View
import android.widget.EditText
import java.io.IOException
import java.security.GeneralSecurityException
import org.openbmt.app.ButtonKind
import org.openbmt.app.KeystoreSealer
import org.openbmt.app.SayWords
import org.openbmt.app.SealedFile
import org.openbmt.app.Tries
import org.openbmt.app.WipeOrder
import org.openbmt.owner.OwnerException
import org.openbmt.owner.OwnerLock

// The PIN lock on the owner phone's setup (OA-60 to OA-65). The setup file is `OBMx(setup text)` sealed by the
// Keystore: the phone and the PIN are both needed. The library locks, opens and changes (`OwnerLock`, which
// judges the PIN and runs Argon2id) and stores nothing; this file stores what it returns, counts the wrong
// tries and wipes (the shared `LockModel.kt`), and shows the screens. The words of a refusal are the library's.

/** The sealed setup file: the Keystore's seal around the library's locked setup. */
internal fun Controller.setupFile() = SealedFile(appDir, KeystoreSealer(AppFiles.KEY_ALIAS), AppFiles.SETUP_FILE)

internal fun Controller.stopTicker() {
    ticker?.let { main.removeCallbacks(it) }
    ticker = null
}

/** The parsed setup and the text it was made from are dropped: the next start asks for the PIN (OA-62). */
internal fun Controller.lockNow() {
    setup = null
}

/** A field for a PIN or a passphrase: its letters are hidden, and no keyboard may learn or suggest them. */
internal fun Controller.secret(hint: String, numeric: Boolean): EditText = ui.field(hint).also {
    it.inputType = if (numeric) {
        InputType.TYPE_CLASS_NUMBER or InputType.TYPE_NUMBER_VARIATION_PASSWORD
    } else {
        InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_FLAG_NO_SUGGESTIONS or InputType.TYPE_TEXT_VARIATION_PASSWORD
    }
    // A password type makes the font monospace; the system sans again.
    ui.style(it)
}

/**
 * The page is drawn first, then [block] runs a moment later on the screen thread: the key from the PIN takes
 * about a second, and the person sees that the app is working and not stuck (OA-62, OA-12).
 */
private fun Controller.slow(block: () -> Unit) {
    page(listOf(ui.words(AppWords.working)))
    main.postDelayed({ block() }, 100)
}

// ---- the start ---------------------------------------------------------------------------------------------

/**
 * Reads the stored setup and shows what comes before the node: the unlock screen for a locked one, the step
 * that sets a PIN for a bare one saved by an older build, and, when the count of wrong tries is at the wipe
 * count, the wipe (OA-61, OA-62, OA-64). The Keystore opens its seal here; the PIN is not yet asked for.
 */
internal fun Controller.openStored() {
    val stored = try {
        setupFile().load() ?: throw IOException("the setup is missing")
    } catch (e: IOException) {
        fault = AppWords.storageUnavailable(e.message)
        return render()
    } catch (e: GeneralSecurityException) {
        fault = AppWords.storageUnavailable(e.message)
        return render()
    }
    val wrong = tries.read(nowSecs()).wrong
    val locked = try {
        OwnerLock.isLocked(stored)
    } catch (e: OwnerException) {
        fault = e.sayWords()
        return render()
    } catch (e: LinkageError) {
        fault = AppWords.libraryMissing(e.message)
        return render()
    }
    when (SetupLock.gate(wrong, locked)) {
        Gate.Wipe -> wipe()
        Gate.Unlock -> showUnlock(stored, null)
        Gate.SetPin -> showUpgrade(stored)
    }
}

/** The setup text, read at every start and checked for its shape; the bytes of [text] are overwritten (OA-60). */
private fun Controller.readSetup(text: ByteArray): OwnerSetup? {
    val parsed = try {
        OwnerSetup.parse(String(text, Charsets.UTF_8))
    } catch (e: SetupFormatException) {
        fault = AppWords.setupNotReadable(e.message)
        null
    }
    text.fill(0)
    return parsed
}

// ---- a PIN for a new or an older setup (OA-61) ---------------------------------------------------------------------

/**
 * The "Choose a PIN" screen of the Key app: a PIN of 6 digits or more, or a passphrase of 8 characters or
 * more, typed twice. [text] is locked under it by the library, and [onLocked] gets the locked bytes to store.
 * What the library refuses (a PIN it does not take) is shown here again with its own words. [back] is null
 * when there is nowhere to go back to.
 */
internal fun Controller.askNewPin(
    text: ByteArray,
    upgrade: Boolean,
    message: SayWords?,
    back: (() -> Unit)?,
    onLocked: (ByteArray) -> Unit,
) {
    val kind = ui.choice(listOf("PIN: 6 digits or more", "Passphrase: 8 characters or more"))
    val first = secret("PIN", true)
    val again = secret("The same PIN again", true)
    kind.setOnCheckedChangeListener { _, _ ->
        val numeric = ui.chosen(kind) != 1
        first.hint = if (numeric) "PIN" else "Passphrase"
        again.hint = if (numeric) "The same PIN again" else "The same passphrase again"
        for (f in listOf(first, again)) {
            f.setText("")
            f.inputType = if (numeric) {
                InputType.TYPE_CLASS_NUMBER or InputType.TYPE_NUMBER_VARIATION_PASSWORD
            } else {
                InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_FLAG_NO_SUGGESTIONS or InputType.TYPE_TEXT_VARIATION_PASSWORD
            }
            ui.style(f)
        }
    }
    val views = ArrayList<View>()
    message?.let { views.add(ui.words(it)) }
    views.add(ui.words(if (upgrade) AppWords.upgradePin else AppWords.choosePin))
    views.addAll(listOf(kind, first, again))
    views.add(
        ui.button("Set the PIN", ButtonKind.Primary) {
            val pin = first.text.toString()
            val same = pin == again.text.toString()
            first.setText("")
            again.setText("")
            if (!same) return@button askNewPin(text, upgrade, AppWords.pinsDiffer, back, onLocked)
            slow {
                try {
                    onLocked(OwnerLock.lock(pin, text))
                } catch (e: OwnerException) {
                    askNewPin(text, upgrade, e.sayWords(), back, onLocked)
                } catch (e: LinkageError) {
                    showMessage(AppWords.libraryMissing(e.message)) { askNewPin(text, upgrade, null, back, onLocked) }
                }
            }
        },
    )
    back?.let { views.add(ui.button("Back") { it() }) }
    page(views)
}

/** A setup saved before the lock existed opens only into this step, then is saved again locked (OA-61). */
private fun Controller.showUpgrade(stored: ByteArray) {
    // A bare text that is not a setup is a fault with words; no PIN is asked for it.
    if (readSetup(stored.copyOf()) == null) return render()
    askNewPin(stored, true, null, null) { locked ->
        page(listOf(ui.words(AppWords.working)))
        async({ setupFile().save(locked) }) { r ->
            when (val e = r.exceptionOrNull()) {
                null -> {
                    // The setup is the same one the node starts from; no second PIN is asked for.
                    setup = readSetup(stored)
                    if (setup == null) render() else begin()
                }
                is IOException -> showMessage(AppWords.fileNotSaved(e.message)) { showUpgrade(stored) }
                is GeneralSecurityException -> showMessage(AppWords.storageUnavailable(e.message)) { showUpgrade(stored) }
                else -> throw e
            }
        }
    }
}

// ---- unlock (OA-62, OA-63) ---------------------------------------------------------------------------------------------

/** What the screens say about the wrong tries so far: the tries left, from the 5th wrong one. */
private fun Controller.triesLines(check: Tries.Check): List<View> {
    val out = ArrayList<View>()
    if (Tries.showsTriesLeft(check.record.wrong)) out.add(ui.text(Tries.triesLeftText(check.record.wrong), 16f, bold = true))
    return out
}

/** The unlock screen, with the wait counted down and the tries left said from the 5th wrong one. */
internal fun Controller.showUnlock(stored: ByteArray, message: SayWords?) {
    stopTicker()
    val now = nowSecs()
    val rec = tries.read(now)
    val check = Tries.check(rec, now)
    if (check.wipe) return wipe()
    if (check.record != rec) {
        try {
            tries.write(check.record)
        } catch (_: IOException) {
        }
    }
    val field = secret("PIN or passphrase", false)
    val countdown = ui.text("", 20f, bold = true)
    val unlock = ui.primary("Unlock") {
        val pin = field.text.toString()
        field.setText("")
        if (pin.isNotEmpty()) {
            triedPin({ showUnlock(stored, it) }, {
                val text = OwnerLock.unlock(stored, pin)
                setup = readSetup(text) ?: throw SetupNotRead()
            }) { begin() }
        }
    }
    fun update() {
        val left = Tries.check(check.record, nowSecs()).waitLeft
        countdown.text = if (left > 0) "Wait ${Tries.waitText(left)}" else ""
        countdown.visibility = if (left > 0) View.VISIBLE else View.GONE
        unlock.isEnabled = left <= 0
    }
    update()
    val views = ArrayList<View>()
    views.add(ui.words(message ?: AppWords.unlockSetup))
    views.addAll(triesLines(check))
    views.add(countdown)
    views.add(field)
    views.add(unlock)
    page(views)
    if (check.waitLeft > 0) {
        val tick = object : Runnable {
            override fun run() {
                update()
                if (Tries.check(check.record, nowSecs()).waitLeft > 0) main.postDelayed(this, 1000) else ticker = null
            }
        }
        ticker = tick
        main.postDelayed(tick, 1000)
    }
}

/** The text was opened with the right PIN but is not a setup this app reads: [fault] says so. */
private class SetupNotRead : Exception()

/**
 * One try of the PIN, counted as OA-63 says: one more wrong try is written before [op] runs, and 0 after it
 * succeeds. A refusal that is not a wrong PIN gives the count back; the 10th wrong try wipes. [back] shows
 * the screen the try came from, with the words to show on it. [done] runs when [op] worked.
 */
internal fun Controller.triedPin(back: (SayWords?) -> Unit, op: () -> Unit, done: () -> Unit) {
    val now = nowSecs()
    val rec = tries.read(now)
    val check = Tries.check(rec, now)
    if (check.wipe) return wipe()
    if (check.waitLeft > 0) return back(AppWords.waitForTry)
    val before = Tries.before(check.record, now)
    try {
        tries.write(before)
    } catch (e: IOException) {
        return showMessage(AppWords.triesNotSaved(e.message)) { back(null) }
    }
    slow {
        fun giveBack() {
            try {
                tries.write(check.record)
            } catch (_: IOException) {
            }
        }
        try {
            op()
            try {
                tries.write(Tries.afterRight())
            } catch (_: IOException) {
                // The count stays one too high: the stricter way.
            }
            done()
        } catch (e: OwnerException) {
            if (e.rule == RULE_PIN_WRONG) {
                if (before.wrong >= Tries.WIPE_AT) wipe() else back(e.sayWords())
            } else {
                giveBack()
                showMessage(e.sayWords()) { back(null) }
            }
        } catch (_: SetupNotRead) {
            // The PIN was right: the count is cleared, and the fault is the setup's.
            try {
                tries.write(Tries.afterRight())
            } catch (_: IOException) {
            }
            render()
        } catch (e: IOException) {
            giveBack()
            showMessage(AppWords.fileNotSaved(e.message)) { back(null) }
        } catch (e: GeneralSecurityException) {
            giveBack()
            showMessage(AppWords.storageUnavailable(e.message)) { back(null) }
        } catch (e: LinkageError) {
            giveBack()
            showMessage(AppWords.libraryMissing(e.message)) { back(null) }
        }
    }
}

// ---- the wipe (OA-64) -----------------------------------------------------------------------------------------------------

/** The delete order of OA-64: the setup file, the ceremony's file, the Keystore key, then the tries file; and say so. */
internal fun Controller.wipe() {
    stopTicker()
    lockNow()
    try {
        WipeOrder.run(appDir, SetupLock.WIPE_FILES, tries) { OwnerWipe.deleteKey() }
    } catch (e: IOException) {
        return page(listOf(ui.words(AppWords.wipeFailed(e.message)), ui.primary("Try again") { begin() }))
    }
    page(listOf(ui.words(AppWords.wiped), ui.primary("Continue") { begin() }))
}

// ---- changing the PIN ---------------------------------------------------------------------------------------------------

/** Changing the PIN asks for the current one, and a wrong one counts like a wrong try at the unlock screen. */
internal fun Controller.showChangePin(message: SayWords?) {
    val now = nowSecs()
    val check = Tries.check(tries.read(now), now)
    if (check.wipe) return wipe()
    val current = secret("Current PIN or passphrase", false)
    val first = secret("New PIN or passphrase", false)
    val again = secret("The new one again", false)
    val views = ArrayList<View>()
    views.add(ui.text("Change the PIN", 20f, bold = true))
    message?.let { views.add(ui.words(it)) }
    views.addAll(triesLines(check))
    if (check.waitLeft > 0) views.add(ui.text("Wait ${Tries.waitText(check.waitLeft)}", 20f, bold = true))
    views.addAll(listOf(current, first, again))
    views.add(
        ui.button("Change the PIN", ButtonKind.Primary) {
            val old = current.text.toString()
            val new = first.text.toString()
            val same = new == again.text.toString()
            for (f in listOf(current, first, again)) f.setText("")
            if (!same) return@button showChangePin(AppWords.pinsDiffer)
            triedPin({ showChangePin(it) }, {
                // The library opens the saved setup with the current PIN and locks it again in one call; the
                // file is replaced only when that returned (OA-60).
                val stored = setupFile().load() ?: throw IOException("the setup is missing")
                setupFile().save(OwnerLock.changePin(stored, old, new))
            }) { showMessage(AppWords.pinChanged) { begin() } }
        },
    )
    views.add(ui.button("Back") { begin() })
    page(views)
}

private const val RULE_PIN_WRONG = "pin-wrong"
