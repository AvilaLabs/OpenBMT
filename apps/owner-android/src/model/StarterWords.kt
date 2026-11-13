package org.openbmt.owner.app

import org.openbmt.app.SayWords
import org.openbmt.app.Tones
import org.openbmt.app.Who

/**
 * Every text of the starter wallet's screens that the app writes itself (`docs/START-SIMPLE-RULES.md`). None is
 * written yet: each is a `WORDS_NEEDED_OA_SSnn_...` placeholder that shows its own name wherever the words would be
 * (the same pattern as `OwnPairWords`, OA-114), named for the rule of the screen. The words of the Rust core (a
 * refusal, the home line, the states of the limit, SS-43) are shown as they arrive; the ones the binding does not
 * carry yet are the `core...` placeholders below, which a call of the library replaces. The list is written to
 * docs/screens/owner-app.md by [AppWords.screensMarkdown]. No Android class is used.
 */
object StarterWords {
    private val registry = ArrayList<Pair<String, String>>()

    private fun need(name: String): String {
        registry.add(name.removePrefix("WORDS_NEEDED_OA_") to name)
        return name
    }

    // ---- the entry (SS-24, the Money page) ----
    val rowTitle = need("WORDS_NEEDED_OA_SS00_ROW_TITLE")
    val rowSub = need("WORDS_NEEDED_OA_SS00_ROW_SUB")
    val screenTitle = need("WORDS_NEEDED_OA_SS00_SCREEN_TITLE")
    val startButton = need("WORDS_NEEDED_OA_SS00_START_BUTTON")
    val backButton = need("WORDS_NEEDED_OA_SS00_BACK_BUTTON")
    val doneButton = need("WORDS_NEEDED_OA_SS00_DONE_BUTTON")
    val noLockHeading = need("WORDS_NEEDED_OA_SS24_NO_LOCK_HEADING")
    val noLockWhat = need("WORDS_NEEDED_OA_SS24_NO_LOCK_WHAT")
    val noLockNext = need("WORDS_NEEDED_OA_SS24_NO_LOCK_NEXT")

    // ---- the plain words before the words (SS-13) ----
    val plainTitle = need("WORDS_NEEDED_OA_SS13_TITLE")
    val plainPen = need("WORDS_NEEDED_OA_SS13_PEN")
    val plainNoPhoto = need("WORDS_NEEDED_OA_SS13_NO_PHOTO")
    val plainNobodyAsks = need("WORDS_NEEDED_OA_SS13_NOBODY_ASKS")
    val plainContinue = need("WORDS_NEEDED_OA_SS13_CONTINUE")

    // ---- the checks before the key and before a payment (SS-33, SS-34) ----
    val checksTitle = need("WORDS_NEEDED_OA_SS33_TITLE")
    val usbName = need("WORDS_NEEDED_OA_SS33_USB_NAME")
    val usbWhy = need("WORDS_NEEDED_OA_SS33_USB_WHY")
    val rootName = need("WORDS_NEEDED_OA_SS33_ROOT_NAME")
    val rootWhy = need("WORDS_NEEDED_OA_SS33_ROOT_WHY")
    val overlayName = need("WORDS_NEEDED_OA_SS33_OVERLAY_NAME")
    val overlayWhy = need("WORDS_NEEDED_OA_SS33_OVERLAY_WHY")
    val serviceName = need("WORDS_NEEDED_OA_SS33_SERVICE_NAME")
    val serviceWhy = need("WORDS_NEEDED_OA_SS33_SERVICE_WHY")
    val serviceNotToolName = need("WORDS_NEEDED_OA_SS33_SERVICE_NOT_TOOL_NAME")
    val serviceNotToolWhy = need("WORDS_NEEDED_OA_SS33_SERVICE_NOT_TOOL_WHY")
    val serviceNotToolRefused = need("WORDS_NEEDED_OA_SS33_SERVICE_NOT_TOOL_REFUSED")
    val patchName = need("WORDS_NEEDED_OA_SS34_PATCH_NAME")
    val patchWhy = need("WORDS_NEEDED_OA_SS34_PATCH_WHY")
    val patchUnknownName = need("WORDS_NEEDED_OA_SS34_PATCH_UNKNOWN_NAME")
    val patchUnknownWhy = need("WORDS_NEEDED_OA_SS34_PATCH_UNKNOWN_WHY")
    val overlayNote = need("WORDS_NEEDED_OA_SS33_OVERLAY_NOTE")
    val confirmButton = need("WORDS_NEEDED_OA_SS33_CONFIRM_BUTTON")
    val confirmAgain = need("WORDS_NEEDED_OA_SS33_CONFIRM_AGAIN")

    // ---- the limit and the date (SS-35) ----
    val limitTitle = need("WORDS_NEEDED_OA_SS35_TITLE")
    val limitLabel = need("WORDS_NEEDED_OA_SS35_LIMIT_LABEL")
    val priceLabel = need("WORDS_NEEDED_OA_SS35_PRICE_LABEL")
    val dateLabel = need("WORDS_NEEDED_OA_SS35_DATE_LABEL")
    val stopLabel = need("WORDS_NEEDED_OA_SS38_STOP_LABEL")
    val limitInvalid = need("WORDS_NEEDED_OA_SS35_LIMIT_INVALID")
    val dateInvalid = need("WORDS_NEEDED_OA_SS35_DATE_INVALID")
    val diceLabel = need("WORDS_NEEDED_OA_SS03_DICE_LABEL")
    val diceInvalid = need("WORDS_NEEDED_OA_SS03_DICE_INVALID")
    val limitContinue = need("WORDS_NEEDED_OA_SS35_CONTINUE")

    // ---- the words, shown and checked (SS-10, SS-11) ----
    val pageTitle = need("WORDS_NEEDED_OA_SS10_PAGE_TITLE")
    val pageNext = need("WORDS_NEEDED_OA_SS10_PAGE_NEXT")
    val pageWritten = need("WORDS_NEEDED_OA_SS10_PAGE_WRITTEN")
    val checkTitle = need("WORDS_NEEDED_OA_SS10_CHECK_TITLE")
    val checkAsk = need("WORDS_NEEDED_OA_SS10_CHECK_ASK")
    val checkTyped = need("WORDS_NEEDED_OA_SS11_TYPED")
    val checkAccepted = need("WORDS_NEEDED_OA_SS11_ACCEPTED")
    val checkNotAccepted = need("WORDS_NEEDED_OA_SS11_NOT_ACCEPTED")
    val keyBackspace = need("WORDS_NEEDED_OA_SS11_KEY_BACKSPACE")
    val keyEnter = need("WORDS_NEEDED_OA_SS11_KEY_ENTER")
    val checkWrong = need("WORDS_NEEDED_OA_SS10_CHECK_WRONG")
    val madeHeading = need("WORDS_NEEDED_OA_SS10_MADE_HEADING")

    // ---- keeping the key, the owner each time (SS-20, SS-21) ----
    val authTitle = need("WORDS_NEEDED_OA_SS21_AUTH_TITLE")
    val authSubtitle = need("WORDS_NEEDED_OA_SS21_AUTH_SUBTITLE")
    val authSeal = need("WORDS_NEEDED_OA_SS21_AUTH_SEAL")
    val authPay = need("WORDS_NEEDED_OA_SS21_AUTH_PAY")
    val authMove = need("WORDS_NEEDED_OA_SS21_AUTH_MOVE")
    val authFailed = need("WORDS_NEEDED_OA_SS21_AUTH_FAILED")
    val sealedHeading = need("WORDS_NEEDED_OA_SS20_SEALED_HEADING")
    val sealedWhat = need("WORDS_NEEDED_OA_SS20_SEALED_WHAT")
    val sealedNext = need("WORDS_NEEDED_OA_SS20_SEALED_NEXT")
    val notSavedHeading = need("WORDS_NEEDED_OA_SS20_NOT_SAVED_HEADING")
    val notSavedWhat = need("WORDS_NEEDED_OA_SS20_NOT_SAVED_WHAT")
    val notSavedNext = need("WORDS_NEEDED_OA_SS20_NOT_SAVED_NEXT")

    // ---- when the phone drops the key (SS-23) ----
    val keyGoneFallbackHeading = need("WORDS_NEEDED_OA_SS23_KEY_GONE_HEADING")
    val keyGoneFallbackWhat = need("WORDS_NEEDED_OA_SS23_KEY_GONE_WHAT")
    val keyGoneFallbackNext = need("WORDS_NEEDED_OA_SS23_KEY_GONE_NEXT")
    val restoreButton = need("WORDS_NEEDED_OA_SS23_RESTORE_BUTTON")
    val restoreTitle = need("WORDS_NEEDED_OA_SS23_RESTORE_TITLE")
    val restoreAsk = need("WORDS_NEEDED_OA_SS23_RESTORE_ASK")
    val restoreFinish = need("WORDS_NEEDED_OA_SS23_RESTORE_FINISH")
    val restoredHeading = need("WORDS_NEEDED_OA_SS23_RESTORED_HEADING")
    val restoredWhat = need("WORDS_NEEDED_OA_SS23_RESTORED_WHAT")
    val restoredNext = need("WORDS_NEEDED_OA_SS23_RESTORED_NEXT")

    // ---- the home line (SS-36) and the upgrade screen (SS-37) ----
    val lineHeld = need("WORDS_NEEDED_OA_SS36_HELD")
    val lineLimit = need("WORDS_NEEDED_OA_SS36_LIMIT")
    val lineMoveBy = need("WORDS_NEEDED_OA_SS36_MOVE_BY")
    val lineNotYetUntil = need("WORDS_NEEDED_OA_SS37_NOT_YET_UNTIL")
    val upgradeTitle = need("WORDS_NEEDED_OA_SS37_TITLE")
    val upgradeNow = need("WORDS_NEEDED_OA_SS37_UPGRADE_NOW")
    val notYet = need("WORDS_NEEDED_OA_SS37_NOT_YET")
    val notYetSentence = need("WORDS_NEEDED_OA_SS37_NOT_YET_SENTENCE")
    val notYetAsk = need("WORDS_NEEDED_OA_SS37_NOT_YET_ASK")
    val notYetConfirm = need("WORDS_NEEDED_OA_SS37_NOT_YET_CONFIRM")
    val notYetDiffers = need("WORDS_NEEDED_OA_SS37_NOT_YET_DIFFERS")
    val notYetGranted = need("WORDS_NEEDED_OA_SS37_NOT_YET_GRANTED")

    // ---- what the core says that the binding does not carry yet (SS-36, SS-37, SS-43) ----
    val coreLine = need("WORDS_NEEDED_OA_SS36_CORE_LINE")
    val coreAtLimit = need("WORDS_NEEDED_OA_SS37_CORE_AT_LIMIT")
    val corePastDate = need("WORDS_NEEDED_OA_SS37_CORE_PAST_DATE")
    val coreNotYet = need("WORDS_NEEDED_OA_SS37_CORE_NOT_YET")
    val coreHeld = need("WORDS_NEEDED_OA_SS37_CORE_HELD")
    val coreDeleted = need("WORDS_NEEDED_OA_SS43_CORE_DELETED")

    // ---- paying a refund (SS-30, SS-31) and the move (SS-42) ----
    val payTitle = need("WORDS_NEEDED_OA_SS30_TITLE")
    val payEmpty = need("WORDS_NEEDED_OA_SS30_EMPTY")
    val payFeeRate = need("WORDS_NEEDED_OA_SS31_FEE_RATE")
    val payAuthorise = need("WORDS_NEEDED_OA_SS31_AUTHORISE")
    val paySendButton = need("WORDS_NEEDED_OA_SS31_SEND_BUTTON")
    val payAddressLabel = need("WORDS_NEEDED_OA_SS31_ADDRESS_LABEL")
    val payAmountLabel = need("WORDS_NEEDED_OA_SS31_AMOUNT_LABEL")
    val payFeeLabel = need("WORDS_NEEDED_OA_SS31_FEE_LABEL")
    val payChangeLabel = need("WORDS_NEEDED_OA_SS31_CHANGE_LABEL")
    val noCoinViewHeading = need("WORDS_NEEDED_OA_SS30_NO_COIN_VIEW_HEADING")
    val noCoinViewWhat = need("WORDS_NEEDED_OA_SS30_NO_COIN_VIEW_WHAT")
    val noCoinViewNext = need("WORDS_NEEDED_OA_SS30_NO_COIN_VIEW_NEXT")
    val signedHeading = need("WORDS_NEEDED_OA_SS30_SIGNED_HEADING")
    val signedWhat = need("WORDS_NEEDED_OA_SS30_SIGNED_WHAT")
    val signedNext = need("WORDS_NEEDED_OA_SS30_SIGNED_NEXT")
    val moveTitle = need("WORDS_NEEDED_OA_SS42_TITLE")
    val moveWalletLabel = need("WORDS_NEEDED_OA_SS42_WALLET_LABEL")
    val moveIndexLabel = need("WORDS_NEEDED_OA_SS42_INDEX_LABEL")
    val moveEverything = need("WORDS_NEEDED_OA_SS42_EVERYTHING")
    val moveInvalid = need("WORDS_NEEDED_OA_SS42_INVALID")
    val moveCredentialOnly = need("WORDS_NEEDED_OA_SS42_CREDENTIAL_ONLY")
    val deleteTitle = need("WORDS_NEEDED_OA_SS43_TITLE")
    val deleteWaiting = need("WORDS_NEEDED_OA_SS43_WAITING")
    val deleteButton = need("WORDS_NEEDED_OA_SS43_DELETE_BUTTON")
    val deleteUnknown = need("WORDS_NEEDED_OA_SS43_CONFIRMATIONS_UNKNOWN")

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
    val noCoinView: SayWords get() = said(noCoinViewHeading, noCoinViewWhat, noCoinViewNext, Tones.STOP)
    val signed: SayWords get() = said(signedHeading, signedWhat, signedNext, Tones.INFO)

    /** Every key and its text, in order, for docs/screens/owner-app.md. */
    fun all(): List<Pair<String, String>> = registry.toList()
}
