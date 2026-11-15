package org.openbmt.counter.app

/**
 * The words of the screens a place of giving adds (GV-11, GV-12, GV-14). The list is written to
 * docs/screens/counter-app.md by [AppWords.screensMarkdown].
 */
object GivingWords {
    private val registry = ArrayList<Pair<String, String>>()

    /** Written words, listed under the key of their rule. */
    private fun say(key: String, text: String): String {
        registry.add(key to text)
        return text
    }

    /** The title of the bar of the amount screen. */
    val BAR = say("GV_BAR", "Give")

    /** The line over the amount the giver types. */
    val AMOUNT_HEADING = say("GV_AMOUNT_HEADING", "Amount of your gift")

    /** The button that opens a gift for the amount typed. */
    val GIVE = say("GV_GIVE", "Give this amount")

    /** The title of the page where the manager types the PIN to reach the settings, the gift list or the totals. */
    val PIN_BAR = say("GV_PIN_BAR", "For the manager")

    /** The button of that page that checks the PIN. */
    val PIN_GO = say("GV_PIN_GO", "Unlock")

    /** The button that takes the manager back to the amount screen. */
    val LEAVE = say("GV_LEAVE", "Back to giving")

    /** The heading of the giving outcome screen: the gift has arrived. */
    val OUTCOME_HEADING = say("GV_OUTCOME_HEADING", "Received")

    /** The sentence of the giving outcome screen that thanks the giver. It never asks anyone to hand something over or hold it back. */
    val OUTCOME_THANKS = say("GV_OUTCOME_THANKS", "Thank you for your gift.")

    /** The button that ends the giving outcome screen and goes back to the amount. */
    val OUTCOME_DONE = say("GV_OUTCOME_DONE", "Done")

    /** Every key and its text, in order, for docs/screens/counter-app.md. */
    fun all(): List<Pair<String, String>> = registry.toList()
}
