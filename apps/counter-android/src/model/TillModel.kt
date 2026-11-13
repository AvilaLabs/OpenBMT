package org.openbmt.counter.app

import org.openbmt.counter.TillOfSale
import org.openbmt.counter.TillRequest

/**
 * CR-266 to CR-271: the labels of a sale from the till (`docs/SCREEN-RULES.md`). The list is written to
 * docs/screens/counter-app.md by [AppWords.screensMarkdown]. No Android class is used.
 */
object WordsOfTill {
    /** CR-267: the heading of the card on Home. */
    const val CARD = "From the till"

    /** CR-267: the label of the request's amount. */
    const val AMOUNT = "Amount"

    /** CR-267: the label of the request's reference. */
    const val REFERENCE = "Till reference"

    /** CR-267: the label of the minutes left. */
    const val MINUTES_LEFT = "Minutes left"

    /** CR-267: the button that starts the sale. */
    const val START = "Start"

    /** CR-267: the button that hides the request. */
    const val NOT_NOW = "Not now"

    /** CR-269: the line on a started sale that the till cancelled. */
    const val CANCELLED = "The till cancelled this sale"

    /** Every key and its text, for docs/screens/counter-app.md. */
    fun all(): List<Pair<String, String>> = listOf(
        "card" to CARD, "amount" to AMOUNT, "reference" to REFERENCE, "minutesLeft" to MINUTES_LEFT,
        "start" to START, "notNow" to NOT_NOW, "cancelled" to CANCELLED,
    )
}

/** What the card of a request shows (CR-267): the label of a line and its value. */
class TillLine(val label: String, val value: String)

/** What the screens write of a sale from the till, from what the library says. No Android class is used. */
object TillModel {
    /**
     * CR-267: the lines of the card of one request: the amount, the reference when the till sent one, and the minutes
     * left. The amount is the till's and is not changed at the counter.
     */
    fun lines(money: Money, request: TillRequest): List<TillLine> {
        val lines = ArrayList<TillLine>()
        lines.add(TillLine(WordsOfTill.AMOUNT, money.show(request.amountMinor)))
        if (request.reference.isNotEmpty()) lines.add(TillLine(WordsOfTill.REFERENCE, request.reference))
        lines.add(TillLine(WordsOfTill.MINUTES_LEFT, request.minutesLeft.toString()))
        return lines
    }

    /**
     * CR-267: the requests the card shows, oldest first as the library lists them, none of them lapsed. A request the
     * library still lists with no minutes left is not shown.
     */
    fun shown(requests: List<TillRequest>): List<TillRequest> = requests.filter { it.minutesLeft > 0 }

    /** What the cards show as one text: the screen of Home is made again only when it changes. */
    fun signature(requests: List<TillRequest>): String =
        requests.joinToString("\u0002") { r -> "${r.sale.joinToString("") { "%02x".format(it) }}\u0001${r.amountMinor}\u0001${r.reference}\u0001${r.minutesLeft}" }

    /** CR-269: the line to show on a started sale, or null when the till did not cancel it. */
    fun cancelledLine(till: TillOfSale?): String? = if (till != null && till.cancelled) WordsOfTill.CANCELLED else null
}
