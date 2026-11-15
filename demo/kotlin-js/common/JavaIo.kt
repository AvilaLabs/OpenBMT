// The classes of java.io, java.nio and java.security that the apps' model
// uses, for the browser build only (see JvmLang.kt). The browser has no files:
// a File names a place in one table in memory, which is what the apps' own
// folder is for the demo.
@file:Suppress("PackageDirectoryMismatch", "unused")

package java.io

open class IOException(message: String? = null, cause: Throwable? = null) : Exception(message, cause)

class File(val parent: String?, val name: String) {
    constructor(dir: File, name: String) : this(dir.path, name)
    constructor(path: String) : this(path.substringBeforeLast('/', ""), path.substringAfterLast('/'))

    val path: String get() = if (parent.isNullOrEmpty()) name else "$parent/$name"

    fun exists(): Boolean = table.containsKey(path)
    fun readBytes(): ByteArray = table[path]?.copyOf() ?: throw IOException("$path does not exist")
    fun writeBytes(bytes: ByteArray) {
        table[path] = bytes.copyOf()
    }
    fun delete(): Boolean = table.remove(path) != null
    fun length(): Long = table[path]?.size?.toLong() ?: 0L
    val isFile: Boolean get() = exists()
    val absolutePath: String get() = path
    val isDirectory: Boolean get() = folders.contains(path)

    /** Makes the folder; there is nothing to fail on in a table, so it always answers true. */
    fun mkdirs(): Boolean {
        folders.add(path)
        return true
    }

    /** Makes an empty file and answers true, or answers false when there is one. */
    fun createNewFile(): Boolean {
        if (exists()) return false
        table[path] = ByteArray(0)
        return true
    }

    companion object {
        /** The files of the demo's one folder. */
        val table = HashMap<String, ByteArray>()

        /** The folders that were made. */
        val folders = HashSet<String>()
    }
}

/** The reading side of a stream of bytes; the model reads what the page hands it. */
abstract class InputStream {
    /** The next byte (0 to 255), or -1 at the end. */
    abstract fun read(): Int

    /** Up to [len] bytes into [b] at [off]; the count, or -1 at the end. */
    open fun read(b: ByteArray, off: Int, len: Int): Int {
        if (len == 0) return 0
        var n = 0
        while (n < len) {
            val c = read()
            if (c < 0) break
            b[off + n] = c.toByte()
            n++
        }
        return if (n == 0) -1 else n
    }
}

open class ByteArrayOutputStream {
    protected var buf = ByteArray(32)
    protected var count = 0

    fun reset() {
        count = 0
    }

    fun size(): Int = count
    fun write(b: Int) {
        grow(1)
        buf[count++] = b.toByte()
    }
    fun write(bytes: ByteArray) = write(bytes, 0, bytes.size)
    fun write(bytes: ByteArray, off: Int, len: Int) {
        grow(len)
        bytes.copyInto(buf, count, off, off + len)
        count += len
    }
    fun toByteArray(): ByteArray = buf.copyOf(count)

    private fun grow(extra: Int) {
        if (count + extra > buf.size) buf = buf.copyOf(maxOf(buf.size * 2, count + extra))
    }
}
