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

    /**
     * GV-13: whether the first run and the home screens offer tips, the POS connector, the till of the Box and the cashier
     * list. Each stays in Settings for a place that also sells.
     */
    fun homeOffersShopSteps(giving: Boolean): Boolean = !giving

    /** GV-13: whether Settings carries the till's row itself, because home does not. */
    fun settingsCarriesTill(giving: Boolean): Boolean = giving
}

/**
 * The words the owner app adds for a place of giving (GV-03, GV-10). None is written yet: each is a placeholder named for
 * what it must say. They are not in the list of the app's words until the words are given.
 */
object GivingWords {
    /** The question of the first run, before the wallet: what kind of place this is. */
    const val TODO_WORDS_PLACE_QUESTION = "TODO_WORDS_PLACE_QUESTION"

    /** The choice of a shop on that question. */
    const val TODO_WORDS_PLACE_CHOICE_SHOP = "TODO_WORDS_PLACE_CHOICE_SHOP"

    /** The choice of a church or another place that takes gifts on that question. */
    const val TODO_WORDS_PLACE_CHOICE_GIVING = "TODO_WORDS_PLACE_CHOICE_GIVING"

    /** The title of the bar of that question's page. */
    const val TODO_WORDS_PLACE_BAR = "TODO_WORDS_PLACE_BAR"

    /** The name of the group of the settings editor that holds the kind of place. */
    const val TODO_WORDS_EDIT_GROUP_PLACE = "TODO_WORDS_EDIT_GROUP_PLACE"

    /** The label of the switch in that group: on is a place of giving. */
    const val TODO_WORDS_EDIT_FIELD_PLACE = "TODO_WORDS_EDIT_FIELD_PLACE"

    /** The words of the settings editor's group of the kind of place: its sentence and the next step. */
    val editGroupPlace = SayWords(
        Tones.INFO, Who.OWNER, TODO_WORDS_EDIT_GROUP_PLACE, "TODO_WORDS_EDIT_GROUP_PLACE_WHAT", "TODO_WORDS_EDIT_GROUP_PLACE_NEXT",
        null, null, null, null,
    )

    /** The words of the switch of the kind of place. */
    val editFieldPlace = SayWords(
        Tones.INFO, Who.OWNER, TODO_WORDS_EDIT_FIELD_PLACE, "TODO_WORDS_EDIT_FIELD_PLACE_WHAT", "TODO_WORDS_EDIT_FIELD_PLACE_NEXT",
        null, null, null, null,
    )
}
