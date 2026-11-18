package org.openbmt.owner.app

import org.openbmt.app.SayWords
import org.openbmt.app.Tones
import org.openbmt.app.Who
import org.openbmt.owner.Place
import org.openbmt.owner.SettingsHeld

/**
 * The owner phone at a place of giving (docs/GIVING-RULES.md, GV-03, GV-13), as small values. No Android class is used.
 */
object Giving {
    /**
     * Whether the shop is a place of giving: the settings in force say so when the phone holds any; before that the answer
     * of the first question, kept in the shop file (GV-03).
     */
    fun isGiving(held: SettingsHeld?, prefs: ShopPrefs): Boolean = (held?.place ?: prefs.place) == Place.Giving
}

/**
 * The words the owner app adds for a place of giving (GV-03, GV-10). The labels of the first run's question are listed
 * by [all] under the key of their rule; the words of the settings editor's group and switch are in [AppWords.all]. The
 * list is written to docs/screens/owner-app.md by [AppWords.screensMarkdown].
 */
object GivingWords {
    private val registry = ArrayList<Pair<String, String>>()

    /** Written words, listed under the key of their rule. */
    private fun say(key: String, text: String): String {
        registry.add(key to text)
        return text
    }

    /** The question of the first run, before the wallet: what kind of place this is. */
    val PLACE_QUESTION = say("GV03_PLACE_QUESTION", "What kind of place is this?")

    /** The choice of a shop on that question. */
    val PLACE_CHOICE_SHOP = say("GV03_PLACE_CHOICE_SHOP", "A shop")

    /** The choice of a church or another place that takes gifts on that question. */
    val PLACE_CHOICE_GIVING = say("GV03_PLACE_CHOICE_GIVING", "A place that takes gifts")

    /** The title of the bar of that question's page. */
    val PLACE_BAR = say("GV03_PLACE_BAR", "Kind of place")

    /** The name of the group of the settings editor that holds the kind of place. */
    const val EDIT_GROUP_PLACE = "Kind of place"

    /** The sentence of that group. */
    const val EDIT_GROUP_PLACE_WHAT =
        "A shop sells things. A place that takes gifts, such as a church or a charity, lets each giver choose the amount."

    /** The next step of that group. */
    const val EDIT_GROUP_PLACE_NEXT =
        "Choose the one this is. A place that takes gifts hides the shop's steps; they stay in the settings."

    /** The label of the switch in that group: on is a place of giving. */
    const val EDIT_FIELD_PLACE = "A place that takes gifts"

    /** The sentence of that switch. */
    const val EDIT_FIELD_PLACE_WHAT = "On, the counter asks each giver for the amount, thanks them, and nothing is handed over."

    /** The next step of that switch. */
    const val EDIT_FIELD_PLACE_NEXT = "Turning it on needs the Key app's approval. Turning it off does not."

    /** The words of the settings editor's group of the kind of place: its sentence and the next step. */
    val editGroupPlace = SayWords(
        Tones.INFO, Who.OWNER, EDIT_GROUP_PLACE, EDIT_GROUP_PLACE_WHAT, EDIT_GROUP_PLACE_NEXT,
        null, null, null, null,
    )

    /** The words of the switch of the kind of place. */
    val editFieldPlace = SayWords(
        Tones.INFO, Who.OWNER, EDIT_FIELD_PLACE, EDIT_FIELD_PLACE_WHAT, EDIT_FIELD_PLACE_NEXT,
        null, null, null, null,
    )

    /** Every key and its text, in order, for docs/screens/owner-app.md. */
    fun all(): List<Pair<String, String>> = registry.toList()
}
