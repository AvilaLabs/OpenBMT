package org.openbmt.owner.app

import org.openbmt.app.SayWords
import org.openbmt.app.Tones
import org.openbmt.app.Who

/**
 * The words of adding a counter to a shop with its own wallet (`docs/OWNER-APP-RULES.md` OA-105 to OA-114): the
 * offer, the counter's request, the bundle, and the wallet page's gap-limit row. They are the approved texts of the
 * design file, pasted as they are; the library's refusals are the library's own words (SC-01 to SC-12). A text that
 * is not written yet, or that does not pass SC-12, is a `WORDS_NEEDED_OA_xxx` constant that shows its own name
 * wherever the words would be, until the words exist (OA-114). The list is written to docs/screens/owner-app.md by
 * [AppWords.screensMarkdown]. No Android class is used.
 */
object OwnPairWords {
    // ---- the offer (OA-108) ----
    const val title = "Add a counter"
    const val offerHeading = "Scan this with the new counter"
    const val offerText = "The counter shows your wallet's first address. Check it against your wallet before you go on."
    const val offerScan = "Scan the counter's code"
    const val endedHeading = "Code ended"
    const val endedText = "This code lasts 10 minutes and has ended. Nothing was paired."
    const val endedNext = "Show a new code."
    const val endedButton = "Show a new code"

    // ---- the counter's request (OA-109) ----
    const val compareHeading = "Compare the four words"
    const val compareText = "Check that {name} shows these same four words."
    const val compareMatch = "They match"
    const val compareDiffer = "They differ"

    // ---- the bundle (OA-110, OA-111) ----
    const val bundleHeading = "Scan this with {name}"
    const val bundleText = "It holds the shop's settings and the counter's first 1,000 addresses. The counter shows Paired when it has taken it."
    const val bundlePaired = "The counter shows Paired"
    const val bundleAgain = "Show the code again"
    const val pairedHeading = "{name} is paired"
    const val pairedText = "It makes its addresses from your wallet and sells within the shop's settings."
    const val pairedNext = "Nothing to do."

    // ---- the gap limit (OA-113) ----
    const val gapTitle = "How far your wallet looks ahead"
    const val gapUsual = "Your wallet's usual setting shows every payment."
    const val gapMore = "Set your wallet to look at least {n} addresses ahead, or it will not show every payment."
    const val gapMoreNext = "In Sparrow: Settings, then Advanced, then Gap limit. Bitcoin Keeper cannot change it."

    // ---- the wallet page and the waiting counters (OA-111, OA-113) ----
    const val walletTitle = "Your wallet"
    const val walletButton = "Your wallet"
    const val waitingLabel = "Waiting for their code"

    // ---- refusals of the app's own steps (OA-105, OA-108) ----
    val walletNotSaved = SayWords(
        Tones.STOP, Who.OWNER, "Wallet not saved",
        "The phone could not save your wallet's description, so nothing was kept.",
        "Try again. Your money is in your wallet, not on this phone.", "OA-105", null, null, null,
    )
    val noWallet = SayWords(
        Tones.STOP, Who.OWNER, "No wallet connected",
        "Adding a counter needs the shop's wallet, and none is kept on this phone.",
        "Connect your wallet first.", "OA-105", null, null, null,
    )
    val codeNotShown = SayWords(
        Tones.STOP, Who.OWNER, "Code not showing",
        "This step needs the code from Add a counter, and none is showing now.",
        "Start again from Add a counter.", "OA-108", null, null, null,
    )

    /** No text of this part is a placeholder now; the list stays for the test that would catch a new one. */
    val placeholders: List<String> = emptyList()

    /** Words that are still to be written: the placeholder stands in the heading and both sentences. */
    fun said(placeholder: String): SayWords =
        SayWords(Tones.STOP, Who.OWNER, placeholder, placeholder, placeholder, null, null, null, null)

    /** Whether these words are a placeholder (OA-114). */
    fun isPlaceholder(words: SayWords): Boolean = words.heading.startsWith("WORDS_NEEDED_")

    fun compare(name: String): String = compareText.replace("{name}", name)

    fun bundle(name: String): String = bundleHeading.replace("{name}", name)

    fun paired(name: String): String = pairedHeading.replace("{name}", name)

    /** Every key and its text, in the order of the words file, for docs/screens/owner-app.md. */
    fun all(): List<Pair<String, String>> {
        val out = ArrayList<Pair<String, String>>()
        fun add(key: String, text: String) { out.add(key to text) }
        add("title", title); add("offerHeading", offerHeading); add("offerText", offerText); add("offerScan", offerScan)
        add("endedHeading", endedHeading); add("endedText", endedText); add("endedNext", endedNext); add("endedButton", endedButton)
        add("compareHeading", compareHeading); add("compareText", compareText)
        add("compareMatch", compareMatch); add("compareDiffer", compareDiffer)
        add("bundleHeading", bundleHeading); add("bundleText", bundleText)
        add("bundlePaired", bundlePaired); add("bundleAgain", bundleAgain)
        add("pairedHeading", pairedHeading); add("pairedText", pairedText); add("pairedNext", pairedNext)
        add("gapTitle", gapTitle); add("gapUsual", gapUsual); add("gapMore", gapMore); add("gapMoreNext", gapMoreNext)
        add("walletTitle", walletTitle); add("walletButton", walletButton); add("waitingLabel", waitingLabel)
        for ((k, w) in listOf("walletNotSaved" to walletNotSaved, "noWallet" to noWallet, "codeNotShown" to codeNotShown)) {
            add("$k.heading", w.heading); add("$k.what", w.what); add("$k.next", w.next)
        }
        for (n in placeholders) add(n, n)
        return out
    }
}
