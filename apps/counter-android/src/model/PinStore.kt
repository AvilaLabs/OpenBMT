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
 * The manager's PIN (docs/COUNTER-RULES.md CR-84 to CR-86): 6 to 12 digits,
 * kept as PBKDF2-HMAC-SHA256 with 600,000 iterations and a random 16-byte
 * salt in one file of the app's own folder, compared in constant time. A
 * wrong try holds the PIN for 1 s, then 2 s, 4 s and so on, up to 5 minutes;
 * the count and the time it is held until are in the file, so closing the app
 * does not clear them. A try is counted and stored before it is compared.
 *
 * The file: `"OBCP" | 1 | iterations u32 | salt 16 | hash 32 | wrong tries
 * u32 | held until u64`, little-endian, 69 bytes. Strict: any other length,
 * magic or version is refused.
 */
class PinStore(
    private val dir: File,
    private val clock: () -> Long,
    private val random: SecureRandom = SecureRandom(),
    private val hasher: PinHasher = Pbkdf2Hasher,
    private val iterations: Int = ITERATIONS,
    name: String = "pin.obcp",
) {
    private val file = File(dir, name)

    /** The file's content in memory. */
    private class Record(val iterations: Int, val salt: ByteArray, val hash: ByteArray, val wrong: Long, val heldUntil: Long)

    fun isSet(): Boolean = file.exists()

    /** Stores a new PIN. The PIN must be [validPin]; any earlier PIN and its count are replaced. */
    fun set(pin: String) {
        require(validPin(pin)) { "a PIN is $MIN_DIGITS to $MAX_DIGITS digits" }
        val salt = ByteArray(SALT_LEN).also { random.nextBytes(it) }
        val chars = pin.toCharArray()
        try {
            write(Record(iterations, salt, hasher.hash(chars, salt, iterations), 0, 0))
        } finally {
            chars.fill('0')
        }
    }

    /**
     * Checks a PIN. While an earlier wrong try holds the PIN, the answer is
     * [PinResult.Locked] and the PIN is not compared (the count does not
     * grow). Otherwise the try is counted and stored first; a store that
     * fails throws and nothing is compared.
     */
    fun check(pin: String): PinResult {
        val rec = read() ?: throw IOException("no PIN is set")
        val now = clock()
        if (now < rec.heldUntil) return PinResult.Locked(rec.heldUntil - now)
        val wrong = rec.wrong + 1
        val wait = delayFor(wrong)
        write(Record(rec.iterations, rec.salt, rec.hash, wrong, now + wait))
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
            write(Record(rec.iterations, rec.salt, rec.hash, 0, 0))
        } catch (_: IOException) {
        }
        return PinResult.Ok
    }

    /** The seconds the PIN is held for, or 0. */
    fun heldFor(): Long {
        val rec = read() ?: return 0
        return maxOf(0, rec.heldUntil - clock())
    }

    private fun read(): Record? {
        if (!file.exists()) return null
        val b = file.readBytes()
        if (b.size != SIZE) throw PinFormatException("the PIN file has a length that is not $SIZE")
        if (!b.copyOfRange(0, 4).contentEquals(MAGIC)) throw PinFormatException("the PIN file has another magic")
        if (b[4].toInt() != VERSION) throw PinFormatException("the PIN file has another version")
        val buf = ByteBuffer.wrap(b).order(ByteOrder.LITTLE_ENDIAN)
        buf.position(5)
        val iter = buf.int
        if (iter < MIN_ITERATIONS || iter > MAX_ITERATIONS) throw PinFormatException("the PIN file has an iteration count out of range")
        val salt = ByteArray(SALT_LEN).also { buf.get(it) }
        val hash = ByteArray(HASH_LEN).also { buf.get(it) }
        val wrong = buf.int.toLong() and 0xffffffffL
        val heldUntil = buf.long
        if (heldUntil < 0) throw PinFormatException("the PIN file has a time out of range")
        return Record(iter, salt, hash, wrong, heldUntil)
    }

    private fun write(r: Record) {
        val buf = ByteBuffer.allocate(SIZE).order(ByteOrder.LITTLE_ENDIAN)
        buf.put(MAGIC).put(VERSION.toByte()).putInt(r.iterations).put(r.salt).put(r.hash)
            .putInt(minOf(r.wrong, 0xffffffffL).toInt()).putLong(r.heldUntil)
        DurableFile.write(dir, file, buf.array())
    }

    companion object {
        val MAGIC = byteArrayOf('O'.code.toByte(), 'B'.code.toByte(), 'C'.code.toByte(), 'P'.code.toByte())
        const val VERSION = 1
        const val ITERATIONS = 600_000
        const val SALT_LEN = 16
        const val HASH_LEN = 32
        const val MIN_DIGITS = 6
        const val MAX_DIGITS = 12
        const val MAX_DELAY_SECS = 300L
        const val SIZE = 4 + 1 + 4 + SALT_LEN + HASH_LEN + 4 + 8

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
