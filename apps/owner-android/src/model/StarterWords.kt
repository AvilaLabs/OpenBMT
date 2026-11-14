package org.openbmt.owner.app

import org.openbmt.app.SayWords
import org.openbmt.app.Tones
import org.openbmt.app.Who

/**
 * Every text of the starter wallet's screens that the app writes itself (`docs/START-SIMPLE-RULES.md`), listed under
 * the rule of its screen. A text not written yet is a `WORDS_NEEDED_OA_SSnn_...` placeholder that shows its own name
 * (the same pattern as `OwnPairWords`, OA-114). The words of the Rust core (a
 * refusal, the home line, the states of the limit, SS-43) are shown as they arrive. The list is written to
 * docs/screens/owner-app.md by [AppWords.screensMarkdown]. No Android class is used.
 */
object StarterWords {
    private val registry = ArrayList<Pair<String, String>>()

    private fun need(name: String): String {
        registry.add(name.removePrefix("WORDS_NEEDED_OA_") to name)
        return name
    }

    /** Written words, listed under the key of their rule. */
    private fun say(key: String, text: String): String {
        registry.add(key to text)
        return text
    }

    // ---- the entry (SS-24, the Money page) ----
    val rowTitle = say("SS00_ROW_TITLE", "Start simple")
    val rowSub = say("SS00_ROW_SUB", "A small wallet on this phone, for a shop's first sales")
    val screenTitle = say("SS00_SCREEN_TITLE", "Start simple")
    val startButton = say("SS00_START_BUTTON", "Start")
    val backButton = say("SS00_BACK_BUTTON", "Back")
    val doneButton = say("SS00_DONE_BUTTON", "Done")
    val noLockHeading = say("SS24_NO_LOCK_HEADING", "Set a screen lock first")
    val noLockWhat = say("SS24_NO_LOCK_WHAT", "The starter wallet's key is locked by the phone's screen lock, and this phone has none.")
    val noLockNext = say("SS24_NO_LOCK_NEXT", "Set a PIN, pattern or password in the phone's settings, then come back.")

    // ---- the plain words before the words (SS-13) ----
    val plainTitle = say("SS13_TITLE", "Before the words")
    val plainPen = say("SS13_PEN", "Write the 12 words on the card, in pen.")
    val plainNoPhoto = say("SS13_NO_PHOTO", "Never take a photo or a screenshot of them. Apps that steal bitcoin read people's photos for exactly this.")
    val plainNobodyAsks = say("SS13_NOBODY_ASKS", "No app and no person will ever ask you for these words. Anyone who does is a thief.")
    val plainContinue = say("SS13_CONTINUE", "Show the words")

    // ---- the checks before the key and before a payment (SS-33, SS-34) ----
    val checksTitle = say("SS33_TITLE", "Checks of this phone")
    val usbName = say("SS33_USB_NAME", "USB debugging is on")
    val usbWhy = say("SS33_USB_WHY", "A computer plugged into this phone could read and control it. Turn it off in the developer options unless you need it.")
    val rootName = say("SS33_ROOT_NAME", "This phone may be rooted")
    val rootWhy = say("SS33_ROOT_WHY", "Apps on a rooted phone can get around the phone's own protections, including the ones that keep this key.")
    val overlayName = say("SS33_OVERLAY_NAME", "An app can draw over others")
    val overlayWhy = say("SS33_OVERLAY_WHY", "An app that draws over other apps can put fake buttons over this one. Check that you know and trust it.")
    val serviceName = say("SS33_SERVICE_NAME", "An accessibility tool is on")
    val serviceWhy = say("SS33_SERVICE_WHY", "Accessibility tools can read the screen. This one says it is a tool for people who need it. Check that you know it.")
    val serviceNotToolName = say("SS33_SERVICE_NOT_TOOL_NAME", "An app reads the screen")
    val serviceNotToolWhy = say("SS33_SERVICE_NOT_TOOL_WHY", "An app that is not an accessibility tool may read and control the screen. Apps that steal bitcoin work this way.")
    val serviceNotToolRefused = say("SS33_SERVICE_NOT_TOOL_REFUSED", "No key is made and no words are shown while it is on. Turn it off in the phone's accessibility settings, then try again.")
    val patchName = say("SS34_PATCH_NAME", "The phone's updates are old")
    val patchWhy = say("SS34_PATCH_WHY", "This phone's last update is more than six months old, so it may lack fixes for known attacks. Update it if you can.")
    val patchUnknownName = say("SS34_PATCH_UNKNOWN_NAME", "Last update not known")
    val patchUnknownWhy = say("SS34_PATCH_UNKNOWN_WHY", "This phone does not say when it was last updated, so it may lack fixes for known attacks.")
    val overlayNote = say("SS33_OVERLAY_NOTE", "Android does not show every app that can draw over others, so one may be on this phone and not be listed here.")
    val confirmButton = say("SS33_CONFIRM_BUTTON", "Go on anyway")
    val confirmAgain = say("SS33_CONFIRM_AGAIN", "Each thing below was found on this phone. Go on only if you understand it.")

    // ---- the limit and the date (SS-35) ----
    val limitTitle = say("SS35_TITLE", "Your limit and your date")
    val limitLabel = say("SS35_LIMIT_LABEL", "Most it may hold")
    val priceLabel = say("SS35_PRICE_LABEL", "Price of one bitcoin")
    val dateLabel = say("SS35_DATE_LABEL", "Move to a key made offline by")
    val boxOnly = say("SS32_BOX_ONLY", "Payments from the starter wallet show on the counters only once the shop has a Box. Until then, this phone alone shows them.")
    val stopLabel = say("SS38_STOP_LABEL", "Stop bitcoin sales above my limit")
    val limitInvalid = say("SS35_LIMIT_INVALID", "Type the most the starter wallet may hold, as a whole number.")
    val dateInvalid = say("SS35_DATE_INVALID", "Type a date after today.")
    val diceLabel = say("SS03_DICE_LABEL", "Dice rolls, if you like")
    val diceInvalid = say("SS03_DICE_INVALID", "Type only numbers from 1 to 6.")
    val limitContinue = say("SS35_CONTINUE", "Make the wallet")

    // ---- the words, shown and checked (SS-10, SS-11) ----
    val pageTitle = say("SS10_PAGE_TITLE", "Write these on the card")
    val pageNext = say("SS10_PAGE_NEXT", "Next four words")
    val pageWritten = say("SS10_PAGE_WRITTEN", "All 12 are written")
    val checkTitle = say("SS10_CHECK_TITLE", "Check the card")
    val checkAsk = say("SS10_CHECK_ASK", "Type the first four letters of word number")
    val checkTyped = say("SS11_TYPED", "Letters typed")
    val checkAccepted = say("SS11_ACCEPTED", "Words checked")
    val checkNotAccepted = say("SS11_NOT_ACCEPTED", "Words checked so far")
    val keyBackspace = say("SS11_KEY_BACKSPACE", "Erase")
    val keyEnter = say("SS11_KEY_ENTER", "Enter")
    val checkWrong = say("SS10_CHECK_WRONG", "That is not the word on the card at that number. The words are shown again.")
    val madeHeading = say("SS10_MADE_HEADING", "Starter wallet made")

    // ---- keeping the key, the owner each time (SS-20, SS-21) ----
    val authTitle = say("SS21_AUTH_TITLE", "Unlock the starter wallet")
    val authSubtitle = say("SS21_AUTH_SUBTITLE", "Your fingerprint, face or phone PIN opens the key for this one step.")
    val authSeal = say("SS21_AUTH_SEAL", "Lock the key on this phone")
    val authPay = say("SS21_AUTH_PAY", "Pay this refund")
    val authMove = say("SS21_AUTH_MOVE", "Move everything to the new wallet")
    val authFailed = say("SS21_AUTH_FAILED", "The phone did not unlock the key. Nothing was signed.")
    val sealedHeading = say("SS20_SEALED_HEADING", "The key is on this phone")
    val sealedWhat = say("SS20_SEALED_WHAT", "The starter wallet's key is locked by this phone's key hardware and opens only with your fingerprint, face or PIN.")
    val sealedNext = say("SS20_SEALED_NEXT", "Keep the card with the 12 words somewhere safe, away from the phone.")
    val notSavedHeading = say("SS20_NOT_SAVED_HEADING", "The key was not kept")
    val notSavedWhat = say("SS20_NOT_SAVED_WHAT", "The phone could not lock the key, so nothing was kept and no wallet was made.")
    val notSavedNext = say("SS20_NOT_SAVED_NEXT", "Try again. If it happens again, make the key on the stick instead.")

    // ---- when the phone drops the key (SS-23) ----
    val keyGoneFallbackHeading = say("SS23_KEY_GONE_HEADING", "Key gone from this phone")
    val keyGoneFallbackWhat = say("SS23_KEY_GONE_WHAT", "The phone deleted the starter wallet's key, as it does when the screen lock is removed. The money is still in the wallet.")
    val keyGoneFallbackNext = say("SS23_KEY_GONE_NEXT", "Type the 12 words from the card to bring it back.")
    val restoreButton = say("SS23_RESTORE_BUTTON", "Type the 12 words")
    val restoreTitle = say("SS23_RESTORE_TITLE", "Bring back the starter key")
    val restoreAsk = say("SS23_RESTORE_ASK", "Type word number")
    val restoreFinish = say("SS23_RESTORE_FINISH", "Check the words")
    val restoredHeading = say("SS23_RESTORED_HEADING", "Starter key back")
    val restoredWhat = say("SS23_RESTORED_WHAT", "The 12 words are the starter wallet's, and the key is locked on this phone again.")
    val restoredNext = say("SS23_RESTORED_NEXT", "Keep the card somewhere safe.")

    // ---- the home line (SS-36) and the upgrade screen (SS-37) ----
    val lineHeld = say("SS36_HELD", "Holds")
    val lineLimit = say("SS36_LIMIT", "Limit")
    val lineMoveBy = say("SS36_MOVE_BY", "Move by")
    val lineNotYetUntil = say("SS37_NOT_YET_UNTIL", "Not yet, until")
    val upgradeTitle = say("SS37_TITLE", "Upgrade your wallet")
    val upgradeNow = say("SS37_UPGRADE_NOW", "Upgrade now")
    val notYet = say("SS37_NOT_YET", "Not yet")
    val notYetSentence = say("SS37_NOT_YET_SENTENCE", "My shop's money stays on a wallet made on my everyday phone.")
    val notYetAsk = say("SS37_NOT_YET_ASK", "To wait up to 7 more days, type this:")
    val notYetConfirm = say("SS37_NOT_YET_CONFIRM", "Wait")
    val notYetDiffers = say("SS37_NOT_YET_DIFFERS", "That is not the sentence exactly. Check each word.")
    val notYetGranted = say("SS37_NOT_YET_GRANTED", "The upgrade waits until the date shown. Sales go on.")

    // ---- paying a refund (SS-30, SS-31) and the move (SS-42) ----
    val payTitle = say("SS30_TITLE", "Pay from the starter wallet")
    val payEmpty = say("SS30_EMPTY", "No refund waits.")
    val payFeeRate = say("SS31_FEE_RATE", "Fee rate")
    val payAuthorise = say("SS31_AUTHORISE", "Check the address and the amount, then unlock")
    val paySendButton = say("SS31_SEND_BUTTON", "Send")
    val payAddressLabel = say("SS31_ADDRESS_LABEL", "To")
    val payAmountLabel = say("SS31_AMOUNT_LABEL", "Amount")
    val payFeeLabel = say("SS31_FEE_LABEL", "Fee")
    val payChangeLabel = say("SS31_CHANGE_LABEL", "Back to the starter wallet")
    val signedHeading = say("SS30_SIGNED_HEADING", "Signed, not sent yet")
    val signedWhat = say("SS30_SIGNED_WHAT", "The payment is signed. Check it once more before it goes.")
    val signedNext = say("SS30_SIGNED_NEXT", "Press Send.")
    val moveTitle = say("SS42_TITLE", "Move to the new wallet")
    val moveWalletLabel = say("SS42_WALLET_LABEL", "New wallet's description")
    val moveEverything = say("SS42_EVERYTHING", "Everything in the starter wallet goes to the new wallet in one payment.")
    val moveInvalid = say("SS42_INVALID", "That description is not a wallet this phone can read.")
    val moveCredentialOnly = say("SS42_CREDENTIAL_ONLY", "This last step takes the phone's PIN, not a fingerprint.")
    val deleteTitle = say("SS43_TITLE", "Delete the starter key")
    val deleteWaiting = say("SS43_WAITING", "The move is not confirmed yet. The key is deleted once it has one confirmation.")
    val deleteButton = say("SS43_DELETE_BUTTON", "Delete the key")
    val lateHeading = say("SS43_LATE_PAYMENT_HEADING", "Paid to the starter wallet")
    val lateWhat = say("SS43_LATE_PAYMENT_WHAT", "A payment came to the starter wallet after its money was moved.")
    val lateNext = say("SS43_LATE_PAYMENT_NEXT", "Move it to the new wallet. If this phone has no starter key, restore it from the card of starter words first.")
    val deleteUnknown = say("SS43_CONFIRMATIONS_UNKNOWN", "The phone does not know yet whether the move is confirmed.")

    /** Words that are still to be written: the placeholder stands in the heading and both sentences. */
    fun said(placeholder: String, tone: Int = Tones.INFO, who: Int = Who.OWNER): SayWords =
        SayWords(tone, who, placeholder, placeholder, placeholder, null, null, null, null)

    /** Words of three placeholders, for a state whose heading, sentence and next step are each one. */
    fun said(heading: String, what: String, next: String, tone: Int = Tones.INFO, who: Int = Who.OWNER): SayWords =
        SayWords(tone, who, heading, what, next, null, null, null, null)

    /** Whether these words are a placeholder (OA-114). */
    fun isPlaceholder(words: SayWords): Boolean = words.heading.startsWith("WORDS_NEEDED_")

    val noLock: SayWords get() = said(noLockHeading, noLockWhat, noLockNext, Tones.STOP)
    val notSaved: SayWords get() = said(notSavedHeading, notSavedWhat, notSavedNext, Tones.STOP)
    val sealed: SayWords get() = said(sealedHeading, sealedWhat, sealedNext, Tones.GOOD)
    val keyGoneFallback: SayWords get() = said(keyGoneFallbackHeading, keyGoneFallbackWhat, keyGoneFallbackNext, Tones.STOP)
    val restored: SayWords get() = said(restoredHeading, restoredWhat, restoredNext, Tones.GOOD)
    val late: SayWords get() = said(lateHeading, lateWhat, lateNext, Tones.INFO)
    val signed: SayWords get() = said(signedHeading, signedWhat, signedNext, Tones.INFO)

    /** Every key and its text, in order, for docs/screens/owner-app.md. */
    fun all(): List<Pair<String, String>> = registry.toList()
}
