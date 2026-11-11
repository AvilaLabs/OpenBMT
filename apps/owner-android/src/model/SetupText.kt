package org.openbmt.owner.app

import org.openbmt.app.Format
import org.openbmt.owner.LinkConfig
import org.openbmt.owner.OwnerNetwork
import org.openbmt.owner.Route
import org.openbmt.owner.SetupFields
import org.openbmt.owner.StartConfig

/** The setup text was not in the form this app reads. [message] says which line and why. */
class SetupFormatException(message: String) : Exception(message)

/** How the link reaches the relay, as the setup text names it. */
sealed class RelayRoute {
    /** An address on the local network, `host:port`. */
    class Direct(val addr: String) : RelayRoute()

    /** A Tor onion service by name; the app gives the proxy. */
    class Onion(val host: String, val port: Int) : RelayRoute()
}

/** The link to the counters through a relay, as the setup text holds it. */
class SetupLink(
    val route: RelayRoute,
    val relayKey: ByteArray,
    val counters: List<ByteArray>,
    val heartbeatEverySecs: Long,
    val checkInGraceSecs: Long,
)

/**
 * What the owner phone is set up with, as the app keeps it: the fields the
 * node needs at each start (`StartConfig`), read from the setup text. The
 * key ceremony on this phone makes it with [format]; the text can also be
 * pasted or read from a file. It is the app's own format: plain lines, so
 * that a file and pasted text carry the same text.
 *
 * ```
 * openbmt-owner-setup 1
 * network: mainnet
 * owner-secret: <64 hex digits>
 * approver-root: <64 hex digits>
 * wallet: <the wallet's descriptor, one line>
 * key1-account: <156 hex digits: the 78 bytes of Key 1's account key>
 * key3-xpub: <Key 3's public key>                     (optional)
 * batch-min-count: <number>
 * refund-depth: <number>
 * birthday: <number>
 * peer: <host:port>                                   (optional, up to 8)
 * relay-route: direct <host:port> | onion <name>:<port>   (optional)
 * relay-key: <64 hex digits>                          (with relay-route)
 * counter: <64 hex digits>                            (optional, up to 16, with relay-route; until settings)
 * heartbeat-every: <seconds>                          (optional)
 * check-in-grace: <seconds>                           (optional)
 * ```
 *
 * The text holds the owner's secret and Key 1's account secret, and nothing
 * of Key 2 or Key 3 but Key 3's public key: no line of this form can carry
 * another key (OA-08). Strict: this header, each key once (`peer` up to 8
 * times, `counter` up to 16), one space after the colon, no other line, lines
 * ended by a line feed (a final one may be missing), the 32-byte values
 * exactly 64 hex digits. Only the shape is checked here; the node judges
 * what the values mean when it starts (OA-07).
 */
class OwnerSetup(
    val network: OwnerNetwork,
    val ownerSecret: ByteArray,
    val approverRoot: ByteArray,
    val wallet: String,
    val key1Account: ByteArray,
    val key3Xpub: String?,
    val batchMinCount: Long,
    val refundDepth: Long,
    val birthday: Long,
    val peers: List<String>,
    val link: SetupLink?,
) {
    /**
     * What `start` is given for the folder [dir] at the time [now], through
     * the Tor proxy [proxy]: the node's peers, the sending of payments and an
     * onion route all use it, and `withoutTor` is never set (OA-05).
     */
    fun startConfig(dir: String, now: Long, proxy: String): StartConfig = StartConfig(
        dir = dir,
        network = network,
        now = now,
        ownerSecret = ownerSecret,
        approverRoot = approverRoot,
        wallet = wallet,
        key1Account = key1Account,
        key3Xpub = key3Xpub,
        batchMinCount = batchMinCount,
        refundDepth = refundDepth,
        birthday = birthday,
        proxy = proxy,
        fixedPeers = peers,
        broadcastProxy = proxy,
        withoutTor = false,
        link = link?.let {
            LinkConfig(
                route = when (val r = it.route) {
                    is RelayRoute.Direct -> Route.Direct(r.addr)
                    is RelayRoute.Onion -> Route.Onion(proxy, r.host, r.port.toLong())
                },
                relayKey = it.relayKey,
                counters = it.counters,
                heartbeatEverySecs = it.heartbeatEverySecs,
                checkInGraceSecs = it.checkInGraceSecs,
            )
        },
    )

    companion object {
        const val HEADER = "openbmt-owner-setup 1"

        /** The most text read. */
        const val MAX_TEXT = 8_000
        const val MAX_PEERS = 8
        const val MAX_COUNTERS = 16
        private const val ACCOUNT_KEY_HEX = 156

        private val KEYS = listOf(
            "network", "owner-secret", "approver-root", "wallet", "key1-account", "key3-xpub",
            "batch-min-count", "refund-depth", "birthday", "peer", "relay-route", "relay-key",
            "counter", "heartbeat-every", "check-in-grace",
        )

        /**
         * The setup text of the values the library gives at the end of a key ceremony: the text [parse] reads
         * and the paste path saves. A ceremony makes no peers and no link, so the text has none.
         */
        fun format(f: SetupFields): String {
            fun hex(bytes: ByteArray) = bytes.joinToString("") { "%02x".format(it) }
            val lines = ArrayList<String>()
            lines.add(HEADER)
            lines.add(
                "network: " + when (f.network) {
                    OwnerNetwork.Mainnet -> "mainnet"
                    OwnerNetwork.Testnet -> "testnet"
                    OwnerNetwork.Signet -> "signet"
                    OwnerNetwork.Regtest -> "regtest"
                },
            )
            lines.add("owner-secret: ${hex(f.ownerSecret)}")
            lines.add("approver-root: ${hex(f.approverRoot)}")
            lines.add("wallet: ${f.wallet}")
            lines.add("key1-account: ${hex(f.key1Account)}")
            f.key3Xpub?.let { lines.add("key3-xpub: $it") }
            lines.add("batch-min-count: ${f.batchMinCount}")
            lines.add("refund-depth: ${f.refundDepth}")
            lines.add("birthday: ${f.birthday}")
            return lines.joinToString("\n") + "\n"
        }

        fun parse(text: String): OwnerSetup {
            if (text.length > MAX_TEXT) throw SetupFormatException("the text is longer than $MAX_TEXT characters")
            val lines = text.split('\n').let { if (it.last().isEmpty()) it.dropLast(1) else it }
            if (lines.firstOrNull()?.trimEnd('\r') != HEADER) {
                throw SetupFormatException("the first line is not \"$HEADER\"")
            }
            val values = HashMap<String, String>()
            val peers = ArrayList<String>()
            val counters = ArrayList<String>()
            for ((n, raw) in lines.withIndex().drop(1)) {
                val line = raw.trimEnd('\r')
                val colon = line.indexOf(": ")
                if (colon <= 0) throw SetupFormatException("line ${n + 1} is not \"key: value\"")
                val key = line.substring(0, colon)
                val value = line.substring(colon + 2)
                if (key !in KEYS) throw SetupFormatException("line ${n + 1} has a key this app does not know")
                if (value.isEmpty() || value != value.trim()) {
                    throw SetupFormatException("line ${n + 1} has an empty value or spaces around it")
                }
                when (key) {
                    "peer" -> {
                        if (peers.size >= MAX_PEERS) throw SetupFormatException("more than $MAX_PEERS peers")
                        if (!HostPort.valid(value)) throw SetupFormatException("line ${n + 1}: the peer is not host:port")
                        peers.add(value)
                    }
                    "counter" -> {
                        if (counters.size >= MAX_COUNTERS) throw SetupFormatException("more than $MAX_COUNTERS counters")
                        if (value in counters) throw SetupFormatException("line ${n + 1} repeats a counter")
                        counters.add(value)
                    }
                    else -> if (values.put(key, value) != null) throw SetupFormatException("line ${n + 1} repeats $key")
                }
            }
            fun need(key: String): String = values[key] ?: throw SetupFormatException("the line \"$key\" is missing")
            fun hex(key: String, value: String, digits: Int): ByteArray {
                if (value.length != digits) throw SetupFormatException("$key is not $digits hex digits")
                return Format.fromHex(value) ?: throw SetupFormatException("$key is not hex")
            }
            fun number(key: String, max: Long): Long {
                val v = Format.toAmount(need(key)) ?: throw SetupFormatException("$key is not a number")
                if (v > max) throw SetupFormatException("$key is too large")
                return v
            }
            val network = when (need("network")) {
                "mainnet" -> OwnerNetwork.Mainnet
                "testnet" -> OwnerNetwork.Testnet
                "signet" -> OwnerNetwork.Signet
                "regtest" -> OwnerNetwork.Regtest
                else -> throw SetupFormatException("the network is not mainnet, testnet, signet or regtest")
            }
            val wallet = need("wallet")
            if (wallet.length > 2_000 || wallet.any { Character.isWhitespace(it) || Character.isISOControl(it) }) {
                throw SetupFormatException("the wallet is not one line of text without spaces")
            }
            val key3 = values["key3-xpub"]
            if (key3 != null && (key3.length > 200 || key3.any { Character.isWhitespace(it) || Character.isISOControl(it) })) {
                throw SetupFormatException("key3-xpub is not one word")
            }
            val link = linkOf(values, counters)
            return OwnerSetup(
                network = network,
                ownerSecret = hex("owner-secret", need("owner-secret"), 64),
                approverRoot = hex("approver-root", need("approver-root"), 64),
                wallet = wallet,
                key1Account = hex("key1-account", need("key1-account"), ACCOUNT_KEY_HEX),
                key3Xpub = key3,
                batchMinCount = number("batch-min-count", 0xffffffffL),
                refundDepth = number("refund-depth", 0xffffffffL),
                birthday = number("birthday", 0xffffffffL),
                peers = peers,
                link = link,
            )
        }

        private fun linkOf(values: Map<String, String>, counters: List<String>): SetupLink? {
            val route = values["relay-route"]
            val relay = values["relay-key"]
            if (route == null) {
                if (relay != null || counters.isNotEmpty() || "heartbeat-every" in values || "check-in-grace" in values) {
                    throw SetupFormatException("the link lines need a \"relay-route\" line")
                }
                return null
            }
            if (relay == null) throw SetupFormatException("the line \"relay-key\" is missing")
            val space = route.indexOf(' ')
            if (space <= 0) throw SetupFormatException("relay-route is not \"direct host:port\" or \"onion name:port\"")
            val kind = route.substring(0, space)
            val where = route.substring(space + 1)
            val hp = HostPort.split(where) ?: throw SetupFormatException("the relay address is not host:port")
            val parsed = when (kind) {
                "direct" -> RelayRoute.Direct(where)
                "onion" -> RelayRoute.Onion(hp.first, hp.second)
                else -> throw SetupFormatException("relay-route is not \"direct host:port\" or \"onion name:port\"")
            }
            fun key(name: String, v: String): ByteArray {
                if (v.length != 64) throw SetupFormatException("$name is not 64 hex digits")
                return Format.fromHex(v) ?: throw SetupFormatException("$name is not hex")
            }
            fun secs(name: String): Long {
                val v = values[name] ?: return 0
                return Format.toAmount(v) ?: throw SetupFormatException("$name is not a number")
            }
            return SetupLink(
                parsed, key("relay-key", relay), counters.map { key("counter", it) },
                secs("heartbeat-every"), secs("check-in-grace"),
            )
        }
    }
}

/** A `host:port` for a proxy, a peer or a relay. (A copy of the counter app's.) */
object HostPort {
    /** The host is 1 to 253 characters of letters, digits, dots, hyphens, or a bracketed IPv6 address; the port is 1 to 65535. */
    fun valid(text: String): Boolean = split(text) != null

    fun split(text: String): Pair<String, Int>? {
        val colon = text.lastIndexOf(':')
        if (colon <= 0 || colon == text.length - 1) return null
        val host = text.substring(0, colon)
        val portText = text.substring(colon + 1)
        if (portText.length > 5 || !portText.all { it in '0'..'9' }) return null
        val port = portText.toInt()
        if (port !in 1..65535) return null
        val hostOk = if (host.startsWith("[")) {
            host.endsWith("]") && host.length > 2 && host.substring(1, host.length - 1).all { it in '0'..'9' || it in 'a'..'f' || it in 'A'..'F' || it == ':' || it == '.' }
        } else {
            host.length <= 253 && host.all { it in 'a'..'z' || it in 'A'..'Z' || it in '0'..'9' || it == '.' || it == '-' }
        }
        return if (hostOk) host to port else null
    }
}
