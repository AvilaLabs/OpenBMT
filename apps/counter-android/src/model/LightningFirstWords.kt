package org.openbmt.counter.app

/**
 * The words of a sale whose code is the Lightning invoice alone (CR-283) and of the notice of a sale that turns
 * while another screen shows (CR-284). No Android class is used.
 */
object LightningFirstWords {
    /** CR-283: the line under the code, and the button that shows the on-chain code. */
    const val LINE = "Over the instant limit: Lightning is paid at once. On-chain waits for a block, about ten minutes."
    const val BUTTON = "Pay on-chain instead"

    /** CR-284: the notice of a sale that turned green, and of one that turned red; [number] is the sale's number as the Sales list shows it. */
    fun paidHeading(number: Long): String = "Sale $number is paid"
    const val PAID_NEXT = "Hand over the goods. Tap to see the sale."

    fun redHeading(number: Long): String = "Sale $number: do not hand over"
    const val RED_NEXT = "Tap to see what to do."

    /** CR-284: the line after the three notices that show; it opens the Sales list. */
    fun moreLine(count: Int): String = "and $count more in Sales"

    /** Every text, for docs/screens/counter-app.md; the sale's number is written as N. */
    fun all(): List<Pair<String, String>> = listOf(
        "line" to LINE,
        "button" to BUTTON,
        "paidHeading" to paidHeading(0).replace("0", "N"),
        "paidNext" to PAID_NEXT,
        "redHeading" to redHeading(0).replace("0", "N"),
        "redNext" to RED_NEXT,
        "moreLine" to moreLine(0).replace("0", "N"),
    )
}
