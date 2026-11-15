// java.nio for the browser build (see JavaIo.kt).
@file:Suppress("PackageDirectoryMismatch", "unused")

package java.nio

class ByteOrder private constructor(val name: String) {
    companion object {
        val BIG_ENDIAN = ByteOrder("BIG_ENDIAN")
        val LITTLE_ENDIAN = ByteOrder("LITTLE_ENDIAN")
    }
}

/** A cursor over a byte array, with the reads and writes of the model. Big-endian until [order] says otherwise. */
class ByteBuffer private constructor(private val bytes: ByteArray, private val start: Int, private var lim: Int) {
    private var pos = 0
    private var little = false

    val limit: Int get() = lim
    var position: Int
        get() = pos
        set(value) {
            pos = value
        }
    fun position(p: Int): ByteBuffer {
        pos = p
        return this
    }
    fun limit(): Int = lim
    fun position(): Int = pos
    fun remaining(): Int = lim - pos
    fun clear(): ByteBuffer {
        pos = 0
        lim = bytes.size - start
        return this
    }
    fun duplicate(): ByteBuffer = ByteBuffer(bytes, start, lim).also {
        it.pos = pos
        it.little = little
    }
    fun array(): ByteArray = bytes
    fun order(o: ByteOrder): ByteBuffer {
        little = o === ByteOrder.LITTLE_ENDIAN
        return this
    }

    private fun at(i: Int): Int = start + i
    private fun need(n: Int, what: String) {
        if (pos + n > lim) throw IndexOutOfBoundsException("$what past the end of the buffer")
    }

    fun get(): Byte {
        need(1, "get")
        return bytes[at(pos++)]
    }
    fun get(dst: ByteArray): ByteBuffer {
        need(dst.size, "get")
        bytes.copyInto(dst, 0, at(pos), at(pos) + dst.size)
        pos += dst.size
        return this
    }
    fun put(b: Byte): ByteBuffer {
        need(1, "put")
        bytes[at(pos++)] = b
        return this
    }
    fun put(src: ByteArray): ByteBuffer {
        need(src.size, "put")
        src.copyInto(bytes, at(pos))
        pos += src.size
        return this
    }

    private fun readN(n: Int): Long {
        need(n, "read")
        var v = 0L
        for (i in 0 until n) {
            val b = bytes[at(pos + (if (little) n - 1 - i else i))].toLong() and 0xff
            v = (v shl 8) or b
        }
        pos += n
        return v
    }
    private fun writeN(v: Long, n: Int) {
        need(n, "put")
        for (i in 0 until n) {
            val shift = 8 * (if (little) i else n - 1 - i)
            bytes[at(pos + i)] = (v ushr shift).toByte()
        }
        pos += n
    }

    val short: Short get() = readN(2).toShort()
    val int: Int get() = readN(4).toInt()
    val long: Long get() = readN(8)
    fun putShort(v: Short): ByteBuffer = also { writeN(v.toLong(), 2) }
    fun putInt(v: Int): ByteBuffer = also { writeN(v.toLong(), 4) }
    fun putLong(v: Long): ByteBuffer = also { writeN(v, 8) }

    companion object {
        fun allocate(n: Int): ByteBuffer = ByteBuffer(ByteArray(n), 0, n)
        fun wrap(bytes: ByteArray): ByteBuffer = ByteBuffer(bytes, 0, bytes.size)
        fun wrap(bytes: ByteArray, offset: Int, length: Int): ByteBuffer = ByteBuffer(bytes, offset, length)
    }
}
