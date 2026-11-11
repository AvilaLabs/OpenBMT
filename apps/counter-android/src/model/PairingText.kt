package org.openbmt.counter.app

import org.openbmt.app.Format
import org.openbmt.counter.CounterNetwork
import org.openbmt.counter.StartConfig

/** The pairing text was not in the form this app reads. [message] says which line and why. */
class PairingFormatException(message: String) : Exception(message)

/**
 * What the counter was paired with, as the app keeps it: the fields the node
 * needs at each start (`StartConfig`), read from the pairing text the owner's
 * phone gives. No pairing format of the rules exists yet (the pairing
 * ceremony is not built), so this is the app's own: plain lines, so that a
 * file, pasted text and later a code read by a camera all carry the same
 * text.
 *
 * ```
 * openbmt-counter-pairing 1
 * network: mainnet
 * identity-secret: <64 hex digits>
 * owner-root: <64 hex digits>
 * approver-root: <64 hex digits>
 * wallet-checksum: <64 hex digits>
 * batch-min-count: <number>
 * settings: <hex of the signed settings>
 * first-batch: <hex of the first signed batch>     (optional)
 * peer: <host:port>                                (optional, up to 8)
 * ```
 *
 * Strict: this header, each of these keys once (`peer` up to 8 times), one
 * space after the colon, no other line, lines ended by a line feed (a final
 * one may be missing), the 32-byte values exactly 64 hex digits.
 */
class Pairing(
    val network: CounterNetwork,
    val identitySecret: ByteArray,
    val ownerRoot: ByteArray,
    val approverRoot: ByteArray,
    val walletChecksum: ByteArray,
    val batchMinCount: Long,
    val settings: ByteArray,
    val firstBatch: ByteArray?,
    val peers: List<String>,
) {
    /** What `start` is given for the folder [dir] at the time [now], through [proxy] or none. */
    fun startConfig(dir: String, now: Long, proxy: String?): StartConfig = StartConfig(
        dir = dir,
        network = network,
        now = now,
        identitySecret = identitySecret,
        ownerRoot = ownerRoot,
        approverRoot = approverRoot,
        walletChecksum = walletChecksum,
        batchMinCount = batchMinCount,
        settings = settings,
        firstBatch = firstBatch,
        proxy = proxy,
        fixedPeers = peers,
    )

    companion object {
        const val HEADER = "openbmt-counter-pairing 1"

        /** The most text read: a first batch of a few thousand addresses is well under it. */
        const val MAX_TEXT = 600_000
        const val MAX_PEERS = 8

        private val KEYS = listOf(
            "network", "identity-secret", "owner-root", "approver-root", "wallet-checksum",
            "batch-min-count", "settings", "first-batch", "peer",
        )

        fun parse(text: String): Pairing {
            if (text.length > MAX_TEXT) throw PairingFormatException("the text is longer than $MAX_TEXT characters")
            val lines = text.split('\n').let { if (it.last().isEmpty()) it.dropLast(1) else it }
            if (lines.firstOrNull()?.trimEnd('\r') != HEADER) {
                throw PairingFormatException("the first line is not \"$HEADER\"")
            }
            val values = HashMap<String, String>()
            val peers = ArrayList<String>()
            for ((n, raw) in lines.withIndex().drop(1)) {
                val line = raw.trimEnd('\r')
                val colon = line.indexOf(": ")
                if (colon <= 0) throw PairingFormatException("line ${n + 1} is not \"key: value\"")
                val key = line.substring(0, colon)
                val value = line.substring(colon + 2)
                if (key !in KEYS) throw PairingFormatException("line ${n + 1} has a key this app does not know")
                if (value.isEmpty() || value != value.trim()) throw PairingFormatException("line ${n + 1} has an empty value or spaces around it")
                if (key == "peer") {
                    if (peers.size >= MAX_PEERS) throw PairingFormatException("more than $MAX_PEERS peers")
                    if (!HostPort.valid(value)) throw PairingFormatException("line ${n + 1}: the peer is not host:port")
                    peers.add(value)
                } else if (values.put(key, value) != null) {
                    throw PairingFormatException("line ${n + 1} repeats $key")
                }
            }
            fun need(key: String): String = values[key] ?: throw PairingFormatException("the line \"$key\" is missing")
            fun key32(key: String): ByteArray {
                val v = need(key)
                if (v.length != 64) throw PairingFormatException("$key is not 64 hex digits")
                return Format.fromHex(v) ?: throw PairingFormatException("$key is not hex")
            }
            val network = when (need("network")) {
                "mainnet" -> CounterNetwork.Mainnet
                "testnet" -> CounterNetwork.Testnet
                "signet" -> CounterNetwork.Signet
                "regtest" -> CounterNetwork.Regtest
                else -> throw PairingFormatException("the network is not mainnet, testnet, signet or regtest")
            }
            val batchMin = Format.toAmount(need("batch-min-count"))
                ?: throw PairingFormatException("batch-min-count is not a number")
            if (batchMin > 0xffffffffL) throw PairingFormatException("batch-min-count is too large")
            val settings = Format.fromHex(need("settings")) ?: throw PairingFormatException("settings is not hex")
            val first = values["first-batch"]?.let { Format.fromHex(it) ?: throw PairingFormatException("first-batch is not hex") }
            return Pairing(
                network, key32("identity-secret"), key32("owner-root"), key32("approver-root"),
                key32("wallet-checksum"), batchMin, settings, first, peers,
            )
        }
    }
}

/** A `host:port` for a proxy or a peer. */
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
