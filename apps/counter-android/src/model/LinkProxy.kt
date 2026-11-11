package org.openbmt.counter.app

/**
 * How the app gives the library the proxy for the link to the owner (CR-134). The link's turn is made
 * every [EVERY_MS] (CR-135).
 *
 * With the shop's Tor setting on, the library reaches every relay through the node's proxy, so there
 * is nothing more to give. With it off, a relay that is an onion service still needs Orbot, and Orbot's
 * address is given only when it answers; a relay at an address on the local network is reached direct
 * and is given none.
 */
object LinkProxy {
    /** Milliseconds between two turns of the link. */
    const val EVERY_MS = 5000L

    /** The proxy address to hand the library for the link, or null for none. */
    fun pick(onionRelay: Boolean, torOn: Boolean, address: String, probe: SocksProbe): String? {
        if (!onionRelay || torOn) return null
        val hp = HostPort.split(address) ?: return null
        return if (probe.answers(hp.first, hp.second)) address else null
    }
}
