package org.openbmt.counter.app

import org.openbmt.app.Format
import org.openbmt.app.SayWords
import org.openbmt.counter.StarterDecision
import org.openbmt.counter.StarterPayment
import org.openbmt.counter.StarterState

/**
 * The labels of the starter line's cards and of the day's starter payments (docs/COUNTER-RULES.md CR-273, CR-274). The
 * words of the stop
 * and of the line that cannot be checked come from the library with the decision. The list is written to
 * docs/screens/counter-app.md by [AppWords.screensMarkdown]. No Android class is used.
 */
object WordsOfStarter {
    /** CR-274: the button of a card that says someone at the counter has seen the payment. */
    const val SEEN = "Seen"

    /** CR-274: the label of a payment's amount. */
    const val AMOUNT = "Amount"

    /** CR-274: the label of a payment's time. */
    const val TIME = "Time"

    /** CR-274: what a payment the owner phone made says. */
    const val BY_OWNER = "Made on the owner phone"

    /** CR-274: what a payment the owner phone did not make says. */
    const val NOT_BY_OWNER = "Not made on the owner phone"

    /** CR-274: the heading of the day's payments from the starter wallet on Today. */
    const val TODAY = "Payments from the starter wallet"

    /** Every key and its text, for docs/screens/counter-app.md. */
    fun all(): List<Pair<String, String>> = listOf(
        "seen" to SEEN, "amount" to AMOUNT, "time" to TIME, "byOwner" to BY_OWNER, "notByOwner" to NOT_BY_OWNER,
        "today" to TODAY,
    )
}

/** What the card of one payment shows (CR-274): a label and its value. */
class StarterLine(val label: String, val value: String)

/** What the screens write of the starter line, from what the library says. No Android class is used. */
object StarterModel {
    /** CR-273: whether New sale offers bitcoin; the library decides, and a stop leaves the customer no bitcoin to send. */
    fun offersBitcoin(state: StarterState?): Boolean = state?.decision != StarterDecision.Stopped

    /** CR-273: the words to show instead of New sale when it offers no bitcoin; null when it does. */
    fun stopWords(state: StarterState?): SayWords? =
        if (offersBitcoin(state)) null else state?.words?.say()

    /** CR-273: the Home line for a line that cannot be checked, or for a stop; null when the line is fine. */
    fun homeLine(state: StarterState?): SayWords? =
        if (state == null || state.decision == StarterDecision.Open) null else state.words?.say()

    /** CR-274: the payments nobody at the counter has marked as seen, in the order the library lists them (newest first). */
    fun unseen(state: StarterState?): List<StarterPayment> = state?.payments.orEmpty().filter { !it.seen }

    /** CR-274: the lines of one payment's card: its amount, its time, and whether the owner phone made it. */
    fun lines(p: StarterPayment): List<StarterLine> = listOf(
        StarterLine(WordsOfStarter.AMOUNT, Format.sats(p.sats)),
        StarterLine(WordsOfStarter.TIME, Format.localTime(p.time)),
        StarterLine(if (p.byOwner) WordsOfStarter.BY_OWNER else WordsOfStarter.NOT_BY_OWNER, ""),
    )

    /** CR-274: the payments of the day from [from] (included) to [to] (not included), in the order the library lists them. */
    fun day(state: StarterState?, from: Long, to: Long): List<StarterPayment> =
        state?.payments.orEmpty().filter { it.time in from until to }

    /** CR-274: one line of Today for a payment: the amount, the time, and who made it. */
    fun todayLine(p: StarterPayment): String =
        "${Format.sats(p.sats)}, ${Format.localTime(p.time)}, ${if (p.byOwner) WordsOfStarter.BY_OWNER else WordsOfStarter.NOT_BY_OWNER}"

    /** What the cards show as one text: Home is made again only when it changes. */
    fun signature(payments: List<StarterPayment>): String =
        payments.joinToString("\u0002") { p -> "${p.txid.joinToString("") { "%02x".format(it) }}\u0001${p.sats}\u0001${p.time}\u0001${p.byOwner}" }
}
