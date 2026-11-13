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
    val scanUnavailable = "The float is not set up yet. Scan its words from the stick to finish."
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
    val payConfirm = "Pay this refund"
    val sweepTitle = "Turn the float off"
    val sweepConfirm = "Move it all to savings"
    val sweepConfirmationsUnknown = "Turning the float off moves all of it back to the shop's savings. That needs what the float holds, which is not known yet."
    val floatUnavailable = "The refund float is off."
    val needCapFirst = "Save a cap for the float first. Then scan its words."
    val scanFailed = "The float's words were not read, so nothing was kept. Scan the code on the stick again."
    val sealFailed = "The float's key was not locked on this phone, so nothing was kept. Try again."
    val unlockWhy = "Your fingerprint, face or phone PIN locks the float's key on this phone."
    val statusNotWatched = "What the float holds shows once this phone has caught up with the latest blocks."
    val refillNoRoom = "The float already holds its cap, so it needs no refill."
    val refundListEmpty = "No refund is waiting to be paid."
    val refundReadyHeading = "These refunds are ready. Tap one to pay it from the float."
    val refundTooBig = "Some ready refunds are more than the float holds. Pay those from the shop's savings, or refill the float first."
    val keyGone = "The float's key is no longer on this phone, so nothing can be paid from it. The money in the float is safe: the stick makes the float's words again from the shop's words."
    val payDoneHeading = "Refund on its way"
    val payDoneWhat = "The float paid this refund, and this phone is sending it to the customer's address."
    val payDoneNext = "Nothing to do."
    val sweepBefore = "Turning the float off moves all it holds back to the shop's savings in one payment. Then the float's key is deleted from this phone."
    val sweepNothing = "The float is empty, so there is nothing to move."
    val sweepWaiting = "The float's key can be deleted once a block holds the move to savings."
    val sweepDepthUnknown = "This phone cannot tell yet whether the move to savings went out. Wait, or sign it again: it spends the same coins to the same address, so nothing is paid twice."
    val sweepDeletedHeading = "Refund float off"
    val sweepDeletedWhat = "All the float held is back in the shop's savings, and its key is gone from this phone."
    val sweepDeletedNext = "Keep the stick and the shop's words as they are."

    // FL-06: scanning the float's words again, to bring back the key the phone dropped. Not written yet (OA-114): each is a
    // placeholder that shows its own name where the words will be.
    val restoreScan = "WORDS_NEEDED_OA_FL06_SCAN"
    val restoreWhat = "WORDS_NEEDED_OA_FL06_WHAT"
    val restoreDone = "WORDS_NEEDED_OA_FL06_DONE"
    val restoreNeedsFloat = "WORDS_NEEDED_OA_FL06_NO_FLOAT"

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
        payConfirm,
        sweepTitle,
        sweepConfirm,
        sweepConfirmationsUnknown,
        floatUnavailable,
        needCapFirst,
        scanFailed,
        sealFailed,
        unlockWhy,
        statusNotWatched,
        refillNoRoom,
        refundListEmpty,
        refundReadyHeading,
        refundTooBig,
        keyGone,
        payDoneHeading,
        payDoneWhat,
        payDoneNext,
        sweepBefore,
        sweepNothing,
        sweepWaiting,
        sweepDepthUnknown,
        sweepDeletedHeading,
        sweepDeletedWhat,
        sweepDeletedNext,
        restoreScan,
        restoreWhat,
        restoreDone,
        restoreNeedsFloat,
    )

    /** FL-06: the texts not written yet, each named after its rule. */
    fun placeholders(): List<String> = all().filter { it.startsWith("WORDS_NEEDED_") }
}
