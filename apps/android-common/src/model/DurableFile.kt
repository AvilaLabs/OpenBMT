package org.openbmt.app

import java.io.File
import java.io.ByteArrayOutputStream
import java.io.FileOutputStream
import java.io.IOException
import java.nio.channels.FileChannel
import java.nio.file.AtomicMoveNotSupportedException
import java.nio.file.Files
import java.nio.file.StandardCopyOption
import java.nio.file.StandardOpenOption
import java.security.MessageDigest
import java.util.concurrent.ConcurrentHashMap

/**
 * A write that a crash cannot cut in half: the bytes go to a temporary file
 * next to [file] (`<name>.tmp`), are synced, and the file is renamed over the
 * real one, so a crash leaves the old content or the new one and never a
 * half. A leftover temporary file is never read, and the next write replaces
 * it.
 */
object DurableFile {
    private const val MAX_CAS_FILE_BYTES = 4 * 1024 * 1024
    private val casProcessLocks = ConcurrentHashMap<String, Any>()

    fun write(dir: File, file: File, bytes: ByteArray) {
        val temp = File(dir, "${file.name}.tmp")
        try {
            FileOutputStream(temp).use { out ->
                out.write(bytes)
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
        syncDirectory(dir)
    }

    /** Cross-instance durable CAS for ledgers: canonical-path process mutex plus sibling OS lock. */
    fun compareAndSwap(dir: File, file: File, expectedBytes: ByteArray?, nextBytes: ByteArray): Boolean {
        require(nextBytes.size <= MAX_CAS_FILE_BYTES) { "ledger exceeds storage limit" }
        require(expectedBytes == null || expectedBytes.size <= MAX_CAS_FILE_BYTES) { "ledger expectation exceeds storage limit" }
        val target = file.canonicalFile
        val parent = target.parentFile ?: dir.canonicalFile
        if (!parent.exists() && !parent.mkdirs() && !parent.isDirectory) throw IOException("cannot create ledger directory")
        val processLock = casProcessLocks.computeIfAbsent(target.path) { Any() }
        return synchronized(processLock) {
            val lockFile = File(parent, ".${target.name}.cas.lock")
            FileChannel.open(lockFile.toPath(), StandardOpenOption.CREATE, StandardOpenOption.WRITE).use { channel ->
                channel.lock().use {
                    val current = readBounded(target)
                    val matches = if (current == null || expectedBytes == null) current == null && expectedBytes == null
                    else MessageDigest.isEqual(current, expectedBytes)
                    if (!matches) return@synchronized false
                    write(parent, target, nextBytes)
                    true
                }
            }
        }
    }

    private fun readBounded(file: File): ByteArray? {
        if (!file.exists()) return null
        file.inputStream().use { input ->
            val out = ByteArrayOutputStream()
            val buffer = ByteArray(8192)
            while (true) {
                val count = input.read(buffer)
                if (count < 0) break
                if (out.size().toLong() + count > MAX_CAS_FILE_BYTES) throw IOException("ledger file exceeds size limit")
                out.write(buffer, 0, count)
            }
            return out.toByteArray()
        }
    }

    /** Makes the rename itself durable. Not every file system lets a directory be opened; then the rename alone stands. */
    private fun syncDirectory(dir: File) {
        try {
            FileChannel.open(dir.toPath(), StandardOpenOption.READ).use { it.force(true) }
        } catch (_: IOException) {
        }
    }
}
