package org.openbmt.owner.app

import org.openbmt.app.SayWords
import org.openbmt.app.Tones
import org.openbmt.app.Who

/**
 * Every text of the screen where the wallet signs a Redirect change of a shop with its own wallet
 * (`docs/APPROVAL-RULES.md` AP-14, AP-15, `docs/OWNER-APP-RULES.md` OA-127), listed in screen order. `{address}` is
 * the first receive address of the pinned wallet, which the app puts in. The list is written to
 * docs/screens/owner-app.md by [AppWords.screensMarkdown]. No Android class is used.
 */
object WalletSigWords {
    private val registry = ArrayList<Pair<String, String>>()

    /** Written words, listed under the key of their rule. */
    private fun say(key: String, text: String): String {
        registry.add(key to text)
        return text
    }

    val heading = say("OA_WALLETSIG_HEADING", "Sign with your wallet")
    val what = say("OA_WALLETSIG_WHAT", "This change decides where Lightning payments go, so your own wallet must approve it.")
    val next = say("OA_WALLETSIG_NEXT", "In your wallet, sign the message below with the address shown, then paste or scan the signature.")
    val addressLabel = say("OA_WALLETSIG_ADDRESS_LABEL", "Address that signs")
    val messageLabel = say("OA_WALLETSIG_MESSAGE_LABEL", "Message to sign")
    val fieldLabel = say("OA_WALLETSIG_FIELD_LABEL", "Signature")
    val send = say("OA_WALLETSIG_SEND", "Send change")
    val scan = say("OA_WALLETSIG_SCAN", "Scan signature")
    val wrongHeading = say("OA_WALLETSIG_WRONG_HEADING", "Signature not accepted")
    val wrongWhat = say("OA_WALLETSIG_WRONG_WHAT", "This signature was not made by {address}, or is not of a kind this phone reads.")
    val wrongNext = say("OA_WALLETSIG_WRONG_NEXT", "Sign the message again with that address and paste the whole signature.")
    val starterHeading = say("OA_WALLETSIG_STARTER_HEADING", "Move to your own wallet")
    val starterWhat = say("OA_WALLETSIG_STARTER_WHAT", "This change decides where Lightning payments go. A starter wallet's key is on this phone, so it cannot approve it.")
    val starterNext = say("OA_WALLETSIG_STARTER_NEXT", "Move the shop to your own wallet first, then make this change.")
    val saved = say("OA_WALLETSIG_SAVED", "Saved. The counters take the change when they next reach this phone.")

    // The words of the screen for the 2-of-3 wallet (AP-16 to AP-18). The heading, the message label, the signature
    // field, "Copy", "Paste", "Scan signature", "Send change" and the refusal words are the ones above.
    /** Two of the wallet's three keys sign the message, each on a card of its own. */
    val cosignerNext = say("OA_WALLETSIG_COSIGNER_NEXT", "Two of the wallet's three keys must sign the message below. Each key has its own card: sign with the address on that card, then paste or scan the signature there.")
    /** The label above one cosigner's fingerprint on its card. */
    val cosignerLabel = say("OA_WALLETSIG_COSIGNER_LABEL", "Fingerprint of this key")
    /** The button on a card that takes the pasted or scanned signature and checks it. */
    val cosignerAdd = say("OA_WALLETSIG_COSIGNER_ADD", "Check signature")
    /** The mark on a card whose signature was taken and checked. */
    val cosignerSigned = say("OA_WALLETSIG_COSIGNER_SIGNED", "Signed")
    /** Beside the disabled "Send change" while fewer than two keys have signed. */
    val cosignerNeedTwo = say("OA_WALLETSIG_COSIGNER_NEED_TWO", "Two keys must sign before the change can be sent.")

    // The names of the two buttons that copy and paste: the words of the decided design name the buttons "Copy" and
    // "Paste" and give no other text for them.
    const val copy = "Copy"
    const val paste = "Paste"

    /**
     * What the code holds (OA-127): `signmessage {path} ascii:{message}`, the form in which signers that read codes take
     * a message to sign, with the derivation [path] of the address that signs; the message alone when there is no path.
     */
    fun qrText(message: String, path: String?): String = if (path == null) message else "signmessage $path ascii:$message"

    /** The words of a signature that was not accepted, with the address that had to sign. */
    fun wrong(address: String): SayWords = SayWords(
        Tones.STOP, Who.OWNER, wrongHeading, wrongWhat.replace("{address}", address), wrongNext, "AP-10", null, null, null,
    )

    /** The words of a Redirect change on a starter wallet (AP-15). */
    fun starter(): SayWords = SayWords(
        Tones.STOP, Who.OWNER, starterHeading, starterWhat, starterNext, "AP-15", null, null, null,
    )

    /** Every written text, with its key, for the list of the app's words. */
    fun all(): List<Pair<String, String>> = registry.toList()
}
