package org.openbmt.app

/**
 * The words of an app's own states as JSON, for the web demo: the object of
 * every state name, with its tone, heading, what happened, what to do next
 * and who does it. It is made from the same list as the app's table in
 * docs/screens, so the page and the app cannot say different things.
 */
object WordsJson {
    private fun tone(n: Int) = when (n) {
        Tones.INFO -> "info"
        Tones.GOOD -> "good"
        Tones.STOP -> "stop"
        else -> "ended"
    }

    private fun who(n: Int) = when (n) {
        Who.NOBODY -> "nobody"
        Who.CUSTOMER -> "the customer"
        Who.CASHIER -> "the cashier"
        Who.MANAGER -> "a manager"
        Who.OWNER -> "the owner"
        Who.KEY_HOLDER -> "the key holder"
        else -> "the Box keeper"
    }

    /** A JSON string: quotes, backslash and control characters escaped, everything else as it is. */
    fun quote(text: String): String {
        val out = StringBuilder("\"")
        for (c in text) {
            when {
                c == '"' -> out.append("\\\"")
                c == '\\' -> out.append("\\\\")
                c == '\n' -> out.append("\\n")
                c.code < 0x20 -> out.append("\\u").append(c.code.toString(16).padStart(4, '0'))
                else -> out.append(c)
            }
        }
        return out.append('"').toString()
    }

    /** The document for [all]: `{ "<state>": { "tone", "heading", "what", "next", "who" }, ... }`, one state to a line. */
    fun of(all: List<Pair<String, SayWords>>): String {
        val out = StringBuilder("{\n")
        for ((i, pair) in all.withIndex()) {
            val (name, w) = pair
            out.append("  ${quote(name)}: {")
            out.append("\"tone\": ${quote(tone(w.tone))}, ")
            out.append("\"heading\": ${quote(w.heading)}, ")
            out.append("\"what\": ${quote(w.what)}, ")
            out.append("\"next\": ${quote(w.next)}, ")
            out.append("\"who\": ${quote(who(w.who))}}")
            out.append(if (i + 1 < all.size) ",\n" else "\n")
        }
        return out.append("}\n").toString()
    }
}
