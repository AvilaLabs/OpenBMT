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
        "Type a name for the counter, the manager's name and a PIN. Then tap Pair with the Key app and scan its code with this phone.",
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
        "Type a shorter name for this counter, then tap Pair with the Key app again.",
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
        "The shop's settings send every connection of this counter through Tor.",
        Who.MANAGER,
        "Nothing to do. The manager can choose how under Tor.",
    )

    val torIsOff = w(
        Tones.INFO, "Tor is off for this shop",
        "The shop's settings do not use Tor, so the price sources and the network can see this shop's address.",
        Who.OWNER,
        "Nothing to do here. The owner can switch Tor on in the shop's settings.",
    )

    // ---- the network (CR-87) -------------------------------------------------------------

    val networkAbout = w(
        Tones.INFO, "Network",
        "When the shop's settings turn Tor on and this counter uses Orbot, it sends everything through Orbot, at the address below.",
        Who.MANAGER,
        "Change the address only if Orbot listens somewhere else, then type the manager's PIN.",
    )

    val networkSaved = w(
        Tones.GOOD, "Address saved",
        "The counter will look for Orbot at the new address when it uses Orbot.",
        Who.MANAGER,
        "Tap Start to use it now.",
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

    // ---- Lightning through the shop's own node (CR-112 to CR-116) ---------------------------

    val lightningAbout = w(
        Tones.INFO, "Lightning through your node",
        "The counter only asks your node for invoices, so give it a key limited to invoices, from a node that runs a current release.",
        Who.MANAGER,
        "Scan or paste the LND code, or fill in the three Core Lightning fields, type the manager's PIN, then test and save.",
    )

    val lightningCare = w(
        Tones.INFO, "Running a Lightning node",
        "A node that takes payments needs channels with room to receive, updates kept current and a backup of its own.",
        Who.OWNER,
        "Watch the channel balances, install each update soon after it comes out, and keep a backup of the node.",
    )

    val lightningTesting = w(
        Tones.INFO, "Testing the node",
        "The counter is asking your node for a test invoice and checking that it is yours.",
        Who.MANAGER,
        "Wait a moment while the node answers.",
    )

    val lightningFieldsNeeded = w(
        Tones.STOP, "Lightning fields needed",
        "The connection needs its code, or for Core Lightning its address, its key and its certificate.",
        Who.MANAGER,
        "Fill them in, then test and save again.",
    )

    val lightningNotReached = w(
        Tones.STOP, "Test did not finish",
        "Your node gave no answer the counter could use, so nothing was saved.",
        Who.MANAGER,
        "Check the address and that the node is running, then try again.",
    )

    val lightningSaved = w(
        Tones.GOOD, "Lightning saved",
        "The test passed, and the connection is saved on this phone where only this app can read it.",
        Who.MANAGER,
        "Wait a moment while the counter starts again.",
    )

    val lightningRemoved = w(
        Tones.INFO, "Lightning removed",
        "The connection to your node is deleted from this phone, and sales show the on-chain code only.",
        Who.MANAGER,
        "Nothing more to do.",
    )

    val lightningWaiting = w(
        Tones.INFO, "Lightning code coming",
        "The counter is asking the shop's node for a Lightning code for this sale.",
        Who.CASHIER,
        "Let the customer pay on-chain with the code above, or wait a few seconds.",
    )

    // ---- Tor and Orbot ------------------------------------------------------------------

    /** The built-in Tor, which is the choice of a new counter (CR-201, CR-203). */
    val torBuiltIn = w(
        Tones.INFO, "Tor",
        "This app runs Tor itself, so Orbot is not needed.",
        Who.NOBODY,
        "Nothing to do. To use Orbot instead, choose Use Orbot.",
    )

    val torUsesOrbot = w(
        Tones.INFO, "Tor through Orbot",
        "This app sends everything through Orbot, which must be installed and running.",
        Who.OWNER,
        "Keep Orbot running. To stop needing it, choose Built-in Tor.",
    )

    val torStarting = w(
        Tones.INFO, "Starting Tor",
        "This app is connecting to the Tor network, which can take up to a minute.",
        Who.NOBODY,
        "Wait on this screen.",
    )

    val torFailed = w(
        Tones.STOP, "Tor did not start",
        "This app could not connect to the Tor network.",
        Who.OWNER,
        "Check that the phone is online, then try again. If it keeps failing, choose Use Orbot.",
    )

    val orbotNotRunning = w(
        Tones.STOP, "Tor is not running",
        "Tor is on for this shop, but Orbot is not running.",
        Who.MANAGER,
        "Open Orbot and turn it on. If it listens at another address, the manager can change it under Network.",
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

    // ---- cashiers and the till (CR-140 to CR-151) ------------------------------------------

    val staffPinNeeded = w(
        Tones.INFO, "Manager's PIN needed",
        "The list of cashiers is changed only with the manager's PIN.",
        Who.MANAGER,
        "Type the manager's PIN, then tap Open.",
    )

    val staffAbout = w(
        Tones.INFO, "Staff",
        "Cashiers sign in at the till with their name and PIN. Each one added or removed is written down for the owner.",
        Who.MANAGER,
        "Add a cashier below, or remove one, or give one a new PIN.",
    )

    val cashierNameNotAccepted = w(
        Tones.STOP, "Cashier name not accepted",
        "A cashier's name is 1 to 24 characters long, with no space at either end, so it was not used.",
        Who.MANAGER,
        "Type another name, then add the cashier again.",
    )

    val cashierNameUsed = w(
        Tones.STOP, "Name already used",
        "Another cashier or the manager has that name, whatever the capital letters, so it was not used.",
        Who.MANAGER,
        "Type a different name, then add the cashier again.",
    )

    val cashiersFull = w(
        Tones.STOP, "Cashier list is full",
        "The list holds at most 30 cashiers, so no one more can be added.",
        Who.MANAGER,
        "Remove a cashier first, then add the new one.",
    )

    val cashierPinNotAccepted = w(
        Tones.STOP, "Cashier PIN not accepted",
        "A cashier's PIN is 4 to 8 digits, and the two entries must be the same, so nothing was saved.",
        Who.CASHIER,
        "Type the PIN twice, then save again.",
    )

    val cashierAdded = w(
        Tones.GOOD, "Cashier added",
        "The cashier is on the list, and the owner's phone gets a record of it.",
        Who.CASHIER,
        "Sign in at the till with the name and the PIN.",
    )

    val cashierRemoved = w(
        Tones.INFO, "Cashier removed",
        "The cashier is off the list and can no longer sign in, and the owner's phone gets a record of it.",
        Who.NOBODY,
        "Nothing more to do.",
    )

    val cashierPinSaved = w(
        Tones.GOOD, "New PIN saved",
        "The cashier's new PIN is saved on this phone. Nothing about it is sent to the owner.",
        Who.CASHIER,
        "Sign in at the till with the new PIN.",
    )

    val ownerNotTold = w(
        Tones.STOP, "Owner not told",
        "The list of cashiers was changed, but the record for the owner could not be written.",
        Who.MANAGER,
        "Tell the owner in person that the list changed.",
    )

    fun cashiersNotRead(problem: String?) = w(
        Tones.STOP, "Cashier list not read",
        "The list of cashiers could not be read, so nobody can sign in and nothing was done.",
        Who.MANAGER,
        "Try again. If it happens again, stop and tell the owner in person.",
        detailOf(problem),
    )

    val whoAtTill = w(
        Tones.INFO, "Who is at the till?",
        "The till is locked, so no sale can be opened until a person signs in.",
        Who.CASHIER,
        "Tap your name, then type your PIN.",
    )

    fun signInWrong(waitSecs: Long) = w(
        Tones.STOP, "PIN not right",
        "That PIN is not the one for this name, so nobody was signed in.",
        Who.CASHIER,
        "Wait ${waitText(waitSecs)}, then try again.",
    )

    fun signInHeld(waitSecs: Long) = w(
        Tones.STOP, "PIN held for now",
        "There were wrong tries in a row on this name, so its PIN is held for a while.",
        Who.CASHIER,
        "Wait ${waitText(waitSecs)}, then try again, or tap Back and pick another name.",
    )

    val overrideAbout = w(
        Tones.INFO, "Accept by hand",
        "Accepting a sale by hand is written down with the name of the person who does it, and the limits apply to each person.",
        Who.CASHIER,
        "Tap Accept, or choose the manager and type the manager's PIN.",
    )

    // ---- today's totals (CR-152 to CR-155) -------------------------------------------------

    val todayAbout = w(
        Tones.INFO, "Today at this counter",
        "These totals are counted from this counter's own records since midnight. No wallet balance is shown here.",
        Who.OWNER,
        "The owner's phone has the full books and the export.",
    )

    val todayWaiting = w(
        Tones.INFO, "Why sales are waiting",
        "A sale waits until its payment is accepted: it may be unpaid, part paid, or paid and not yet confirmed.",
        Who.CASHIER,
        "Open the sale from Sales to see which. The count changes once the payment is accepted or the sale ends.",
    )

    val todayStopped = w(
        Tones.STOP, "Sales not to be handed over",
        "A payment for these sales was replaced or reopened, so the goods are not to be handed over.",
        Who.MANAGER,
        "Open the sale from Sales and follow what it says.",
    )

    val todayPeopleNeedPin = w(
        Tones.INFO, "Lines per person",
        "The lines per person show who opened each sale and did each override or refund, so they need the manager's PIN.",
        Who.MANAGER,
        "Type the manager's PIN, then tap Show the lines.",
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

    /** CR-214: the page of a request just made; [date] is when it ends and [sats] what the customer sends. */
    fun requestMade(date: String, sats: String) = w(
        Tones.INFO, "Request ready",
        "The customer can pay this request from anywhere until $date. The amount in bitcoin is fixed at $sats.",
        Who.CASHIER,
        "Share the link or the code with the customer. The counter shows Paid once the payment is confirmed.",
    )

    /** CR-214: the page of a request that is open again, from the list on Home. */
    fun requestOpen(date: String) = w(
        Tones.INFO, "Waiting for payment",
        "This request is open until $date. It is paid once the payment is confirmed.",
        Who.NOBODY,
        "Nothing to do.",
    )

    val refundAsked = w(
        Tones.GOOD, "Refund asked for",
        "The refund request is saved, and it is paid only after the owner approves it.",
        Who.CASHIER,
        "Nothing more to do here. Tell the customer the owner pays refunds from the owner's phone.",
    )

    val refundWithdrawn = w(
        Tones.INFO, "Refund withdrawn",
        "The withdrawal is sent to the owner. A refund the owner has already started approving may still be paid.",
        Who.CASHIER,
        "Do not give it in cash until the owner says it was dropped.",
    )

    /** CR-217: on the details of a sale paid over Lightning, in place of the button that asks for a bitcoin refund. */
    val lightningRefundCash = w(
        Tones.ENDED, "Refund in cash",
        "This sale was paid over Lightning, and the owner's phone can only check payments it sees on the chain, so a bitcoin refund cannot be sent.",
        Who.CASHIER,
        "Give the refund in cash from the till.",
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
        "torBuiltIn" to torBuiltIn,
        "torUsesOrbot" to torUsesOrbot,
        "torStarting" to torStarting,
        "torFailed" to torFailed,
        "networkAbout" to networkAbout,
        "networkSaved" to networkSaved,
        "ownNodeAbout" to ownNodeAbout,
        "ownNodeSaved" to ownNodeSaved,
        "ownNodeRemoved" to ownNodeRemoved,
        "lightningAbout" to lightningAbout,
        "lightningCare" to lightningCare,
        "lightningTesting" to lightningTesting,
        "lightningFieldsNeeded" to lightningFieldsNeeded,
        "lightningNotReached" to lightningNotReached,
        "lightningSaved" to lightningSaved,
        "lightningRemoved" to lightningRemoved,
        "lightningWaiting" to lightningWaiting,
        "managerNeeded" to managerNeeded,
        "pinWrong" to pinWrong(4),
        "pinLocked" to pinLocked(300),
        "staffPinNeeded" to staffPinNeeded,
        "staffAbout" to staffAbout,
        "cashierNameNotAccepted" to cashierNameNotAccepted,
        "cashierNameUsed" to cashierNameUsed,
        "cashiersFull" to cashiersFull,
        "cashierPinNotAccepted" to cashierPinNotAccepted,
        "cashierAdded" to cashierAdded,
        "cashierRemoved" to cashierRemoved,
        "cashierPinSaved" to cashierPinSaved,
        "ownerNotTold" to ownerNotTold,
        "cashiersNotRead" to cashiersNotRead("a problem"),
        "whoAtTill" to whoAtTill,
        "signInWrong" to signInWrong(4),
        "signInHeld" to signInHeld(300),
        "overrideAbout" to overrideAbout,
        "todayAbout" to todayAbout,
        "todayWaiting" to todayWaiting,
        "todayStopped" to todayStopped,
        "todayPeopleNeedPin" to todayPeopleNeedPin,
        "priceReady" to priceReady("73,554.18 USD", 3),
        "sourceNotAnswered" to sourceNotAnswered,
        "sourceRefusedStatus" to sourceRefusedStatus(503),
        "sourceReplyTooLong" to sourceReplyTooLong,
        "amountNeeded" to amountNeeded,
        "tipChoose" to WordsOfTips.choose("12.50 USD"),
        "cameraOff" to cameraOff,
        "cameraNotStarted" to cameraNotStarted("a problem"),
        "notANumber" to notANumber,
        "nameNeeded" to nameNeeded,
        "destinationNeeded" to destinationNeeded,
        "reasonNeeded" to reasonNeeded,
        "requestCopied" to requestCopied,
        "requestMade" to requestMade("2026-11-14 10:00", "73,554 sats"),
        "requestOpen" to requestOpen("2026-11-14 10:00"),
        "refundAsked" to refundAsked,
        "refundWithdrawn" to refundWithdrawn,
        "lightningRefundCash" to lightningRefundCash,
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
