package org.openbmt.counter.app

import org.openbmt.app.SayWords
import org.openbmt.app.Tones
import org.openbmt.app.Who

/**
 * The Lightning key of Shop Lightning (docs/SHOP-LIGHTNING-RULES.md SL-72, SL-76): where the sealed seed is kept
 * and the words of the screen that reads it from the stick's code. The reasons for on-chain only (SL-41) and the
 * screen's own message come from the library. No Android class is used.
 */
object ShopLightningWords {
    /** The file the sealed seed record is kept in, in `noBackupFilesDir`, and the Keystore key that seals it (SL-76). */
    const val FILE = "shop-lightning.obkl"
    const val KEY_ALIAS = "openbmt-counter-shop-lightning-v1"

    /** The row of the manager's menu and the title of the page (SL-76). */
    const val BAR = "Lightning key"

    /** The button that starts the camera, and the one that goes back. */
    const val SCAN = "Scan the stick's code"
    const val BACK = "Back"

    /** The line that says the key was read and kept, and its button. */
    const val KEPT = "Lightning key kept"
    const val KEPT_BUTTON = "Done"

    /** Whether these words are a placeholder. */
    fun isPlaceholder(words: SayWords): Boolean = words.heading.startsWith("WORDS_NEEDED_")

    /** The page says the key was read and kept. */
    val kept: SayWords = SayWords(
        Tones.GOOD, Who.MANAGER, KEPT,
        "The key matches the shop's settings and is kept on this counter only. Lightning starts now.",
        "Put the stick back in its safe place.",
        "SL-76", null, null, null,
    )

    /** Every key and its text, for docs/screens/counter-app.md. */
    fun all(): List<Pair<String, String>> = listOf(
        "bar" to BAR, "scan" to SCAN, "back" to BACK, "kept" to KEPT, "keptButton" to KEPT_BUTTON,
    )
}
