package org.openbmt.owner.app

import org.openbmt.app.SayWords
import org.openbmt.app.Tones
import org.openbmt.app.Who

/**
 * Every text of the owner's payment-request screens that the app writes itself (`docs/REQUEST-RULES.md`, RQ-35,
 * RQ-36), listed under the rule of its screen. The library's own words (a refusal, the lines of a row, the state of a
 * request, the line above a link) are shown as they arrive. Each of the app's own messages is listed as its heading
 * and its two sentences, under keys that end in `_HEADING`, `_WHAT` and `_NEXT`. The list is written to
 * docs/screens/owner-app.md by [AppWords.screensMarkdown]. No Android class is used.
 */
object RequestWords {
    private val registry = ArrayList<Pair<String, String>>()

    /** Written words, listed under the key of their rule. */
    private fun say(key: String, text: String): String {
        registry.add(key to text)
        return text
    }

    /** A message of the app's own: its heading and two sentences are listed under [key] with their endings. */
    private fun words(key: String, tone: Int, code: String, heading: String, what: String, next: String): SayWords =
        SayWords(tone, Who.OWNER, say(key + "_HEADING", heading), say(key + "_WHAT", what), say(key + "_NEXT", next), code, null, null, null)

    // ---- Home and the list (RQ-35, RQ-21, RQ-29) ----
    val homeButton = say("RQ_35_HOME_BUTTON", "Request a payment")
    val homeRowTitle = say("RQ_35_HOME_ROW_TITLE", "Payment requests")
    val homeRowSub = say("RQ_35_HOME_ROW_SUB", "Ask a customer to pay from anywhere")
    val listTitle = say("RQ_35_LIST_TITLE", "Payment requests")
    val partReady = say("RQ_25_PART_READY_TO_SEND", "Ready to send")
    val partOverdue = say("RQ_21_PART_OVERDUE", "Overdue")
    val partDueSoon = say("RQ_21_PART_DUE_SOON", "Due soon")
    val partWaiting = say("RQ_21_PART_WAITING", "Waiting")
    val partFinished = say("RQ_35_PART_FINISHED", "Finished")
    val back = say("RQ_35_BACK", "Back")
    val payReturns = say("RQ_29_PAY_RETURNS", "Pay the returns")

    // ---- the page of one request (RQ-35, RQ-14, RQ-19, RQ-20, RQ-16, RQ-22, RQ-25) ----
    val repeatsLine = say("RQ_24_REPEATS_LINE", "This request repeats")
    val shareAgain = say("RQ_14_SHARE_AGAIN", "Share")
    val sendAgain = say("RQ_22_SEND_AGAIN", "Send again")
    val cancelRequest = say("RQ_19_CANCEL", "Cancel the request")
    val closeRequest = say("RQ_20_CLOSE", "Close as it is")
    val countPaid = say("RQ_16_COUNT_PAID", "Count as paid")
    val stopRepeating = say("RQ_25_STOP_REPEATING", "Stop repeating")
    val returnMoney = say("RQ_26_RETURN", "Return money")

    // ---- what the page holds (RQ-15, RQ-12, RQ-26, RQ-29) ----
    val owedLabel = say("RQ_15_OWED_LABEL", "Still owed")
    val paidLabel = say("RQ_15_PAID_LABEL", "Paid so far")
    val requestAddressLabel = say("RQ_15_ADDRESS_LABEL", "Address")
    val quotesLabel = say("RQ_12_QUOTES_LABEL", "Amounts asked")
    val paymentsLabel = say("RQ_15_PAYMENTS_LABEL", "Payments")
    val paymentBlock = say("RQ_15_PAYMENT_BLOCK", "In block")
    val paymentWaiting = say("RQ_15_PAYMENT_WAITING", "Not in a block yet")
    val paymentNotCredited = say("RQ_15_PAYMENT_NOT_CREDITED", "Not counted toward it")
    val returnsLabel = say("RQ_29_RETURNS_LABEL", "Returns")
    val returnQueued = say("RQ_29_RETURN_QUEUED", "Waiting to be paid")
    val returnSent = say("RQ_29_RETURN_SENT", "Sent, not confirmed yet")
    val returnDone = say("RQ_29_RETURN_DONE", "Returned")
    val returnCancelled = say("RQ_29_RETURN_CANCELLED", "Cancelled")

    // ---- the form (RQ-05 to RQ-09, RQ-24) ----
    val formTitle = say("RQ_05_FORM_TITLE", "New request")
    val noteLabel = say("RQ_06_NOTE_LABEL", "What it is for")
    val noteHint = say("RQ_06_NOTE_HINT", "Deposit for a booking")
    val pricedLabel = say("RQ_07_PRICED_LABEL", "Priced in")
    val pricedBitcoin = say("RQ_07_PRICED_BITCOIN", "Bitcoin")
    val pricedCurrency = say("RQ_07_PRICED_CURRENCY", "The shop's currency")
    val amountBitcoinLabel = say("RQ_08_AMOUNT_BITCOIN_LABEL", "Amount in sats")
    val amountCurrencyLabel = say("RQ_08_AMOUNT_CURRENCY_LABEL", "Amount in the shop's currency")
    val dueLabel = say("RQ_09_DUE_LABEL", "Due on")
    val dueHint = say("RQ_09_DUE_HINT", "Year-month-day")
    val repeatLabel = say("RQ_24_REPEAT_LABEL", "Repeats")
    val repeatNone = say("RQ_24_REPEAT_NONE", "Never")
    val repeatWeek = say("RQ_24_REPEAT_WEEK", "Every week")
    val repeatMonth = say("RQ_24_REPEAT_MONTH", "Every month")
    val repeatYear = say("RQ_24_REPEAT_YEAR", "Every year")
    val makeButton = say("RQ_03_MAKE_BUTTON", "Make the request")
    val noteRefused = words(
        "RQ_06_NOTE_REFUSED", Tones.STOP, "RQ-06", "Say what it is for",
        "A request needs a short note of what it is for, so it can be told apart from the others later.",
        "Write a note of up to 64 characters on one line, then make the request again.",
    )
    val amountRefused = words(
        "RQ_08_AMOUNT_REFUSED", Tones.STOP, "RQ-08", "Amount not read",
        "The amount could not be read as an amount above zero that a request can ask for in this way.",
        "Type the amount in digits and check what it is priced in, then make the request again.",
    )
    val dueRefused = words(
        "RQ_09_DUE_REFUSED", Tones.STOP, "RQ-09", "Due date not read",
        "The due date could not be read as a day written as the year, the month and the day.",
        "Write it as year-month-day, such as 2026-12-31, then make the request again.",
    )
    val dueRange = words(
        "RQ_09_DUE_RANGE", Tones.STOP, "RQ-09", "Due date too early or late",
        "A request is due on a day from today to a year from now, and this day is outside that.",
        "Pick a day from today to a year from now, then make the request again.",
    )
    val made = words(
        "RQ_03_MADE", Tones.GOOD, "RQ-03", "Request made",
        "It has an address of its own, and nothing is sent to anyone until you share it.",
        "Share it with the customer as a link, or show its code to a customer who is here.",
    )

    // ---- sharing (RQ-12 to RQ-14) ----
    val shareTitle = say("RQ_12_SHARE_TITLE", "Share the request")
    val rateFetching = words(
        "RQ_12_RATE_FETCHING", Tones.INFO, "RQ-12", "Getting the rate",
        "The shop's price sources are being asked for the rate of now, through Tor.",
        "Wait a moment. Nothing is shared until the rate has come in.",
    )
    val quoteLabel = say("RQ_12_QUOTE_LABEL", "Asked for")
    val quoteSatsLabel = say("RQ_12_QUOTE_SATS_LABEL", "In sats")
    val quoteUntilLabel = say("RQ_12_QUOTE_UNTIL_LABEL", "Holds until")
    val shareLink = say("RQ_13_SHARE_LINK", "Share the link")
    val sendsNothing = say(
        "RQ_13_SENDS_NOTHING",
        "This phone sends nothing itself. Share the link through whatever you already use, and the customer pays it from any wallet.",
    )
    val showCode = say("RQ_13_SHOW_CODE", "Show the code")
    val codeTitle = say("RQ_13_CODE_TITLE", "Code to pay")
    val codeWords = words(
        "RQ_13_CODE", Tones.INFO, "RQ-13", "Scan to pay",
        "This code holds the same link, for a customer who is here to scan it with any wallet.",
        "Let the customer scan it, then go back.",
    )

    // ---- returning money (RQ-26 to RQ-29) ----
    val returnTitle = say("RQ_26_RETURN_TITLE", "Return money")
    val returnInLabel = say("RQ_26_RETURN_IN_LABEL", "Return in")
    val returnInSats = say("RQ_26_RETURN_IN_SATS", "Sats")
    val returnInCurrency = say("RQ_26_RETURN_IN_CURRENCY", "The shop's currency")
    val returnAmountSatsLabel = say("RQ_26_RETURN_AMOUNT_SATS_LABEL", "Amount in sats")
    val returnAmountCurrencyLabel = say("RQ_26_RETURN_AMOUNT_CURRENCY_LABEL", "Amount in the shop's currency")
    val returnAmountRefused = words(
        "RQ_26_RETURN_AMOUNT_REFUSED", Tones.STOP, "RQ-26", "Return amount not read",
        "The amount to return could not be read as an amount above zero.",
        "Type the amount in digits, then try again.",
    )
    val askAddress = say("RQ_27_ASK_ADDRESS", "Ask for an address")
    val addressLabel = say("RQ_28_ADDRESS_LABEL", "The customer's address")
    val addressNeeded = words(
        "RQ_28_ADDRESS_NEEDED", Tones.STOP, "RQ-28", "No address yet",
        "A return goes to a bitcoin address of the customer's own, and none is filled in.",
        "Ask the customer for one, then paste or scan what they send.",
    )
    val scanAddress = say("RQ_28_SCAN_ADDRESS", "Scan an address")
    val addReturn = say("RQ_29_ADD_RETURN", "Add the return")
    val checkTitle = say("RQ_28_CHECK_TITLE", "Check it with the customer")
    val checkText = say(
        "RQ_28_CHECK_TEXT",
        "Compare these groups with the customer by another way than the message the address came in, such as a call or in person.",
    )
    val checkNext = say(
        "RQ_28_CHECK_NEXT",
        "Someone who can change your messages can change an address in them. The return waits in the queue until you pay the returns.",
    )

    /** The app's own messages, under their keys, for the checks of the words. */
    fun messages(): List<Pair<String, SayWords>> = listOf(
        "RQ_06_NOTE_REFUSED" to noteRefused,
        "RQ_08_AMOUNT_REFUSED" to amountRefused,
        "RQ_09_DUE_REFUSED" to dueRefused,
        "RQ_09_DUE_RANGE" to dueRange,
        "RQ_03_MADE" to made,
        "RQ_12_RATE_FETCHING" to rateFetching,
        "RQ_13_CODE" to codeWords,
        "RQ_26_RETURN_AMOUNT_REFUSED" to returnAmountRefused,
        "RQ_28_ADDRESS_NEEDED" to addressNeeded,
    )

    /** Every key and its text, in order, for docs/screens/owner-app.md. */
    fun all(): List<Pair<String, String>> = registry.toList()
}
