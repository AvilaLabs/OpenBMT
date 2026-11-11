package org.openbmt.key.app

/**
 * Splits the app's own text lines into the parts the screens lay out as cards (four check words, a key and its
 * fingerprint, the words of a recovery key). Nothing is added and nothing is left out. No Android class is used,
 * so the JVM test runs it.
 */
object ScreenParts {
    /** The words of a line, split at spaces: the four words of a pairing or a wallet check become four cards. */
    fun words(line: String): List<String> = line.trim().split(Regex("\\s+")).filter { it.isNotEmpty() }

    /** "Key 1: ABCD 1234" as the key and the value; a line without the colon is the key alone. */
    fun keyValue(line: String): Pair<String, String> {
        val at = line.indexOf(": ")
        return if (at < 0) line to "" else line.substring(0, at) to line.substring(at + 2)
    }

    /** Twelve or more words shown at a time stay one numbered list. */
    fun numbered(words: List<String>): String = words.withIndex().joinToString("\n") { (i, w) -> "${i + 1}. $w" }

    /** Whether [count] words go through the cards of four (a group of four) or stay a list. */
    fun asCards(count: Int): Boolean = count in 1..8
}
