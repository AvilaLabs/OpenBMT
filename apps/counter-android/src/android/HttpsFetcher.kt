package org.openbmt.counter.app

import java.io.IOException
import java.net.HttpURLConnection
import java.net.InetSocketAddress
import java.net.Proxy
import java.net.URL
import org.openbmt.counter.RateSource

/**
 * RT-40 to RT-45 on the platform's `HttpsURLConnection`: one GET of a
 * source's address, no cookies, no cache, no redirect, no compression, a
 * fixed generic User-Agent (the same for every shop and every source), a
 * timeout of [PriceLimits.FETCH_TIMEOUT_SECS] on the connection, on each read
 * and on the whole fetch, and at most [PriceLimits.MAX_RESPONSE_BYTES] bytes
 * read. Anything but a 200 with a body inside the limit is a missing sample
 * (RT-44).
 *
 * With Tor on, the connection goes through the SOCKS proxy of the [gate] with
 * the host name left to the proxy to look up; if the proxy does not answer,
 * nothing is sent (CR-87). With Tor off the connection is direct, and the
 * proxy of the system is not used either.
 */
class HttpsFetcher(private val gate: TorGate) : Fetcher {
    override fun fetch(source: RateSource): FetchResult {
        if (!gate.available()) return FetchResult.TorMissing
        val proxy = proxyOf()
        val deadline = System.nanoTime() + PriceLimits.FETCH_TIMEOUT_SECS * 1_000_000_000L
        var conn: HttpURLConnection? = null
        try {
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
            val code = conn.responseCode
            if (code != 200) return FetchResult.Status(code)
            val out = java.io.ByteArrayOutputStream()
            val buf = ByteArray(4096)
            conn.inputStream.use { input ->
                while (true) {
                    if (System.nanoTime() > deadline) return FetchResult.NotReached("the fetch took longer than the timeout")
                    val room = PriceLimits.MAX_RESPONSE_BYTES + 1 - out.size()
                    if (room <= 0) return FetchResult.TooLong
                    val n = input.read(buf, 0, minOf(buf.size, room))
                    if (n < 0) break
                    out.write(buf, 0, n)
                }
            }
            if (out.size() > PriceLimits.MAX_RESPONSE_BYTES) return FetchResult.TooLong
            return FetchResult.Body(out.toByteArray())
        } catch (e: IOException) {
            return FetchResult.NotReached(e.javaClass.simpleName)
        } catch (e: SecurityException) {
            return FetchResult.NotReached(e.javaClass.simpleName)
        } finally {
            conn?.disconnect()
        }
    }

    private fun proxyOf(): Proxy {
        val hp = gate.proxy?.let { HostPort.split(it) } ?: return Proxy.NO_PROXY
        // The proxy's own address is the one name this phone may look up (Orbot is at
        // 127.0.0.1); the source's name is left to the proxy to look up.
        return Proxy(Proxy.Type.SOCKS, InetSocketAddress(hp.first, hp.second))
    }

    companion object {
        /** Generic on purpose: the same string whatever the shop, the app or the version. */
        const val USER_AGENT = "Mozilla/5.0"
    }
}
