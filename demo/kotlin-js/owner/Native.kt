package org.openbmt.owner

/**
 * The browser's twin of the one native function of the library: the same call,
 * served by the page, which hands the request frame to the WebAssembly build
 * of the same Rust library (the global function `openbmtOwnerCall`).
 */
object Native {
    private val host: dynamic = js("(function (request) { return globalThis.openbmtOwnerCall(request); })")

    /** One request frame in, one response frame out (see PROTOCOL.md). Null when the page has no answer. */
    fun call(request: ByteArray): ByteArray? {
        val answer: dynamic = host(request)
        if (answer == null || answer == undefined) return null
        val size = answer.length as Int
        return ByteArray(size) { i -> (answer[i] as Int).toByte() }
    }
}
