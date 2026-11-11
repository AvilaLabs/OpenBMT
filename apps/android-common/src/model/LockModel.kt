package org.openbmt.app

import java.io.File
import java.io.IOException
import java.nio.ByteBuffer
import java.nio.ByteOrder

/** The count of wrong PINs and the time of the last one, in seconds since 1970 (SR-75, OA-63). */
data class TriesRecord(val wrong: Int, val lastWrongAt: Long)

/**
 * The pure parts of the PIN lock around the library, shared by the Key app and the owner app
 * (docs/SIGNER-RULES.md SR-75 to SR-77, docs/OWNER-APP-RULES.md OA-62 to OA-64): how many wrong tries are
 * allowed, how long the app makes the person wait, and when the app locks itself again. The library checks
 * the PIN; the counting belongs to the app. No Android class is used, so the JVM test runs it.
 */
object Tries {
    /** The 10th wrong try wipes the key from this phone (SR-76). */
    const val WIPE_AT = 10

    /** From the 5th wrong try the screen says how many tries are left (SR-75). */
    const val WARN_FROM = 5

    /** The wait before the next try, in seconds, by the number of wrong tries so far (SR-75). */
    private val WAITS = longArrayOf(0, 0, 0, 0, 0, 30, 60, 300, 900, 3600)

    fun waitSeconds(wrong: Int): Long = if (wrong in WAITS.indices) WAITS[wrong] else 0

    fun triesLeft(wrong: Int): Int = (WIPE_AT - wrong).coerceAtLeast(0)

    fun showsTriesLeft(wrong: Int): Boolean = wrong >= WARN_FROM

    /** What the app does with the record before a try: wipe, wait, or let the try go. */
    class Check(
        /** The record to go on with; it is stored when it is not the one that was read. */
        val record: TriesRecord,
        val waitLeft: Long,
        val wipe: Boolean,
    )

    /**
     * SR-75, SR-76: [rec] as it is at [now]. A record at the wipe count wipes. A clock behind the time of the
     * last wrong try restarts the wait from [now], and the record with that time is the one to store.
     */
    fun check(rec: TriesRecord, now: Long): Check {
        if (rec.wrong >= WIPE_AT) return Check(rec, 0, true)
        val r = if (rec.wrong > 0 && now < rec.lastWrongAt) TriesRecord(rec.wrong, now) else rec
        val waited = if (r.wrong > 0) now - r.lastWrongAt else 0
        return Check(r, (waitSeconds(r.wrong) - waited).coerceAtLeast(0), false)
    }

    /** SR-75: what is written before a try: one more wrong try, now. A right PIN then writes [afterRight]. */
    fun before(rec: TriesRecord, now: Long) = TriesRecord(rec.wrong + 1, now)

    fun afterRight() = TriesRecord(0, 0)

    /** "30 s", "4 min 20 s", "1 h 0 min": the wait left, for the person to read. */
    fun waitText(seconds: Long): String {
        val s = seconds.coerceAtLeast(0)
        return when {
            s >= 3600 -> "${s / 3600} h ${s % 3600 / 60} min"
            s >= 60 -> "${s / 60} min ${s % 60} s"
            else -> "$s s"
        }
    }

    /** SR-75: how many tries are left, as the screen says it. */
    fun triesLeftText(wrong: Int): String {
        val n = triesLeft(wrong)
        return if (n == 1) "1 try left before this key is wiped." else "$n tries left before this key is wiped."
    }
}

/**
 * SR-77: the app locks when it has been in the background for more than 60 seconds. The times are those of
 * a monotonic clock (milliseconds since the phone started), which a person cannot set.
 */
object Relock {
    const val AFTER_MS = 60_000L

    /** True when the app was paused at [pausedAt] and is resumed at [now] after more than 60 seconds. */
    fun needed(pausedAt: Long?, now: Long): Boolean = pausedAt == null || now < pausedAt || now - pausedAt > AFTER_MS
}

/** What the app does when a system screen it waited on gives its result back, or the app is resumed. */
enum class WaitOutcome { Continue, Lock }

/**
 * SR-77: the 60 seconds include the time spent in the system's file picker or permission dialog. The app
 * notes the monotonic time when it hands over to the system ([waitStart]) and, when the result comes back
 * (a file picked, the picker cancelled, the permission answered) or the app is resumed, asks here. A
 * result that comes back after more than 60 seconds is dropped, whatever it was, and the app locks; no
 * wait noted locks too (it fails closed). The decision does not depend on what the result was.
 */
object ExternalWait {
    fun decide(waitStart: Long?, now: Long): WaitOutcome =
        if (Relock.needed(waitStart, now)) WaitOutcome.Lock else WaitOutcome.Continue
}

/**
 * The file of the count of wrong tries (SR-75): `"OBKT" | 1 | wrong u32 LE | last wrong time u64 LE`, in the
 * app's no-backup folder, written through [DurableFile]. Strict: any other length or version is refused.
 */
class TriesFile(private val dir: File, val name: String = NAME) {
    private val file = File(dir, name)

    /**
     * The record. No file is a record of no wrong tries. A file that cannot be read counts as 9 wrong tries
     * just now, so that the one wait that is longest applies and one try is left (it fails closed).
     */
    fun read(now: Long): TriesRecord {
        if (!file.exists()) return TriesRecord(0, 0)
        return try {
            decode(file.readBytes())
        } catch (_: IOException) {
            TriesRecord(Tries.WIPE_AT - 1, now)
        }
    }

    /** Throws [IOException] when the record could not be made durable: the try is then not made. */
    fun write(rec: TriesRecord) = DurableFile.write(dir, file, encode(rec))

    fun exists(): Boolean = file.exists()

    /** Deletes the file and its temporary file; throws [IOException] when one is still there. */
    fun delete() {
        deleteOrThrow(File(dir, "$name.tmp"))
        deleteOrThrow(file)
    }

    companion object {
        const val NAME = "tries.obkt"
        private const val SIZE = 4 + 1 + 4 + 8
        private val MAGIC = byteArrayOf('O'.code.toByte(), 'B'.code.toByte(), 'K'.code.toByte(), 'T'.code.toByte())

        fun encode(rec: TriesRecord): ByteArray {
            if (rec.wrong < 0 || rec.lastWrongAt < 0) throw IOException("a record with a negative number")
            val out = ByteBuffer.allocate(SIZE).order(ByteOrder.LITTLE_ENDIAN)
            out.put(MAGIC).put(1).putInt(rec.wrong).putLong(rec.lastWrongAt)
            return out.array()
        }

        fun decode(bytes: ByteArray): TriesRecord {
            if (bytes.size != SIZE) throw IOException("the tries file has a bad length")
            if (!bytes.copyOfRange(0, 4).contentEquals(MAGIC) || bytes[4].toInt() != 1) {
                throw IOException("the tries file is not of this version")
            }
            val b = ByteBuffer.wrap(bytes, 5, 12).order(ByteOrder.LITTLE_ENDIAN)
            val wrong = b.int
            val at = b.long
            if (wrong < 0 || at < 0) throw IOException("the tries file holds a negative number")
            return TriesRecord(wrong, at)
        }
    }
}

/** A file that is not there afterwards, or an [IOException]. */
internal fun deleteOrThrow(file: File) {
    if (file.exists() && !file.delete() && file.exists()) throw IOException("${file.name} could not be deleted")
}

/**
 * SR-76, OA-64: the wipe. The sealed files of the app ([files], in this order, each with its temporary file)
 * go first, then the Keystore key ([deleteKey]), then the file of tries, so that a wipe that stops half way
 * is finished at the next start (the tries file still says 10). Any failure throws [IOException] and the
 * later steps are not taken. Which files an app lists is the app's own rule.
 */
object WipeOrder {
    fun run(dir: File, files: List<String>, tries: TriesFile, deleteKey: () -> Unit) {
        for (name in files) {
            deleteOrThrow(File(dir, "$name.tmp"))
            deleteOrThrow(File(dir, name))
        }
        try {
            deleteKey()
        } catch (e: IOException) {
            throw e
        } catch (e: java.security.GeneralSecurityException) {
            throw IOException("the key of the saved state could not be deleted", e)
        }
        tries.delete()
    }
}
