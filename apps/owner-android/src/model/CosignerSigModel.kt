package org.openbmt.owner.app

import org.openbmt.owner.OwnCosigned
import org.openbmt.owner.OwnCosigner

/**
 * The state of the screen where two of the 2-of-3 wallet's cosigners sign a Redirect change (`docs/APPROVAL-RULES.md`
 * AP-16 to AP-18). The library checks each signature when it is taken and keeps the text of those held; this holds what
 * the screen needs from it: which cosigners have signed, and whether the change can be sent. No Android class is used.
 */
object CosignerSigModel {
    /** The fewest different cosigners that must have signed (AP-17). */
    const val NEEDED = 2

    /** What the screen holds between two signatures: the library's [held] text and the places that signed. */
    class State(val held: String = "", val signed: List<Int> = emptyList()) {
        /** The state after the library took a signature. */
        fun took(answer: OwnCosigned) = State(answer.held, answer.signed)
    }

    /** True once two different cosigners have signed, which is when Send is enabled. */
    fun ready(signed: List<Int>): Boolean = signed.toSet().size >= NEEDED

    /** True when this cosigner's signature was taken. */
    fun isSigned(cosigner: OwnCosigner, signed: List<Int>): Boolean = cosigner.place in signed

    /** The cosigners marked with whether they signed, in the order of the wallet's description, for the cards. */
    fun cards(cosigners: List<OwnCosigner>, signed: List<Int>): List<OwnCosigner> =
        cosigners.sortedBy { it.place }.map { OwnCosigner(it.place, it.fingerprint, it.address, it.path, it.qr, isSigned(it, signed)) }

    /** The line beside Send while it is disabled, or null when it can be sent. */
    fun sendHint(signed: List<Int>): String? = if (ready(signed)) null else WalletSigWords.cosignerNeedTwo
}
