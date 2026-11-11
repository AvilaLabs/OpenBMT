package org.openbmt.counter.app

import org.openbmt.app.SayWords
import org.openbmt.app.Tones
import org.openbmt.app.Who

/**
 * The words of the tip step and of the lines that say tips (CR-188, CR-191, CR-192), as the counter library's
 * `say` module has them: a test of `openbmt-counter-ffi` reads this file and checks each against the library's.
 * The refund line is the one line of the app's own.
 */
object WordsOfTips {
    /** CR-191: the line of Today with the total of the tips. */
    const val LINE = "Tips"

    /** CR-188: the button for a tip the customer types. */
    const val OTHER = "Other amount"

    /** CR-188: the button for no tip. */
    const val NONE = "No tip"

    /** CR-191: the line of Today with the tips taken by one person; [name] is cleaned text from the list of cashiers. */
    fun personLabel(name: String) = "Tips taken by $name"

    /** CR-188: the button of a suggested tip, "15% · 1.50 USD"; [tip] is written in the shop's currency. */
    fun button(percent: Int, tip: String) = "$percent% \u00B7 $tip"

    /** CR-192: the line of a refund's details that says how much of it is the tip. */
    fun refundLine(tip: String) = "Of which tip: $tip"

    /** CR-188: the screen where the customer chooses a tip; [amount] is the sale's amount in the shop's currency. */
    fun choose(amount: String) = SayWords(
        Tones.INFO, Who.CUSTOMER,
        "Add a tip?",
        "The sale is $amount. A tip goes to the shop with it.",
        "Choose a tip, or No tip.",
        "CR-188", null, null, null,
    )
}
