package org.openbmt.counter.app

import org.openbmt.app.SayWords
import org.openbmt.app.Tones
import org.openbmt.app.Who

/**
 * The Lightning key of Shop Lightning (docs/SHOP-LIGHTNING-RULES.md SL-72, SL-76): where the sealed seed is kept
 * and the words of the screen that reads it from the stick's code. Every text of the screen is a placeholder until
 * the words are written; the reasons for on-chain only (SL-41) come from the library. No Android class is used.
 */
object ShopLightningWords {
    /** The file the sealed seed record is kept in, in `noBackupFilesDir`, and the Keystore key that seals it (SL-76). */
    const val FILE = "shop-lightning.obkl"
    const val KEY_ALIAS = "openbmt-counter-shop-lightning-v1"

    /** The row of the manager's menu and the title of the page (SL-76). */
    const val BAR = "WORDS_NEEDED_SL76_APP_BAR"

    /** The button that starts the camera, and the one that goes back. */
    const val SCAN = "WORDS_NEEDED_SL76_APP_SCAN"
    const val BACK = "WORDS_NEEDED_SL76_APP_BACK"

    /** The line that says the key was read and kept, and its button. */
    const val KEPT = "WORDS_NEEDED_SL76_APP_KEPT"
    const val KEPT_BUTTON = "WORDS_NEEDED_SL76_APP_KEPT_BUTTON"

    /** Every text of this part that is not written yet. */
    val placeholders: List<String> = listOf(BAR, SCAN, BACK, KEPT, KEPT_BUTTON)

    /** Words that are still to be written: the placeholder stands in the heading and both sentences. */
    fun said(placeholder: String): SayWords =
        SayWords(Tones.INFO, Who.MANAGER, placeholder, placeholder, placeholder, null, null, null, null)

    /** Whether these words are a placeholder. */
    fun isPlaceholder(words: SayWords): Boolean = words.heading.startsWith("WORDS_NEEDED_")

    /** The page says the key was read and kept. */
    val kept: SayWords = said(KEPT)

    /** Every key and its text, for docs/screens/counter-app.md. */
    fun all(): List<Pair<String, String>> = listOf(
        "bar" to BAR, "scan" to SCAN, "back" to BACK, "kept" to KEPT, "keptButton" to KEPT_BUTTON,
    )
}
