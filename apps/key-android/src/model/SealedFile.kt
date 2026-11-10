package org.openbmt.key.app

import java.io.File
import java.io.FileOutputStream
import java.io.IOException
import java.nio.ByteBuffer
import java.nio.channels.FileChannel
import java.nio.file.AtomicMoveNotSupportedException
import java.nio.file.Files
import java.nio.file.StandardCopyOption
import java.nio.file.StandardOpenOption
import java.security.SecureRandom
import org.openbmt.key.KeyStore

/** An encrypted blob: the IV and the ciphertext with its tag. */
class Sealed(val iv: ByteArray, val ciphertext: ByteArray)

/**
 * The cipher behind the file. On Android it is AES-256-GCM under a key of the
 * Android Keystore; the tests use a fake. [aad] is authenticated and not
 * stored. [open] must throw when the blob does not decrypt.
 */
interface Sealer {
    /** Encrypt [plain] under a fresh 12-byte IV. */
    fun seal(plain: ByteArray, aad: ByteArray): Sealed

    fun open(sealed: Sealed, aad: ByteArray): ByteArray
}

/** The file did not have the layout of [StoreFormat]. */
class StoreFormatException(message: String) : IOException(message)

/**
 * The layout of the saved state: `"OBKS" | 1 | iv length u8 (12) | iv |
 * ciphertext length u32 LE | ciphertext`. The first five bytes are the
 * authenticated data of the cipher. The ciphertext holds the library's
 * `OBM7` envelope, which this layer never reads. Strict: any other length,
 * version or leftover byte is refused.
 */
object StoreFormat {
    val MAGIC = byteArrayOf('O'.code.toByte(), 'B'.code.toByte(), 'K'.code.toByte(), 'S'.code.toByte())
    const val VERSION = 1
    const val IV_LEN = 12
    const val TAG_LEN = 16

    /** The most a state file may hold. The library's envelope is far smaller. */
    const val MAX_CIPHERTEXT = 4 * 1024 * 1024

    /** The authenticated data: magic and version. */
    val AAD: ByteArray = MAGIC + byteArrayOf(VERSION.toByte())

    fun encode(sealed: Sealed): ByteArray {
        if (sealed.iv.size != IV_LEN) throw StoreFormatException("the IV is not $IV_LEN bytes")
        val n = sealed.ciphertext.size
        if (n < TAG_LEN || n > MAX_CIPHERTEXT) throw StoreFormatException("the ciphertext has a bad length")
        val out = ByteBuffer.allocate(AAD.size + 1 + IV_LEN + 4 + n)
        out.order(java.nio.ByteOrder.LITTLE_ENDIAN)
        out.put(AAD).put(IV_LEN.toByte()).put(sealed.iv).putInt(n).put(sealed.ciphertext)
        return out.array()
    }

    fun decode(bytes: ByteArray): Sealed {
        val head = AAD.size + 1 + IV_LEN + 4
        if (bytes.size < head) throw StoreFormatException("the file is too short")
        if (!bytes.copyOfRange(0, AAD.size).contentEquals(AAD)) {
            throw StoreFormatException("the file is not a saved state of this version")
        }
        if (bytes[AAD.size].toInt() != IV_LEN) throw StoreFormatException("the IV length is not $IV_LEN")
        val iv = bytes.copyOfRange(AAD.size + 1, AAD.size + 1 + IV_LEN)
        val n = ByteBuffer.wrap(bytes, AAD.size + 1 + IV_LEN, 4).order(java.nio.ByteOrder.LITTLE_ENDIAN).int
        if (n < TAG_LEN || n > MAX_CIPHERTEXT) throw StoreFormatException("the ciphertext has a bad length")
        if (bytes.size != head + n) throw StoreFormatException("the file length does not match its header")
        return Sealed(iv, bytes.copyOfRange(head, head + n))
    }
}

/**
 * The state file: the library's bytes, sealed, in one file. A save writes a
 * temporary file next to it, syncs it, then renames it over the real one, so
 * a crash leaves the old state or the new one and never a half. [load] gives
 * null on a first start (no file) and throws on a file that does not read or
 * does not decrypt; a leftover temporary file is never read.
 */
class SealedFile(private val dir: File, private val sealer: Sealer, name: String = "state.obks") {
    private val file = File(dir, name)
    private val temp = File(dir, "$name.tmp")

    fun load(): ByteArray? {
        if (!file.exists()) return null
        val sealed = StoreFormat.decode(file.readBytes())
        return sealer.open(sealed, StoreFormat.AAD)
    }

    fun save(bytes: ByteArray) {
        val blob = StoreFormat.encode(sealer.seal(bytes, StoreFormat.AAD))
        try {
            FileOutputStream(temp).use { out ->
                out.write(blob)
                out.flush()
                out.fd.sync()
            }
            try {
                Files.move(temp.toPath(), file.toPath(), StandardCopyOption.ATOMIC_MOVE)
            } catch (e: AtomicMoveNotSupportedException) {
                throw IOException("the file system cannot rename atomically", e)
            }
        } catch (e: Throwable) {
            temp.delete()
            throw e
        }
        syncDirectory()
    }

    /** Makes the rename itself durable. Not every file system lets a directory be opened; then the rename alone stands. */
    private fun syncDirectory() {
        try {
            FileChannel.open(dir.toPath(), StandardOpenOption.READ).use { it.force(true) }
        } catch (_: IOException) {
        }
    }
}

/** The library's [KeyStore] over a [SealedFile], the platform's random source and the clock. */
class PersistentStore(
    private val file: SealedFile,
    private val random: SecureRandom = SecureRandom(),
    private val clock: () -> Long = { System.currentTimeMillis() / 1000 },
) : KeyStore {
    override fun load(): ByteArray? = file.load()

    override fun save(bytes: ByteArray) = file.save(bytes)

    override fun random(n: Int): ByteArray = ByteArray(n).also { random.nextBytes(it) }

    override fun now(): Long = clock()
}
