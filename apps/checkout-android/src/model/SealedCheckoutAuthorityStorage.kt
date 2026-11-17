package org.openbmt.checkout

import java.io.ByteArrayOutputStream
import java.io.DataInputStream
import java.io.DataOutputStream
import java.io.File
import java.io.FileOutputStream
import java.io.IOException
import java.io.RandomAccessFile
import java.nio.channels.FileChannel
import java.nio.file.Files
import java.nio.file.StandardCopyOption
import java.nio.file.StandardOpenOption
import java.security.MessageDigest
import java.util.concurrent.ConcurrentHashMap

/** Minimal cipher boundary keeps the model storage testable without Android classes. */
class CheckoutAuthoritySealedValue(val iv: ByteArray, val ciphertext: ByteArray)

interface CheckoutAuthorityRecordSealer {
    fun seal(plain: ByteArray, aad: ByteArray): CheckoutAuthoritySealedValue
    fun open(sealed: CheckoutAuthoritySealedValue, aad: ByteArray): ByteArray
}

/**
 * Scope-bound authenticated file storage with serialized compare-and-swap.
 *
 * The injected [sealer] is expected to provide authenticated encryption. Android production code
 * supplies KeystoreSealer; the test suite uses a deterministic fake to exercise this file/CAS
 * boundary without an Android runtime.
 */
class SealedCheckoutAuthorityStorage(
    directory: File,
    scope: CheckoutAuthorityScope,
    private val sealer: CheckoutAuthorityRecordSealer,
    name: String = DEFAULT_FILE_NAME,
) : CheckoutAuthorityStorage {
    private val directory = directory.absoluteFile
    private val safeName = validateFileName(name)
    private val file = File(this.directory, safeName)
    private val lockFile = File(this.directory, safeName + ".lock")
    private val aad = scopeAad(scope, safeName)
    private val processLock = PROCESS_LOCKS.computeIfAbsent(file.canonicalPath) { Any() }

    override fun read(): ByteArray? = withExclusiveLock { readUnlocked() }

    override fun compareAndSwap(expectedBytes: ByteArray?, nextBytes: ByteArray): Boolean {
        require(nextBytes.size in 1..MAX_PLAINTEXT_BYTES)
        val expectedSnapshot = expectedBytes?.copyOf()
        val nextSnapshot = nextBytes.copyOf()
        return try {
            withExclusiveLock {
                val current = readUnlocked()
                try {
                    if (!sameBytes(current, expectedSnapshot)) return@withExclusiveLock false
                    val sealed = try {
                        sealer.seal(nextSnapshot, aad.copyOf())
                    } catch (_: Exception) {
                        throw IOException("Checkout authority record could not be sealed")
                    }
                    writeUnlocked(encode(sealed))
                    true
                } finally {
                    current?.fill(0)
                }
            }
        } finally {
            expectedSnapshot?.fill(0)
            nextSnapshot.fill(0)
        }
    }

    private fun <T> withExclusiveLock(block: () -> T): T = synchronized(processLock) {
        if (!directory.exists() && !directory.mkdirs()) throw IOException("Checkout authority directory unavailable")
        if (!directory.isDirectory) throw IOException("Checkout authority directory unavailable")
        try {
            RandomAccessFile(lockFile, "rw").use { lock ->
                lock.channel.use { channel -> channel.lock().use { block() } }
            }
        } catch (error: IOException) {
            throw error
        } catch (_: Exception) {
            throw IOException("Checkout authority storage unavailable")
        }
    }

    private fun readUnlocked(): ByteArray? {
        if (!file.exists()) return null
        val size = file.length()
        if (size !in MIN_ENVELOPE_BYTES.toLong()..MAX_ENVELOPE_BYTES.toLong()) {
            throw IOException("Checkout authority record has an invalid size")
        }
        val bytes = try { Files.readAllBytes(file.toPath()) } catch (_: Exception) {
            throw IOException("Checkout authority record could not be read")
        }
        if (bytes.size.toLong() != size) throw IOException("Checkout authority record changed while reading")
        val sealed = decode(bytes)
        val plain = try {
            sealer.open(sealed, aad.copyOf())
        } catch (_: Exception) {
            throw IOException("Checkout authority record authentication failed")
        }
        if (plain.size !in 1..MAX_PLAINTEXT_BYTES) {
            plain.fill(0)
            throw IOException("Checkout authority record has an invalid size")
        }
        return plain
    }

    private fun writeUnlocked(bytes: ByteArray) {
        var temp: File? = null
        try {
            temp = File.createTempFile("." + file.name + ".", ".tmp", directory)
            FileOutputStream(temp).use { output ->
                output.write(bytes)
                output.flush()
                output.fd.sync()
            }
            Files.move(
                temp.toPath(), file.toPath(), StandardCopyOption.ATOMIC_MOVE, StandardCopyOption.REPLACE_EXISTING,
            )
            temp = null
            // Some Android filesystems cannot fsync a directory; the synced temp plus atomic rename
            // still guarantees a complete old or new record. A failed data write/rename is reported.
            try { FileChannel.open(directory.toPath(), StandardOpenOption.READ).use { it.force(true) } }
            catch (_: Exception) { }
        } catch (error: IOException) {
            throw error
        } catch (_: Exception) {
            throw IOException("Checkout authority record could not be committed")
        } finally {
            temp?.delete()
        }
    }

    private fun encode(sealed: CheckoutAuthoritySealedValue): ByteArray {
        if (sealed.iv.size != IV_BYTES || sealed.ciphertext.size !in MIN_CIPHERTEXT_BYTES..MAX_CIPHERTEXT_BYTES) {
            throw IOException("Checkout authority sealed record is invalid")
        }
        val out = ByteArrayOutputStream(HEADER_BYTES + sealed.ciphertext.size)
        DataOutputStream(out).use { data ->
            data.write(MAGIC)
            data.writeByte(FORMAT_VERSION)
            data.writeByte(sealed.iv.size)
            data.writeInt(sealed.ciphertext.size)
            data.write(sealed.iv)
            data.write(sealed.ciphertext)
        }
        return out.toByteArray()
    }

    private fun decode(bytes: ByteArray): CheckoutAuthoritySealedValue {
        if (bytes.size < MIN_ENVELOPE_BYTES || bytes.size > MAX_ENVELOPE_BYTES) {
            throw IOException("Checkout authority record has an invalid size")
        }
        try {
            DataInputStream(bytes.inputStream()).use { data ->
                val magic = ByteArray(MAGIC.size).also(data::readFully)
                if (!magic.contentEquals(MAGIC) || data.readUnsignedByte() != FORMAT_VERSION ||
                    data.readUnsignedByte() != IV_BYTES) throw IOException("Checkout authority record format is unsupported")
                val ciphertextSize = data.readInt()
                if (ciphertextSize !in MIN_CIPHERTEXT_BYTES..MAX_CIPHERTEXT_BYTES ||
                    bytes.size != HEADER_BYTES + IV_BYTES + ciphertextSize) {
                    throw IOException("Checkout authority record has an invalid size")
                }
                val iv = ByteArray(IV_BYTES).also(data::readFully)
                val ciphertext = ByteArray(ciphertextSize).also(data::readFully)
                if (data.available() != 0) throw IOException("Checkout authority record has trailing data")
                return CheckoutAuthoritySealedValue(iv, ciphertext)
            }
        } catch (error: IOException) {
            throw error
        } catch (_: Exception) {
            throw IOException("Checkout authority record format is invalid")
        }
    }

    private fun sameBytes(left: ByteArray?, right: ByteArray?): Boolean = when {
        left == null || right == null -> left == null && right == null
        else -> MessageDigest.isEqual(left, right)
    }

    override fun toString(): String = "SealedCheckoutAuthorityStorage(redacted)"

    private companion object {
        const val DEFAULT_FILE_NAME = "checkout-authority.obks"
        const val FORMAT_VERSION = 1
        const val IV_BYTES = 12
        const val HEADER_BYTES = 10 // 4-byte magic, version, IV length, and ciphertext length
        const val MIN_CIPHERTEXT_BYTES = 16 // AES-GCM authentication tag
        const val MAX_PLAINTEXT_BYTES = 1024 * 1024
        const val MAX_CIPHERTEXT_BYTES = MAX_PLAINTEXT_BYTES + MIN_CIPHERTEXT_BYTES
        const val MIN_ENVELOPE_BYTES = HEADER_BYTES + IV_BYTES + MIN_CIPHERTEXT_BYTES
        const val MAX_ENVELOPE_BYTES = HEADER_BYTES + IV_BYTES + MAX_CIPHERTEXT_BYTES
        val MAGIC = byteArrayOf('O'.code.toByte(), 'C'.code.toByte(), 'A'.code.toByte(), 'S'.code.toByte())
        val SCOPE_AAD_PREFIX = "OpenBMT checkout authority scope v1".toByteArray(Charsets.US_ASCII)
        val PROCESS_LOCKS = ConcurrentHashMap<String, Any>()

        fun validateFileName(name: String): String {
            if (name.isEmpty() || name.length > 96 || name == "." || name == ".." ||
                name.any { !(it in 'a'..'z' || it in 'A'..'Z' || it in '0'..'9' || it == '.' || it == '_' || it == '-') }) {
                throw IllegalArgumentException("Invalid checkout authority file name")
            }
            return name
        }

        fun scopeAad(scope: CheckoutAuthorityScope, name: String): ByteArray {
            val out = ByteArrayOutputStream()
            DataOutputStream(out).use { data ->
                data.write(MAGIC)
                data.writeByte(FORMAT_VERSION)
                data.write(SCOPE_AAD_PREFIX)
                data.writeByte(0)
                writeScopePart(data, name)
                writeScopePart(data, scope.merchantId)
                writeScopePart(data, scope.storeId)
                writeScopePart(data, scope.locationId)
                writeScopePart(data, scope.registerId)
                writeScopePart(data, scope.deviceId)
            }
            return out.toByteArray()
        }

        private fun writeScopePart(data: DataOutputStream, value: String) {
            val bytes = value.toByteArray(Charsets.UTF_8)
            data.writeShort(bytes.size)
            data.write(bytes)
        }
    }
}
