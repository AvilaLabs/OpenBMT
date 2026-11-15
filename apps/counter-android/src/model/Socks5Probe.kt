package org.openbmt.counter.app

import java.io.IOException
import java.net.InetSocketAddress
import java.net.Socket

/**
 * The real probe: connect to the proxy and send the SOCKS5 greeting (version
 * 5, one method, "no authentication"); a SOCKS5 server answers `05 00`. Only
 * a proxy that says so counts as running, not any open port. Nothing but the
 * greeting is sent, and no name or address of the shop's traffic.
 */
class Socks5Probe(private val timeoutMs: Int = 2000) : SocksProbe {
    override fun answers(host: String, port: Int): Boolean {
        return try {
            Socket().use { s ->
                s.soTimeout = timeoutMs
                s.connect(InetSocketAddress(host, port), timeoutMs)
                s.getOutputStream().apply { write(byteArrayOf(5, 1, 0)); flush() }
                val reply = ByteArray(2)
                var got = 0
                val input = s.getInputStream()
                while (got < 2) {
                    val n = input.read(reply, got, 2 - got)
                    if (n < 0) return false
                    got += n
                }
                reply[0].toInt() == 5 && reply[1].toInt() == 0
            }
        } catch (_: IOException) {
            false
        }
    }
}
