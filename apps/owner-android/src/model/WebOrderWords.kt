package org.openbmt.owner.app

/**
 * The owner app's own words on the web store screens (`docs/WEB-ORDER-RULES.md`). The words the library gives (why
 * web orders are off, why a visit failed, why an order was refused) come from the library. The list is written to
 * docs/screens/owner-app.md by [AppWords.screensMarkdown]. No Android class is used.
 */
object WebOrderWords {
    // ---- the row under Settings and this phone, and the page ----
    const val rowTitle = "Web store"
    const val rowSub = "Bitcoin at checkout in your own online store"
    const val pageTitle = "Your web store"
    const val back = "Back"
    const val whatItIs = "Customers of your WooCommerce store can pay in bitcoin at checkout. Each order gets a new address of the shop's wallet, and this phone tells the store when the order is paid."
    const val noStore = "No store is paired with this phone yet."
    const val start = "Pair a store"

    // ---- pairing a store (WO-05, WO-06) ----
    const val pairTitle = "Pair your store"
    const val addressLabel = "The store's web address"
    const val addressHint = "https://shop.example.com"
    const val nameLabel = "Shop name shown at checkout"
    const val fieldsMissing = "Type the store's address and the shop's name first."
    const val makeCode = "Make the pairing code"
    const val codeTitle = "Pairing code"
    const val codePaste = "On a computer, open the store's admin pages, then the OpenBMT payment settings in WooCommerce, and paste this code there."
    const val codeCopy = "Copy the code"
    const val codeCopied = "Copied. Paste it in the store's settings."
    const val fingerprintLabel = "Key check"
    const val compare = "Once the code is in, the store shows a key check. It must be the same as the one here."
    const val match = "They are the same"
    const val differ = "They differ"
    const val waiting = "A pairing was started and not finished."
    const val continuePairing = "Finish the pairing"

    // ---- the store's page (WO-11, WO-12, WO-20 to WO-22, WO-30 to WO-32) ----
    const val shopLabel = "Shop name"
    const val storeLabel = "Store address"
    const val lastVisitLabel = "Last visit"
    const val neverVisited = "Not visited yet"
    const val nextVisitLabel = "Next try"
    const val visitNow = "Visit the store now"
    const val sentLabel = "Addresses given to the store"
    const val leftLabel = "Addresses not used yet"
    const val ordersLabel = "Orders taken"
    const val waitingLabel = "Orders waiting for payment"
    const val toldLabel = "Orders the store was told are paid"
    const val noticesLabel = "Paid notices still to send"
    const val refusedTitle = "Orders this phone refused. Do not ship them before you have checked them in the store."
    const val refusedOrderLabel = "Refused order"

    // ---- unpairing (WO-07) ----
    const val unpair = "Unpair the store"
    const val unpairTitle = "Unpairing the store"
    const val unpairWhat = "This phone stops taking the store's orders. Unpair it in the store's settings too, so that it stops offering bitcoin at checkout."
    const val unpairConfirm = "Unpair"
    const val unpairKeep = "Addresses already given to the store stay watched, so a late payment is still seen."

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

}
