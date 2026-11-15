package org.openbmt.app

import java.io.File

/**
 * The browser's twin of the Android file of the same name: the page has no
 * file system, so a write is the new bytes in the one table that [File] is.
 * A write is whole or it is not made, as on the phone.
 */
object DurableFile {
    fun write(dir: File, file: File, bytes: ByteArray) {
        file.writeBytes(bytes)
    }
}
