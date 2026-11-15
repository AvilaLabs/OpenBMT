@file:Suppress("PackageDirectoryMismatch", "unused")

package java.nio.charset

/** Strict reading of UTF-8: a malformed byte is an error and not a replacement. */
class CharacterCodingException : java.io.IOException("the bytes are not valid in the character set")

class CodingErrorAction private constructor(val name: String) {
    companion object {
        val REPORT = CodingErrorAction("REPORT")
        val REPLACE = CodingErrorAction("REPLACE")
    }
}

class CharsetDecoder internal constructor() {
    private var strict = false

    fun onMalformedInput(action: CodingErrorAction): CharsetDecoder = also { strict = action === CodingErrorAction.REPORT }
    fun onUnmappableCharacter(action: CodingErrorAction): CharsetDecoder = this

    fun decode(buffer: java.nio.ByteBuffer): CharSequence {
        val bytes = ByteArray(buffer.remaining()).also { buffer.get(it) }
        return try {
            bytes.decodeToString(throwOnInvalidSequence = strict)
        } catch (e: CharacterCodingException) {
            throw e
        } catch (e: Throwable) {
            throw CharacterCodingException()
        }
    }
}
