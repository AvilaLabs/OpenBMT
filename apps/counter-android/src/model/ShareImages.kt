package org.openbmt.counter.app

import java.io.File
import java.io.IOException

/** CR-214: the name of a shared request image, and the folder rule the provider and the counter both follow. */
object ShareNames {
    const val FOLDER = "share"
    const val BYTES = 16

    /** Whether [name] is exactly [BYTES] random bytes in lower-case hex: no path part, no extension, nothing else. */
    fun valid(name: String): Boolean = name.length == BYTES * 2 && name.all { it in '0'..'9' || it in 'a'..'f' }

    /** The file of [name] in [folder], or null when [name] is not of that form or no such file exists. */
    fun resolve(folder: File, name: String): File? = if (valid(name)) File(folder, name).takeIf { it.isFile } else null

    /** The name made from [bytes]. */
    fun hex(bytes: ByteArray): String = bytes.joinToString("") { "%02x".format(it.toInt() and 0xff) }
}

/**
 * CR-214: what the counter has handed out. Each image has a fresh name in [folder]; [begin] first ends every earlier
 * one, [end] revokes each handed-out address through [revoke] and then deletes every file in the folder.
 */
class ShareLedger(private val folder: File, private val revoke: (String) -> Unit, private val fresh: () -> ByteArray) {
    private val names = mutableListOf<String>()

    /** The names handed out and not yet ended. */
    fun handedOut(): List<String> = names.toList()

    /** Ends every earlier share, then makes a file for a new one, written by [write], and returns its name. */
    @Throws(IOException::class)
    fun begin(write: (File) -> Unit): String {
        end()
        if (!folder.isDirectory && !folder.mkdirs()) throw IOException("the share folder could not be made")
        val name = ShareNames.hex(fresh())
        val file = File(folder, name)
        try {
            write(file)
        } catch (e: IOException) {
            file.delete()
            throw e
        }
        names.add(name)
        return name
    }

    /** Revokes every address handed out, then deletes every file in the folder. */
    fun end() {
        for (n in names) revoke(n)
        names.clear()
        folder.listFiles()?.forEach { it.delete() }
    }
}
