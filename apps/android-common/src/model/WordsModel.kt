package org.openbmt.app

/** The four colours of docs/SCREEN-RULES.md SC-06; [Palette] has the value of each, light and dark. */
enum class Colour {
    Blue,
    Green,
    Red,
    Neutral,
    ;

    /** The band colour in the light scheme. */
    val argb: Int get() = Palette.light.of(this).band

    /** The band colour for the phone's night mode. */
    fun band(night: Boolean): Int = Palette.of(night).of(this).band
}

/** The numbers of the tone field of the words (PROTOCOL.md of the libraries). */
object Tones {
    const val INFO = 0
    const val GOOD = 1
    const val STOP = 2
    const val ENDED = 3
}

/** The numbers of the who field of the words (PROTOCOL.md of the libraries). */
object Who {
    const val NOBODY = 0
    const val CUSTOMER = 1
    const val CASHIER = 2
    const val MANAGER = 3
    const val OWNER = 4
    const val KEY_HOLDER = 5
    const val BOX_KEEPER = 6
}

/** Text from another device, with where it came from (SC-08). */
data class SayQuoted(val from: String, val text: String)

/**
 * What a person is shown, as the two apps' bindings give it: the fields of
 * the library's `Words`. Each app's binding has its own `Words` class; each
 * converts to this one, so the colours, the checks and the views are written
 * once.
 */
data class SayWords(
    val tone: Int,
    val who: Int,
    val heading: String,
    val what: String,
    val next: String,
    val code: String?,
    val at: Long?,
    val quoted: SayQuoted?,
    val detail: String?,
)

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

        /**
         * The colour of a screen's main button: the screen's own tone, except that a stop screen gets the info
         * blue, so that red stays what stops and what destroys.
         */
        fun buttonColour(tone: Int): Colour = colourOf(tone).let { if (it == Colour.Red) Colour.Blue else it }

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
        fun of(words: SayWords, clock: (Long) -> String = Format::localTime): WordsModel {
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
