package org.openbmt.owner.app

import org.openbmt.app.SayWords
import org.openbmt.app.Tones
import org.openbmt.app.Who

/**
 * The words of adding a counter to a shop with its own wallet (`docs/OWNER-APP-RULES.md` OA-105 to OA-114): the
 * offer, the counter's request, the bundle, the wallet page's gap-limit row, connecting the Box (BX-18) and the
 * change that waits for a counter (OA-119 to OA-123). The library's refusals are the library's own words (SC-01 to
 * SC-12). The list is written to docs/screens/owner-app.md by [AppWords.screensMarkdown]. No Android class is used.
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

    // ---- connecting the shop's Box, with no Key app (BX-17, BX-18, CR-130, CR-132) ----
    const val boxTitle = "Connect your Box"
    const val boxAddressHint = "host:port or name.onion:port"
    const val boxInviteHeading = "Enter this on the Box"
    const val boxInviteText = "Enter this invitation on the Box, on its status page or in its invitation file. Only someone at the Box can pair it."
    const val boxInviteNext = "The Box has it"
    const val boxAddressLabel = "The Box's address"
    const val boxAddressScan = "Scan the Box's address"
    const val boxAddressLook = "Find the Box"
    const val boxWordsHeading = "Check the Box's four words"
    const val boxWordsText = "The Box's status page shows four words. If they are not these same four, another device is answering in its place."
    const val boxWordsMatch = "They are the same"
    const val boxWordsDiffer = "They are not the same"
    const val boxStartAgain = "Start again"
    const val boxRestHeading = "Before the Box joins"
    const val boxRestText = "Connecting the Box rests on this phone alone. After it, a change that loosens the shop's settings needs a counter to confirm it."
    const val boxSend = "Connect the Box"
    const val boxPairedHeading = "Box connected"
    const val boxPairedText = "The Box took its welcome and this phone saved the link. Counters paired from now on reach the shop through it."
    const val boxPairedNext = "Counters paired before the Box take its relay code next."
    const val boxRelayHeading = "Give counters the relay code"
    const val boxRelayText = "On each counter paired before the Box, open its relay screen and scan this code. Mark each one here once it has."
    const val boxRelayList = "Counters still to scan it"
    const val boxRelayTaken = "Taken by"
    const val boxRelayLater = "Do the rest later"

    // ---- a change confirmed at a counter (OA-119 to OA-122) ----
    const val waitingRowTitle = "Change waiting for a counter"
    const val waitingHeading = "Confirm it at a counter"
    const val waitingText = "This change loosens the shop's settings, so it waits. On a counter the shop already has, open Change to confirm and check these words."
    const val waitingWordsLabel = "The four words"
    const val waitingFieldsLabel = "What it loosens"
    const val waitingEnds = "If no counter confirms it within seven days, it is dropped and the settings stay as they are."
    const val waitingDropped = "No change is waiting"
    const val pairedLater = "This counter takes sales now. It joins the other devices through the Box once a counter the shop already has confirms the change."

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

    /** The four words of the Box differ: nothing was sent to it (BX-18). */
    val boxDifferStopped = SayWords(
        Tones.STOP, Who.OWNER, "Box not connected",
        "The four words differ, so the device that answered may not be your Box, and nothing was sent to it.",
        "Check the Box's address, enter a new invitation on it and start again.", "BX-18", null, null, null,
    )

    /** The Box took its welcome and the link could not be saved (BX-18). */
    val boxLinkNotSaved = SayWords(
        Tones.STOP, Who.OWNER, "Link to the Box not saved",
        "The Box took its welcome, but this phone could not save the link to it, so it cannot reach the Box yet.",
        "Free some storage on this phone, then tap Connect the Box again.", "BX-18", null, null, null,
    )

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
        add("boxTitle", boxTitle); add("boxAddressHint", boxAddressHint)
        add("boxInviteHeading", boxInviteHeading)
        add("boxInviteText", boxInviteText)
        add("boxInviteNext", boxInviteNext)
        add("boxAddressLabel", boxAddressLabel)
        add("boxAddressScan", boxAddressScan)
        add("boxAddressLook", boxAddressLook)
        add("boxWordsHeading", boxWordsHeading)
        add("boxWordsText", boxWordsText)
        add("boxWordsMatch", boxWordsMatch)
        add("boxWordsDiffer", boxWordsDiffer)
        add("boxRestHeading", boxRestHeading)
        add("boxRestText", boxRestText)
        add("boxSend", boxSend)
        add("boxPairedHeading", boxPairedHeading)
        add("boxPairedText", boxPairedText)
        add("boxPairedNext", boxPairedNext)
        add("boxRelayHeading", boxRelayHeading)
        add("boxRelayText", boxRelayText)
        add("boxRelayList", boxRelayList)
        add("boxRelayTaken", boxRelayTaken)
        add("boxRelayLater", boxRelayLater)
        add("waitingRowTitle", waitingRowTitle)
        add("waitingHeading", waitingHeading)
        add("waitingText", waitingText)
        add("waitingWordsLabel", waitingWordsLabel)
        add("waitingFieldsLabel", waitingFieldsLabel)
        add("waitingEnds", waitingEnds)
        add("waitingDropped", waitingDropped)
        add("pairedLater", pairedLater)
        add("boxStartAgain", boxStartAgain)
        for ((k, w) in listOf("boxDifferStopped" to boxDifferStopped, "boxLinkNotSaved" to boxLinkNotSaved)) {
            add("$k.heading", w.heading); add("$k.what", w.what); add("$k.next", w.next)
        }
        return out
    }
}
