package org.openbmt.owner.app

import java.io.IOException
import java.net.InetSocketAddress
import java.net.Socket

/** Whether something that speaks SOCKS5 answers at an address. */
fun interface SocksProbe {
    fun answers(host: String, port: Int): Boolean
}

/**
 * The real probe: connect to the proxy and send the SOCKS5 greeting (version
 * 5, one method, "no authentication"); a SOCKS5 server answers `05 00`. Only
 * a proxy that says so counts as running, not any open port. Nothing but the
 * greeting is sent, and no name or address of the shop's traffic. (A copy of
 * the counter app's probe.)
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

/**
 * What Tor means for the owner app (OA-05). The owner phone always goes
 * through Tor: the node's peers and the sending of payments through [proxy]
 * (Orbot's own address by default), and the link's onion route through the
 * same proxy. There is no setting that turns it off and no fallback: if the
 * proxy does not answer, [available] is false and the node is not started.
 */
class TorGate(val address: String, private val probe: SocksProbe = Socks5Probe()) {
    /** The `host:port` the node is given. */
    val proxy: String get() = address

    /** True when the proxy answers a SOCKS5 greeting. */
    fun available(): Boolean {
        val hp = HostPort.split(address) ?: return false
        return probe.answers(hp.first, hp.second)
    }
}
