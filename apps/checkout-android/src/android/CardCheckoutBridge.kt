package org.openbmt.checkout

/** Optional card hook. The base APK contains no Square SDK and returns no bridge. */
interface CardCheckoutBridge {
    /** Called only after the attempt and stable provider ID are durable in CheckoutLedger. */
    fun begin(attemptId: String, referenceId: String, amountMinor: Long, currency: String, listener: Listener)

    /** Looks up an existing attempt only. It must never show a new payment form or create a second charge. */
    fun reconcile(attemptId: String, referenceId: String, amountMinor: Long, currency: String, listener: Listener)

    interface Listener {
        /** A terminal or in-flight SDK observation, still interpreted by the ledger. */
        fun onEvidence(evidenceId: String, result: CardResult, amountMinor: Long)

        /** The SDK outcome is missing or uncertain; the attempt remains locked for reconciliation. */
        fun onUncertain()
    }
}
