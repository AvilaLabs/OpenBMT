package org.openbmt.owner.app

import org.openbmt.app.Format
import org.openbmt.app.SayWords

/** The text of a request or a response was not in the form this app reads. */
class ApprovalTextException(message: String) : Exception(message)

/**
 * The request for the Key app and the Key app's response are bytes the
 * library makes and reads. As a file they are those bytes. As text they are
 * lower-case hex digits, 64 to a line; reading ignores spaces and line ends.
 * This undoes the armor and nothing more: what the bytes mean is judged by
 * the library when they are taken (OA-07, OA-13).
 */
object ApprovalText {
    /** The most bytes read: a request to the library is at most 1 MiB with its other fields. */
    const val MAX_BYTES = 1_000_000

    private const val LINE = 64

    fun encode(bytes: ByteArray): String {
        val hex = bytes.joinToString("") { "%02x".format(it) }
        val out = StringBuilder()
        var i = 0
        while (i < hex.length) {
            out.append(hex, i, minOf(i + LINE, hex.length)).append('\n')
            i += LINE
        }
        return out.toString()
    }

    fun decode(text: String): ByteArray {
        if (text.length > 2 * MAX_BYTES + MAX_BYTES / 16) throw ApprovalTextException("the text is longer than a response can be")
        val digits = text.filterNot { it == ' ' || it == '\n' || it == '\r' || it == '\t' }
        if (digits.isEmpty()) throw ApprovalTextException("the text is empty")
        if (digits.length % 2 != 0) throw ApprovalTextException("the text has an odd number of hex digits")
        if (digits.length / 2 > MAX_BYTES) throw ApprovalTextException("the text is longer than a response can be")
        return Format.fromHex(digits) ?: throw ApprovalTextException("the text is not hex")
    }

    /** The start of the text of [bytes] for a screen: [chars] characters of it, then how many bytes there are in all. */
    fun preview(bytes: ByteArray, chars: Int = 320): String {
        val text = encode(bytes.copyOfRange(0, minOf(bytes.size, chars / 2))).replace("\n", "")
        return if (bytes.size * 2 <= chars) text else "$text ... (${bytes.size} bytes in all)"
    }

    /** A short name for a file of a session: the first 4 bytes of its number in hex. */
    fun fileName(prefix: String, sessionId: ByteArray): String =
        prefix + "-" + sessionId.take(4).joinToString("") { "%02x".format(it) } + ".obm"
}

/**
 * Where the Key app's response comes from. Pasted text and a file picked from
 * the phone are the sources today; a camera that reads a code will be another
 * source behind the same two calls, and nothing else of the app changes.
 * [begin] gives the bytes to [onBytes], or the words of why there are none
 * to [onFailed]; it may do either later, on the screen thread. Every source
 * gives bytes no longer than [ApprovalText.MAX_BYTES].
 */
interface ResponseSource {
    /** What the button for this source says. */
    val label: String

    fun begin(onBytes: (ByteArray) -> Unit, onFailed: (SayWords) -> Unit)
}

/** The response as pasted text, read from [text] when the button is pressed. */
class PastedResponse(private val text: () -> String) : ResponseSource {
    override val label = "Use the pasted response"

    override fun begin(onBytes: (ByteArray) -> Unit, onFailed: (SayWords) -> Unit) {
        try {
            onBytes(ApprovalText.decode(text()))
        } catch (e: ApprovalTextException) {
            onFailed(AppWords.responseNotReadable(e.message))
        }
    }
}
