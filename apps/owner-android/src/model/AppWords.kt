package org.openbmt.owner.app

import org.openbmt.app.Format
import org.openbmt.app.SayWords
import org.openbmt.app.Tones
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
        "Paste the setup text from the key ceremony, or choose its file, then save.",
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

    // ---- what was done --------------------------------------------------------------------

    val requestReady = w(
        Tones.GOOD, "Request ready",
        "The request for the Key app is ready, and nothing has been signed yet.",
        Who.OWNER,
        "Copy it or save it as a file, give it to the Key app, then bring back its response.",
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
    )
}
