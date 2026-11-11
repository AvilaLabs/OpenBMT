package org.openbmt.app

/**
 * The checks of `openbmt_core::say::check` (docs/SCREEN-RULES.md SC-02 to
 * SC-05, SC-08, SC-12), as far as text can be checked, for the words an app
 * writes itself. The apps' tests run every one of their words through
 * [problem].
 */
object SayCheck {
    private val jargon = listOf(
        "change mark", "receive mark", "dust", "slice", "exposure", "sighash", "p2wsh", "p2wpkh", "p2tr",
        "p2pkh", "p2sh", "witness", "outpoint", "psbt", "xpub", "descriptor", "mempool", "utxo", "nonce",
        "hash", "unix", "internal", "panic", "decode",
    )

    private fun plainProblem(text: String): String? {
        if (text.any { Character.isISOControl(it) }) return "control character"
        if (Regex("[A-Z]{2}-[0-9]").containsMatchIn(text)) return "rule number"
        if (Regex("[0-9a-fA-F]{12,}").findAll(text).any { m -> m.value.any { it.isDigit() } && m.value.any { it.isLetter() } }) {
            return "long hex"
        }
        if (Regex("[0-9]{9,}").containsMatchIn(text)) return "raw number"
        if (Regex("[A-Za-z0-9]\\(").containsMatchIn(text)) return "call notation"
        if (listOf("_", "::", "{", "}", "0x").any { text.contains(it) }) return "notation"
        val lower = text.lowercase()
        jargon.firstOrNull { lower.contains(it) }?.let { return "jargon $it" }
        return null
    }

    private fun sentenceProblem(text: String): String? {
        if (text.isEmpty() || text.length > 140) return "length"
        if (!text.endsWith(".") && !text.endsWith("?")) return "no full stop"
        if (text[0].isLowerCase()) return "small first letter"
        return plainProblem(text)
    }

    /** Why the words do not pass, or null when they do. */
    fun problem(w: SayWords): String? {
        if (w.heading.isEmpty() || w.heading.length > 28 || w.heading.endsWith(".")) return "heading length"
        plainProblem(w.heading)?.let { return "heading: $it" }
        sentenceProblem(w.what)?.let { return "what: $it" }
        sentenceProblem(w.next)?.let { return "next: $it" }
        if (w.what == w.next) return "same sentences"
        if (w.who !in 0..6 || w.tone !in 0..3) return "tone or who"
        w.detail?.let { if (it.length > 200 || it.any { c -> Character.isISOControl(c) }) return "detail" }
        w.quoted?.let { if (it.text != Format.cleanForeign(it.text)) return "quoted not clean" }
        return null
    }

    /** The words of the whole list differ from one another (SC-07): no two share a heading and a first sentence. */
    fun distinctProblem(all: List<Pair<String, SayWords>>): String? {
        for ((i, a) in all.withIndex()) {
            for (b in all.drop(i + 1)) {
                if (a.second.heading == b.second.heading && a.second.what == b.second.what) {
                    return "${a.first} and ${b.first} read the same"
                }
            }
        }
        return null
    }
}
