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

    companion object {
        /** The files of the demo's one folder. */
        val table = HashMap<String, ByteArray>()
    }
}

class ByteArrayOutputStream {
    private var buf = ByteArray(32)
    private var count = 0

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
