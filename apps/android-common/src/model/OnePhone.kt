package org.openbmt.app

import java.security.MessageDigest

/**
 * What the owner app and the counter app agree on when they run on one phone (`docs/COUNTER-RULES.md` CR-277 to
 * CR-282, `docs/OWNER-APP-RULES.md` OA-124): the two package names, the one activity and the permission that guards
 * it, the two actions and the four extras. Nothing else passes between the apps. No Android class is used.
 */
object OnePhone {
    const val OWNER_PACKAGE = "org.openbmt.owner.app"
    const val COUNTER_PACKAGE = "org.openbmt.counter.app"

    /** The counter app's one activity for pairing on this phone, started by its explicit name. */
    const val PAIR_ACTIVITY = "org.openbmt.counter.app.PairHereActivity"

    /** The counter app's launcher activity, which "Open the counter" starts. */
    const val COUNTER_MAIN = "org.openbmt.counter.app.MainActivity"

    /** The signature-level permission both apps declare; the activity asks its caller for it. */
    const val PERMISSION = "org.openbmt.permission.PAIR_ON_THIS_PHONE"

    const val ACTION_OFFER = "org.openbmt.counter.PAIR_OFFER"
    const val ACTION_BUNDLE = "org.openbmt.counter.PAIR_BUNDLE"

    /** The owner phone's offer, in an intent of [ACTION_OFFER]. */
    const val EXTRA_OFFER = "offer"

    /** The counter's request, in the result of [ACTION_OFFER]. */
    const val EXTRA_REQUEST = "request"

    /** The owner phone's bundle, in an intent of [ACTION_BUNDLE]. */
    const val EXTRA_BUNDLE = "bundle"

    /** Whether the counter is paired, in the result of [ACTION_BUNDLE]: the only thing that result says. */
    const val EXTRA_PAIRED = "paired"

    /**
     * The SHA-256 of the one certificate an app is signed with, or null when it has none or more than one: an app
     * with several signers is never taken as the same publisher.
     */
    fun ownDigest(certs: List<ByteArray>): ByteArray? {
        if (certs.size != 1 || certs[0].isEmpty()) return null
        return MessageDigest.getInstance("SHA-256").digest(certs[0])
    }

    /** CR-277: the counter's activity acts only for the owner app's package. */
    fun callerAllowed(calling: String?): Boolean = calling == OWNER_PACKAGE

    /** What the owner app finds of the counter app on the phone (OA-124). */
    enum class Peer { Missing, OtherSigner, Same }

    /** [installed] says the package is there; [sameSigner] is the system's answer to `hasSigningCertificate` with [ownDigest]. */
    fun peer(installed: Boolean, sameSigner: Boolean): Peer = when {
        !installed -> Peer.Missing
        sameSigner -> Peer.Same
        else -> Peer.OtherSigner
    }
}
