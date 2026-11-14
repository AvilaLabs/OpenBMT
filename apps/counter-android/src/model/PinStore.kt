package org.openbmt.counter.app

import java.io.File
import java.io.IOException
import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.security.MessageDigest
import java.security.SecureRandom
import javax.crypto.SecretKeyFactory
import javax.crypto.spec.PBEKeySpec
import org.openbmt.app.DurableFile

/** What turns a PIN and a salt into the bytes that are stored. The platform's PBKDF2 on Android and on the JVM. */
fun interface PinHasher {
    fun hash(pin: CharArray, salt: ByteArray, iterations: Int): ByteArray
}

/** PBKDF2 with HMAC-SHA-256 from the platform's `SecretKeyFactory`: 256 bits out. */
object Pbkdf2Hasher : PinHasher {
    override fun hash(pin: CharArray, salt: ByteArray, iterations: Int): ByteArray {
        val spec = PBEKeySpec(pin, salt, iterations, PinStore.HASH_LEN * 8)
        try {
            return SecretKeyFactory.getInstance("PBKDF2WithHmacSHA256").generateSecret(spec).encoded
        } catch (e: java.security.GeneralSecurityException) {
            throw IOException("the platform has no PBKDF2 with HMAC-SHA-256", e)
        } finally {
            spec.clearPassword()
        }
    }
}

/** The answer to a PIN that was typed. */
sealed class PinResult {
    object Ok : PinResult()

    /** Wrong; the next try is allowed after [waitSecs] seconds. */
    class Wrong(val waitSecs: Long) : PinResult()

    /** Not looked at: an earlier wrong try still holds the PIN for [waitSecs] seconds. */
    class Locked(val waitSecs: Long) : PinResult()
}

/** The file did not have the layout of [PinStore]. */
class PinFormatException(message: String) : IOException(message)

/**
 * One PIN as it is kept: how it was hashed, the salt, the hash, the wrong tries in a row and where the hold of the last
 * wrong try began ([holdStart] and [holdBoot], see [PinHold]; the hold's length follows from the count).
 */
class PinRecord(
    val iterations: Int,
    val salt: ByteArray,
    val hash: ByteArray,
    val wrong: Long,
    val holdStart: Long,
    val holdBoot: Long,
)

/**
 * The PIN rules of CR-84 and CR-85 on one [PinRecord], for the manager's PIN
 * and for each cashier's (CR-141): the hash with a random salt, the count and
 * the hold of the wrong tries, the compare in constant time.
 */
object PinCheck {
    /** A new record for [pin]: a fresh salt, no wrong tries. */
    fun make(pin: String, random: SecureRandom, hasher: PinHasher, iterations: Int): PinRecord {
        val salt = ByteArray(PinStore.SALT_LEN).also { random.nextBytes(it) }
        val chars = pin.toCharArray()
        try {
            return PinRecord(iterations, salt, hasher.hash(chars, salt, iterations), 0, PinHold.NONE, 0)
        } finally {
            chars.fill('0')
        }
    }

    /**
     * Checks [pin] against [rec] at [now]. While an earlier wrong try holds the PIN, the answer is
     * [PinResult.Locked] and the PIN is not compared (the count does not grow). Otherwise the try is
     * counted and handed to [save] first; a [save] that fails throws and nothing is compared.
     */
    fun attempt(rec: PinRecord, pin: String, now: PinNow, hasher: PinHasher, save: (PinRecord) -> Unit): PinResult {
        val held = held(rec, now)
        if (held.remainingSecs > 0) {
            if (held.restart) save(stamped(rec, now))
            return PinResult.Locked(held.remainingSecs)
        }
        val wrong = rec.wrong + 1
        val wait = PinStore.delayFor(wrong)
        save(PinRecord(rec.iterations, rec.salt, rec.hash, wrong, now.elapsedMs, now.boot))
        val chars = pin.toCharArray()
        val same = try {
            MessageDigest.isEqual(hasher.hash(chars, rec.salt, rec.iterations), rec.hash)
        } finally {
            chars.fill('0')
        }
        if (!same) return PinResult.Wrong(wait)
        // A reset that cannot be written leaves the count as it is, which only
        // makes the next wrong try wait longer.
        try {
            save(PinRecord(rec.iterations, rec.salt, rec.hash, 0, PinHold.NONE, 0))
        } catch (_: IOException) {
        }
        return PinResult.Ok
    }

    /** The hold of [rec] looked at [now]. */
    fun held(rec: PinRecord, now: PinNow): HoldCheck =
        PinHold.check(rec.holdStart, rec.holdBoot, now, PinStore.delayFor(rec.wrong))

    /** [rec] with a hold that begins [now]: what is stored when the phone has started again since the hold began. */
    fun stamped(rec: PinRecord, now: PinNow): PinRecord =
        PinRecord(rec.iterations, rec.salt, rec.hash, rec.wrong, now.elapsedMs, now.boot)
}

/**
 * The manager's PIN (docs/COUNTER-RULES.md CR-84 to CR-86): 6 to 12 digits,
 * kept as PBKDF2-HMAC-SHA256 with 600,000 iterations and a random 16-byte
 * salt in one file of the app's own folder, compared in constant time. A
 * wrong try holds the PIN for 1 s, then 2 s, 4 s and so on, up to 5 minutes;
 * the count and where the hold began are in the file, so closing the app
 * does not clear them. The hold is measured by the time since the phone
 * started and the phone's boot count, never by the date ([PinHold]). A try is
 * counted and stored before it is compared.
 *
 * The file: `"OBCP" | 2 | iterations u32 | salt 16 | hash 32 | wrong tries
 * u32 | hold start u64 | hold boot u32`, little-endian, 73 bytes. Strict: any
 * other length, magic or version is refused, except the 69 bytes of version 1
 * (the date a hold ends in place of the last two fields), which is read as a
 * PIN with no hold and written as version 2 at the next change.
 */
class PinStore(
    private val dir: File,
    private val clock: () -> Long,
    private val random: SecureRandom = SecureRandom(),
    private val hasher: PinHasher = Pbkdf2Hasher,
    private val iterations: Int = ITERATIONS,
    name: String = "pin.obcp",
    private val boot: () -> Long = { 0L },
) {
    private val file = File(dir, name)

    private fun now() = PinNow(clock(), boot() and 0xffffffffL)

    fun isSet(): Boolean = file.exists()

    /** Stores a new PIN. The PIN must be [validPin]; any earlier PIN and its count are replaced. */
    fun set(pin: String) {
        require(validPin(pin)) { "a PIN is $MIN_DIGITS to $MAX_DIGITS digits" }
        write(PinCheck.make(pin, random, hasher, iterations))
    }

    /**
     * Checks a PIN. While an earlier wrong try holds the PIN, the answer is
     * [PinResult.Locked] and the PIN is not compared (the count does not
     * grow). Otherwise the try is counted and stored first; a store that
     * fails throws and nothing is compared.
     */
    fun check(pin: String): PinResult {
        val rec = read() ?: throw IOException("no PIN is set")
        return PinCheck.attempt(rec, pin, now(), hasher) { write(it) }
    }

    /** The seconds the PIN is held for, or 0. A hold that the phone's restart cut off begins again here. */
    fun heldFor(): Long {
        val rec = read() ?: return 0
        val held = PinCheck.held(rec, now())
        if (held.restart) {
            try {
                write(PinCheck.stamped(rec, now()))
            } catch (_: IOException) {
            }
        }
        return held.remainingSecs
    }

    private fun read(): PinRecord? {
        if (!file.exists()) return null
        val b = file.readBytes()
        // The file of version 1 held the date a hold ends, which says nothing of the time since the phone started: no hold is kept from it.
        val legacy = b.size == LEGACY_SIZE && b[4].toInt() == LEGACY_VERSION
        if (b.size != SIZE && !legacy) throw PinFormatException("the PIN file has a length that is not $SIZE")
        if (!b.copyOfRange(0, 4).contentEquals(MAGIC)) throw PinFormatException("the PIN file has another magic")
        if (b[4].toInt() != VERSION && !legacy) throw PinFormatException("the PIN file has another version")
        val buf = ByteBuffer.wrap(b).order(ByteOrder.LITTLE_ENDIAN)
        buf.position(5)
        val iter = buf.int
        if (iter < MIN_ITERATIONS || iter > MAX_ITERATIONS) throw PinFormatException("the PIN file has an iteration count out of range")
        val salt = ByteArray(SALT_LEN).also { buf.get(it) }
        val hash = ByteArray(HASH_LEN).also { buf.get(it) }
        val wrong = buf.int.toLong() and 0xffffffffL
        val holdStart = buf.long
        if (holdStart < 0) throw PinFormatException("the PIN file has a time out of range")
        if (legacy) return PinRecord(iter, salt, hash, wrong, PinHold.NONE, 0)
        val holdBoot = buf.int.toLong() and 0xffffffffL
        return PinRecord(iter, salt, hash, wrong, holdStart, holdBoot)
    }

    private fun write(r: PinRecord) {
        val buf = ByteBuffer.allocate(SIZE).order(ByteOrder.LITTLE_ENDIAN)
        buf.put(MAGIC).put(VERSION.toByte()).putInt(r.iterations).put(r.salt).put(r.hash)
            .putInt(minOf(r.wrong, 0xffffffffL).toInt()).putLong(r.holdStart).putInt(r.holdBoot.toInt())
        DurableFile.write(dir, file, buf.array())
    }

    companion object {
        val MAGIC = byteArrayOf('O'.code.toByte(), 'B'.code.toByte(), 'C'.code.toByte(), 'P'.code.toByte())
        const val VERSION = 2
        private const val LEGACY_VERSION = 1
        const val ITERATIONS = 600_000
        const val SALT_LEN = 16
        const val HASH_LEN = 32
        const val MIN_DIGITS = 6
        const val MAX_DIGITS = 12
        const val MAX_DELAY_SECS = 300L
        const val SIZE = 4 + 1 + 4 + SALT_LEN + HASH_LEN + 4 + 8 + 4
        private const val LEGACY_SIZE = SIZE - 4

        // The file is refused when its count is far from the one this build writes.
        private const val MIN_ITERATIONS = 1_000
        private const val MAX_ITERATIONS = 10_000_000

        /** 6 to 12 digits, nothing else. */
        fun validPin(pin: String): Boolean =
            pin.length in MIN_DIGITS..MAX_DIGITS && pin.all { it in '0'..'9' }

        /** The seconds held after the [wrong]th wrong try in a row: 1, 2, 4, up to 300. */
        fun delayFor(wrong: Long): Long {
            if (wrong <= 0) return 0
            val shift = minOf(wrong - 1, 20)
            return minOf(1L shl shift.toInt(), MAX_DELAY_SECS)
        }
    }
}
