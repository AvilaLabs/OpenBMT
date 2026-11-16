package org.openbmt.checkout

/** JVM harness tests for durable transitions; the coordinator owns compilation and execution. */
object CheckoutLedgerTest {
    @JvmStatic fun main(args: Array<String>) {
        bitcoinRestartCancelAndReorg()
        bitcoinCancellationAcknowledgementRace()
        lateCanceledPaymentReviewAndOverpayment()
        uncertainCardAndRefundEvidence()
        tenderExclusionAndQuoteFiatSnapshot()
        globalIdentifiersAndEvidenceBounds()
        refundSurvivesBitcoinReorgRestart()
        duplicateAndCorruptStorage()
    }

    private fun bitcoinRestartCancelAndReorg() {
        val storage = MemoryStorage()
        var ledger = CheckoutLedger(storage)
        ledger.createSale("sale-1", 2500, "USD", 10)
        val pending = ledger.beginBitcoinAttempt("sale-1", "btc-1", 41, 2500, 10000, "quote-1", 11)
        check(pending.counterIndex == 41L && pending.expectedSats == 10000L)
        ledger.requestCancellation("sale-1", "btc-1")
        ledger = CheckoutLedger(storage) // app/process restart
        val arrived = ledger.recordBitcoinEvidence("sale-1", "btc-1", "txout-1", "tx-1:0", 4000, 0, 1, 12)
        check(arrived.cancellationRequested && arrived.state == PaymentState.UNCONFIRMED && arrived.receivedSats == 4000L)
        val settledPartial = ledger.recordBitcoinEvidence("sale-1", "btc-1", "txout-1", "tx-1:0", 4000, 1, 1, 13)
        check(settledPartial.state == PaymentState.PARTIAL) // partial and confirmed are distinct dimensions
        val full = ledger.recordBitcoinEvidence("sale-1", "btc-1", "txout-2", "tx-2:0", 6000, 1, 1, 14)
        check(full.state == PaymentState.CONFIRMED && full.receivedSats == 10000L)
        val reorg = ledger.recordBitcoinEvidence("sale-1", "btc-1", "txout-1", "tx-1:0", 4000, 0, 1, 15, BitcoinChainState.REORGED)
        check(reorg.state == PaymentState.PARTIAL && reorg.receivedSats == 6000L)
        val recovered = CheckoutLedger(storage).getSale("sale-1")
        check(recovered.attempts.single().bitcoinEvidence.size == 2)
    }

    private fun uncertainCardAndRefundEvidence() {
        val storage = MemoryStorage()
        val ledger = CheckoutLedger(storage)
        ledger.createSale("sale-2", 5000, "USD", 20)
        ledger.beginCardAttempt("sale-2", "card-1", "square-sandbox", "stable-123", 5000)
        ledger.markCancellationUnknown("sale-2", "card-1")
        check(runCatching { ledger.beginBitcoinAttempt("sale-2", "btc-again", 42, 5000, 20000, "q", 21) }.isFailure)
        val approved = ledger.recordCardEvidence("sale-2", "card-1", "payment-1", "stable-123", CardResult.APPROVED, 5000, 22)
        check(approved.state == PaymentState.UNKNOWN)
        check(runCatching { ledger.beginCardAttempt("sale-2", "card-2", "square-sandbox", "stable-124", 5000) }.isFailure)
        ledger.recordCardEvidence("sale-2", "card-1", "payment-1", "stable-123", CardResult.COMPLETED, 5000, 23)
        ledger.createSale("sale-2b", 100, "USD", 24)
        ledger.beginCardAttempt("sale-2b", "card-2b", "square-sandbox", "stable-2b", 100)
        ledger.markCancellationUnknown("sale-2b", "card-2b")
        ledger.recordCardEvidence("sale-2b", "card-2b", "payment-2b", "stable-2b", CardResult.FAILED, 0, 25)
        check(ledger.beginCardAttempt("sale-2b", "card-2c", "square-sandbox", "stable-2c", 100).state == PaymentState.UNKNOWN)
        check(ledger.getSale("sale-2").paid)
        val requested = ledger.requestRefund("sale-2", "refund-1", "card-1", 3000, 0, 24)
        check(requested.state == RefundState.REQUESTED && requested.actuallyPaidMinor == 0L)
        val waiting = ledger.recordRefundEvidence("sale-2", "refund-1", "refund-ev-1", 0, 0, true, 25)
        check(waiting.state == RefundState.PENDING && waiting.actuallyPaidMinor == 0L)
        val paid = ledger.recordRefundEvidence("sale-2", "refund-1", "refund-ev-1", 2000, 0, false, 26)
        check(paid.state == RefundState.PARTIAL && paid.actuallyPaidMinor == 2000L)
        check(runCatching { ledger.recordRefundEvidence("sale-2", "refund-1", "refund-ev-2", 1500, 0, false, 27) }.isFailure)
        check(CheckoutLedger(storage).getSale("sale-2").refunds.single().state == RefundState.PARTIAL)
    }

    private fun bitcoinCancellationAcknowledgementRace() {
        val storage = MemoryStorage()
        val ledger = CheckoutLedger(storage)
        ledger.createSale("cancel-race", 100, "USD", 15)
        ledger.beginBitcoinAttempt("cancel-race", "btc-cancel-1", 700, 100, 1000, "quote-cancel-1", 16)
        val requested = ledger.requestCancellation("cancel-race", "btc-cancel-1")
        check(requested.state == PaymentState.AWAITING && !requested.cancellationAcknowledged)
        check(runCatching { ledger.beginBitcoinAttempt("cancel-race", "btc-cancel-2", 701, 100, 1000, "quote-cancel-2", 17) }.isFailure)
        val acknowledged = ledger.acknowledgeBitcoinCancellation("cancel-race", "btc-cancel-1")
        check(acknowledged.state == PaymentState.CANCELED && acknowledged.cancellationAcknowledged)
        ledger.beginBitcoinAttempt("cancel-race", "btc-cancel-2", 701, 100, 1000, "quote-cancel-2", 17)
        val late = ledger.recordBitcoinEvidence("cancel-race", "btc-cancel-1", "late-output", "late-tx:0", 1000, 0, 1, 18)
        check(late.state == PaymentState.UNCONFIRMED)
        check(ledger.getSale("cancel-race").attempts.size == 2)
    }

    private fun lateCanceledPaymentReviewAndOverpayment() {
        val storage = MemoryStorage()
        var ledger = CheckoutLedger(storage)
        ledger.createSale("late-replacement", 1000, "USD", 18)
        ledger.beginBitcoinAttempt("late-replacement", "btc-old", 710, 1000, 1000, "quote-old", 19)
        ledger.requestCancellation("late-replacement", "btc-old")
        ledger.acknowledgeBitcoinCancellation("late-replacement", "btc-old")
        ledger.beginCardAttempt("late-replacement", "card-new", "square-sandbox", "stable-new", 1000)
        ledger.recordCardEvidence("late-replacement", "card-new", "card-payment", "stable-new", CardResult.COMPLETED, 1000, 20)
        check(ledger.getSale("late-replacement").paid)
        ledger.recordBitcoinEvidence("late-replacement", "btc-old", "late-output", "late-tx:0", 1000, 1, 1, 21)
        val late = ledger.getSale("late-replacement")
        check(late.lateBitcoinReceipt && late.requiresReview && !late.paid && late.overpaidMinor == 1000L)
        ledger = CheckoutLedger(storage)
        val recovered = ledger.getSale("late-replacement")
        check(recovered.lateBitcoinReceipt && recovered.requiresReview && !recovered.paid && recovered.overpaidMinor == 1000L)

        ledger.createSale("overpaid-btc", 100, "USD", 22)
        ledger.beginBitcoinAttempt("overpaid-btc", "btc-overpaid", 711, 100, 100, "quote-overpaid", 23)
        ledger.recordBitcoinEvidence("overpaid-btc", "btc-overpaid", "overpay-output", "overpay-tx:0", 150, 1, 1, 24)
        val overpaid = ledger.getSale("overpaid-btc")
        check(overpaid.requiresReview && !overpaid.paid && overpaid.overpaidSats == 50L && overpaid.overpaidMinor == 0L)
    }

    private fun duplicateAndCorruptStorage() {
        val storage = MemoryStorage()
        var ledger = CheckoutLedger(storage)
        ledger.createSale("sale-3", 100, "USD", 30)
        ledger.beginCardAttempt("sale-3", "card-3", "square-sandbox", "stable-3", 100)
        ledger.recordCardEvidence("sale-3", "card-3", "payment-3", "stable-3", CardResult.FAILED, 0, 31)
        ledger = CheckoutLedger(storage)
        check(ledger.getSale("sale-3").attempts.single().state == PaymentState.CANCELED)
        check(runCatching { ledger.recordCardEvidence("sale-3", "card-3", "payment-3", "stable-3", CardResult.COMPLETED, 100, 31) }.isFailure)
        val validBytes = storage.bytes!!.copyOf()
        val stableAt = findBytes(validBytes, "stable-3".toByteArray())
        check(stableAt >= 0)
        val badBoolean = validBytes.copyOf().also { it[stableAt + "stable-3".length] = 2 }
        storage.bytes = badBoolean
        check(runCatching { CheckoutLedger(storage) }.isFailure)
        val badUtf8 = validBytes.copyOf().also { it[findBytes(it, "square-sandbox".toByteArray())] = 0xff.toByte() }
        storage.bytes = badUtf8
        check(runCatching { CheckoutLedger(storage) }.isFailure)
        storage.bytes = validBytes
        storage.failNextWrite = true
        check(runCatching { ledger.createSale("not-durable", 20, "USD", 32) }.isFailure)
        check(runCatching { ledger.getSale("sale-3") }.isFailure) // uncertain writes poison this instance
        check(CheckoutLedger(storage).getSale("sale-3").saleId == "sale-3")
        storage.bytes = byteArrayOf(0, 1, 2)
        check(runCatching { CheckoutLedger(storage) }.isFailure) // never silently reset corruption
    }

    private fun tenderExclusionAndQuoteFiatSnapshot() {
        val storage = MemoryStorage()
        val ledger = CheckoutLedger(storage)
        ledger.createSale("sale-4", 2500, "USD", 40)
        ledger.beginBitcoinAttempt("sale-4", "btc-4", 44, 2500, 10000, "quote-4", 41)
        val fullUnconfirmed = ledger.recordBitcoinEvidence("sale-4", "btc-4", "output-4", "tx-4:0", 10000, 0, 1, 42)
        check(fullUnconfirmed.state == PaymentState.UNCONFIRMED)
        check(ledger.getSale("sale-4").receivedMinor == 2500L && !ledger.getSale("sale-4").paid)
        check(runCatching { ledger.beginCardAttempt("sale-4", "card-4", "square-sandbox", "id-4", 2500) }.isFailure)
        check(runCatching { ledger.recordBitcoinEvidence("sale-4", "btc-4", "different-output-id", "tx-4:0", 10000, 1, 1, 43) }.isFailure)
        val confirmed = ledger.recordBitcoinEvidence("sale-4", "btc-4", "output-4", "tx-4:0", 10000, 1, 1, 44)
        check(confirmed.state == PaymentState.CONFIRMED && ledger.getSale("sale-4").paid)
    }

    private fun globalIdentifiersAndEvidenceBounds() {
        val storage = MemoryStorage()
        val ledger = CheckoutLedger(storage)
        ledger.createSale("sale-a", 1000, "USD", 50)
        ledger.createSale("sale-b", 1000, "USD", 51)
        ledger.beginBitcoinAttempt("sale-a", "btc-a", 501, 1000, 1000, "quote-a", 52)
        check(runCatching { ledger.beginBitcoinAttempt("sale-b", "btc-b", 501, 1000, 1000, "quote-b", 53) }.isFailure)
        ledger.beginCardAttempt("sale-b", "card-b", "square-sandbox", "global-card-id", 1000)
        ledger.recordCardEvidence("sale-b", "card-b", "pay-b", "global-card-id", CardResult.FAILED, 0, 54)
        ledger.createSale("sale-c", 1000, "USD", 55)
        check(runCatching { ledger.beginCardAttempt("sale-c", "card-c", "square-sandbox", "global-card-id", 1000) }.isFailure)

        ledger.createSale("sale-overflow", 10, "USD", 56)
        ledger.beginBitcoinAttempt("sale-overflow", "btc-overflow", 502, 10, Long.MAX_VALUE, "quote-overflow", 57)
        ledger.recordBitcoinEvidence("sale-overflow", "btc-overflow", "out-1", "tx-1:0", Long.MAX_VALUE, 0, 1, 58)
        check(runCatching { ledger.recordBitcoinEvidence("sale-overflow", "btc-overflow", "out-2", "tx-2:0", 1, 0, 1, 59) }.isFailure)
        check(ledger.getSale("sale-overflow").attempts.single().bitcoinEvidence.size == 1)
    }

    private fun refundSurvivesBitcoinReorgRestart() {
        val storage = MemoryStorage()
        var ledger = CheckoutLedger(storage)
        ledger.createSale("refund-reorg", 1000, "USD", 60)
        ledger.beginBitcoinAttempt("refund-reorg", "btc-refund", 800, 1000, 1000, "quote-refund", 61)
        ledger.recordBitcoinEvidence("refund-reorg", "btc-refund", "refund-output", "refund-tx:0", 1000, 1, 1, 62)
        ledger.requestRefund("refund-reorg", "refund-req", "btc-refund", 0, 500, 63)
        ledger.recordBitcoinEvidence("refund-reorg", "btc-refund", "refund-output", "refund-tx:0", 1000, 0, 1, 64, BitcoinChainState.REORGED)
        ledger = CheckoutLedger(storage)
        val recovered = ledger.getSale("refund-reorg")
        check(recovered.attempts.single().state == PaymentState.REORGED)
        check(recovered.refunds.single().state == RefundState.REQUESTED)
        check(recovered.requiresReview && !recovered.paid)
        check(runCatching { ledger.beginCardAttempt("refund-reorg", "card-after-reorg", "square-sandbox", "stable-after-reorg", 1000) }.isFailure)
    }

    private class MemoryStorage : DurableLedgerStorage {
        var bytes: ByteArray? = null
        var failNextWrite = false
        override fun read(): ByteArray? = bytes?.copyOf()
        override fun writeAtomically(bytes: ByteArray) {
            if (failNextWrite) { failNextWrite = false; this.bytes = bytes.copyOf(); error("simulated post-commit write failure") }
            this.bytes = bytes.copyOf()
        }
    }

    private fun findBytes(haystack: ByteArray, needle: ByteArray): Int {
        if (needle.isEmpty()) return 0
        for (start in 0..(haystack.size - needle.size)) {
            var match = true
            for (offset in needle.indices) if (haystack[start + offset] != needle[offset]) { match = false; break }
            if (match) return start
        }
        return -1
    }
}
