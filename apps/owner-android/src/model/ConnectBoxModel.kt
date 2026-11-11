package org.openbmt.owner.app

import org.openbmt.owner.BoxLink

/**
 * What the screens that connect a Box write and decide, as plain text and
 * plain values (OA-40 to OA-48). Everything that has a meaning is the
 * library's: the claim is fetched, read and checked by the node and the
 * link client (BX-13, BX-14), the settings that add the Box are made by the
 * node (AS-83, AS-84), the welcome is made by the Key app and handed over
 * by the node (BX-15). What is here is the shape of what is typed, the
 * lines of the link that are added to the setup text once the Box has taken
 * its welcome. No Android class is used.
 */
object ConnectBoxModel {
    /** What the first screen says before anything is typed: the steps, in the order the owner does them. */
    val explanation: List<String> = listOf(
        "On the Box, type the invitation the Key app shows. Then type the Box's address here.",
        "The Key app checks the Box and approves it. Last, this phone hands the Box its welcome.",
    )

    private val LINK_KEYS = listOf("relay-route", "relay-key", "counter", "heartbeat-every", "check-in-grace")

    private fun hex(bytes: ByteArray): String = bytes.joinToString("") { "%02x".format(it) }

    /**
     * The address of a Box as typed: `host:port` on the local network, or a v3 `.onion` name and a port. Spaces
     * around it are dropped and capitals are made small, as the library reads them. Only the shape is judged
     * here (OA-07); whether it is a Box is for the call to say.
     */
    fun address(text: String): Form<String> {
        val t = text.trim().lowercase()
        if (t.isEmpty() || t.length > 300 || !HostPort.valid(t)) return Form.Refused(BoxWords.addressNotRead)
        return Form.Ok(t)
    }

    /** Whether [address] names a Tor onion service. */
    fun isOnion(address: String): Boolean = HostPort.split(address)?.first?.endsWith(".onion") == true

    /**
     * What the library is given for [address]: the proxy of Orbot reaches an onion name; an address on the
     * local network is reached directly, with no proxy.
     */
    fun linkFor(address: String, proxy: String): BoxLink = BoxLink(address, if (isOnion(address)) proxy else "")

    /** The `relay-route` value of the setup text for a Box at [address]. */
    fun routeLine(address: String): String = (if (isOnion(address)) "onion " else "direct ") + address

    /** The four words, written large to be compared with the Box's and the Key app's. */
    fun wordsLine(words: List<String>): String = words.joinToString("   ")

    /** A Box's key as groups of eight hex digits, in full. */
    fun keyGroups(key: ByteArray): String = hex(key).chunked(8).joinToString(" ")

    /** A short name for a Box in lists: the first four bytes of its key. */
    fun boxLabel(key: ByteArray): String = "Box " + hex(key.copyOf(4))

    /** The lines of the link a paired Box adds: its route, its key and the counters the settings name. */
    fun linkLines(address: String, boxKey: ByteArray, counters: List<ByteArray>): List<String> =
        listOf("relay-route: ${routeLine(address)}", "relay-key: ${hex(boxKey)}") +
            counters.map { "counter: ${hex(it)}" }

    /**
     * [setup] with the link of a paired Box: the lines of a link it held (route, key, counters and the two
     * intervals) are replaced, and the Box is the relay, with the default intervals. The counters the settings
     * name are written as counter lines, up to [OwnerSetup.MAX_COUNTERS]; they are only a start (LK-67), the link
     * follows the settings, so a link with no counter line is valid. The caller checks the text with
     * [OwnerSetup.parse] before it is saved.
     */
    fun withLink(setup: String, address: String, boxKey: ByteArray, counters: List<ByteArray>): String {
        val counters = counters.take(OwnerSetup.MAX_COUNTERS)
        val lines = setup.split('\n').let { if (it.last().isEmpty()) it.dropLast(1) else it }.map { it.trimEnd('\r') }
        val kept = lines.filter { l -> LINK_KEYS.none { l.startsWith("$it: ") } }
        return (kept + linkLines(address, boxKey, counters)).joinToString("\n") + "\n"
    }
}
