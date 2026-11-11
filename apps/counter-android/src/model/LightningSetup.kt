package org.openbmt.counter.app

import org.openbmt.counter.LnKind
import org.openbmt.counter.LnRequest
import org.openbmt.counter.LnStep
import org.openbmt.counter.LnTransport

/** What a manager typed or scanned, ready for the library's setup (CR-115). */
class LnInput(val kind: LnKind, val address: String, val secret: String, val cert: String)

/**
 * The Lightning screen's inputs and the test they start. The library reads
 * every field and says in words what is wrong with it; the app only checks
 * that a field was not left empty.
 */
object LightningSetup {
    /** The file the sealed connection is kept in, in `noBackupFilesDir`, and the Keystore key that seals it. */
    const val FILE = "lightning.obkl"
    const val KEY_ALIAS = "openbmt-counter-lightning-v1"

    /** The most steps a test may take before it is given up (LN-20 asks for one or two requests). */
    const val MAX_STEPS = 6

    /** LND: the `lndconnect://` text, scanned or pasted. */
    fun lnd(code: String): LnInput? = code.trim().takeIf { it.isNotEmpty() }?.let { LnInput(LnKind.Lnd, it, "", "") }

    /** Core Lightning: `host:port`, the rune, and the certificate as PEM or a SHA-256 fingerprint, one field each. */
    fun cln(address: String, rune: String, cert: String): LnInput? {
        val a = address.trim()
        val r = rune.trim()
        val c = cert.trim()
        return if (a.isEmpty() || r.isEmpty() || c.isEmpty()) null else LnInput(LnKind.Cln, a, r, c)
    }

    /**
     * Carries a test from its first request to the record to seal: each
     * request goes to [send], and its answer to [reply], which gives the next
     * request or the record. A request that got no answer goes to [failed],
     * which throws the words of why the test stopped. Null means the test
     * ended with neither a record nor a next request (nothing is saved).
     */
    fun drive(
        first: LnRequest,
        send: (LnRequest) -> LnOutcome,
        reply: (Long, Long, ByteArray) -> LnStep,
        failed: (Long, LnTransport) -> Unit,
    ): ByteArray? {
        var request = first
        for (step in 0 until MAX_STEPS) {
            when (val out = send(request)) {
                is LnOutcome.Failed -> {
                    failed(request.id, out.why)
                    return null
                }
                is LnOutcome.Answer -> {
                    val next = reply(request.id, out.status.toLong(), out.body)
                    next.record?.let { return it }
                    request = next.next ?: return null
                }
            }
        }
        return null
    }
}
