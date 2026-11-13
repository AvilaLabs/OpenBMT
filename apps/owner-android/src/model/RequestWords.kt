package org.openbmt.owner.app

import org.openbmt.app.SayWords
import org.openbmt.app.Tones
import org.openbmt.app.Who

/**
 * Every text of the owner's payment-request screens that the app writes itself (`docs/REQUEST-RULES.md`, RQ-35,
 * RQ-36), listed under the rule of its screen. No text is written yet: each is a `WORDS_NEEDED_RQ_<rule>_<SUFFIX>`
 * placeholder that shows its own name (the same pattern as `StarterWords`). The library's own words (a refusal, the
 * lines of a row, the state of a request, the line above a link) are shown as they arrive. The list is written to
 * docs/screens/owner-app.md by [AppWords.screensMarkdown]. No Android class is used.
 */
object RequestWords {
    private val registry = ArrayList<Pair<String, String>>()

    /** A text not written yet, listed under its name without the `WORDS_NEEDED_` start. */
    private fun need(name: String): String {
        registry.add(name.removePrefix("WORDS_NEEDED_") to name)
        return name
    }

    // ---- Home and the list (RQ-35, RQ-21, RQ-29) ----
    val homeButton = need("WORDS_NEEDED_RQ_35_HOME_BUTTON")
    val homeRowTitle = need("WORDS_NEEDED_RQ_35_HOME_ROW_TITLE")
    val homeRowSub = need("WORDS_NEEDED_RQ_35_HOME_ROW_SUB")
    val listTitle = need("WORDS_NEEDED_RQ_35_LIST_TITLE")
    val partReady = need("WORDS_NEEDED_RQ_25_PART_READY_TO_SEND")
    val partOverdue = need("WORDS_NEEDED_RQ_21_PART_OVERDUE")
    val partDueSoon = need("WORDS_NEEDED_RQ_21_PART_DUE_SOON")
    val partWaiting = need("WORDS_NEEDED_RQ_21_PART_WAITING")
    val partFinished = need("WORDS_NEEDED_RQ_35_PART_FINISHED")
    val back = need("WORDS_NEEDED_RQ_35_BACK")
    val payReturns = need("WORDS_NEEDED_RQ_29_PAY_RETURNS")

    // ---- the page of one request (RQ-35, RQ-14, RQ-19, RQ-20, RQ-16, RQ-22, RQ-25) ----
    val repeatsLine = need("WORDS_NEEDED_RQ_24_REPEATS_LINE")
    val shareAgain = need("WORDS_NEEDED_RQ_14_SHARE_AGAIN")
    val sendAgain = need("WORDS_NEEDED_RQ_22_SEND_AGAIN")
    val cancelRequest = need("WORDS_NEEDED_RQ_19_CANCEL")
    val closeRequest = need("WORDS_NEEDED_RQ_20_CLOSE")
    val countPaid = need("WORDS_NEEDED_RQ_16_COUNT_PAID")
    val stopRepeating = need("WORDS_NEEDED_RQ_25_STOP_REPEATING")
    val returnMoney = need("WORDS_NEEDED_RQ_26_RETURN")

    // ---- what the page holds (RQ-15, RQ-12, RQ-26, RQ-29) ----
    val owedLabel = need("WORDS_NEEDED_RQ_15_OWED_LABEL")
    val paidLabel = need("WORDS_NEEDED_RQ_15_PAID_LABEL")
    val quotesLabel = need("WORDS_NEEDED_RQ_12_QUOTES_LABEL")
    val paymentsLabel = need("WORDS_NEEDED_RQ_15_PAYMENTS_LABEL")
    val paymentBlock = need("WORDS_NEEDED_RQ_15_PAYMENT_BLOCK")
    val paymentWaiting = need("WORDS_NEEDED_RQ_15_PAYMENT_WAITING")
    val paymentNotCredited = need("WORDS_NEEDED_RQ_15_PAYMENT_NOT_CREDITED")
    val returnsLabel = need("WORDS_NEEDED_RQ_29_RETURNS_LABEL")
    val returnQueued = need("WORDS_NEEDED_RQ_29_RETURN_QUEUED")
    val returnSent = need("WORDS_NEEDED_RQ_29_RETURN_SENT")
    val returnDone = need("WORDS_NEEDED_RQ_29_RETURN_DONE")
    val returnCancelled = need("WORDS_NEEDED_RQ_29_RETURN_CANCELLED")

    // ---- the form (RQ-05 to RQ-09, RQ-24) ----
    val formTitle = need("WORDS_NEEDED_RQ_05_FORM_TITLE")
    val noteLabel = need("WORDS_NEEDED_RQ_06_NOTE_LABEL")
    val noteHint = need("WORDS_NEEDED_RQ_06_NOTE_HINT")
    val pricedLabel = need("WORDS_NEEDED_RQ_07_PRICED_LABEL")
    val pricedBitcoin = need("WORDS_NEEDED_RQ_07_PRICED_BITCOIN")
    val pricedCurrency = need("WORDS_NEEDED_RQ_07_PRICED_CURRENCY")
    val amountBitcoinLabel = need("WORDS_NEEDED_RQ_08_AMOUNT_BITCOIN_LABEL")
    val amountCurrencyLabel = need("WORDS_NEEDED_RQ_08_AMOUNT_CURRENCY_LABEL")
    val dueLabel = need("WORDS_NEEDED_RQ_09_DUE_LABEL")
    val dueHint = need("WORDS_NEEDED_RQ_09_DUE_HINT")
    val repeatLabel = need("WORDS_NEEDED_RQ_24_REPEAT_LABEL")
    val repeatNone = need("WORDS_NEEDED_RQ_24_REPEAT_NONE")
    val repeatWeek = need("WORDS_NEEDED_RQ_24_REPEAT_WEEK")
    val repeatMonth = need("WORDS_NEEDED_RQ_24_REPEAT_MONTH")
    val repeatYear = need("WORDS_NEEDED_RQ_24_REPEAT_YEAR")
    val makeButton = need("WORDS_NEEDED_RQ_03_MAKE_BUTTON")
    val noteRefused = need("WORDS_NEEDED_RQ_06_NOTE_REFUSED")
    val amountRefused = need("WORDS_NEEDED_RQ_08_AMOUNT_REFUSED")
    val dueRefused = need("WORDS_NEEDED_RQ_09_DUE_REFUSED")
    val dueRange = need("WORDS_NEEDED_RQ_09_DUE_RANGE")
    val made = need("WORDS_NEEDED_RQ_03_MADE")

    // ---- sharing (RQ-12 to RQ-14) ----
    val shareTitle = need("WORDS_NEEDED_RQ_12_SHARE_TITLE")
    val rateFetching = need("WORDS_NEEDED_RQ_12_RATE_FETCHING")
    val quoteLabel = need("WORDS_NEEDED_RQ_12_QUOTE_LABEL")
    val quoteSatsLabel = need("WORDS_NEEDED_RQ_12_QUOTE_SATS_LABEL")
    val quoteUntilLabel = need("WORDS_NEEDED_RQ_12_QUOTE_UNTIL_LABEL")
    val shareLink = need("WORDS_NEEDED_RQ_13_SHARE_LINK")
    val sendsNothing = need("WORDS_NEEDED_RQ_13_SENDS_NOTHING")
    val showCode = need("WORDS_NEEDED_RQ_13_SHOW_CODE")
    val codeTitle = need("WORDS_NEEDED_RQ_13_CODE_TITLE")

    // ---- returning money (RQ-26 to RQ-29) ----
    val returnTitle = need("WORDS_NEEDED_RQ_26_RETURN_TITLE")
    val returnInLabel = need("WORDS_NEEDED_RQ_26_RETURN_IN_LABEL")
    val returnInSats = need("WORDS_NEEDED_RQ_26_RETURN_IN_SATS")
    val returnInCurrency = need("WORDS_NEEDED_RQ_26_RETURN_IN_CURRENCY")
    val returnAmountSatsLabel = need("WORDS_NEEDED_RQ_26_RETURN_AMOUNT_SATS_LABEL")
    val returnAmountCurrencyLabel = need("WORDS_NEEDED_RQ_26_RETURN_AMOUNT_CURRENCY_LABEL")
    val returnAmountRefused = need("WORDS_NEEDED_RQ_26_RETURN_AMOUNT_REFUSED")
    val askAddress = need("WORDS_NEEDED_RQ_27_ASK_ADDRESS")
    val addressLabel = need("WORDS_NEEDED_RQ_28_ADDRESS_LABEL")
    val addressNeeded = need("WORDS_NEEDED_RQ_28_ADDRESS_NEEDED")
    val scanAddress = need("WORDS_NEEDED_RQ_28_SCAN_ADDRESS")
    val addReturn = need("WORDS_NEEDED_RQ_29_ADD_RETURN")
    val checkTitle = need("WORDS_NEEDED_RQ_28_CHECK_TITLE")
    val checkText = need("WORDS_NEEDED_RQ_28_CHECK_TEXT")
    val checkNext = need("WORDS_NEEDED_RQ_28_CHECK_NEXT")

    /** Words that are still to be written: the placeholder stands in the heading and both sentences. */
    fun said(placeholder: String, tone: Int = Tones.INFO, who: Int = Who.OWNER): SayWords =
        SayWords(tone, who, placeholder, placeholder, placeholder, null, null, null, null)

    /** Whether these words are a placeholder (RQ-36). */
    fun isPlaceholder(words: SayWords): Boolean = words.heading.startsWith("WORDS_NEEDED_")

    /** A refusal of a form, before any call: placeholder words in the tone of a stop. */
    fun refusal(placeholder: String): SayWords = said(placeholder, Tones.STOP)

    /** Every key and its text, in order, for docs/screens/owner-app.md. */
    fun all(): List<Pair<String, String>> = registry.toList()
}
