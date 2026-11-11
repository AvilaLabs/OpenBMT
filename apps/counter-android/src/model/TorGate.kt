package org.openbmt.counter.app

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

/**
 * What Tor means for the app (CR-87). With Tor off in the shop's signed
 * settings, connections are direct and [proxy] is null. With Tor on, the
 * proxy is the address on the counter ([address], Orbot's own by default),
 * and every connection goes through it: the node's peers through the node's
 * proxy setting, the price fetches through a SOCKS `java.net.Proxy`. If the
 * proxy does not answer, [available] is false and the caller sends nothing
 * around it.
 */
class TorGate(val torOn: Boolean, val address: String, private val probe: SocksProbe = Socks5Probe()) {
    /** The `host:port` the node is given: the proxy with Tor on, none with Tor off. */
    val proxy: String? get() = if (torOn) address else null

    /** True when no proxy is needed or the proxy answers. */
    fun available(): Boolean {
        if (!torOn) return true
        val hp = HostPort.split(address) ?: return false
        return probe.answers(hp.first, hp.second)
    }
}
