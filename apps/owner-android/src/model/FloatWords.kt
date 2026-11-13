package org.openbmt.owner.app

/**
 * The owner app's own words on the float screens (`docs/FLOAT-RULES.md`).
 */
object FloatWords {
    val rowTitle = "Refund float"
    val back = "Back"
    val setupTitle = "Set up the refund float"
    val childOnly = "The float has 12 words of its own, made from the shop's words on the stick. Only the float's words come to this phone, never the shop's."
    val scanChild = "Scan the float's words"
    val storageUnavailable = "This phone could not open its record of the float. Restart the app. If it keeps failing, check the phone's free space."
    val childIndex = "The first time, make the float's words on the stick with number 0."
    val nextIndex = "After a lost or replaced phone, make the next number on the stick. The new phone first moves what is left in the old float."
    val scanUnavailable = "The float is not set up. Scanning its words comes in a later version."
    val capSaved = "Cap saved. No money was moved."
    val capLabel = "Most the float may hold, in sats"
    val capInvalid = "Type a whole number of sats above zero."
    val saveCap = "Save the cap"
    val unavailable = "What the float holds is not known yet."
    val statusReady = "The float is ready for refunds."
    val statusLow = "The float is low. Refill it from the shop's savings."
    val refillAction = "Refill the float"
    val heldLabel = "The float holds"
    val refillLinksSavings = "A refill is a payment from the shop's savings to the float. Anyone who looks at the chain can see that the two belong together."
    val refundPay = "Pay a refund from the float"
    val refundUnavailable = "Paying a refund from the float comes in a later version. Refunds in the queue can still be paid from savings."
    val payConfirm = "Pay this refund"
    val sweepTitle = "Turn the float off"
    val sweepConfirm = "Move it all to savings"
    val sweepConfirmationsUnknown = "Turning the float off moves all of it back to the shop's savings. That needs what the float holds, which is not known yet."
    val floatUnavailable = "The refund float is off."

    /** Every text, in screen order. */
    fun all(): List<String> = listOf(
        rowTitle,
        back,
        setupTitle,
        childOnly,
        scanChild,
        storageUnavailable,
        childIndex,
        nextIndex,
        scanUnavailable,
        capSaved,
        capLabel,
        capInvalid,
        saveCap,
        unavailable,
        statusReady,
        statusLow,
        refillAction,
        heldLabel,
        refillLinksSavings,
        refundPay,
        refundUnavailable,
        payConfirm,
        sweepTitle,
        sweepConfirm,
        sweepConfirmationsUnknown,
        floatUnavailable,
    )
}
