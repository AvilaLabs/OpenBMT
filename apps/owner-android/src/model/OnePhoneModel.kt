package org.openbmt.owner.app

import java.io.File
import java.io.IOException
import org.openbmt.app.DurableFile

/**
 * The owner app's note that a counter of the shop is on this phone (OA-125), kept in a file of its own in the app's
 * no-backup folder: `"OBMP" | 1 | counter id (32)`. Read strictly: any other length, magic or version is no note.
 * It names a counter; whether that counter is in force is asked of the settings (OA-126). No Android class is used.
 */
object OnePhoneFile {
    const val NAME = "samephone.obkp"
    private val MAGIC = byteArrayOf('O'.code.toByte(), 'B'.code.toByte(), 'M'.code.toByte(), 'P'.code.toByte())
    private const val VERSION = 1
    private const val ID_LEN = 32

    fun encode(counter: ByteArray): ByteArray {
        if (counter.size != ID_LEN) throw IOException("a counter's id is $ID_LEN bytes")
        return MAGIC + byteArrayOf(VERSION.toByte()) + counter
    }

    /** The counter's id in [bytes], or null when they are not exactly a note. */
    fun decode(bytes: ByteArray): ByteArray? {
        if (bytes.size != MAGIC.size + 1 + ID_LEN) return null
        if (!bytes.copyOfRange(0, MAGIC.size).contentEquals(MAGIC)) return null
        if (bytes[MAGIC.size].toInt() != VERSION) return null
        return bytes.copyOfRange(MAGIC.size + 1, bytes.size)
    }

    /** The counter named in [dir]'s note; null when there is none or it does not read. */
    fun read(dir: File): ByteArray? {
        val file = File(dir, NAME)
        if (!file.exists()) return null
        return try {
            decode(file.readBytes())
        } catch (_: IOException) {
            null
        }
    }

    /** Keeps the note, written so that a crash leaves the old one or the new one. Throws [IOException]. */
    fun write(dir: File, counter: ByteArray) = DurableFile.write(dir, File(dir, NAME), encode(counter))
}

/** What the owner app makes of the counter app's answers on one phone (OA-124 to OA-126), apart from the system. */
object OnePhoneModel {
    /** Android's `Activity.RESULT_OK`, so that this file needs no Android class. */
    const val RESULT_OK = -1

    /** What the counter app's answer to the offer comes to. */
    sealed class Answer {
        /** No answer, a cancel or a refusal: the ceremony ends with words and a new try starts from a new offer (OA-125). */
        object Ended : Answer()

        /** A request, to be judged as one read by the camera is (OA-109), or refused with the words of that judgment. */
        class Request(val result: SourceResult) : Answer()
    }

    /** The result of the pairing activity started with the offer: [request] is its one extra. */
    fun offerAnswer(resultCode: Int, request: ByteArray?): Answer =
        if (resultCode != RESULT_OK || request == null || request.isEmpty()) Answer.Ended else Answer.Request(OwnPairing.requestBytes(request))

    /** The result of the pairing activity started with the bundle: the counter is paired only when it says so (OA-125). */
    fun paired(resultCode: Int, pairedExtra: Boolean?): Boolean = resultCode == RESULT_OK && pairedExtra == true

    /** OA-126: the note names a counter that is in force when the settings list it. */
    fun inForce(noted: ByteArray?, inForce: List<ByteArray>): Boolean = noted != null && inForce.any { it.contentEquals(noted) }

    /** What to do with an answer of the counter app when it arrives (OA-62, OA-125). */
    enum class Arrival {
        /** Go on at once with the answer. */
        Resume,

        /** The app was locked, or relocked while the counter app was in front: keep the answer until the unlock. */
        Hold,

        /** The app was started again and has no pairing for the answer: say the counter app stopped. */
        Lost,

        /** As [Lost], but the app is locked: say so after the unlock. */
        HoldLost,
    }

    /**
     * [waiting]: the app noted that it was waiting for the answer; [haveDone]: the pairing it belongs to is in memory;
     * [tooLong]: the wait passed 60 seconds; [running]: the node runs, so the app is unlocked.
     */
    fun arrival(waiting: Boolean, haveDone: Boolean, tooLong: Boolean, running: Boolean): Arrival = when {
        haveDone -> if (waiting && !tooLong) Arrival.Resume else Arrival.Hold
        running -> Arrival.Lost
        else -> Arrival.HoldLost
    }
}

/** What goes on after the unlock: one answer, in memory only, taken once and dropped by a wipe or a wrong PIN (OA-62). */
class OneHold {
    private var held: (() -> Unit)? = null

    fun hold(then: () -> Unit) {
        held = then
    }

    fun take(): (() -> Unit)? = held.also { held = null }

    fun drop() {
        held = null
    }

    val isHeld: Boolean get() = held != null
}
