package org.openbmt.owner.app

import org.openbmt.app.SayWords
import org.openbmt.app.Tones
import org.openbmt.app.Who

/**
 * The owner app's own words on the Shop Lightning screens (`docs/SHOP-LIGHTNING-RULES.md`, SL-04, SL-45, SL-70 to
 * SL-78). No sentence is written yet: every text is a `WORDS_NEEDED_SL<rule>_<NAME>` placeholder that shows its own
 * name wherever the words will stand, and the rule in it is the one the text is for. The words the library gives for a
 * refusal (no Box, a node's ID that is not one, a provider's address that is not one, a shop with its own wallet) come
 * with the library's own placeholders, `WORDS_NEEDED_SL<rule>_<NAME>_HEADING`, `_WHAT` and `_NEXT`. The list is
 * written to docs/screens/owner-app.md by [AppWords.screensMarkdown]. No Android class is used.
 */
object ShopLightningWords {
    // ---- the row under Settings and this phone, and the page when it is off (SL-01, SL-04, SL-73, SL-76) ----
    const val rowTitle = "WORDS_NEEDED_SL04_ROW_TITLE"
    const val rowSubOff = "WORDS_NEEDED_SL04_ROW_SUB_OFF"
    const val rowSubOn = "WORDS_NEEDED_SL45_ROW_SUB_ON"
    const val pageTitle = "WORDS_NEEDED_SL04_PAGE_TITLE"
    const val back = "WORDS_NEEDED_SL04_BACK"
    const val whatItIs = "WORDS_NEEDED_SL01_WHAT_IT_IS"
    const val noBox = "WORDS_NEEDED_SL73_NO_BOX"
    const val ownWallet = "WORDS_NEEDED_SL76_OWN_WALLET"
    const val turnOn = "WORDS_NEEDED_SL76_TURN_ON"
    // ---- the turn-on steps, and the approval of a change (SL-03, SL-28, SL-29, SL-32, SL-34, SL-71, SL-72, SL-76) ----
    const val stepOf = "WORDS_NEEDED_SL76_STEP_OF"
    const val counterTitle = "WORDS_NEEDED_SL76_STEP_COUNTER_TITLE"
    const val counterWhat = "WORDS_NEEDED_SL03_STEP_COUNTER_WHAT"
    const val noCounters = "WORDS_NEEDED_SL03_NO_COUNTERS"
    const val nodeTitle = "WORDS_NEEDED_SL76_STEP_NODE_TITLE"
    const val nodeWhat = "WORDS_NEEDED_SL76_STEP_NODE_WHAT"
    const val scanNode = "WORDS_NEEDED_SL76_SCAN_NODE"
    const val nodeScanned = "WORDS_NEEDED_SL76_NODE_SCANNED"
    const val nodeNotId = "WORDS_NEEDED_SL76_NODE_NOT_ID"
    const val nodeScanCancelled = "WORDS_NEEDED_SL76_NODE_SCAN_CANCELLED"
    const val providerTitle = "WORDS_NEEDED_SL76_STEP_PROVIDER_TITLE"
    const val providerWhat = "WORDS_NEEDED_SL28_STEP_PROVIDER_WHAT"
    const val providerKeyLabel = "WORDS_NEEDED_SL28_PROVIDER_KEY_LABEL"
    const val providerAddrLabel = "WORDS_NEEDED_SL32_PROVIDER_ADDR_LABEL"
    const val providerMissing = "WORDS_NEEDED_SL71_PROVIDER_MISSING"
    const val limitsTitle = "WORDS_NEEDED_SL76_STEP_LIMITS_TITLE"
    const val capLabel = "WORDS_NEEDED_SL34_CAP_LABEL"
    const val capWhat = "WORDS_NEEDED_SL34_CAP_WHAT"
    const val feeLabel = "WORDS_NEEDED_SL29_FEE_LABEL"
    const val feeWhat = "WORDS_NEEDED_SL29_FEE_WHAT"
    const val numberInvalid = "WORDS_NEEDED_SL71_NUMBER_INVALID"
    const val next = "WORDS_NEEDED_SL76_NEXT"
    const val approveTitle = "WORDS_NEEDED_SL76_STEP_APPROVE_TITLE"
    const val approveFree = "WORDS_NEEDED_SL76_APPROVE_FREE"
    const val approveLoosen = "WORDS_NEEDED_SL76_APPROVE_LOOSEN"
    const val approveRedirect = "WORDS_NEEDED_SL76_APPROVE_REDIRECT"
    const val nothingChanged = "WORDS_NEEDED_SL76_NOTHING_CHANGED"
    const val queue = "WORDS_NEEDED_SL76_QUEUE"
    const val queuedKeyApp = "WORDS_NEEDED_SL76_QUEUED_KEY_APP"
    const val queuedDone = "WORDS_NEEDED_SL77_QUEUED_DONE"
    const val buildRequest = "WORDS_NEEDED_SL76_BUILD_REQUEST"
    const val later = "WORDS_NEEDED_SL76_LATER"
    // ---- the page when it is on: the facts of the newest report and the settings (SL-41, SL-45, SL-78) ----
    const val onTitle = "WORDS_NEEDED_SL45_ON_TITLE"
    const val counterLabel = "WORDS_NEEDED_SL70_COUNTER_LABEL"
    const val nodeLabel = "WORDS_NEEDED_SL72_NODE_LABEL"
    const val balanceLabel = "WORDS_NEEDED_SL45_BALANCE_LABEL"
    const val roomLabel = "WORDS_NEEDED_SL45_ROOM_LABEL"
    const val providerLabel = "WORDS_NEEDED_SL28_PROVIDER_LABEL"
    const val feesPaidLabel = "WORDS_NEEDED_SL45_FEES_PAID_LABEL"
    const val boxLabel = "WORDS_NEEDED_SL73_BOX_LABEL"
    const val lastStoredLabel = "WORDS_NEEDED_SL45_LAST_STORED_LABEL"
    const val neverStored = "WORDS_NEEDED_SL45_NEVER_STORED"
    const val lastSweepLabel = "WORDS_NEEDED_SL33_LAST_SWEEP_LABEL"
    const val noSweep = "WORDS_NEEDED_SL33_NO_SWEEP"
    const val cameLabel = "WORDS_NEEDED_SL78_CAME_LABEL"
    const val noReport = "WORDS_NEEDED_SL78_NO_REPORT"
    const val whyLabel = "WORDS_NEEDED_SL41_WHY_LABEL"
    const val offered = "WORDS_NEEDED_SL41_OFFERED"
    const val secondDeviceAway = "WORDS_NEEDED_SL41_SECOND_DEVICE_AWAY"
    const val chainBehind = "WORDS_NEEDED_SL41_CHAIN_BEHIND"
    const val noUsableChannel = "WORDS_NEEDED_SL41_NO_USABLE_CHANNEL"
    const val providerSilent = "WORDS_NEEDED_SL41_PROVIDER_SILENT"
    const val changeLimits = "WORDS_NEEDED_SL77_CHANGE_LIMITS"
    const val changeProvider = "WORDS_NEEDED_SL77_CHANGE_PROVIDER"
    const val save = "WORDS_NEEDED_SL77_SAVE"
    // ---- turning it off (SL-77) ----
    const val turnOff = "WORDS_NEEDED_SL77_TURN_OFF"
    const val turnOffWhat = "WORDS_NEEDED_SL77_TURN_OFF_WHAT"
    const val turnOffConfirm = "WORDS_NEEDED_SL77_TURN_OFF_CONFIRM"
    const val turnOffKeep = "WORDS_NEEDED_SL77_TURN_OFF_KEEP"

    /** Words that are still to be written: the placeholder stands in the heading and both sentences. */
    fun said(placeholder: String): SayWords =
        SayWords(Tones.STOP, Who.OWNER, placeholder, placeholder, placeholder, null, null, null, null)

    /** A node's ID that was scanned and is not 66 lower-case hexadecimal digits (SL-76, SK-35). */
    val nodeNotIdWords: SayWords = said(nodeNotId)

    /** The scan of the stick's code was cancelled (SL-76). */
    val nodeScanCancelledWords: SayWords = said(nodeScanCancelled)

    /** A typed number that is not a whole number above zero (SL-34, SL-29, SL-71). */
    val numberInvalidWords: SayWords = said(numberInvalid)

    /** The provider's two fields were not both filled (SL-71). */
    val providerMissingWords: SayWords = said(providerMissing)

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

    /** Whether a text is a placeholder of this file. */
    fun isPlaceholder(text: String): Boolean = text.startsWith("WORDS_NEEDED_SL")
}
