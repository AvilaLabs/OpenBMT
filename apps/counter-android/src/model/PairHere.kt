package org.openbmt.counter.app

import java.io.File
import java.io.IOException
import org.openbmt.app.DurableFile

/**
 * The words of pairing with the owner app on the same phone (`docs/COUNTER-RULES.md` CR-277 to CR-282). The list is
 * written to docs/screens/counter-app.md by [AppWords.screensMarkdown]. No Android class is used.
 */
object PairHereWords {
    const val TITLE = "Pair with the owner app"
    const val TEXT = "The owner app on this phone asks to make this app its counter. Payments will go to the shop's wallet."
    const val PAIR = "Pair"
    const val CANCEL = "Cancel"
    const val ALREADY = "This counter is already paired. Nothing was changed."
    const val NOT_OWNER = "Only the OpenBMT Owner app on this phone can pair this counter here."
    const val PAIRED_TITLE = "Paired"
    const val PAIRED_TEXT = "This phone now takes payments for the shop. Open OpenBMT Counter when you sell."
    const val DONE = "Done"

    /** What the screen says of an offer that is not an owner phone's (the Key app's, or one that does not read as one). */
    const val NOT_TAKEN = "The owner app sent a code this pairing does not take. Nothing was changed."

    /** Every key and its text, for docs/screens/counter-app.md. */
    fun all(): List<Pair<String, String>> = listOf(
        "pairHereTitle" to TITLE, "pairHereText" to TEXT, "pairHerePair" to PAIR, "pairHereCancel" to CANCEL,
        "pairHereAlready" to ALREADY, "pairHereNotOwner" to NOT_OWNER, "pairHerePairedTitle" to PAIRED_TITLE,
        "pairHerePairedText" to PAIRED_TEXT, "pairHereDone" to DONE, "pairHereNotTaken" to NOT_TAKEN,
    )
}

/** That this counter was paired on the owner's phone, and whether the wallet it was paired with is the starter wallet (CR-281). */
class SamePhoneMark(val starter: Boolean)

/**
 * The counter's note that it was paired on this phone (CR-281), kept in a file of its own in the app's no-backup
 * folder beside the pairing record: `"OBMH" | 1 | starter u8 (0 or 1)`. It is read only to choose the line between sales.
 * Strict: any other length, magic, version or flag is no note. The flag is the offer's starter mark (CR-256), seen
 * when the offer was read, and is cleared when the counter switches to its own wallet (CR-261).
 */
object SamePhone {
    const val FILE = "samephone.obkh"
    private val MAGIC = byteArrayOf('O'.code.toByte(), 'B'.code.toByte(), 'M'.code.toByte(), 'H'.code.toByte())
    private const val VERSION = 1
    private const val SIZE = 6

    fun encode(starter: Boolean): ByteArray = MAGIC + byteArrayOf(VERSION.toByte(), if (starter) 1 else 0)

    /** The note in [bytes], or null when they are not exactly a note. */
    fun decode(bytes: ByteArray): SamePhoneMark? {
        if (bytes.size != SIZE) return null
        if (!bytes.copyOfRange(0, MAGIC.size).contentEquals(MAGIC)) return null
        if (bytes[4].toInt() != VERSION) return null
        return when (bytes[5].toInt()) {
            0 -> SamePhoneMark(false)
            1 -> SamePhoneMark(true)
            else -> null
        }
    }

    /** The note kept in [dir]; null when there is none or it does not read. */
    fun read(dir: File): SamePhoneMark? {
        val file = File(dir, FILE)
        if (!file.exists()) return null
        return try {
            decode(file.readBytes())
        } catch (_: IOException) {
            null
        }
    }

    /**
     * Whether the offer's bytes carry the starter mark: the frame `OBM-` and version 2 (CR-256), with the network, the
     * owner root, the floor and at least the flags byte after them. Anything shorter or framed otherwise is not starter.
     * The library has read the offer; this only picks the words of a line.
     */
    fun offerIsStarter(offer: ByteArray): Boolean =
        offer.size >= 4 + 1 + 1 + 32 + 4 + 1 &&
            offer[0] == 'O'.code.toByte() && offer[1] == 'B'.code.toByte() && offer[2] == 'M'.code.toByte() && offer[3] == '-'.code.toByte() &&
            offer[4].toInt() == 2

    /** Keeps the note, written so that a crash leaves the old one or the new one. Throws [IOException]. */
    fun write(dir: File, starter: Boolean) = DurableFile.write(dir, File(dir, FILE), encode(starter))

    /** The counter switched to its own wallet: the note stays, without the starter mark; when that cannot be written the note goes. */
    fun clearStarter(dir: File) {
        val mark = read(dir) ?: return
        if (!mark.starter) return
        try {
            write(dir, false)
        } catch (_: IOException) {
            delete(dir)
        }
    }

    /** Deletes the note and its temporary file; a note that stays is read as it is, so a failure is not an error here. */
    fun delete(dir: File) {
        File(dir, "$FILE.tmp").delete()
        File(dir, FILE).delete()
    }

    /** CR-281: the line between sales: the same-phone line only for a counter paired on this phone with the starter wallet. */
    fun line(mark: SamePhoneMark?): String =
        if (mark?.starter == true) AppWords.COUNTER_LINE_SAME_PHONE else AppWords.COUNTER_LINE
}

/**
 * What the counter holds between the two intents of one-phone pairing (CR-279, CR-280): that an offer was taken, and
 * whether it carried the starter mark. In the app's memory only, as the library's ceremony is: when the app is
 * stopped, both are gone.
 */
object PairHereHold {
    @Volatile
    private var starter: Boolean? = null

    fun taken(starter: Boolean) {
        this.starter = starter
    }

    /** The mark of the offer taken, once; null when there was none (the app was stopped, or none was taken). */
    fun take(): Boolean? = starter.also { starter = null }

    fun clear() {
        starter = null
    }
}
