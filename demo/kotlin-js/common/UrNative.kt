package org.openbmt.common

/**
 * The browser's twin of the second function of the QR library: the global
 * function `openbmtQrCall` of the page, the WebAssembly build of the same one.
 */
object UrNative {
    private val host: dynamic = js("(function (request) { return globalThis.openbmtQrCall ? globalThis.openbmtQrCall(request) : null; })")

    fun call(request: ByteArray): ByteArray? {
        val answer: dynamic = host(request)
        if (answer == null || answer == undefined) return null
        val size = answer.length as Int
        return ByteArray(size) { i -> (answer[i] as Int).toByte() }
    }
}
