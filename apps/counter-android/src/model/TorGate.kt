package org.openbmt.counter.app

/** Whether something that speaks SOCKS5 answers at an address. */
fun interface SocksProbe {
    fun answers(host: String, port: Int): Boolean
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
