package org.openbmt.owner.app

import org.openbmt.app.CameraWords
import org.openbmt.app.Format
import org.openbmt.app.SayWords
import org.openbmt.app.Tones
import org.openbmt.app.UrWords
import org.openbmt.app.Who
import org.openbmt.owner.OwnerException

/**
 * The words of the owner app's own states and refusals (docs/SCREEN-RULES.md).
 * The library gives words for everything its node knows (the coins, the
 * payments, the queue, the link, the old addresses, every refusal of a rule:
 * `docs/screens/owner.md`, `docs/screens/owner-node.md`, and the words the
 * `openbmt-owner-ffi` binding carries); these are the words of what the app
 * itself decides: setting up, Orbot, what was typed, what was copied or
 * saved, and its own faults. The list is written to docs/screens/owner-app.md
 * by the JVM test, and every one of them passes the checks of
 * `openbmt_core::say::check` as far as text can be checked.
 */
object AppWords {
    private fun w(tone: Int, heading: String, what: String, who: Int, next: String, detail: String? = null) =
        SayWords(tone, who, heading, what, next, null, null, null, detail)

    private fun detailOf(problem: String?): String? = problem?.let { Format.cleanForeign(it) }?.takeIf { it.isNotEmpty() }

    // ---- setting up ------------------------------------------------------------------

    val notSetUp = w(
        Tones.INFO, "Not set up yet",
        "This phone has not been set up as the shop's owner phone yet.",
        Who.OWNER,
        "Make a new shop wallet with the Key app, or use setup text you already have.",
    )

    /** The screen of the second choice: setup text from a key ceremony made elsewhere. */
    val setupTextNeeded = w(
        Tones.INFO, "Setup text needed",
        "This phone takes the setup text of a key ceremony made elsewhere, pasted or from a file.",
        Who.OWNER,
        "Paste the setup text, or choose its file, then save.",
    )

    val setupSaved = w(
        Tones.GOOD, "Owner phone set up",
        "The setup is saved on this phone, sealed with a key that stays in the phone.",
        Who.OWNER,
        "Keep Orbot running and the app open. It starts looking for the network now.",
    )

    fun setupNotReadable(problem: String?) = w(
        Tones.STOP, "Setup text not read",
        "The setup text is not in the form this app reads, so nothing was saved.",
        Who.OWNER,
        "Copy the setup text again from the key ceremony, then try again.",
        detailOf(problem),
    )

    val torAddressNotAccepted = w(
        Tones.STOP, "Tor address not accepted",
        "The address of Orbot must be a host, a colon and a port, like 127.0.0.1:9050.",
        Who.OWNER,
        "Type the address again.",
    )

    val torAlwaysOn = w(
        Tones.INFO, "Everything goes through Tor",
        "This app sends every connection through Tor, with Orbot, and has no way to turn that off.",
        Who.OWNER,
        "Check the address of Orbot below, and keep Orbot running.",
    )

    // ---- Tor and Orbot ------------------------------------------------------------------

    val orbotNotRunning = w(
        Tones.STOP, "Tor is not running",
        "This app sends everything through Tor, but Orbot is not running.",
        Who.OWNER,
        "Open Orbot and turn it on.",
    )

    // ---- what was typed -------------------------------------------------------------------

    val notANumber = w(
        Tones.STOP, "Check what you typed",
        "An amount, a number or a rate is empty or has something other than digits in it.",
        Who.OWNER,
        "Type whole numbers only, then try again.",
    )

    val counterNeeded = w(
        Tones.INFO, "Counter needed",
        "A refund is asked for on one counter's sale, and no counter is chosen.",
        Who.OWNER,
        "Choose the counter, or paste its identity of 64 digits, then try again.",
    )

    val counterNotAccepted = w(
        Tones.STOP, "Counter identity not read",
        "A counter's identity is exactly 64 digits and letters from a to f.",
        Who.OWNER,
        "Type the identity again, or choose the counter from the list.",
    )

    val responseNeeded = w(
        Tones.INFO, "Response needed",
        "The Key app's response is needed to finish the approval.",
        Who.OWNER,
        "Paste the response or choose its file, then press the button again.",
    )

    fun responseNotReadable(problem: String?) = w(
        Tones.STOP, "Response text not read",
        "The text is not in the form this app reads, so the Key app's response was not used.",
        Who.OWNER,
        "Copy the response again from the Key app, or choose its file instead.",
        detailOf(problem),
    )

    // ---- the approval as a code -------------------------------------------------------------

    /** The request is on the screen as a code that changes. */
    val requestCodeAnimated = w(
        Tones.INFO, "Request shown as a code",
        "The request is on the screen as a code that changes, one part after another.",
        Who.OWNER,
        "On the Key app, choose Scan and point it at this code until it has read every part. Then press Done.",
    )

    /** The request is on the screen as one code. */
    val requestCodeStill = w(
        Tones.INFO, "Request shown as one code",
        "The request is on the screen as one code that does not change.",
        Who.OWNER,
        "On the Key app, choose Scan and point it at this code. Then press Done.",
    )

    /** No code could be made of the request. */
    val requestCodeNotMade = w(
        Tones.STOP, "No code could be made",
        "The request could not be turned into a code, so nothing is shown.",
        Who.OWNER,
        "Go back and save the request as a file, or copy it as text, instead.",
    )

    /** A scan ended with no code read: cancelled, or the permission dialog was dismissed. */
    val scanCancelled = w(
        Tones.INFO, "Scan cancelled",
        "No code was read, so nothing was taken from the Key app.",
        Who.OWNER,
        "Scan the Key app's code again, or choose its file or paste its text instead.",
    )

    /** A single code was read where the response is a code that changes. */
    val scanNotAResponse = w(
        Tones.STOP, "Not the Key app's code",
        "The code read is not one the Key app shows for a response, so nothing was used.",
        Who.OWNER,
        "On the Key app, show the answer as a code, and scan that one.",
    )

    // ---- adding a counter -------------------------------------------------------------------

    /** The request of a counter is being read; the four words take a moment to work out. */
    val counterReading = w(
        Tones.INFO, "Reading the counter",
        "The counter's request is being read and its four words worked out, which takes a moment.",
        Who.OWNER,
        "Wait on this screen.",
    )

    /** A single code was read where a counter's request is a code that may change. */
    val counterNotACode = w(
        Tones.STOP, "Not a counter's code",
        "The code read is not one a counter shows for its request, so nothing was queued.",
        Who.OWNER,
        "On the counter, choose Pair and scan the code it shows.",
    )

    /** The scan of a counter's code was cancelled, or its permission dialog was dismissed. */
    val counterScanCancelled = w(
        Tones.INFO, "Counter scan cancelled",
        "No code was read, so nothing was queued.",
        Who.OWNER,
        "Scan the counter's code again when you are ready.",
    )

    /** A complete code that is longer than a counter's request can be. */
    val counterCodeNotRequest = w(
        Tones.STOP, "Not a counter's request",
        "The code holds more than a counter's request can, so nothing was queued.",
        Who.OWNER,
        "On the counter, choose Pair and scan the code it shows.",
    )

    /** The shares of the instant limit once the counter is added: equal, until the settings editor exists. */
    fun sharesNote(counters: Int) = w(
        Tones.INFO, "Equal shares",
        if (counters <= 1) {
            "This is the only counter, so its share of the instant limit is all of it."
        } else {
            "Each counter's share of the instant limit becomes 1/$counters."
        },
        Who.OWNER,
        "Custom shares come later, with the settings editor.",
    )

    /** What the owner does with the four words. */
    val counterCompare = w(
        Tones.INFO, "Compare the four words",
        "Check that the counter shows these same four words, and that the Key app shows them too.",
        Who.OWNER,
        "If they match, say so below. If they differ, say so and nothing is paired.",
    )

    /** A phone with no settings needs a rate to start them. */
    val counterRateNeeded = w(
        Tones.INFO, "Rate needed",
        "This phone holds no settings yet, and the starting limits are in dollars, so it needs today's rate.",
        Who.OWNER,
        "Type satoshis to a dollar as a whole number above 0, then say the words match.",
    )

    /** The owner said the words differ, and the queued request was taken out. */
    val counterCancelled = w(
        Tones.INFO, "Counter not added",
        "The four words did not match, so the counter's request was taken out of the queue.",
        Who.OWNER,
        "Start again on the counter, and scan the new code it shows.",
    )

    /** The Key app did not approve: nothing waits for the counter. */
    val counterNotPaired = w(
        Tones.STOP, "Counter not paired",
        "The Key app did not approve this counter, so no code for it was made.",
        Who.OWNER,
        "Read the words above, then scan the counter's code again to start over.",
    )

    /** The code for the counter is on the screen with the first addresses in it, and it changes. */
    val counterCodeAnimated = w(
        Tones.INFO, "Code for the counter",
        "The code for the counter is on the screen in many parts, with its first addresses. Taking it takes about a minute.",
        Who.OWNER,
        "Scan this with the counter and hold both phones steady. The counter counts the parts it has and " +
            "shows Paired when it has them all.",
    )

    /** The code for the counter is on the screen as one code. */
    val counterCodeStill = w(
        Tones.INFO, "Code for the counter",
        "The code for the counter is on the screen as one code that does not change.",
        Who.OWNER,
        "Scan this with the counter. It shows Paired when it has taken it.",
    )

    /** The code has no first addresses: they were handed over already. */
    val counterCodeNoBatch = w(
        Tones.INFO, "Code without addresses",
        "The first addresses are not in this code, because the link handed them over already.",
        Who.OWNER,
        "Scan this with the counter. It shows Paired when it has taken it.",
    )

    /** No code could be made of the bundle. */
    val counterCodeNotMade = w(
        Tones.STOP, "No code could be made",
        "The code for the counter could not be made, so nothing is shown. What waits for the counter is kept.",
        Who.OWNER,
        "Try to show it again.",
    )

    /** The owner says the counter shows Paired. */
    val counterDelivered = w(
        Tones.GOOD, "Counter paired",
        "The counter shows Paired, so what waited for it is marked delivered.",
        Who.OWNER,
        "The counter takes its sales from its first addresses now.",
    )

    // ---- making a new shop wallet ---------------------------------------------------------------

    /** Key 1 is being made; it takes a moment. */
    val ceremonyMakingKey = w(
        Tones.INFO, "Making key 1",
        "Key 1 is being made on this phone, which takes a moment.",
        Who.OWNER,
        "Wait on this screen.",
    )

    /** Key 1 is made and the answer of the Key app is next. */
    val ceremonyScanNext = w(
        Tones.INFO, "Scan the Key app",
        "Open the Key app on the other phone and choose Set up keys.",
        Who.OWNER,
        "Press the button below and scan the code it shows.",
    )

    /** A single code was read where the Key app's answer is a code that may change. */
    val ceremonyNotAnAnswer = w(
        Tones.STOP, "Not the Key app's answer",
        "The code read is not one the Key app shows for its answer, so nothing was used.",
        Who.OWNER,
        "On the Key app, choose Set up keys, and scan the code it shows.",
    )

    /** A complete code that is longer than the Key app's answer can be. */
    val ceremonyCodeTooLong = w(
        Tones.STOP, "Code too long for an answer",
        "The code holds more than the Key app's answer can, so nothing was used.",
        Who.OWNER,
        "On the Key app, choose Set up keys, and scan the code it shows.",
    )

    /** The scan of the Key app's answer was cancelled, or its permission dialog was dismissed. */
    val ceremonyScanCancelled = w(
        Tones.INFO, "Key app scan cancelled",
        "No code was read, so no answer was taken from the Key app.",
        Who.OWNER,
        "Scan the Key app's code again when you are ready.",
    )

    /** What the owner does with the Key app's four words. */
    val ceremonyCompareRoot = w(
        Tones.INFO, "Compare the Key app's words",
        "Check that the Key app shows these same four words as its own.",
        Who.OWNER,
        "If they match, say so below. If they differ, say so and the answer is thrown away.",
    )

    /** The owner said the Key app's words differ, and the answer was thrown away. */
    val ceremonyRootDiffers = w(
        Tones.STOP, "Answer thrown away",
        "The four words differ, so this answer may not come from the Key app you hold, and it was thrown away.",
        Who.OWNER,
        "Scan again on the Key app you hold. If the words still differ, choose Start again.",
    )

    /** The Key app holds no key 2: it comes from the device that does. */
    val ceremonyKey2Needed = w(
        Tones.INFO, "Key 2 needed",
        "The Key app holds no key 2, so key 2 comes from the device that holds it.",
        Who.OWNER,
        "Scan its code or paste its text, then continue.",
    )

    /** The text of key 2 is empty or longer than a key's text can be. */
    val ceremonyKey2Text = w(
        Tones.STOP, "Key 2 text not accepted",
        "The text for key 2 is empty, is not text, or is longer than a key's text can be, so nothing was used.",
        Who.OWNER,
        "Copy it again from the device that holds key 2, then try again.",
    )

    /** The scan of key 2 was cancelled, or its permission dialog was dismissed. */
    val ceremonyKey2ScanCancelled = w(
        Tones.INFO, "Key 2 scan cancelled",
        "No code was read, so key 2 was not taken.",
        Who.OWNER,
        "Scan its code again, or paste its text instead.",
    )

    /** The wallet is being built; it takes a moment. */
    val ceremonyBuilding = w(
        Tones.INFO, "Building the wallet",
        "The wallet is being built from the three keys, which takes a moment.",
        Who.OWNER,
        "Wait on this screen.",
    )

    /** The wallet is built and its words are on the screen. */
    val ceremonyCheckWallet = w(
        Tones.INFO, "Check the wallet",
        "The checksum words and the fingerprints below are the wallet's. The Key app shows the same ones.",
        Who.OWNER,
        "Show the Key app its setup next, and compare there.",
    )

    /** The setup for the Key app is on the screen as a code that changes. */
    val ceremonyCodeAnimated = w(
        Tones.INFO, "Setup for the Key app",
        "The setup for the Key app is on the screen as a code that changes, one part after another.",
        Who.OWNER,
        "On the Key app, scan this code until it has read every part, then compare the words.",
    )

    /** The setup for the Key app is on the screen as one code. */
    val ceremonyCodeStill = w(
        Tones.INFO, "Setup for the Key app",
        "The setup for the Key app is on the screen as one code that does not change.",
        Who.OWNER,
        "On the Key app, scan this code, then compare the words.",
    )

    /** No code could be made of the setup. */
    val ceremonyCodeNotMade = w(
        Tones.STOP, "No code could be made",
        "The setup for the Key app could not be turned into a code, so nothing is shown.",
        Who.OWNER,
        "Go back and try to show it again.",
    )

    /** The owner compares the wallet's words with the Key app's. */
    val ceremonyCompareSetup = w(
        Tones.INFO, "Compare with the Key app",
        "After it takes the setup, the Key app shows the checksum words and the fingerprints.",
        Who.OWNER,
        "Check that they are the same as on this phone, then say so below.",
    )

    /** The owner says the words differ: the wallet is not finished. */
    val ceremonyWordsDiffer = w(
        Tones.STOP, "Wallet not finished",
        "The words differ, so the wallet is not finished, and nothing should be paid to it.",
        Who.OWNER,
        "Choose Start again on both phones.",
    )

    /** The wallet was made on a phone that did not keep the setup for the Key app. */
    val ceremonyScanAgain = w(
        Tones.INFO, "Scan the Key app again",
        "The wallet was built, but the setup for the Key app is not kept on this phone.",
        Who.OWNER,
        "Scan the Key app's code again to show the setup again.",
    )

    /** The owner confirmed, and the setup is being saved. */
    val ceremonyFinishing = w(
        Tones.INFO, "Finishing the setup",
        "The setup is being finished and saved on this phone.",
        Who.OWNER,
        "Wait on this screen.",
    )

    /** A finished setup waits to be saved. */
    val ceremonySaveNext = w(
        Tones.INFO, "Save the setup",
        "The words were confirmed, but the setup was not saved on this phone yet.",
        Who.OWNER,
        "Press the button below to save it and start.",
    )

    /** An unfinished setup was found at the start. */
    val ceremonyResume = w(
        Tones.INFO, "Setup in progress",
        "A new shop wallet was started on this phone and is not finished.",
        Who.OWNER,
        "Continue where it stopped, or start again.",
    )

    /** The record of the unfinished setup does not read. */
    fun ceremonyRecordNotRead(problem: String?) = w(
        Tones.STOP, "Setup record not read",
        "The record of the unfinished setup could not be read, so nothing was done.",
        Who.OWNER,
        "Choose Start again and make the wallet again. Nothing has been paid to it.",
        detailOf(problem),
    )

    /** The question before the unfinished setup is thrown away. */
    val ceremonyStartAgain = w(
        Tones.STOP, "Start again?",
        "The keys made so far are thrown away, and nothing has been paid to them.",
        Who.OWNER,
        "Choose Start again only if the Key app starts again too.",
    )

    /** The unfinished setup was thrown away. */
    val ceremonyThrownAway = w(
        Tones.INFO, "Started over",
        "The keys made so far were thrown away, and nothing was paid to them.",
        Who.OWNER,
        "Make a new shop wallet again, or use setup text you already have.",
    )

    // ---- what was done --------------------------------------------------------------------

    val requestReady = w(
        Tones.GOOD, "Request ready",
        "The request for the Key app is ready, and nothing has been signed yet.",
        Who.OWNER,
        "Show it as a code, or save it as a file or copy it as text, give it to the Key app, then bring back its response.",
    )

    val requestCopied = w(
        Tones.INFO, "Request copied",
        "The request is copied as text, written in lower case.",
        Who.OWNER,
        "Paste it where the Key app can read it.",
    )

    fun requestSaved(name: String) = w(
        Tones.INFO, "Request saved",
        "The request is saved as a file.",
        Who.OWNER,
        "Carry the file to the Key app and open it there.",
        detailOf(name),
    )

    val refundQueuedAtDepth = w(
        Tones.GOOD, "Refund queued",
        "The refund is in the queue, and the sale is deep enough to be refunded.",
        Who.OWNER,
        "Bring this phone and the Key app together and start an approval.",
    )

    val refundQueuedWaiting = w(
        Tones.INFO, "Refund queued, sale new",
        "The refund is in the queue, but the sale is not deep enough yet to be refunded.",
        Who.OWNER,
        "Wait for more blocks. The refund can be approved when the sale is deep enough.",
    )

    val cashOutQueued = w(
        Tones.GOOD, "Cash-out queued",
        "The cash-out is in the queue and has not been sent for approval yet.",
        Who.OWNER,
        "Bring this phone and the Key app together and start an approval.",
    )

    val entryCancelled = w(
        Tones.INFO, "Entry cancelled",
        "The entry is taken out of the queue.",
        Who.NOBODY,
        "Nothing to do.",
    )

    val entryRequeued = w(
        Tones.INFO, "Entry queued again",
        "The entry that was sent back is in the queue again.",
        Who.OWNER,
        "Start an approval when the Key app is at hand.",
    )

    val sessionAbandoned = w(
        Tones.INFO, "Approval abandoned",
        "The open approval is closed, and its entries are back in the queue.",
        Who.NOBODY,
        "Nothing to do. Start a new approval when the Key app is at hand.",
    )

    val alertCleared = w(
        Tones.INFO, "Alert cleared",
        "The alert is cleared from this phone.",
        Who.NOBODY,
        "Nothing to do. If the cause is still there, the alert comes back.",
    )

    val noteDismissed = w(
        Tones.INFO, "Note dismissed",
        "The note about a refund request is removed from the list.",
        Who.NOBODY,
        "Nothing to do.",
    )

    // ---- what the node does not hold ---------------------------------------------------------

    val noLink = w(
        Tones.INFO, "No link to the counters",
        "This phone was set up without a relay, so no counter can report to it.",
        Who.OWNER,
        "Nothing to do here. A setup text with a relay adds the link.",
    )

    val noSettingsYet = w(
        Tones.INFO, "No settings yet",
        "The phone holds no settings for the shop yet, so there is nothing to show.",
        Who.OWNER,
        "Nothing to do. They arrive with the first approval that carries settings.",
    )

    // ---- the till ------------------------------------------------------------------------------

    val posNotConnected = w(
        Tones.INFO, "No till connected",
        "This phone is not reading any till, so no order is matched against the counters' sales.",
        Who.OWNER,
        "Choose the till's vendor, type its location, and paste a read-only sign-in, then connect.",
    )

    val posNeedsLink = w(
        Tones.INFO, "Till needs the link",
        "Matching a till needs the link to the counters, and this phone was set up without a relay.",
        Who.OWNER,
        "Nothing to do here. A setup text with a relay adds the link.",
    )

    val posTokenNotAccepted = w(
        Tones.STOP, "Sign-in not accepted",
        "A read-only sign-in is 1 to 512 letters, digits and signs, with no space in it.",
        Who.OWNER,
        "Paste it again from the till's system, or make a new one there.",
    )

    val posConnected = w(
        Tones.GOOD, "Till connected",
        "The till is connected, and its sign-in is sealed on this phone and is not shown again.",
        Who.NOBODY,
        "Nothing to do. Keep the app open: the till is read about once a minute.",
    )

    val posRemoved = w(
        Tones.INFO, "Till taken off",
        "The till's settings, its sign-in and the matches kept for it are removed from this phone.",
        Who.NOBODY,
        "Nothing to do. Connect a till again when you want its orders matched.",
    )

    val posAcknowledged = w(
        Tones.INFO, "Alert acknowledged",
        "The alert is acknowledged on this phone only: the Box and the counters do not learn of it.",
        Who.NOBODY,
        "Nothing to do.",
    )

    val posTokenMissing = w(
        Tones.STOP, "Sign-in missing",
        "A till is connected, but this phone holds no sign-in for it, so nothing is read.",
        Who.OWNER,
        "Replace the sign-in with a new read-only one from the till's system.",
    )

    val posAddressRefused = w(
        Tones.STOP, "Till address refused",
        "The library asked for an address that is not the till's system, so nothing was sent.",
        Who.OWNER,
        "Close the app and open it again. If it happens again, do not rely on the matching.",
    )

    val posPageTooLong = w(
        Tones.STOP, "Till answer too long",
        "The till's system sent an answer longer than 8 megabytes, so it was not read.",
        Who.OWNER,
        "Nothing to do now. The next read is in about a minute.",
    )

    val posReadCutOff = w(
        Tones.STOP, "Till read cut off",
        "One read asked for more pages than a read may take, so it was ended.",
        Who.OWNER,
        "Nothing to do now. The next read is in about a minute.",
    )

    fun posNotReached(problem: String?) = w(
        Tones.STOP, "Till not reached",
        "The till's system did not answer through Tor, so nothing was read.",
        Who.OWNER,
        "Check that Orbot is running and the phone is online. The next read is in about a minute.",
        detailOf(problem),
    )

    // ---- the node ----------------------------------------------------------------------------

    val starting = w(
        Tones.INFO, "Starting",
        "The owner node is starting and looking for the network.",
        Who.NOBODY,
        "Wait a moment. Keep the app open.",
    )

    val stopped = w(
        Tones.STOP, "Owner node stopped",
        "The owner node has stopped, so the wallet is no longer followed.",
        Who.OWNER,
        "Close the app and open it again.",
    )

    // ---- the app's own faults --------------------------------------------------------------

    fun libraryMissing(problem: String?) = w(
        Tones.STOP, "The app is incomplete",
        "The part of this app that runs the owner node did not load, so nothing was done.",
        Who.OWNER,
        "Do not use this copy. Install the app again from the place you got it.",
        detailOf(problem),
    )

    /** An error the Kotlin side made itself, with no words from the library. */
    fun fault(e: OwnerException) = w(
        Tones.STOP, "Answer not readable",
        "The app could not read an answer from the owner node, so nothing was changed.",
        Who.OWNER,
        "Close the app and open it again. If it happens again, stop and do not spend.",
        "${e.rule}: ${e.text}".take(200),
    )

    fun storageUnavailable(problem: String?) = w(
        Tones.STOP, "Phone storage not usable",
        "The phone's protected storage did not start, so the saved setup was not opened.",
        Who.OWNER,
        "Restart the phone and open this app again. If it happens again, stop and do not spend.",
        detailOf(problem),
    )

    fun fileNotRead(problem: String?) = w(
        Tones.STOP, "File not read",
        "The file could not be opened or read, so nothing was done.",
        Who.OWNER,
        "Pick the file again, or paste the text instead.",
        detailOf(problem),
    )

    fun fileNotSaved(problem: String?) = w(
        Tones.STOP, "Not saved",
        "The app could not write to this phone's storage, so nothing was saved.",
        Who.OWNER,
        "Try again. If it happens again, free some space on the phone.",
        detailOf(problem),
    )

    /** SC-12: the list of words as the table of docs/screens/owner-app.md. */
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
            "# Owner app: what the app itself tells a person\n\n" +
                "Made by `AppWords.screensMarkdown` in\n" +
                "`apps/owner-android/src/model/AppWords.kt`. Do not edit it by hand: run\n" +
                "`apps/owner-android/tests/run-jvm.sh` with the environment variable\n" +
                "`OPENBMT_WRITE_SCREENS=1` to write it again.\n\n" +
                "Everything about the coins, the payments, the queue, the link, the\n" +
                "old addresses and the refusals of the rules is shown with the words of\n" +
                "the library (`docs/screens/owner.md`, `docs/screens/owner-node.md`,\n" +
                "`openbmt-owner-ffi`). These are the words of what the app decides itself.\n\n" +
                "| State | Heading | What happened | What to do next | Who | Code |\n" +
                "|---|---|---|---|---|---|\n",
        )
        for ((name, w) in all()) {
            out.append("| ${cell(name)} | ${cell(w.heading)} | ${cell(w.what)} | ${cell(w.next)} | ${who(w.who)} | ${w.code ?: ""} |\n")
        }
        return out.toString()
    }

    /** Every fixed words of the app, for the test that checks them all and writes the list. */
    fun all(): List<Pair<String, SayWords>> = listOf(
        "notSetUp" to notSetUp,
        "setupSaved" to setupSaved,
        "setupNotReadable" to setupNotReadable("a problem"),
        "torAddressNotAccepted" to torAddressNotAccepted,
        "torAlwaysOn" to torAlwaysOn,
        "orbotNotRunning" to orbotNotRunning,
        "notANumber" to notANumber,
        "counterNeeded" to counterNeeded,
        "counterNotAccepted" to counterNotAccepted,
        "responseNeeded" to responseNeeded,
        "responseNotReadable" to responseNotReadable("a problem"),
        "requestReady" to requestReady,
        "requestCopied" to requestCopied,
        "requestCodeAnimated" to requestCodeAnimated,
        "requestCodeStill" to requestCodeStill,
        "requestCodeNotMade" to requestCodeNotMade,
        "scanCancelled" to scanCancelled,
        "scanNotAResponse" to scanNotAResponse,
        "counterReading" to counterReading,
        "counterNotACode" to counterNotACode,
        "counterScanCancelled" to counterScanCancelled,
        "counterCodeNotRequest" to counterCodeNotRequest,
        "sharesNote" to sharesNote(2),
        "sharesNoteOne" to sharesNote(1),
        "counterCompare" to counterCompare,
        "counterRateNeeded" to counterRateNeeded,
        "counterCancelled" to counterCancelled,
        "counterNotPaired" to counterNotPaired,
        "counterCodeAnimated" to counterCodeAnimated,
        "counterCodeStill" to counterCodeStill,
        "counterCodeNoBatch" to counterCodeNoBatch,
        "counterCodeNotMade" to counterCodeNotMade,
        "counterDelivered" to counterDelivered,
        "ceremonyMakingKey" to ceremonyMakingKey,
        "ceremonyScanNext" to ceremonyScanNext,
        "ceremonyNotAnAnswer" to ceremonyNotAnAnswer,
        "ceremonyCodeTooLong" to ceremonyCodeTooLong,
        "ceremonyScanCancelled" to ceremonyScanCancelled,
        "ceremonyCompareRoot" to ceremonyCompareRoot,
        "ceremonyRootDiffers" to ceremonyRootDiffers,
        "ceremonyKey2Needed" to ceremonyKey2Needed,
        "ceremonyKey2Text" to ceremonyKey2Text,
        "ceremonyKey2ScanCancelled" to ceremonyKey2ScanCancelled,
        "ceremonyBuilding" to ceremonyBuilding,
        "ceremonyCheckWallet" to ceremonyCheckWallet,
        "ceremonyCodeAnimated" to ceremonyCodeAnimated,
        "ceremonyCodeStill" to ceremonyCodeStill,
        "ceremonyCodeNotMade" to ceremonyCodeNotMade,
        "ceremonyCompareSetup" to ceremonyCompareSetup,
        "ceremonyWordsDiffer" to ceremonyWordsDiffer,
        "ceremonyScanAgain" to ceremonyScanAgain,
        "ceremonyFinishing" to ceremonyFinishing,
        "ceremonySaveNext" to ceremonySaveNext,
        "ceremonyResume" to ceremonyResume,
        "ceremonyStartAgain" to ceremonyStartAgain,
        "ceremonyThrownAway" to ceremonyThrownAway,
        "ceremonyRecordNotRead" to ceremonyRecordNotRead("a problem"),
        "setupTextNeeded" to setupTextNeeded,
        "requestSaved" to requestSaved("request.obm"),
        "refundQueuedAtDepth" to refundQueuedAtDepth,
        "refundQueuedWaiting" to refundQueuedWaiting,
        "cashOutQueued" to cashOutQueued,
        "entryCancelled" to entryCancelled,
        "entryRequeued" to entryRequeued,
        "sessionAbandoned" to sessionAbandoned,
        "alertCleared" to alertCleared,
        "noteDismissed" to noteDismissed,
        "noLink" to noLink,
        "noSettingsYet" to noSettingsYet,
        "posNotConnected" to posNotConnected,
        "posNeedsLink" to posNeedsLink,
        "posTokenNotAccepted" to posTokenNotAccepted,
        "posConnected" to posConnected,
        "posRemoved" to posRemoved,
        "posAcknowledged" to posAcknowledged,
        "posTokenMissing" to posTokenMissing,
        "posAddressRefused" to posAddressRefused,
        "posPageTooLong" to posPageTooLong,
        "posReadCutOff" to posReadCutOff,
        "posNotReached" to posNotReached("a problem"),
        "starting" to starting,
        "stopped" to stopped,
        "libraryMissing" to libraryMissing("no library"),
        "fault" to fault(OwnerException("response-null", "the library gave no response", 4)),
        "storageUnavailable" to storageUnavailable("a problem"),
        "fileNotRead" to fileNotRead("a problem"),
        "fileNotSaved" to fileNotSaved("a problem"),
        "cameraOff" to CameraWords.off(Who.OWNER),
        "cameraNotStarted" to CameraWords.notStarted(Who.OWNER, "a problem"),
    ) + UrWords.forReading(Who.OWNER)
}
