package org.openbmt.key.app

import org.openbmt.app.Format
import org.openbmt.app.Tones
import org.openbmt.app.Who
import org.openbmt.key.CheckStatus
import org.openbmt.key.KeyException
import org.openbmt.key.KitCheck
import org.openbmt.key.SetupState
import org.openbmt.key.Words

/**
 * The words of the app's own states and refusals (docs/SCREEN-RULES.md). The
 * library gives words for its errors only, so the words of its setup states,
 * its line checks and its kit check are written out here, the same as the
 * ones in docs/screens/key.md; a test compares them with that file. Every
 * one of them passes the checks of `openbmt_core::say::check`, which the
 * JVM test repeats.
 */
object AppWords {
    private fun w(tone: Int, heading: String, what: String, who: Int, next: String, detail: String? = null) =
        Words(tone, who, heading, what, next, null, null, null, detail)

    // ---- what the app refuses to run under ----------------------------------

    val debuggingOn = w(
        Tones.STOP, "USB debugging is on",
        "USB debugging is switched on, so a computer could read this phone.",
        Who.KEY_HOLDER,
        "Switch USB debugging off in the developer settings, then open this app again.",
    )

    val wirelessDebuggingOn = w(
        Tones.STOP, "Wireless debugging is on",
        "Wireless debugging is switched on, so a computer on the network could read this phone.",
        Who.KEY_HOLDER,
        "Switch wireless debugging off in the developer settings, then open this app again.",
    )

    val accessibilityOn = w(
        Tones.STOP, "A screen helper is on",
        "An accessibility service is switched on, and it could read or press things in this app.",
        Who.KEY_HOLDER,
        "Switch off every accessibility service in the settings, then open this app again.",
    )

    // ---- where the key is kept -------------------------------------------------

    /** Shown when the saved key is held by software alone. */
    val keyInSoftware = w(
        Tones.STOP, "Key kept in software",
        "This phone has no protected chip for keys, so the saved key is held by software alone.",
        Who.KEY_HOLDER,
        "Use this phone for tests only. For real funds, use a phone with a protected key chip.",
    )

    // ---- the app's own faults --------------------------------------------------

    fun libraryMissing(problem: String?) = w(
        Tones.STOP, "The app is incomplete",
        "The part of this app that holds the key did not load, so nothing was done.",
        Who.KEY_HOLDER,
        "Do not use this copy. Install the app again from the place you got it.",
        problem?.let { Format.cleanForeign(it) },
    )

    /** An error the Kotlin side made itself, with no words from the library. */
    fun fault(e: KeyException) = w(
        Tones.STOP, "Answer not readable",
        "The app could not read an answer from its own key part, so nothing was changed.",
        Who.KEY_HOLDER,
        "Close the app and open it again. If it happens again, stop and tell the owner in person.",
        "${e.rule}: ${e.text}".take(200),
    )

    fun keystoreUnavailable(problem: String?) = w(
        Tones.STOP, "Key chip not usable",
        "The phone's key storage did not start, so the saved key was not opened.",
        Who.KEY_HOLDER,
        "Restart the phone and open this app again. If it happens again, stop and tell the owner in person.",
        problem?.let { Format.cleanForeign(it) },
    )

    fun fileNotRead(problem: String?) = w(
        Tones.STOP, "File not read",
        "The file could not be opened or read, so nothing was done.",
        Who.KEY_HOLDER,
        "Pick the file again, or ask the owner to send it again.",
        problem?.let { Format.cleanForeign(it) },
    )

    fun fileNotSaved(problem: String?) = w(
        Tones.STOP, "File not saved",
        "The answer file could not be written, so the owner's phone has not got it yet.",
        Who.KEY_HOLDER,
        "Save the file again, in a place the owner's phone can read.",
        problem?.let { Format.cleanForeign(it) },
    )

    // ---- what was typed --------------------------------------------------------

    val notANumber = w(
        Tones.STOP, "Check what you typed",
        "One of the amounts is empty or has something other than digits in it.",
        Who.KEY_HOLDER,
        "Type whole numbers of sats only, then try again.",
    )

    val identityNotReadable = w(
        Tones.STOP, "Owner identity not read",
        "The owner identity must be 64 letters and digits from 0 to 9 and a to f.",
        Who.KEY_HOLDER,
        "Copy it again from the owner's phone, then try again.",
    )

    val flipsNotReadable = w(
        Tones.STOP, "Check the coin flips",
        "The flips may hold only H for heads and T for tails.",
        Who.KEY_HOLDER,
        "Remove any other letter or digit, then try again.",
    )

    // ---- the end of a session ----------------------------------------------------

    val answersReady = w(
        Tones.GOOD, "Answers ready",
        "Every line has an answer, and the answer file is signed and ready on this phone.",
        Who.KEY_HOLDER,
        "Save the answer file, then take it to the owner's phone.",
    )

    // ---- the library's states, as in docs/screens/key.md -------------------------

    fun state(s: SetupState): Words = when (s) {
        is SetupState.Empty -> w(
            Tones.INFO, "No key yet", "This app has no key yet.", Who.KEY_HOLDER,
            "Make the key, or type the words of an existing key.",
        )
        is SetupState.HasIdentity -> when {
            s.wordsWaiting -> w(
                Tones.INFO, "Write the words down",
                "The words of this key are ready, and they are shown only once.", Who.KEY_HOLDER,
                "Write them on paper now; they cannot be shown again.",
            )
            s.holdsKey2 -> w(
                Tones.INFO, "Waiting for the wallet",
                "This app holds key 2 and is waiting for the wallet from the owner's phone.",
                Who.KEY_HOLDER,
                "Give the owner's phone this app's key details, then accept the wallet it makes.",
            )
            else -> w(
                Tones.INFO, "Waiting, without key 2",
                "This app has its own identity but not key 2, and is waiting for the wallet.",
                Who.KEY_HOLDER,
                "Give the owner's phone this app's identity, then accept the wallet it makes.",
            )
        }
        is SetupState.Ready ->
            if (s.holdsKey2) {
                w(
                    Tones.GOOD, "Ready to approve",
                    "The wallet is accepted, and this app can answer the owner's requests.", Who.NOBODY,
                    "Nothing to do. It waits for a request from the owner's phone.",
                )
            } else {
                w(
                    Tones.INFO, "Ready, without key 2",
                    "The wallet is accepted, but this phone does not hold key 2.", Who.NOBODY,
                    "Nothing to do for payments here. This app answers other requests only.",
                )
            }
    }

    fun check(s: CheckStatus): Words = when (s) {
        CheckStatus.Passed -> w(
            Tones.GOOD, "Check passed", "This check of the line passed.", Who.NOBODY,
            "Nothing to do for this check.",
        )
        CheckStatus.Failed -> w(
            Tones.STOP, "Check not passed",
            "This check of the line did not pass, so the line cannot be approved.", Who.KEY_HOLDER,
            "Decline the line, and tell the owner in person which check it was.",
        )
        CheckStatus.NotChecked -> w(
            Tones.ENDED, "Check not run", "An earlier check did not pass, so this one was not run.",
            Who.KEY_HOLDER, "Look at the first check that did not pass.",
        )
    }

    fun kit(k: KitCheck): Words = when (k) {
        is KitCheck.Same -> w(
            Tones.GOOD, "Kit matches this phone",
            "The wallet on the paper is the same as the wallet on this phone.", Who.NOBODY,
            "Nothing to do. Only the wallet is compared, not the other lines on the paper.",
        )
        is KitCheck.Different -> {
            val detail = "first difference at position ${k.firstDifference}"
            when {
                k.sameChecksumWords -> w(
                    Tones.STOP, "Same check words, other keys",
                    "The paper shows the same four words but another wallet, which should not happen by accident.",
                    Who.KEY_HOLDER, "Stop, do not use this kit, and tell the owner in person.", detail,
                )
                !k.parses -> w(
                    Tones.STOP, "Paper is not a wallet",
                    "The text scanned from the paper is not a wallet this app can read.", Who.KEY_HOLDER,
                    "Scan the wallet code again; if it still differs, do not use this kit and print a new one.",
                    detail,
                )
                else -> w(
                    Tones.STOP, "Kit is for another wallet",
                    "The paper and this phone do not describe the same wallet.", Who.KEY_HOLDER,
                    "Do not use this kit to recover; print a new kit from the owner's phone and check it.",
                    detail,
                )
            }
        }
    }

    /** Every fixed words of the app, for the test that checks them all. */
    fun all(): List<Pair<String, Words>> = listOf(
        "debuggingOn" to debuggingOn,
        "wirelessDebuggingOn" to wirelessDebuggingOn,
        "accessibilityOn" to accessibilityOn,
        "keyInSoftware" to keyInSoftware,
        "libraryMissing" to libraryMissing("no library"),
        "fault" to fault(KeyException("response-null", "the library gave no response", 4)),
        "keystoreUnavailable" to keystoreUnavailable("a problem"),
        "fileNotRead" to fileNotRead("a problem"),
        "fileNotSaved" to fileNotSaved("a problem"),
        "notANumber" to notANumber,
        "identityNotReadable" to identityNotReadable,
        "flipsNotReadable" to flipsNotReadable,
        "answersReady" to answersReady,
        "Empty" to state(SetupState.Empty),
        "HasIdentity(WordsWaiting)" to state(SetupState.HasIdentity(true, true)),
        "HasIdentity(Key2)" to state(SetupState.HasIdentity(true, false)),
        "HasIdentity(NoKey2)" to state(SetupState.HasIdentity(false, false)),
        "Ready(Key2)" to state(SetupState.Ready(true)),
        "Ready(NoKey2)" to state(SetupState.Ready(false)),
        "Passed" to check(CheckStatus.Passed),
        "Failed" to check(CheckStatus.Failed),
        "NotChecked" to check(CheckStatus.NotChecked),
        "Kit Same" to kit(KitCheck.Same),
        "Kit SameCheckWords" to kit(KitCheck.Different(3, true, true)),
        "Kit NotAWallet" to kit(KitCheck.Different(3, false, false)),
        "Kit OtherWallet" to kit(KitCheck.Different(3, false, true)),
    )
}
