package org.openbmt.counter.app

import org.openbmt.app.SayWords
import org.openbmt.app.Tones
import org.openbmt.app.Who

/**
 * The words of the page and the home line for the switch from the starter wallet (docs/COUNTER-RULES.md CR-259 to
 * CR-264). The button labels and the menu row are the ones the rules name. A text that is not written yet is a
 * `WORDS_NEEDED_CR_26x_xxx` constant that shows its own name wherever the words would be, until the words exist
 * (CR-264). The list is written to docs/screens/counter-app.md by [AppWords.screensMarkdown]. No Android class is
 * used.
 */
object SwitchWords {
    /** The row of the manager's menu and the title of the page (CR-260). */
    const val BAR = "New wallet to check"

    /** The two buttons of the page (CR-260). */
    const val SAME = "Same address"
    const val DIFFERENT = "Different"

    /** The home line while a switch waits (CR-260). */
    const val HOME_LINE = "WORDS_NEEDED_CR_260_HOME_LINE"

    /** The line that says to compare the address with the new wallet's own screen, not with the owner phone (CR-260). */
    const val COMPARE = "WORDS_NEEDED_CR_260_COMPARE"

    /** The label above the PIN field. */
    const val PIN_LABEL = "Manager's PIN"

    /** Every text of this part that is not written yet. */
    val placeholders: List<String> = listOf(HOME_LINE, COMPARE)

    /** Words that are still to be written: the placeholder stands in the heading and both sentences. */
    fun said(placeholder: String): SayWords =
        SayWords(Tones.INFO, Who.MANAGER, placeholder, placeholder, placeholder, null, null, null, null)

    /** Whether these words are a placeholder (CR-264). */
    fun isPlaceholder(words: SayWords): Boolean = words.heading.startsWith("WORDS_NEEDED_")

    /** The home line while a switch waits. */
    val homeLine: SayWords = said(HOME_LINE)

    /** Every key and its text, for docs/screens/counter-app.md. */
    fun all(): List<Pair<String, String>> = listOf(
        "bar" to BAR, "same" to SAME, "different" to DIFFERENT, "pinLabel" to PIN_LABEL,
        "homeLine" to HOME_LINE, "compare" to COMPARE,
    )
}
