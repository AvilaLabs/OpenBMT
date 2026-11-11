package org.openbmt.key.app

/**
 * The offer of the "Pair a counter" screen (DESIGN section 2.9, step 1). The library makes it
 * (`pairOfferBytes`: the owner phone's root, this app's root, the wallet's ID and the network); this
 * only refuses to draw bytes that are not an offer, so a wrong value is never shown as a code for a
 * counter to read.
 */
object PairOfferModel {
    /** The first four bytes of an offer, and the version byte after them. */
    private val FRAME = byteArrayOf('O'.code.toByte(), 'B'.code.toByte(), 'M'.code.toByte(), 'O'.code.toByte(), 1)

    /** Magic, version, owner root, approver root, wallet ID and one byte for the network. */
    const val LENGTH = 5 + 32 + 32 + 32 + 1

    /** Whether [bytes] have the length and the frame of an offer. */
    fun looksLikeOffer(bytes: ByteArray): Boolean =
        bytes.size == LENGTH && FRAME.indices.all { bytes[it] == FRAME[it] }
}
