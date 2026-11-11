package org.openbmt.key.app

import org.openbmt.app.CameraWords
import org.openbmt.app.Format
import org.openbmt.app.SayWords
import org.openbmt.app.Tones
import org.openbmt.app.UrWords
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

    // ---- the camera ---------------------------------------------------------------

    private fun fromSay(s: SayWords) = Words(s.tone, s.who, s.heading, s.what, s.next, null, null, null, s.detail)

    /** The camera permission was refused, or "don't ask again" was chosen. */
    val cameraOff = fromSay(CameraWords.off(Who.KEY_HOLDER))

    fun cameraNotStarted(problem: String?) = fromSay(CameraWords.notStarted(Who.KEY_HOLDER, problem))

    /** A scanned code is a UR (a request or an answer), where a field takes plain text. */
    val codeNotForField = w(
        Tones.INFO, "Not a text code",
        "This code is part of a request or an answer, and this field takes plain text.",
        Who.KEY_HOLDER,
        "Scan the plain text code, or paste the text or choose a file instead.",
    )

    /** The words of reading and showing codes that change, with this app's key holder as the one who acts. */
    fun urWords(): List<Pair<String, Words>> = UrWords.all(Who.KEY_HOLDER).map { (name, say) -> name to fromSay(say) }

    // ---- what was typed --------------------------------------------------------

    val notANumber = w(
        Tones.STOP, "Check what you typed",
        "One of the amounts is empty or has something other than digits in it.",
        Who.KEY_HOLDER,
        "Type whole numbers of sats only, then try again.",
    )

    // ---- the PIN lock (SR-71 to SR-77) -----------------------------------------------

    /** A new or restored key is made behind a PIN, so the PIN comes first. */
    val choosePin = w(
        Tones.INFO, "Choose a PIN",
        "The key on this phone is kept behind a PIN, which the app asks for every time it opens.",
        Who.KEY_HOLDER,
        "Choose at least 6 digits, or a passphrase of at least 8 characters, and do not forget it.",
    )

    /** A key saved by an older build has no PIN: it is locked behind one and used as it is. */
    val upgradePin = w(
        Tones.INFO, "Set a PIN for this key",
        "The key on this phone was saved without a PIN, so anyone who opens the app could use it.",
        Who.KEY_HOLDER,
        "Choose a PIN now; the key is kept as it is and locked behind it.",
    )

    val pinsDiffer = w(
        Tones.STOP, "The two PINs differ",
        "The PIN typed the second time is not the same as the first, so nothing was set.",
        Who.KEY_HOLDER,
        "Type the same PIN in both fields, then try again.",
    )

    val pinChanged = w(
        Tones.GOOD, "PIN changed",
        "The key on this phone is now locked behind the new PIN.",
        Who.NOBODY,
        "Nothing to do. Use the new PIN from now on.",
    )

    /** The page is drawn before the slow step: the key from the PIN takes about a second. */
    val working = w(
        Tones.INFO, "Working",
        "The app is opening the key, which takes a moment.",
        Who.NOBODY,
        "Wait. Do not close the app.",
    )

    val waitForTry = w(
        Tones.INFO, "Wait before the next try",
        "There were too many wrong tries, so the app waits before it takes another.",
        Who.KEY_HOLDER,
        "Wait for the time shown, then type the PIN again.",
    )

    fun triesNotSaved(problem: String?) = w(
        Tones.STOP, "Try not counted",
        "The phone could not keep the count of wrong tries, so the PIN was not tried.",
        Who.KEY_HOLDER,
        "Try again. If it keeps failing, stop and tell the owner in person.",
        problem?.let { Format.cleanForeign(it) },
    )

    /** SR-76: the wipe happened, and what to do next. */
    val wiped = w(
        Tones.STOP, "Key wiped from this phone",
        "This phone's copy of key 2 was wiped after 10 wrong tries. The money is not lost: the other two keys still exist.",
        Who.KEY_HOLDER,
        "The wallet needs two of three keys. Restore key 2 from its 24 words on this phone, or tell the owner in person.",
    )

    fun wipeFailed(problem: String?) = w(
        Tones.STOP, "Wipe not finished",
        "The key could not be wiped from this phone completely, so the app will try again when it opens.",
        Who.KEY_HOLDER,
        "Close the app and open it again. If it keeps failing, uninstall the app.",
        problem?.let { Format.cleanForeign(it) },
    )

    // ---- cash-out addresses (SR-78, SR-79) ----------------------------------------------

    val cashOut = w(
        Tones.INFO, "Cash-out addresses",
        "Payments to these addresses count against the higher limits, once the 24 hours of waiting are over.",
        Who.KEY_HOLDER,
        "Add an address by scanning the deposit code the exchange shows with this phone's camera.",
    )

    val scanDeposit = w(
        Tones.INFO, "Scan the deposit code",
        "Hold this phone's camera to the deposit code the exchange shows. An address cannot be typed or pasted here.",
        Who.KEY_HOLDER,
        "Then type a name for it, and check the address on the next screen.",
    )

    val notADepositCode = w(
        Tones.STOP, "Not a deposit code",
        "This code is part of a request or an answer, not a deposit code, so nothing was taken.",
        Who.KEY_HOLDER,
        "Scan the deposit code the exchange shows.",
    )

    val nameAddress = w(
        Tones.INFO, "Name this address",
        "Type a name for the address, such as the exchange's name, so you can tell it apart later.",
        Who.KEY_HOLDER,
        "Then check the address on the next screen.",
    )

    val checkAddress = w(
        Tones.INFO, "Check this address",
        "Compare every group of four with what the exchange shows. One wrong character sends money to the wrong place.",
        Who.KEY_HOLDER,
        "If they all match, add it. It can be used 24 hours from now, by this phone's clock.",
    )

    val addressAdded = w(
        Tones.GOOD, "Address added",
        "The address is saved, and it counts as a cash-out address in 24 hours, by this phone's clock.",
        Who.NOBODY,
        "Nothing to do. Addresses can be added or removed here at any time.",
    )

    val removeAddress = w(
        Tones.INFO, "Remove this address?",
        "Removing it takes effect at once: payments to it count as payments to any other address.",
        Who.KEY_HOLDER,
        "Remove it only if it is not needed. It can be added again, and then it waits 24 hours.",
    )

    val addressRemoved = w(
        Tones.GOOD, "Address removed",
        "The address is no longer a cash-out address, and the change took effect at once.",
        Who.NOBODY,
        "Nothing to do.",
    )

    // ---- the key ceremony (DESIGN section 2.1, steps 2 and 5) -----------------------

    /** The screen before key 3 is scanned. The plain words are the only ones the screen shows up front. */
    val key3Needed = w(
        Tones.INFO, "Key 3, the recovery key",
        "Key 3 is made on its own in another publisher's wallet or on a hardware wallet, never in this app.",
        Who.KEY_HOLDER,
        "Do that step alone, write its words on separate paper, then scan key 3 here: a key code from the other wallet app, or its key as text.",
    )

    /** A `ur:` text was pasted for key 3: only the camera reads that kind of code. */
    val key3UrNotRead = w(
        Tones.INFO, "Code is read by the camera",
        "A key code is read only when it is scanned, not from pasted text, so nothing was taken.",
        Who.KEY_HOLDER,
        "Scan the key code, or paste the key as text.",
    )

    val key3NotText = w(
        Tones.STOP, "Code is not text",
        "The code holds something other than text, so it cannot be key 3 and nothing was taken.",
        Who.KEY_HOLDER,
        "Export key 3 as text in the other wallet, then scan it again, or paste the text.",
    )

    /** Key 3 is taken: the words of `openbmt_signer::key3_taken_words`, as in docs/screens/key.md. */
    val key3Taken = Words(
        Tones.GOOD, Who.KEY_HOLDER, "Key 3 taken",
        "This app now holds the public part of key 3, and nothing else of it.",
        "Compare the short code shown here with the one in the other wallet app, then go on.",
        "SR-67", null, null, null,
    )

    /** The person says the fingerprints differ. */
    val key3Mismatch = w(
        Tones.STOP, "Key 3 does not match",
        "The fingerprint here is not the one the other wallet shows, so this is not the key 3 that was made.",
        Who.KEY_HOLDER,
        "Export key 3 again from the other wallet and scan it again; nothing goes on until they match.",
    )

    /** The public answer is on the screen: the words of `openbmt_signer::answer_ready_words`. */
    val publicAnswerReady = Words(
        Tones.INFO, Who.OWNER, "Answer ready for the owner",
        "This app shows its public details, which the owner's phone needs to build the wallet.",
        "Scan the answer with the owner's phone, then wait for the wallet code.",
        "SR-69", null, null, null,
    )

    val scanOwnerWallet = w(
        Tones.INFO, "Scan the owner's wallet",
        "The owner phone builds the wallet from the keys it scanned, then shows it as a code.",
        Who.KEY_HOLDER,
        "Scan that code here, then compare the words and fingerprints it shows.",
    )

    /** The wallet is accepted and is shown for the comparison with the owner phone. */
    val checkWallet = w(
        Tones.INFO, "Check the wallet",
        "The wallet was read, and its four words and three fingerprints are below.",
        Who.KEY_HOLDER,
        "The owner phone and the counter must show these same words and fingerprints.",
    )

    /** The person says the words or fingerprints differ. */
    val walletMismatch = w(
        Tones.STOP, "Wallet words differ",
        "The words or fingerprints here differ from the owner phone's, so this is not the wallet to use.",
        Who.KEY_HOLDER,
        "Nothing was registered. Check the owner phone, then scan its wallet code again.",
    )

    /** SR-67: the owner phone's wallet does not hold the key 3 that was scanned here. */
    val walletLacksKey3 = w(
        Tones.STOP, "Wallet lacks this key 3",
        "The owner phone's wallet does not hold the key 3 scanned here, so nothing was accepted.",
        Who.KEY_HOLDER,
        "Do not go on. Tell the owner in person, then make the wallet again on the owner phone.",
    )

    /** The lines of the ceremony screens that are not words of a state; none uses a word of the jargon list. */
    object Text {
        const val SCAN_KEY3 = "Scan key 3"
        const val PASTE_KEY3 = "Or paste the text of key 3"
        const val ON_OWNER_PHONE = "On the owner phone, choose Scan the Key app."
        const val ADVANCED = "Advanced"
        const val ADVANCED_KEY3 =
            "The other wallet must export the account key (xpub) of a multisig P2WSH wallet at m/48'/coin'/0'/2' " +
                "with its origin, as [fingerprint/path]key. A single-signature export is refused. " +
                "A plain text code is read, and so is a ur: code of type crypto-account, crypto-hdkey or bytes. Pasting takes text only."
    }

    val flipsNotReadable = w(
        Tones.STOP, "Check the coin flips",
        "The flips may hold only H for heads and T for tails.",
        Who.KEY_HOLDER,
        "Remove any other letter or digit, then try again.",
    )

    // ---- pairing a counter -------------------------------------------------------

    /** The offer on the screen: nothing in it is secret. */
    val pairOffer = w(
        Tones.INFO, "Pair a counter",
        "The code below names this shop's two keys, its wallet and its network, for a counter to read.",
        Who.KEY_HOLDER,
        "Scan this with the counter: open the counter app, tap Pair with the Key app, and hold its camera to this code.",
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
        is SetupState.Locked -> w(
            Tones.INFO, "Enter your PIN", "The key on this phone is kept behind a PIN.", Who.KEY_HOLDER,
            "Type the PIN to unlock the app.",
        )
        is SetupState.WalletPending -> w(
            Tones.INFO, "Wallet waiting for you",
            "The wallet from the owner's phone is held but not registered, so nothing can be signed yet.",
            Who.KEY_HOLDER,
            "Compare its words and fingerprints with the owner's phone, then confirm or discard it.",
        )
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

    /** SC-12: the list of words as the table of docs/screens/key-app.md. */
    fun screensMarkdown(): String {
        fun who(n: Int) = when (n) {
            Who.NOBODY -> "nobody"
            Who.CUSTOMER -> "the customer"
            Who.CASHIER -> "the cashier"
            Who.MANAGER -> "a manager"
            Who.OWNER -> "the owner"
            Who.KEY_HOLDER -> "the key holder"
            else -> "the Box keeper"
        }
        fun cell(text: String) = text.replace('|', '/')
        val out = StringBuilder(
            "# Key app: what the app itself tells a person\n\n" +
                "Made by `AppWords.screensMarkdown` in\n" +
                "`apps/key-android/src/model/AppWords.kt`. Do not edit it by hand: run\n" +
                "`apps/key-android/tests/run-jvm.sh` with the environment variable\n" +
                "`OPENBMT_WRITE_SCREENS=1` to write it again.\n\n" +
                "Everything the rules refuse is shown with the words of the library\n" +
                "(`docs/screens/key.md`, `openbmt-key-ffi`). These are the words of what\n" +
                "the app decides itself, with the key ceremony screens (`docs/DESIGN.md`\n" +
                "section 2.1, steps 2 and 5) among them.\n\n" +
                "| State | Heading | What happened | What to do next | Who | Code |\n" +
                "|---|---|---|---|---|---|\n",
        )
        for ((name, w) in all()) {
            out.append("| ${cell(name)} | ${cell(w.heading)} | ${cell(w.what)} | ${cell(w.next)} | ${who(w.who)} | ${w.code ?: ""} |\n")
        }
        return out.toString()
    }

    /** Every fixed words of the app, for the test that checks them all and writes the list. */
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
        "cameraOff" to cameraOff,
        "cameraNotStarted" to cameraNotStarted("a problem"),
        "codeNotForField" to codeNotForField,
        "notANumber" to notANumber,
        "flipsNotReadable" to flipsNotReadable,
        "answersReady" to answersReady,
        "pairOffer" to pairOffer,
        "choosePin" to choosePin,
        "upgradePin" to upgradePin,
        "pinsDiffer" to pinsDiffer,
        "pinChanged" to pinChanged,
        "working" to working,
        "waitForTry" to waitForTry,
        "triesNotSaved" to triesNotSaved("a problem"),
        "wiped" to wiped,
        "wipeFailed" to wipeFailed("a problem"),
        "cashOut" to cashOut,
        "scanDeposit" to scanDeposit,
        "notADepositCode" to notADepositCode,
        "nameAddress" to nameAddress,
        "checkAddress" to checkAddress,
        "addressAdded" to addressAdded,
        "removeAddress" to removeAddress,
        "addressRemoved" to addressRemoved,
        "key3Needed" to key3Needed,
        "key3UrNotRead" to key3UrNotRead,
        "key3NotText" to key3NotText,
        "key3Taken" to key3Taken,
        "key3Mismatch" to key3Mismatch,
        "publicAnswerReady" to publicAnswerReady,
        "scanOwnerWallet" to scanOwnerWallet,
        "checkWallet" to checkWallet,
        "walletMismatch" to walletMismatch,
        "walletLacksKey3" to walletLacksKey3,
        "Empty" to state(SetupState.Empty),
        "HasIdentity(WordsWaiting)" to state(SetupState.HasIdentity(true, true)),
        "HasIdentity(Key2)" to state(SetupState.HasIdentity(true, false)),
        "HasIdentity(NoKey2)" to state(SetupState.HasIdentity(false, false)),
        "Locked" to state(SetupState.Locked),
        "WalletPending" to state(SetupState.WalletPending(true)),
        "Ready(Key2)" to state(SetupState.Ready(true)),
        "Ready(NoKey2)" to state(SetupState.Ready(false)),
        "Passed" to check(CheckStatus.Passed),
        "Failed" to check(CheckStatus.Failed),
        "NotChecked" to check(CheckStatus.NotChecked),
        "Kit Same" to kit(KitCheck.Same),
        "Kit SameCheckWords" to kit(KitCheck.Different(3, true, true)),
        "Kit NotAWallet" to kit(KitCheck.Different(3, false, false)),
        "Kit OtherWallet" to kit(KitCheck.Different(3, false, true)),
    ) + urWords()
}
