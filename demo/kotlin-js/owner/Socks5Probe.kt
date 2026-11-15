package org.openbmt.owner.app

/** The browser's twin of the SOCKS5 probe: a page opens no sockets, so nothing answers. */
class Socks5Probe(private val timeoutMs: Int = 2000) : SocksProbe {
    override fun answers(host: String, port: Int): Boolean = false
}
