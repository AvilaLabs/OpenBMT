package org.openbmt.counter.app

import org.openbmt.app.Format

/**
 * The words of the band of a part paid sale (CR-285): what is still missing leads, headed "Still to pay", and the
 * lines under it give the missing sats and what has arrived of the sale. No Android class is used.
 */
object StillToPayWords {
    /** CR-285: the heading over the large amount. */
    const val HEADING = "Still to pay"

    /** CR-285: the line under the large amount, "4,881 sats to send". */
    fun toSend(missingSats: Long): String = "${Format.sats(missingSats)} to send"

    /**
     * CR-285: what has arrived and the whole sale, "Paid 6,000 sats of 10,881. The sale is 8.00 USD." The second
     * number has no unit (SC-09). A sale with no amount in a currency ([sale] null, CR-197) ends after "of 10,881."
     */
    fun paidLine(receivedSats: Long, needsSats: Long, sale: String?): String {
        val of = "Paid ${Format.sats(receivedSats)} of ${number(needsSats)}."
        return if (sale == null) of else "$of The sale is $sale."
    }

    /** The sats with separators and no unit: "10,881". */
    private fun number(sats: Long): String = Format.sats(sats).substringBeforeLast(' ')

    /** Every text, for docs/screens/counter-app.md; the numbers are written as N and the sale as AMOUNT. */
    fun all(): List<Pair<String, String>> = listOf(
        "heading" to HEADING,
        "toSend" to toSend(0).replace("0 sats", "N sats"),
        "paidLine" to paidLine(0, 0, "AMOUNT").replace("0 sats of 0", "N sats of N"),
        "paidLineNoAmount" to paidLine(0, 0, null).replace("0 sats of 0", "N sats of N"),
    )
}
