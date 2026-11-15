// The text calls of the JVM that the apps' model uses, for the browser build
// only (see JvmLang.kt).
@file:Suppress("PackageDirectoryMismatch", "unused")

package kotlin.text

/** A character set: the three the model names. */
class Charset internal constructor(val name: String) {
    fun newDecoder(): java.nio.charset.CharsetDecoder = java.nio.charset.CharsetDecoder()
}

object Charsets {
    val UTF_8 = Charset("UTF-8")
    val US_ASCII = Charset("US-ASCII")
    val ISO_8859_1 = Charset("ISO-8859-1")
}

fun String.toByteArray(charset: Charset): ByteArray = when (charset.name) {
    "UTF-8" -> encodeToByteArray()
    else -> ByteArray(length) { i -> (this[i].code and 0xff).toByte() }
}

/** The text of [bytes] in [charset]: bytes with the high bit set are not ASCII and read as the replacement character. */
fun String(bytes: ByteArray, charset: Charset): String = when (charset.name) {
    "UTF-8" -> bytes.decodeToString()
    "US-ASCII" -> bytes.map { b -> if (b >= 0) b.toInt().toChar() else '�' }.toCharArray().concatToString()
    else -> bytes.map { b -> (b.toInt() and 0xff).toChar() }.toCharArray().concatToString()
}

fun String(bytes: ByteArray, offset: Int, length: Int, charset: Charset): String =
    String(bytes.copyOfRange(offset, offset + length), charset)

fun String.lowercase(locale: java.util.Locale): String = lowercase()

fun String.codePointCount(begin: Int, end: Int): Int {
    var n = 0
    var i = begin
    while (i < end) {
        i += if (this[i].isHighSurrogate() && i + 1 < end && this[i + 1].isLowSurrogate()) 2 else 1
        n++
    }
    return n
}

/** `"%02x".format(n)` and `String.format("%08X", n)`: zero-padded hexadecimal, the two forms the model writes. */
fun String.format(vararg args: Any?): String {
    val spec = Regex("%0(\\d+)([xX])")
    val m = spec.matchEntire(this) ?: throw IllegalArgumentException("format $this is not supported")
    val width = m.groupValues[1].toInt()
    val n = when (val a = args[0]) {
        is Byte -> a.toLong() and 0xff
        is Int -> a.toLong() and 0xffffffffL
        is Long -> a
        else -> throw IllegalArgumentException("format of $a is not supported")
    }
    val digits = n.toString(16).padStart(width, '0')
    return if (m.groupValues[2] == "X") digits.uppercase() else digits
}

fun String.Companion.format(format: String, vararg args: Any?): String = format.format(*args)
