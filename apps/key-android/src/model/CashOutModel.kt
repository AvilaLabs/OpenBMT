package org.openbmt.key.app

import java.nio.ByteBuffer
import java.nio.charset.CharacterCodingException
import java.nio.charset.CodingErrorAction
import org.openbmt.app.Format
import org.openbmt.key.AllowlistView
import org.openbmt.key.Words

/** What reading a scanned deposit code gave: its text for the library, or the words of why not. */
sealed class DepositInput {
    class Text(val text: String) : DepositInput()

    class Refused(val words: Words) : DepositInput()
}

/** One cash-out address as the list shows it (SR-79). */
class CashOutRow(
    val label: String,
    /** The address in groups of four, or the script when it has no address form. */
    val address: String,
    /** The address as the library takes it back, for "Remove"; null when the entry has none. */
    val removable: String?,
    val enrolled: String,
    /** "active", or "active in 3h 5m". */
    val status: String,
)

/**
 * The pure parts of the "Cash-out addresses" screens (docs/SIGNER-RULES.md SR-78, SR-79): reading the
 * code the camera scanned, the address in groups of four, and the time left before an entry counts. The
 * address itself is never typed or pasted: it only comes from the camera. No Android class is used.
 */
object CashOut {
    /**
     * The text of a scanned code. A plain text code goes on (a bare address or a `bitcoin:` URI; the library
     * reads it and decides); a `ur:` code and bytes that are not text do not.
     */
    fun readCode(bytes: ByteArray): DepositInput {
        val text = try {
            Charsets.UTF_8.newDecoder()
                .onMalformedInput(CodingErrorAction.REPORT)
                .onUnmappableCharacter(CodingErrorAction.REPORT)
                .decode(ByteBuffer.wrap(bytes))
                .toString()
        } catch (_: CharacterCodingException) {
            return DepositInput.Refused(AppWords.notADepositCode)
        }
        return if (text.trimStart().startsWith("ur:", ignoreCase = true)) {
            DepositInput.Refused(AppWords.notADepositCode)
        } else {
            DepositInput.Text(text.trim())
        }
    }

    /** "bc1q xyzw ...": the address in groups of four, as the person compares it with the exchange's screen. */
    fun groupsOfFour(address: String): String = address.chunked(4).joinToString(" ")

    /** SR-79: "active", or "active in 3h 5m", from the current time of SR-31 ([now], in seconds). */
    fun status(activeAt: Long?, now: Long): String {
        if (activeAt == null) return "not active"
        if (activeAt <= now) return "active"
        val left = activeAt - now
        var hours = left / 3600
        var minutes = (left % 3600 + 59) / 60
        if (minutes == 60L) {
            hours += 1
            minutes = 0
        }
        return "active in ${hours}h ${minutes}m"
    }

    /** SR-79: the row of one entry. [now] is the time the rules use now (`effectiveTimeNow`). */
    fun row(e: AllowlistView, now: Long): CashOutRow = CashOutRow(
        label = Format.cleanForeign(e.label),
        address = e.address?.let { groupsOfFour(it) } ?: e.scriptHex,
        removable = e.address,
        enrolled = Format.localTime(e.enrolledAt),
        status = status(e.activeAt, now),
    )
}
