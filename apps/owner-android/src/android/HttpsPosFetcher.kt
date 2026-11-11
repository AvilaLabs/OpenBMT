package org.openbmt.owner.app

import java.io.IOException
import java.net.HttpURLConnection
import java.net.InetSocketAddress
import java.net.Proxy
import java.net.URL
import org.openbmt.owner.PosMethod
import org.openbmt.owner.PosRequest

/**
 * The one place the app opens a connection of its own (OA-15, OA-17): one
 * request of the library to the till's system, on the platform's
 * `HttpsURLConnection`, through the SOCKS proxy of the [gate] with the host
 * name left to the proxy to look up. If the proxy does not answer, nothing is
 * sent. No cookies, no cache, no redirect, no compression, a fixed generic
 * User-Agent, a timeout on the connection and on each read and on the whole
 * page, and at most [PosLimits.MAX_PAGE_BYTES] bytes read. The address is one
 * of [PosLimits.HOSTS] or nothing is sent.
 *
 * The sign-in is put in the `Authorization` header here and nowhere else. It
 * is never logged, never in an error text, and not in the address.
 */
class HttpsPosFetcher(private val gate: TorGate) : PosFetcher {
    override fun fetch(request: PosRequest, token: String): PosFetch {
        if (!PosUrl.allowed(request.url)) return PosFetch.NotReached("the address is not one the app reads")
        if (!gate.available()) return PosFetch.TorMissing
        val hp = HostPort.split(gate.proxy) ?: return PosFetch.TorMissing
        val deadline = System.nanoTime() + PosLimits.DEADLINE_MS * 1_000_000L
        var conn: HttpURLConnection? = null
        try {
            // The proxy's own address is the one name this phone may look up (Orbot is at 127.0.0.1);
            // the till's name is left to the proxy to look up.
            val proxy = Proxy(Proxy.Type.SOCKS, InetSocketAddress(hp.first, hp.second))
            conn = URL(request.url).openConnection(proxy) as HttpURLConnection
            conn.requestMethod = if (request.method == PosMethod.Post) "POST" else "GET"
            conn.useCaches = false
            conn.defaultUseCaches = false
            conn.instanceFollowRedirects = false
            conn.connectTimeout = PosLimits.TIMEOUT_MS
            conn.readTimeout = PosLimits.TIMEOUT_MS
            conn.setRequestProperty("User-Agent", USER_AGENT)
            conn.setRequestProperty("Accept", "application/json")
            conn.setRequestProperty("Accept-Encoding", "identity")
            conn.setRequestProperty("Authorization", "Bearer $token")
            if (request.method == PosMethod.Post) {
                conn.doOutput = true
                conn.setRequestProperty("Content-Type", "application/json")
                conn.setFixedLengthStreamingMode(request.body.size)
                conn.outputStream.use { it.write(request.body) }
            }
            val code = conn.responseCode
            // A refusal carries a small body the library does not need; an answer of 200 carries the page.
            val stream = if (code in 200..299) conn.inputStream else conn.errorStream
            val body = if (stream == null) {
                ByteArray(0)
            } else if (code == 200) {
                stream.use { readCapped(it, PosLimits.MAX_PAGE_BYTES) { System.nanoTime() > deadline } } ?: return PosFetch.TooLong
            } else {
                ByteArray(0)
            }
            return PosFetch.Reply(code.toLong(), body)
        } catch (e: IOException) {
            // The kind of problem only: a message of a connection can hold the address.
            return PosFetch.NotReached(e.javaClass.simpleName)
        } catch (e: SecurityException) {
            return PosFetch.NotReached(e.javaClass.simpleName)
        } finally {
            conn?.disconnect()
        }
    }

    companion object {
        /** Generic on purpose: the same string whatever the shop, the app or the version. */
        const val USER_AGENT = "Mozilla/5.0"
    }
}
