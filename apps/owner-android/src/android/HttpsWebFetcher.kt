package org.openbmt.owner.app

import java.io.IOException
import java.net.HttpURLConnection
import java.net.InetSocketAddress
import java.net.Proxy
import java.net.URL
import org.openbmt.owner.WebCall

/**
 * The one place the app opens a connection to the shop's own store (WO-02, WO-06): one call of the node, on the
 * platform's `HttpsURLConnection`, through the SOCKS proxy of the [gate] with the host name left to the proxy to look
 * up. The app always goes through Tor (OA-05): if the proxy does not answer, nothing is sent. An address that is not
 * `https` is refused before anything is sent (WO-06), and the certificate is the one the phone's own trust store
 * accepts. No cookies, no cache, no redirect, no compression, a fixed generic User-Agent, a timeout on the connection
 * and on each read and on the whole answer, and at most [WebLimits.MAX_ANSWER_BYTES] bytes read, as for the till
 * ([HttpsPosFetcher]). The headers and the body are the node's, sent as they are; the pairing's MAC header is in none
 * of the errors.
 */
class HttpsWebFetcher(private val gate: TorGate) : WebFetcher {
    override fun send(call: WebCall): WebFetch {
        if (!WebLimits.isHttps(call.url)) return WebFetch.NotReached
        if (!gate.available()) return WebFetch.TorMissing
        val hp = HostPort.split(gate.proxy) ?: return WebFetch.TorMissing
        val deadline = System.nanoTime() + PosLimits.DEADLINE_MS * 1_000_000L
        var conn: HttpURLConnection? = null
        try {
            val proxy = Proxy(Proxy.Type.SOCKS, InetSocketAddress(hp.first, hp.second))
            conn = URL(call.url).openConnection(proxy) as HttpURLConnection
            conn.requestMethod = "POST"
            conn.useCaches = false
            conn.defaultUseCaches = false
            conn.instanceFollowRedirects = false
            conn.connectTimeout = PosLimits.TIMEOUT_MS
            conn.readTimeout = PosLimits.TIMEOUT_MS
            conn.setRequestProperty("User-Agent", HttpsPosFetcher.USER_AGENT)
            conn.setRequestProperty("Accept", "application/json")
            conn.setRequestProperty("Accept-Encoding", "identity")
            for ((name, value) in call.headers) conn.setRequestProperty(name, value)
            conn.setRequestProperty("Content-Type", call.contentType)
            conn.doOutput = true
            conn.setFixedLengthStreamingMode(call.body.size)
            conn.outputStream.use { it.write(call.body) }
            val code = conn.responseCode
            // The store's refusals carry a small reason the node reads; both kinds of answer are read, capped.
            val stream = if (code in 200..299) conn.inputStream else conn.errorStream
            val body = if (stream == null) {
                ByteArray(0)
            } else {
                stream.use { readCapped(it, WebLimits.MAX_ANSWER_BYTES) { System.nanoTime() > deadline } } ?: return WebFetch.TooLong
            }
            return WebFetch.Answer(code.toLong(), body)
        } catch (e: IOException) {
            return WebFetch.NotReached
        } catch (e: SecurityException) {
            return WebFetch.NotReached
        } finally {
            conn?.disconnect()
        }
    }
}

/** The limits of a call to the store. */
object WebLimits {
    /** The most bytes taken from the store in one answer: its replies are lists of orders, a few lines each. */
    const val MAX_ANSWER_BYTES = 1024 * 1024

    /** True for an address that starts with `https://` and names a host (WO-06). */
    fun isHttps(url: String): Boolean = url.startsWith("https://") && url.length > "https://".length
}
