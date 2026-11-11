package org.openbmt.key.app

import org.openbmt.key.InvitationView

/**
 * The "Pair a Box" screen's pure parts (docs/SIGNER-RULES.md SR-81): the invitation as six groups
 * of five characters, and the time left on it. The library makes and stores the invitation
 * (`makeInvitation`); this only refuses to draw text that is not one, so a wrong value is never
 * shown as a code for a Box to take.
 */
object BoxModel {
    const val GROUPS = 6
    const val GROUP_LEN = 5

    /** Crockford base32, upper case: no I, L, O or U. */
    private const val ALPHABET = "0123456789ABCDEFGHJKMNPQRSTVWXYZ"

    /** The groups of an invitation's [text], or null when it is not six groups of five characters. */
    fun groups(text: String): List<String>? {
        val parts = text.split('-')
        if (parts.size != GROUPS) return null
        if (!parts.all { it.length == GROUP_LEN && it.all { c -> c in ALPHABET } }) return null
        return parts
    }

    /** The groups on one line, spaced so that each stands apart; null when [text] is not an invitation. */
    fun spaced(text: String): String? = groups(text)?.joinToString("   ")

    /** The seconds left at [now], never below 0. */
    fun secsLeft(invitation: InvitationView, now: Long): Long = (invitation.expiresAt - now).coerceAtLeast(0)

    /** Whether the invitation has run out at [now]. */
    fun runOut(invitation: InvitationView, now: Long): Boolean = secsLeft(invitation, now) == 0L

    /** Minutes and seconds as `29:59`. */
    fun timeLeft(secs: Long): String {
        val s = secs.coerceAtLeast(0)
        return "${s / 60}:${(s % 60).toString().padStart(2, '0')}"
    }

    /** A key as groups of eight hex characters, to show a Box's key in full. */
    fun hexGroups(key: ByteArray): String =
        key.joinToString("") { "%02x".format(it.toInt() and 0xff) }.chunked(8).joinToString(" ")
}
