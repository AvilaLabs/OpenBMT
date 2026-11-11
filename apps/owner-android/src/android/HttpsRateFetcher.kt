package org.openbmt.owner.app

import java.io.IOException
import java.net.HttpURLConnection
import java.net.InetSocketAddress
import java.net.Proxy
import java.net.URL
import org.openbmt.owner.PriceSource

/**
 * RT-40 to RT-45 for the owner phone (OA-101), on the platform's `HttpsURLConnection`: one GET of a price
 * source's address, through the SOCKS proxy of the [gate] with the host name left to the proxy to look up. If
 * the proxy does not answer, nothing is sent. No cookies, no cache, no redirect, no compression, a fixed generic
 * User-Agent (the same for every shop and every source), a timeout of [PriceLimits.FETCH_TIMEOUT_SECS] on the
 * connection, on each read and on the whole fetch, and at most [PriceLimits.MAX_RESPONSE_BYTES] bytes read.
 * Anything but a 200 with a body inside the limit is a missing sample (RT-44). The address is the table's, which
 * the library gives; no request depends on the shop.
 */
class HttpsRateFetcher(private val gate: TorGate) : RateFetcher {
    override fun fetch(source: PriceSource): RateFetch {
        if (!gate.available()) return RateFetch.TorMissing
        val hp = HostPort.split(gate.proxy) ?: return RateFetch.TorMissing
        val deadline = System.nanoTime() + PriceLimits.FETCH_TIMEOUT_SECS * 1_000_000_000L
        var conn: HttpURLConnection? = null
        try {
            // The proxy's own address is the one name this phone may look up (Orbot is at 127.0.0.1);
            // the source's name is left to the proxy to look up.
            val proxy = Proxy(Proxy.Type.SOCKS, InetSocketAddress(hp.first, hp.second))
            conn = URL(source.url).openConnection(proxy) as HttpURLConnection
            conn.requestMethod = "GET"
            conn.useCaches = false
            conn.defaultUseCaches = false
            conn.instanceFollowRedirects = false
            conn.connectTimeout = (PriceLimits.FETCH_TIMEOUT_SECS * 1000).toInt()
            conn.readTimeout = (PriceLimits.FETCH_TIMEOUT_SECS * 1000).toInt()
            conn.setRequestProperty("User-Agent", USER_AGENT)
            conn.setRequestProperty("Accept", "application/json")
            conn.setRequestProperty("Accept-Encoding", "identity")
            if (conn.responseCode != 200) return RateFetch.NotAnswered
            val out = java.io.ByteArrayOutputStream()
            val buf = ByteArray(4096)
            conn.inputStream.use { input ->
                while (true) {
                    if (System.nanoTime() > deadline) return RateFetch.NotAnswered
                    val room = PriceLimits.MAX_RESPONSE_BYTES + 1 - out.size()
                    if (room <= 0) return RateFetch.NotAnswered
                    val n = input.read(buf, 0, minOf(buf.size, room))
                    if (n < 0) break
                    out.write(buf, 0, n)
                }
            }
            if (out.size() > PriceLimits.MAX_RESPONSE_BYTES) return RateFetch.NotAnswered
            return RateFetch.Body(out.toByteArray())
        } catch (_: IOException) {
            return RateFetch.NotAnswered
        } catch (_: SecurityException) {
            return RateFetch.NotAnswered
        } finally {
            conn?.disconnect()
        }
    }

    companion object {
        /** Generic on purpose: the same string whatever the shop, the app or the version. */
        const val USER_AGENT = "Mozilla/5.0"
    }
}
