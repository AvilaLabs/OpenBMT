package org.openbmt.owner.app

import org.openbmt.app.SayWords
import org.openbmt.app.Tones
import org.openbmt.app.Who

/**
 * The owner app's own words on the Shop Lightning screens (`docs/SHOP-LIGHTNING-RULES.md`, SL-04, SL-45, SL-70 to
 * SL-78), listed in screen order. The words the library gives for a refusal (no Box, a node's ID that is not one, a
 * provider's address that is not one, a shop with its own wallet) are the library's own. The list is
 * written to docs/screens/owner-app.md by [AppWords.screensMarkdown]. No Android class is used.
 */
object ShopLightningWords {
    // ---- the row under Settings and this phone, and the page when it is off (SL-01, SL-04, SL-73, SL-76) ----
    const val rowTitle = "Shop Lightning"
    const val rowSubOff = "Off: Lightning payments on one of the shop's counters"
    const val rowSubOn = "On: one counter takes Lightning payments"
    const val pageTitle = "Lightning on a counter"
    const val back = "Back"
    const val whatItIs = "One of the shop's counters takes Lightning payments with a node of its own. The shop's Box keeps a second copy of the node's state, a provider the shop picks opens its channels, and money above a cap moves to the shop's wallet."
    const val noBox = "Shop Lightning needs the shop's Box, which keeps the second copy of the node's state. Add a Box first."
    const val ownWallet = "For a shop paid to its own wallet, Shop Lightning cannot be turned on from this phone yet."
    const val turnOn = "Turn on"
    // ---- the turn-on steps, and the approval of a change (SL-03, SL-28, SL-29, SL-32, SL-34, SL-71, SL-72, SL-76) ----
    const val stepOf = "Step {n} of {of}"
    const val counterTitle = "Pick the Lightning counter"
    const val counterWhat = "Lightning is taken on this counter only, and it holds the node's key. Pick one that stays at the shop."
    const val noCounters = "The shop has no counter yet. Add a counter first."
    const val nodeTitle = "Scan the node's ID"
    const val nodeWhat = "On the stick that holds the Lightning key, choose to show the ID of Shop Lightning's node, then scan the code it shows."
    const val scanNode = "Scan the ID"
    const val nodeScanned = "Check that its start and its end are the ones the stick shows."
    const val nodeNotId = "Not a node's ID"
    const val nodeScanCancelled = "ID scan cancelled"
    const val providerTitle = "Pick a provider"
    const val providerWhat = "A liquidity provider opens the channels that payments arrive in, for a fee. The shop picks any provider that offers LSPS2 and can change it at any time. Copy its node ID and its address from its own page."
    const val providerKeyLabel = "Provider's node ID"
    const val providerAddrLabel = "Provider's address, IP and port"
    const val providerMissing = "Provider not complete"
    const val limitsTitle = "Set the limits"
    const val capLabel = "Cap, in sats"
    const val capWhat = "The most the node holds. Above it, everything over half the cap moves to the shop's wallet. One day of Lightning sales is a good start."
    const val feeLabel = "Most for one opening, in sats"
    const val feeWhat = "A payment that needs a new channel costs the provider's fee, shown before the invoice is made. A sale whose opening costs more than this is paid on-chain instead."
    const val numberInvalid = "Not a whole number"
    const val next = "Next"
    const val approveTitle = "Check the change"
    const val approveFree = "This change only tightens the settings, so it needs no approval from the Key app."
    const val approveLoosen = "This change loosens the settings, so the Key app must approve it."
    const val approveRedirect = "This change sends payments somewhere new, so the Key app must approve it."
    const val nothingChanged = "These are the settings already in force, so there is nothing to change."
    const val queue = "Queue the change"
    const val queuedKeyApp = "The change is queued. It takes effect once the Key app approves it."
    const val queuedDone = "The change is queued and goes to the counters at their next check-in."
    const val buildRequest = "Make the request for the Key app"
    const val later = "Later"
    // ---- the page when it is on: the facts of the newest report and the settings (SL-41, SL-45, SL-78) ----
    const val onTitle = "Shop Lightning is on"
    const val counterLabel = "Lightning counter"
    const val nodeLabel = "Node's ID"
    const val balanceLabel = "Balance and cap"
    const val roomLabel = "Room to receive"
    const val providerLabel = "Provider"
    const val feesPaidLabel = "Opening fees paid"
    const val boxLabel = "Second copy on"
    const val lastStoredLabel = "Last stored"
    const val neverStored = "Not yet"
    const val lastSweepLabel = "Last move to the wallet"
    const val noSweep = "None yet"
    const val cameLabel = "Report from the counter"
    const val noReport = "No report yet"
    const val whyLabel = "Lightning now"
    const val offered = "Offered"
    const val secondDeviceAway = "Not offered: the Box cannot be reached"
    const val chainBehind = "Not offered: the counter is behind the chain"
    const val noUsableChannel = "Not offered: no channel can take a payment"
    const val providerSilent = "Not offered: the provider does not answer"
    const val changeLimits = "Change the limits"
    const val changeProvider = "Change the provider"
    const val save = "Save"
    // ---- turning it off (SL-77) ----
    const val turnOff = "Turn off"
    const val turnOffWhat = "Turning Shop Lightning off closes every channel of the node. Their money goes to the shop's wallet once the closes confirm, and the counter then deletes the Lightning key. Sales go on, paid on-chain."
    const val turnOffConfirm = "Turn off and close the channels"
    const val turnOffKeep = "Keep it on"

    private fun stop(heading: String, what: String, next: String, code: String): SayWords =
        SayWords(Tones.STOP, Who.OWNER, heading, what, next, code, null, null, null)

    /** A scanned code that is not 66 lower-case hexadecimal digits (SL-76, SK-35). */
    val nodeNotIdWords: SayWords = stop(
        nodeNotId,
        "The code read is not the ID of a Lightning node, so no ID was kept.",
        "Scan the code the stick shows for the ID of Shop Lightning's node.",
        "SL-76",
    )

    /** The scan of the stick's code was cancelled (SL-76). */
    val nodeScanCancelledWords: SayWords = stop(
        nodeScanCancelled,
        "No code was read, so no node's ID was kept.",
        "Scan the stick's code again when you are ready.",
        "SL-76",
    )

    /** A typed number that is not a whole number, or a cap of zero (SL-34, SL-29, SL-71). */
    val numberInvalidWords: SayWords = stop(
        numberInvalid,
        "Each amount here is a whole number of sats, and the cap is above zero.",
        "Type the amounts in digits, then go on.",
        "SL-71",
    )

    /** The provider's two fields were not both filled (SL-71). */
    val providerMissingWords: SayWords = stop(
        providerMissing,
        "A provider needs both its node ID and its address, and one of them is empty.",
        "Fill in both from the provider's own page.",
        "SL-71",
    )

    /** The shop has no counter to run the node (SL-03). */
    val noCountersWords: SayWords = stop(
        "No counter yet",
        "Shop Lightning runs on one of the shop's counters, and the shop has none.",
        "Add a counter, then turn Shop Lightning on.",
        "SL-03",
    )

    /** The app's own messages, for the checks of the words. */
    fun messages(): List<Pair<String, SayWords>> = listOf(
        "nodeNotId" to nodeNotIdWords,
        "nodeScanCancelled" to nodeScanCancelledWords,
        "numberInvalid" to numberInvalidWords,
        "providerMissing" to providerMissingWords,
        "noCounters" to noCountersWords,
    )

    /** The text that says why Lightning is not offered, for each cause of the report (SL-41). */
    fun cause(c: org.openbmt.owner.NotOffered): String = when (c) {
        org.openbmt.owner.NotOffered.Offered -> offered
        org.openbmt.owner.NotOffered.SecondDeviceAway -> secondDeviceAway
        org.openbmt.owner.NotOffered.ChainBehind -> chainBehind
        org.openbmt.owner.NotOffered.NoUsableChannel -> noUsableChannel
        org.openbmt.owner.NotOffered.ProviderSilent -> providerSilent
    }

    /** Every text with the name it is listed under, in screen order, for the screens table and the checks. */
    fun all(): List<Pair<String, String>> = listOf(
        "rowTitle" to rowTitle,
        "rowSubOff" to rowSubOff,
        "rowSubOn" to rowSubOn,
        "pageTitle" to pageTitle,
        "back" to back,
        "whatItIs" to whatItIs,
        "noBox" to noBox,
        "ownWallet" to ownWallet,
        "turnOn" to turnOn,
        "stepOf" to stepOf,
        "counterTitle" to counterTitle,
        "counterWhat" to counterWhat,
        "noCounters" to noCounters,
        "nodeTitle" to nodeTitle,
        "nodeWhat" to nodeWhat,
        "scanNode" to scanNode,
        "nodeScanned" to nodeScanned,
        "nodeNotId" to nodeNotId,
        "nodeScanCancelled" to nodeScanCancelled,
        "providerTitle" to providerTitle,
        "providerWhat" to providerWhat,
        "providerKeyLabel" to providerKeyLabel,
        "providerAddrLabel" to providerAddrLabel,
        "providerMissing" to providerMissing,
        "limitsTitle" to limitsTitle,
        "capLabel" to capLabel,
        "capWhat" to capWhat,
        "feeLabel" to feeLabel,
        "feeWhat" to feeWhat,
        "numberInvalid" to numberInvalid,
        "next" to next,
        "approveTitle" to approveTitle,
        "approveFree" to approveFree,
        "approveLoosen" to approveLoosen,
        "approveRedirect" to approveRedirect,
        "nothingChanged" to nothingChanged,
        "queue" to queue,
        "queuedKeyApp" to queuedKeyApp,
        "queuedDone" to queuedDone,
        "buildRequest" to buildRequest,
        "later" to later,
        "onTitle" to onTitle,
        "counterLabel" to counterLabel,
        "nodeLabel" to nodeLabel,
        "balanceLabel" to balanceLabel,
        "roomLabel" to roomLabel,
        "providerLabel" to providerLabel,
        "feesPaidLabel" to feesPaidLabel,
        "boxLabel" to boxLabel,
        "lastStoredLabel" to lastStoredLabel,
        "neverStored" to neverStored,
        "lastSweepLabel" to lastSweepLabel,
        "noSweep" to noSweep,
        "cameLabel" to cameLabel,
        "noReport" to noReport,
        "whyLabel" to whyLabel,
        "offered" to offered,
        "secondDeviceAway" to secondDeviceAway,
        "chainBehind" to chainBehind,
        "noUsableChannel" to noUsableChannel,
        "providerSilent" to providerSilent,
        "changeLimits" to changeLimits,
        "changeProvider" to changeProvider,
        "save" to save,
        "turnOff" to turnOff,
        "turnOffWhat" to turnOffWhat,
        "turnOffConfirm" to turnOffConfirm,
        "turnOffKeep" to turnOffKeep,
    )

}
