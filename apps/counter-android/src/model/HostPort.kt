package org.openbmt.counter.app

/** A `host:port` for the proxy. */
object HostPort {
    /** The host is 1 to 253 characters of letters, digits, dots, hyphens, or a bracketed IPv6 address; the port is 1 to 65535. */
    fun valid(text: String): Boolean = split(text) != null

    fun split(text: String): Pair<String, Int>? {
        val colon = text.lastIndexOf(':')
        if (colon <= 0 || colon == text.length - 1) return null
        val host = text.substring(0, colon)
        val portText = text.substring(colon + 1)
        if (portText.length > 5 || !portText.all { it in '0'..'9' }) return null
        val port = portText.toInt()
        if (port !in 1..65535) return null
        val hostOk = if (host.startsWith("[")) {
            host.endsWith("]") && host.length > 2 && host.substring(1, host.length - 1).all { it in '0'..'9' || it in 'a'..'f' || it in 'A'..'F' || it == ':' || it == '.' }
        } else {
            host.length <= 253 && host.all { it in 'a'..'z' || it in 'A'..'Z' || it in '0'..'9' || it == '.' || it == '-' }
        }
        return if (hostOk) host to port else null
    }
}
