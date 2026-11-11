package org.openbmt.counter.app

import org.openbmt.app.Format
import org.openbmt.app.SayWords
import org.openbmt.app.Tones
import org.openbmt.app.UrWords
import org.openbmt.app.Who
import org.openbmt.counter.CounterException

/**
 * The words of the counter app's own states and refusals (docs/SCREEN-RULES.md).
 * The library gives words for what its node knows (the sale, the health of the
 * check, the price, a refusal of a rule); these are the words of everything
 * the app itself decides: setting up, the PIN, Tor and Orbot, a price source
 * that did not answer, and what was typed. The list is written to
 * docs/screens/counter-app.md by the JVM test, and every one of them passes
 * the checks of `openbmt_core::say::check` as far as text can be checked.
 */
object AppWords {
    private fun w(tone: Int, heading: String, what: String, who: Int, next: String, detail: String? = null) =
        SayWords(tone, who, heading, what, next, null, null, null, detail)

    private fun detailOf(problem: String?): String? = problem?.let { Format.cleanForeign(it) }?.takeIf { it.isNotEmpty() }

    // ---- setting up ------------------------------------------------------------------

    val notSetUp = w(
        Tones.INFO, "Not set up yet",
        "This counter has not been paired with the shop yet, so it cannot take a sale.",
        Who.MANAGER,
        "Type a name, a PIN and the address of Orbot, then tap Pair, and scan the Key app's code with this phone.",
    )

    val pairingSaved = w(
        Tones.GOOD, "Counter set up",
        "The counter is paired with the shop, and the pairing and the PIN are saved on this phone.",
        Who.CASHIER,
        "Keep the app open at the counter. It starts looking for the network now.",
    )

    // ---- pairing, step by step (CR-98) -----------------------------------------------

    val pairingScanOffer = w(
        Tones.INFO, "Scan the Key app",
        "The Key app shows a code on its Pair a counter screen. This counter reads the shop's keys and network from it.",
        Who.MANAGER,
        "Open Pair a counter on the Key app, then tap Scan and hold this phone's camera to its code.",
    )

    val pairingCompare = w(
        Tones.INFO, "Compare the four words",
        "The Key app shows four words for this pairing, and the same four are below. If they are not the same, someone changed the code on the way.",
        Who.MANAGER,
        "Check all four words on the Key app, then tap They match, or tap They differ if any word is not the same.",
    )

    val pairingScanBundle = w(
        Tones.INFO, "Scan the owner's phone",
        "The four words matched. The owner's phone now shows the code that finishes the pairing.",
        Who.MANAGER,
        "On the owner's phone open the counter's code, then tap Scan and hold this phone's camera to it.",
    )

    /** The app was left while a pairing was open: the library forgot it and nothing was saved. */
    val pairingForgotten = w(
        Tones.STOP, "Pairing not finished",
        "The app was closed or left before the pairing was finished, so it was forgotten and nothing was saved.",
        Who.MANAGER,
        "Start again from the Key app: open Pair a counter, then tap Pair on this counter and scan its code.",
    )

    val pinNotAccepted = w(
        Tones.STOP, "PIN not accepted",
        "A PIN is 6 to 12 digits, and the two entries must be the same, so nothing was saved.",
        Who.MANAGER,
        "Type a PIN of 6 to 12 digits twice, then save again.",
    )

    val counterNameNotAccepted = w(
        Tones.STOP, "Counter name not accepted",
        "The name of this counter is 1 to 32 characters long, so it was not used.",
        Who.MANAGER,
        "Type a shorter name for this counter, then tap Pair again.",
    )

    val nameNotAccepted = w(
        Tones.STOP, "Name not accepted",
        "A name is 1 to 64 characters long, so it was not saved.",
        Who.CASHIER,
        "Type the name again.",
    )

    val torAddressNotAccepted = w(
        Tones.STOP, "Tor address not accepted",
        "The address of Orbot must be a host, a colon and a port, like 127.0.0.1:9050.",
        Who.MANAGER,
        "Type the address again.",
    )

    val torIsOn = w(
        Tones.INFO, "Tor is on for this shop",
        "The shop's settings send every connection of this counter through Tor, with Orbot.",
        Who.MANAGER,
        "Check the address of Orbot below, and keep Orbot running.",
    )

    val torIsOff = w(
        Tones.INFO, "Tor is off for this shop",
        "The shop's settings do not use Tor, so the price sources and the network can see this shop's address.",
        Who.OWNER,
        "Nothing to do here. The owner can switch Tor on in the shop's settings.",
    )

    // ---- the own node (CR-100 to CR-102) ---------------------------------------------------

    val ownNodeAbout = w(
        Tones.INFO, "Your own node",
        "A node of your own, reached by a Tor address, can check each payment as well as the light check does.",
        Who.MANAGER,
        "Type its Tor address and the manager's PIN, then save.",
    )

    val ownNodeSaved = w(
        Tones.INFO, "Own node saved",
        "The counter will ask your own node about each payment, once it has started again.",
        Who.MANAGER,
        "Wait a moment while the counter starts again.",
    )

    val ownNodeRemoved = w(
        Tones.INFO, "Own node removed",
        "The counter checks payments with the light check only.",
        Who.MANAGER,
        "Wait a moment while the counter starts again.",
    )

    // ---- Tor and Orbot ------------------------------------------------------------------

    val orbotNotRunning = w(
        Tones.STOP, "Tor is not running",
        "Tor is on for this shop, but Orbot is not running.",
        Who.MANAGER,
        "Open Orbot and turn it on.",
    )

    // ---- the PIN -------------------------------------------------------------------------

    val managerNeeded = w(
        Tones.INFO, "A manager is needed",
        "This step needs the manager's PIN.",
        Who.MANAGER,
        "Ask the manager to type the PIN.",
    )

    fun pinWrong(waitSecs: Long) = w(
        Tones.STOP, "Wrong PIN",
        "That PIN is not right, so nothing was done.",
        Who.MANAGER,
        "Wait ${waitText(waitSecs)}, then try again.",
    )

    fun pinLocked(waitSecs: Long) = w(
        Tones.STOP, "PIN locked for now",
        "There were wrong tries in a row, so the PIN is locked for a while.",
        Who.MANAGER,
        "Wait ${waitText(waitSecs)}, then try again.",
    )

    /** "4 seconds", "2 minutes": the wait of a locked PIN, said to be read. */
    fun waitText(secs: Long): String = when {
        secs <= 1 -> "1 second"
        secs < 60 -> "$secs seconds"
        secs < 120 -> "1 minute"
        else -> "${(secs + 59) / 60} minutes"
    }

    // ---- the price ------------------------------------------------------------------------

    /** The price a sale would be opened at: [price] is already written for reading, like 73,554.18 USD. */
    fun priceReady(price: String, sources: Int) = w(
        Tones.GOOD, "Price ready",
        "Bitcoin is priced at $price, from $sources price sources.",
        Who.NOBODY,
        "Nothing to do. The app asks the sources again every minute.",
    )

    val sourceNotAnswered = w(
        Tones.INFO, "Price source not reached",
        "A price source did not answer in time, so its price is missing this turn.",
        Who.NOBODY,
        "Nothing to do. The app asks again in a minute.",
    )

    fun sourceRefusedStatus(code: Int) = w(
        Tones.INFO, "Price source said no",
        "A price source answered with a refusal, so its price is missing this turn.",
        Who.NOBODY,
        "Nothing to do. The app asks again in a minute.",
        "Status $code.",
    )

    val sourceReplyTooLong = w(
        Tones.INFO, "Price reply too long",
        "A price source sent more than the app reads, so its price is missing this turn.",
        Who.NOBODY,
        "Nothing to do. The app asks again in a minute.",
    )

    // ---- what was typed -------------------------------------------------------------------

    val amountNeeded = w(
        Tones.INFO, "Amount needed",
        "Type an amount above zero to start a sale.",
        Who.CASHIER,
        "Press the digits of the amount, then press Open.",
    )

    // ---- the camera ----------------------------------------------------------------

    /** The camera permission was refused, or "don't ask again" was chosen. */
    val cameraOff = w(
        Tones.STOP, "Camera is off for this app",
        "This app is not allowed to use the camera, and pairing needs it to read the codes, so nothing was paired.",
        Who.MANAGER,
        "Allow the camera for this app in the phone's settings, then start the pairing again.",
    )

    fun cameraNotStarted(problem: String?) = w(
        Tones.STOP, "Camera did not start",
        "The camera could not be opened, so no code was read and nothing was paired.",
        Who.MANAGER,
        "Close any other app that uses the camera, then start the pairing again.",
        detailOf(problem)?.take(200),
    )

    val notANumber = w(
        Tones.STOP, "Check what you typed",
        "The amount is empty or has something other than digits in it.",
        Who.CASHIER,
        "Type whole numbers only, then try again.",
    )

    val nameNeeded = w(
        Tones.INFO, "Name needed",
        "A refund or an override is written down with the name of the person who asked for it.",
        Who.CASHIER,
        "Type your name on the home screen, then try again.",
    )

    val destinationNeeded = w(
        Tones.INFO, "Address needed",
        "A refund needs the address the customer wants the money sent to.",
        Who.CASHIER,
        "Paste the address or the payment request the customer shows, then try again.",
    )

    val reasonNeeded = w(
        Tones.INFO, "Reason needed",
        "A refund is written down with the reason for it.",
        Who.CASHIER,
        "Type a short reason, then try again.",
    )

    // ---- what was done --------------------------------------------------------------------

    val requestCopied = w(
        Tones.INFO, "Request copied",
        "The payment request is copied, written in lower case.",
        Who.CASHIER,
        "Paste it where the customer can use it.",
    )

    val refundAsked = w(
        Tones.GOOD, "Refund asked for",
        "The refund request is saved, and it is paid only after the owner approves it.",
        Who.CASHIER,
        "Nothing more to do here. Tell the customer the owner pays refunds from the owner's phone.",
    )

    val refundWithdrawn = w(
        Tones.INFO, "Refund withdrawn",
        "The request is withdrawn, and the owner is told it is not wanted.",
        Who.NOBODY,
        "Nothing to do. If the owner has already paid it, tell the owner.",
    )

    val cashRefunded = w(
        Tones.GOOD, "Cash refund saved",
        "The cash refund from the till is written down, and the bitcoin is kept.",
        Who.CASHIER,
        "Give the customer the cash.",
    )

    // ---- the node ----------------------------------------------------------------------------

    val starting = w(
        Tones.INFO, "Starting",
        "The counter is starting and looking for the network.",
        Who.NOBODY,
        "Wait a moment. Keep the app open.",
    )

    val stopped = w(
        Tones.STOP, "Counter stopped",
        "The counter has stopped, so no sale can be opened.",
        Who.CASHIER,
        "Close the app and open it again. If it stays stopped, take another kind of payment.",
    )

    // ---- the app's own faults --------------------------------------------------------------

    fun libraryMissing(problem: String?) = w(
        Tones.STOP, "The app is incomplete",
        "The part of this app that runs the counter did not load, so nothing was done.",
        Who.OWNER,
        "Do not use this copy. Install the app again from the place you got it.",
        detailOf(problem),
    )

    /** An error the Kotlin side made itself, with no words from the library. */
    fun fault(e: CounterException) = w(
        Tones.STOP, "Answer not readable",
        "The app could not read an answer from the counter part, so nothing was changed.",
        Who.OWNER,
        "Close the app and open it again. If it happens again, stop and tell the owner in person.",
        "${e.rule}: ${e.text}".take(200),
    )

    fun storageUnavailable(problem: String?) = w(
        Tones.STOP, "Phone storage not usable",
        "The phone's protected storage did not start, so the saved pairing was not opened.",
        Who.OWNER,
        "Restart the phone and open this app again. If it happens again, stop and tell the owner in person.",
        detailOf(problem),
    )

    fun fileNotRead(problem: String?) = w(
        Tones.STOP, "File not read",
        "The file could not be opened or read, so nothing was done.",
        Who.MANAGER,
        "Try again. If it happens again, stop and tell the owner in person.",
        detailOf(problem),
    )

    fun fileNotSaved(problem: String?) = w(
        Tones.STOP, "Not saved",
        "The app could not write to this phone's storage, so nothing was saved.",
        Who.MANAGER,
        "Try again. If it happens again, stop and tell the owner in person.",
        detailOf(problem),
    )

    /** SC-12: the list of words as the table of docs/screens/counter-app.md. */
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
            "# Counter app: what the app itself tells a person\n\n" +
                "Made by `AppWords.screensMarkdown` in\n" +
                "`apps/counter-android/src/model/AppWords.kt`. Do not edit it by hand: run\n" +
                "`apps/counter-android/tests/run-jvm.sh` with the environment variable\n" +
                "`OPENBMT_WRITE_SCREENS=1` to write it again.\n\n" +
                "Everything about a sale, the health of the check, the price and the\n" +
                "refusals of the rules is shown with the words of the library\n" +
                "(`docs/screens/counter.md`, `docs/screens/rates.md`, `openbmt-counter-ffi`).\n" +
                "These are the words of what the app decides itself.\n\n" +
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
        "pairingSaved" to pairingSaved,
        "pairingScanOffer" to pairingScanOffer,
        "pairingCompare" to pairingCompare,
        "pairingScanBundle" to pairingScanBundle,
        "pairingForgotten" to pairingForgotten,
        "counterNameNotAccepted" to counterNameNotAccepted,
        "pinNotAccepted" to pinNotAccepted,
        "nameNotAccepted" to nameNotAccepted,
        "torAddressNotAccepted" to torAddressNotAccepted,
        "torIsOn" to torIsOn,
        "torIsOff" to torIsOff,
        "orbotNotRunning" to orbotNotRunning,
        "ownNodeAbout" to ownNodeAbout,
        "ownNodeSaved" to ownNodeSaved,
        "ownNodeRemoved" to ownNodeRemoved,
        "managerNeeded" to managerNeeded,
        "pinWrong" to pinWrong(4),
        "pinLocked" to pinLocked(300),
        "priceReady" to priceReady("73,554.18 USD", 3),
        "sourceNotAnswered" to sourceNotAnswered,
        "sourceRefusedStatus" to sourceRefusedStatus(503),
        "sourceReplyTooLong" to sourceReplyTooLong,
        "amountNeeded" to amountNeeded,
        "cameraOff" to cameraOff,
        "cameraNotStarted" to cameraNotStarted("a problem"),
        "notANumber" to notANumber,
        "nameNeeded" to nameNeeded,
        "destinationNeeded" to destinationNeeded,
        "reasonNeeded" to reasonNeeded,
        "requestCopied" to requestCopied,
        "refundAsked" to refundAsked,
        "refundWithdrawn" to refundWithdrawn,
        "cashRefunded" to cashRefunded,
        "starting" to starting,
        "stopped" to stopped,
        "libraryMissing" to libraryMissing("no library"),
        "fault" to fault(CounterException("response-null", "the library gave no response", 4)),
        "storageUnavailable" to storageUnavailable("a problem"),
        "fileNotRead" to fileNotRead("a problem"),
        "fileNotSaved" to fileNotSaved("a problem"),
    ) + UrWords.forReading(Who.MANAGER)
}
