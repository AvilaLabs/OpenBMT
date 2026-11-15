package org.openbmt.owner.app

/** Whether something that speaks SOCKS5 answers at an address. */
fun interface SocksProbe {
    fun answers(host: String, port: Int): Boolean
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
