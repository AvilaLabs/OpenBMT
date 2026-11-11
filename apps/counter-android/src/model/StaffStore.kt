package org.openbmt.counter.app

import java.io.File
import java.io.IOException
import java.security.SecureRandom
import java.util.Locale
import org.openbmt.app.DurableFile
import org.openbmt.app.SayWords

/** The file of cashiers did not have the form [StaffStore] writes (CR-140). */
class StaffFormatException(message: String) : IOException(message)

/** A cashier as it is kept: the name and the PIN. */
class Cashier(val name: String, val pin: PinRecord)

/**
 * The list of cashiers (docs/COUNTER-RULES.md CR-140, CR-141): at most 30, in
 * one text file of the app's `noBackupFilesDir`, written through [DurableFile]
 * (a new file, a sync, a rename, a sync of the folder; CR-40). The first line
 * is `openbmt-cashiers 1`, then one line for each cashier:
 * `iterations salt hash wrong heldUntil name`, the numbers in canonical decimal,
 * the salt (16 bytes) and the hash (32 bytes) in lower-case hex, and the name
 * the rest of the line. The PIN is kept as the manager's is (PBKDF2-HMAC-SHA256,
 * 600,000 iterations, a random salt; [PinCheck]) and each cashier has a count of
 * wrong tries and a hold of their own (CR-85). The file is read whole and
 * refused whole when any line is not exactly so, when two names are the same
 * ignoring case, when a name is the manager's or when there are more than 30.
 * A missing file is an empty list. The file is not backed up and does not move
 * to another phone (`res/xml`).
 */
class StaffStore(
    private val dir: File,
    private val clock: () -> Long,
    private val manager: () -> String,
    private val random: SecureRandom = SecureRandom(),
    private val hasher: PinHasher = Pbkdf2Hasher,
    private val iterations: Int = PinStore.ITERATIONS,
    name: String = FILE,
) {
    private val file = File(dir, name)

    /** The names, in the order they were added. */
    @Synchronized
    fun names(): List<String> = read().map { it.name }

    /** Adds a cashier. The name and the PIN must pass [validName] and [validPin], and the name must be free. */
    @Synchronized
    fun add(name: String, pin: String) {
        require(validName(name)) { "a cashier's name is 1 to $MAX_NAME characters" }
        require(validPin(pin)) { "a cashier's PIN is $MIN_DIGITS to $MAX_DIGITS digits" }
        val list = read()
        require(list.size < MAX_CASHIERS) { "there are at most $MAX_CASHIERS cashiers" }
        require(!taken(name, list.map { it.name }, manager())) { "that name is used" }
        write(list + Cashier(name, PinCheck.make(pin, random, hasher, iterations)))
    }

    /** Removes a cashier; the others and their counts are as they were. */
    @Synchronized
    fun remove(name: String) {
        val list = read()
        if (list.none { it.name == name }) throw IOException("no such cashier")
        write(list.filter { it.name != name })
    }

    /** A new PIN for a cashier: a fresh salt, and the count and the hold start over (CR-142). */
    @Synchronized
    fun setPin(name: String, pin: String) {
        require(validPin(pin)) { "a cashier's PIN is $MIN_DIGITS to $MAX_DIGITS digits" }
        val list = read()
        if (list.none { it.name == name }) throw IOException("no such cashier")
        val fresh = PinCheck.make(pin, random, hasher, iterations)
        write(list.map { if (it.name == name) Cashier(name, fresh) else it })
    }

    /** CR-85 for one cashier: the answer to a PIN that was typed, counted and stored before it is compared. */
    @Synchronized
    fun check(name: String, pin: String): PinResult {
        val rec = read().firstOrNull { it.name == name } ?: throw IOException("no such cashier")
        return PinCheck.attempt(rec.pin, pin, clock(), hasher) { changed -> save(name, changed) }
    }

    /** The seconds this cashier's PIN is held for, or 0. */
    @Synchronized
    fun heldFor(name: String): Long {
        val rec = read().firstOrNull { it.name == name } ?: return 0
        return maxOf(0, rec.pin.heldUntil - clock())
    }

    private fun save(name: String, changed: PinRecord) {
        write(read().map { if (it.name == name) Cashier(name, changed) else it })
    }

    private fun read(): List<Cashier> {
        if (!file.exists()) return emptyList()
        if (file.length() > MAX_FILE) throw StaffFormatException("the cashier file is longer than $MAX_FILE bytes")
        val text = try {
            Charsets.UTF_8.newDecoder().decode(java.nio.ByteBuffer.wrap(file.readBytes())).toString()
        } catch (_: java.nio.charset.CharacterCodingException) {
            throw StaffFormatException("the cashier file is not UTF-8")
        }
        if (!text.endsWith("\n")) throw StaffFormatException("the cashier file does not end with a line end")
        val lines = text.dropLast(1).split('\n')
        if (lines[0] != HEADER) throw StaffFormatException("the cashier file has another version line")
        if (lines.size - 1 > MAX_CASHIERS) throw StaffFormatException("the cashier file has more than $MAX_CASHIERS cashiers")
        val out = ArrayList<Cashier>()
        for ((i, line) in lines.drop(1).withIndex()) {
            val c = parse(line, i + 2)
            if (taken(c.name, out.map { it.name }, manager())) throw StaffFormatException("line ${i + 2} repeats a name or has the manager's")
            out.add(c)
        }
        return out
    }

    private fun parse(line: String, number: Int): Cashier {
        val parts = line.split(' ', limit = 6)
        if (parts.size != 6) throw StaffFormatException("line $number does not have six fields")
        val iter = canonical(parts[0], number)
        if (iter < MIN_ITERATIONS || iter > MAX_ITERATIONS) throw StaffFormatException("line $number has an iteration count out of range")
        val salt = hex(parts[1], PinStore.SALT_LEN, number)
        val hash = hex(parts[2], PinStore.HASH_LEN, number)
        val wrong = canonical(parts[3], number)
        if (wrong > 0xffffffffL) throw StaffFormatException("line $number has a count out of range")
        val held = canonical(parts[4], number)
        if (!validName(parts[5])) throw StaffFormatException("line $number has a name that is not a name")
        return Cashier(parts[5], PinRecord(iter.toInt(), salt, hash, wrong, held))
    }

    private fun canonical(text: String, number: Int): Long {
        if (!Regex("0|[1-9][0-9]{0,18}").matches(text)) throw StaffFormatException("line $number has a number that is not canonical")
        return text.toLongOrNull() ?: throw StaffFormatException("line $number has a number out of range")
    }

    private fun hex(text: String, bytes: Int, number: Int): ByteArray {
        if (text.length != bytes * 2 || !text.all { it in '0'..'9' || it in 'a'..'f' }) throw StaffFormatException("line $number has a salt or a hash that is not $bytes bytes of hex")
        return ByteArray(bytes) { text.substring(2 * it, 2 * it + 2).toInt(16).toByte() }
    }

    private fun write(list: List<Cashier>) {
        val out = StringBuilder(HEADER).append('\n')
        for (c in list) {
            val r = c.pin
            out.append(r.iterations).append(' ').append(hex(r.salt)).append(' ').append(hex(r.hash)).append(' ')
                .append(minOf(r.wrong, 0xffffffffL)).append(' ').append(r.heldUntil).append(' ').append(c.name).append('\n')
        }
        DurableFile.write(dir, file, out.toString().toByteArray(Charsets.UTF_8))
    }

    private fun hex(b: ByteArray): String = b.joinToString("") { "%02x".format(it) }

    companion object {
        const val FILE = "cashiers.txt"
        const val HEADER = "openbmt-cashiers 1"
        const val MAX_CASHIERS = 30
        const val MAX_NAME = 24
        const val MIN_DIGITS = 4
        const val MAX_DIGITS = 8

        // The file is refused when it is longer than 30 lines of the longest form can be.
        private const val MAX_FILE = 16_384L
        private const val MIN_ITERATIONS = 1_000L
        private const val MAX_ITERATIONS = 10_000_000L

        /** A name is 1 to 24 characters, with no control character and no space at either end (CR-141). */
        fun validName(name: String): Boolean =
            name.codePointCount(0, name.length) in 1..MAX_NAME && name.none { Character.isISOControl(it) } && name == name.trim()

        /** 4 to 8 digits, nothing else (CR-141). */
        fun validPin(pin: String): Boolean = pin.length in MIN_DIGITS..MAX_DIGITS && pin.all { it in '0'..'9' }

        /** Whether [name] is the manager's or another cashier's, ignoring case (CR-141). */
        fun taken(name: String, others: List<String>, manager: String): Boolean {
            val key = name.lowercase(Locale.ROOT)
            return key == manager.lowercase(Locale.ROOT) || others.any { it.lowercase(Locale.ROOT) == key }
        }
    }
}

/** What is checked on the Staff screen before a cashier is added or given a new PIN. */
object StaffCheck {
    /** The words of the first thing wrong with a name for a new cashier, or null when it is fine. */
    fun nameProblem(name: String, others: List<String>, manager: String): SayWords? = when {
        !StaffStore.validName(name) -> AppWords.cashierNameNotAccepted
        StaffStore.taken(name, others, manager) -> AppWords.cashierNameUsed
        others.size >= StaffStore.MAX_CASHIERS -> AppWords.cashiersFull
        else -> null
    }

    /** The words of what is wrong with the PIN typed twice, or null when it is fine. */
    fun pinProblem(pin1: String, pin2: String): SayWords? =
        if (!StaffStore.validPin(pin1) || pin1 != pin2) AppWords.cashierPinNotAccepted else null
}
