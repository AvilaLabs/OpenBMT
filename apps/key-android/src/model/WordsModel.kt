package org.openbmt.key.app

import org.openbmt.key.Words

/** The four colours of docs/SCREEN-RULES.md SC-06. */
enum class Colour(val argb: Int) {
    Blue(0xFF1565C0.toInt()),
    Green(0xFF2E7D32.toInt()),
    Red(0xFFC62828.toInt()),
    Neutral(0xFF546E7A.toInt()),
}

/**
 * What a `WordsView` draws, as text: the colour, the three lines and who, the
 * quoted text marked as from elsewhere, and the lines under "Details".
 */
class WordsModel(
    val colour: Colour,
    val heading: String,
    val what: String,
    val next: String,
    /** Who takes the next step, as a sentence. */
    val who: String,
    /** The marking line above quoted text, or null. */
    val quotedLabel: String?,
    /** The quoted text with its quotation marks, or null. */
    val quotedText: String?,
    /** The lines of the "Details" fold: rule, time, detail. Empty when there are none. */
    val details: List<String>,
) {
    companion object {
        const val DETAILS_TITLE = "Details"

        fun colourOf(tone: Int): Colour = when (tone) {
            Tones.INFO -> Colour.Blue
            Tones.GOOD -> Colour.Green
            Tones.STOP -> Colour.Red
            else -> Colour.Neutral
        }

        fun whoText(who: Int): String = when (who) {
            Who.NOBODY -> "Nobody needs to act."
            Who.CUSTOMER -> "The customer does this step."
            Who.CASHIER -> "The cashier does this step."
            Who.MANAGER -> "A manager does this step."
            Who.OWNER -> "The owner does this step."
            Who.KEY_HOLDER -> "The key holder does this step."
            Who.BOX_KEEPER -> "The Box keeper does this step."
            else -> "Ask the owner who does this step."
        }

        /** [clock] turns seconds since 1970 into the person's own time. */
        fun of(words: Words, clock: (Long) -> String = Format::localTime): WordsModel {
            val quoted = words.quoted
            val details = ArrayList<String>()
            words.code?.let { details.add("Rule: $it") }
            words.at?.let { details.add("Time: ${clock(it)}") }
            words.detail?.let { details.add("Detail: $it") }
            return WordsModel(
                colour = colourOf(words.tone),
                heading = words.heading,
                what = words.what,
                next = words.next,
                who = whoText(words.who),
                // The library cleans quoted text already; it is cleaned again here
                // because this is the last place before it is drawn (SC-08).
                quotedLabel = quoted?.let { "Text from elsewhere, not written by this app. ${it.from}:" },
                quotedText = quoted?.let { "“${Format.cleanForeign(it.text)}”" },
                details = details,
            )
        }
    }
}
