package org.openbmt.app

import java.io.File
import java.io.FileOutputStream
import java.io.IOException
import java.nio.channels.FileChannel
import java.nio.file.AtomicMoveNotSupportedException
import java.nio.file.Files
import java.nio.file.StandardCopyOption
import java.nio.file.StandardOpenOption

/**
 * A write that a crash cannot cut in half: the bytes go to a temporary file
 * next to [file] (`<name>.tmp`), are synced, and the file is renamed over the
 * real one, so a crash leaves the old content or the new one and never a
 * half. A leftover temporary file is never read, and the next write replaces
 * it.
 */
object DurableFile {
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

    /** Makes the rename itself durable. Not every file system lets a directory be opened; then the rename alone stands. */
    private fun syncDirectory(dir: File) {
        try {
            FileChannel.open(dir.toPath(), StandardOpenOption.READ).use { it.force(true) }
        } catch (_: IOException) {
        }
    }
}
