package org.openbmt.owner.app

/**
 * The owner app's own words on the web store screens (`docs/WEB-ORDER-RULES.md`). No sentence is written yet: every
 * text is a `WORDS_NEEDED_WO_<rule>_<SUFFIX>` placeholder that shows its own name wherever the words will stand, and
 * the rule in it is the one the text is for. The words the library gives (why web orders are off, why a visit failed,
 * why an order was refused) come with the library's own placeholders, `WORDS_NEEDED_WO_<rule>_HEADING`, `_WHAT` and
 * `_NEXT`. The list is written to docs/screens/owner-app.md by [AppWords.screensMarkdown]. No Android class is used.
 */
object WebOrderWords {
    // ---- the row under Settings and this phone, and the page ----
    const val rowTitle = "WORDS_NEEDED_WO_05_ROW_TITLE"
    const val rowSub = "WORDS_NEEDED_WO_05_ROW_SUB"
    const val pageTitle = "WORDS_NEEDED_WO_05_PAGE_TITLE"
    const val back = "WORDS_NEEDED_WO_05_BACK"
    const val whatItIs = "WORDS_NEEDED_WO_01_WHAT"
    const val noStore = "WORDS_NEEDED_WO_05_NO_STORE"
    const val start = "WORDS_NEEDED_WO_05_START"

    // ---- pairing a store (WO-05, WO-06) ----
    const val pairTitle = "WORDS_NEEDED_WO_05_PAIR_TITLE"
    const val addressLabel = "WORDS_NEEDED_WO_06_ADDRESS_LABEL"
    const val addressHint = "WORDS_NEEDED_WO_06_ADDRESS_HINT"
    const val nameLabel = "WORDS_NEEDED_WO_05_NAME_LABEL"
    const val fieldsMissing = "WORDS_NEEDED_WO_06_FIELDS_MISSING"
    const val makeCode = "WORDS_NEEDED_WO_05_MAKE_CODE"
    const val codeTitle = "WORDS_NEEDED_WO_05_CODE_TITLE"
    const val codePaste = "WORDS_NEEDED_WO_05_CODE_PASTE"
    const val codeCopy = "WORDS_NEEDED_WO_05_CODE_COPY"
    const val codeCopied = "WORDS_NEEDED_WO_05_CODE_COPIED"
    const val fingerprintLabel = "WORDS_NEEDED_WO_05_FINGERPRINT_LABEL"
    const val compare = "WORDS_NEEDED_WO_05_COMPARE"
    const val match = "WORDS_NEEDED_WO_05_MATCH"
    const val differ = "WORDS_NEEDED_WO_05_DIFFER"
    const val waiting = "WORDS_NEEDED_WO_05_WAITING"
    const val continuePairing = "WORDS_NEEDED_WO_05_CONTINUE"

    // ---- the store's page (WO-11, WO-12, WO-20 to WO-22, WO-30 to WO-32) ----
    const val shopLabel = "WORDS_NEEDED_WO_05_SHOP_LABEL"
    const val storeLabel = "WORDS_NEEDED_WO_06_STORE_LABEL"
    const val lastVisitLabel = "WORDS_NEEDED_WO_21_LAST_VISIT_LABEL"
    const val neverVisited = "WORDS_NEEDED_WO_21_NEVER"
    const val nextVisitLabel = "WORDS_NEEDED_WO_21_NEXT_VISIT_LABEL"
    const val visitNow = "WORDS_NEEDED_WO_21_VISIT_NOW"
    const val noSender = "WORDS_NEEDED_WO_02_NO_SENDER"
    const val sentLabel = "WORDS_NEEDED_WO_12_SENT_LABEL"
    const val leftLabel = "WORDS_NEEDED_WO_11_LEFT_LABEL"
    const val ordersLabel = "WORDS_NEEDED_WO_20_ORDERS_LABEL"
    const val waitingLabel = "WORDS_NEEDED_WO_20_WAITING_LABEL"
    const val toldLabel = "WORDS_NEEDED_WO_30_TOLD_LABEL"
    const val noticesLabel = "WORDS_NEEDED_WO_32_NOTICES_LABEL"
    const val refusedTitle = "WORDS_NEEDED_WO_22_REFUSED_TITLE"
    const val refusedOrderLabel = "WORDS_NEEDED_WO_22_REFUSED_ORDER_LABEL"

    // ---- unpairing (WO-07) ----
    const val unpair = "WORDS_NEEDED_WO_07_UNPAIR"
    const val unpairTitle = "WORDS_NEEDED_WO_07_UNPAIR_TITLE"
    const val unpairWhat = "WORDS_NEEDED_WO_07_UNPAIR_WHAT"
    const val unpairConfirm = "WORDS_NEEDED_WO_07_UNPAIR_CONFIRM"
    const val unpairKeep = "WORDS_NEEDED_WO_07_UNPAIR_KEEP"

    /** Every text with the name it is listed under, in screen order, for the screens table and the checks. */
    fun all(): List<Pair<String, String>> = listOf(
        "rowTitle" to rowTitle,
        "rowSub" to rowSub,
        "pageTitle" to pageTitle,
        "back" to back,
        "whatItIs" to whatItIs,
        "noStore" to noStore,
        "start" to start,
        "pairTitle" to pairTitle,
        "addressLabel" to addressLabel,
        "addressHint" to addressHint,
        "nameLabel" to nameLabel,
        "fieldsMissing" to fieldsMissing,
        "makeCode" to makeCode,
        "codeTitle" to codeTitle,
        "codePaste" to codePaste,
        "codeCopy" to codeCopy,
        "codeCopied" to codeCopied,
        "fingerprintLabel" to fingerprintLabel,
        "compare" to compare,
        "match" to match,
        "differ" to differ,
        "waiting" to waiting,
        "continuePairing" to continuePairing,
        "shopLabel" to shopLabel,
        "storeLabel" to storeLabel,
        "lastVisitLabel" to lastVisitLabel,
        "neverVisited" to neverVisited,
        "nextVisitLabel" to nextVisitLabel,
        "visitNow" to visitNow,
        "noSender" to noSender,
        "sentLabel" to sentLabel,
        "leftLabel" to leftLabel,
        "ordersLabel" to ordersLabel,
        "waitingLabel" to waitingLabel,
        "toldLabel" to toldLabel,
        "noticesLabel" to noticesLabel,
        "refusedTitle" to refusedTitle,
        "refusedOrderLabel" to refusedOrderLabel,
        "unpair" to unpair,
        "unpairTitle" to unpairTitle,
        "unpairWhat" to unpairWhat,
        "unpairConfirm" to unpairConfirm,
        "unpairKeep" to unpairKeep,
    )

    /** Whether a text is a placeholder of this file. */
    fun isPlaceholder(text: String): Boolean = text.startsWith("WORDS_NEEDED_WO_")
}
