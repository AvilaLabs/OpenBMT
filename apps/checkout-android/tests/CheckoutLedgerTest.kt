package org.openbmt.checkout

import java.io.ByteArrayOutputStream
import java.io.DataOutputStream
import java.nio.file.Files
import java.security.MessageDigest
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicReference

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
        immutableBasketAndLegacyMigration()
        pricedSaleRoundTripAndV6Upgrade()
        authenticatedSaleCommandIsAtomicAndIdempotent()
        replaySurvivesAuthorizedPolicyChanges()
        ledgerCompareAndSwapRejectsStaleInstances()
        strictSaleCommandDecodeRejectsCorruption()
        migratesV8SaleCommandDigestsWithoutChangingOnRead()
        migratesV9CoverageReadOnlyAndTracksTypedCash()
        authenticatedCashCommandIsReplaySafeAndVersionBound()
        cashIntentSurvivesRestartAndFinalizesOnce()
        cashIntentReconcilesLateTenderEvidence()
        stableSaleSerializesLocalCommit()
        cashAcknowledgmentAndMigration()
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
        check(runCatching { ledger.canRecordCashPayment("sale-3") }.isFailure) // eligibility must not hide uncertain storage as false
        check(CheckoutLedger(storage).getSale("sale-3").saleId == "sale-3")
        storage.bytes = byteArrayOf(0, 1, 2)
        check(runCatching { CheckoutLedger(storage) }.isFailure) // never silently reset corruption
    }

    private fun cashAcknowledgmentAndMigration() {
        val storage = MemoryStorage()
        var ledger = CheckoutLedger(storage)
        ledger.createSale("cash-exact", 725, "USD", 100)
        check(ledger.canRecordCashPayment("cash-exact"))
        val exact = ledger.recordCashPayment("cash-exact", "cash-1", 725, 101)
        check(exact == CashPayment("cash-1", 725, 725, 0, 101))
        check(ledger.getSale("cash-exact").let { it.paid && it.receivedMinor == 725L && it.remainingMinor == 0L && it.cashPayments.single() == exact })
        check(!ledger.canRecordCashPayment("cash-exact"))
        check(ledger.recordCashPayment("cash-exact", "cash-1", 725, 101) == exact) // stable retry after completion
        check(runCatching { ledger.recordCashPayment("cash-exact", "cash-1", 726, 101) }.isFailure)
        check(runCatching { ledger.recordCashPayment("cash-exact", "cash-1", 725, 102) }.isFailure)
        check(runCatching { ledger.recordCashPayment("cash-exact", "cash-2", 1, 102) }.isFailure)
        check(runCatching { ledger.recordCashPayment("cash-exact", "bad-id", 0, 102) }.isFailure)
        check(runCatching { ledger.recordCashPayment("cash-exact", "bad-time", 725, -1) }.isFailure)
        val restarted = CheckoutLedger(storage)
        check(restarted.getSale("cash-exact").cashPayments.single() == exact)
        check(restarted.recordCashPayment("cash-exact", "cash-1", 725, 101) == exact)

        ledger.createSale("cash-change", 1300, "USD", 103)
        val change = ledger.recordCashPayment("cash-change", "cash-2", 2000, 104)
        check(change == CashPayment("cash-2", 1300, 2000, 700, 104))
        check(ledger.getSale("cash-change").let { it.paid && it.receivedMinor == 1300L && it.overpaidMinor == 0L && it.cashPayments.single().changeMinor == 700L })
        ledger.createSale("cash-after-partial-card", 1000, "USD", 104)
        check(runCatching { ledger.recordCashPayment("cash-after-partial-card", "cash-2", 2000, 104) }.isFailure) // payment ids are global across sales
        ledger.beginCardAttempt("cash-after-partial-card", "partial-card", "square", "partial-stable", 1000)
        check(!ledger.canRecordCashPayment("cash-after-partial-card"))
        ledger.recordCardEvidence("cash-after-partial-card", "partial-card", "partial-payment", "partial-stable", CardResult.COMPLETED, 300, 105)
        check(ledger.canRecordCashPayment("cash-after-partial-card"))
        val balanceCash = ledger.recordCashPayment("cash-after-partial-card", "balance-cash", 800, 106)
        check(balanceCash == CashPayment("balance-cash", 700, 800, 100, 106))
        check(ledger.getSale("cash-after-partial-card").let { it.paid && it.receivedMinor == 1000L && it.attempts.single().receivedMinor == 300L })

        ledger.createSale("cash-under", 500, "USD", 105)
        check(runCatching { ledger.recordCashPayment("cash-under", "under", 499, 106) }.isFailure)
        check(ledger.getSale("cash-under").remainingMinor == 500L)
        ledger.createSale("cash-overflow", Long.MAX_VALUE, "USD", 107)
        check(ledger.recordCashPayment("cash-overflow", "max-cash", Long.MAX_VALUE, 108).changeMinor == 0L)
        check(ledger.getSale("cash-overflow").let { it.paid && it.receivedMinor == Long.MAX_VALUE })

        ledger.createSale("cash-non-usd", 100, "CAD", 109)
        check(runCatching { ledger.recordCashPayment("cash-non-usd", "cad-cash", 100, 110) }.isFailure)
        ledger.createSale("cash-card-unknown", 100, "USD", 111)
        ledger.beginCardAttempt("cash-card-unknown", "unknown-card", "square", "unknown-stable", 100)
        check(!ledger.canRecordCashPayment("cash-card-unknown"))
        check(runCatching { ledger.recordCashPayment("cash-card-unknown", "blocked-card", 100, 112) }.isFailure)
        ledger.createSale("cash-btc-active", 100, "USD", 113)
        ledger.beginBitcoinAttempt("cash-btc-active", "active-btc", 991, 100, 100, "active-q", 114)
        check(!ledger.canRecordCashPayment("cash-btc-active"))
        check(runCatching { ledger.recordCashPayment("cash-btc-active", "blocked-btc", 100, 115) }.isFailure)

        ledger.createSale("cash-late-btc", 100, "USD", 116)
        ledger.beginBitcoinAttempt("cash-late-btc", "late-btc", 992, 100, 100, "late-q", 117)
        ledger.requestCancellation("cash-late-btc", "late-btc")
        ledger.acknowledgeBitcoinCancellation("cash-late-btc", "late-btc")
        ledger.recordCashPayment("cash-late-btc", "before-late", 100, 118)
        ledger.recordBitcoinEvidence("cash-late-btc", "late-btc", "late-output", "late-tx:0", 100, 1, 1, 119)
        check(ledger.getSale("cash-late-btc").let { it.requiresReview && !it.paid && it.lateBitcoinReceipt && it.overpaidMinor == 100L })
        check(runCatching { ledger.recordCashPayment("cash-late-btc", "after-late", 1, 120) }.isFailure)

        val legacy = MemoryStorage().also { it.bytes = legacySaleBytes(4, "legacy-v4") }
        val beforeRead = legacy.bytes!!.copyOf()
        val oldLedger = CheckoutLedger(legacy)
        check(oldLedger.getSale("legacy-v4").let { it.paid && it.cashPayments.isEmpty() })
        check(legacy.bytes!!.contentEquals(beforeRead)) // opening and reading legacy data never rewrites it
        oldLedger.createSale("after-v4", 25, "USD", 120, listOf(SaleItem("water", "Water", 25, 1)))
        val afterMigration = CheckoutLedger(legacy)
        check(afterMigration.getSale("legacy-v4").let { it.paid && it.cashPayments.isEmpty() })
        check(afterMigration.getSale("after-v4").items.single().productId == "water")
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

    private fun immutableBasketAndLegacyMigration() {
        val storage = MemoryStorage()
        val ledger = CheckoutLedger(storage)
        val mutableCart = mutableListOf(SaleItem("tea", "Green tea", 250, 2), SaleItem("mug", "Small mug", 500, 1))
        val sale = ledger.createSale("basket", 1000, "USD", 70, mutableCart)
        check(sale.items == mutableCart && sale.items.size == 2)
        ledger.beginCardAttempt("basket", "basket-card", "square-sandbox", "basket-stable", 1000)
        ledger.recordCardEvidence("basket", "basket-card", "basket-payment", "basket-stable", CardResult.COMPLETED, 1000, 71)
        ledger.requestRefund("basket", "basket-refund", "basket-card", 250, 0, 72)
        mutableCart[0] = SaleItem("tea", "Changed later", 1, 1)
        check(ledger.getSale("basket").items == listOf(SaleItem("tea", "Green tea", 250, 2), SaleItem("mug", "Small mug", 500, 1)))
        val exposed = ledger.getSale("basket").items as MutableList<SaleItem>
        exposed.clear()
        val restored = CheckoutLedger(storage).getSale("basket")
        check(restored.items.size == 2 && restored.refunds.single().state == RefundState.REQUESTED)
        check(runCatching { ledger.createSale("wrong-total", 999, "USD", 73, listOf(SaleItem("tea", "Green tea", 250, 2), SaleItem("mug", "Small mug", 500, 1))) }.isFailure)
        check(runCatching { ledger.createSale("overflow-price", Long.MAX_VALUE, "USD", 74, listOf(SaleItem("big", "Big", Long.MAX_VALUE, 2))) }.isFailure)
        check(runCatching { ledger.createSale("bad-quantity", 100, "USD", 75, listOf(SaleItem("tea", "Green tea", 100, 0))) }.isFailure)
        check(runCatching { ledger.createSale("too-many-items", 101, "USD", 76, List(101) { SaleItem("p$it", "Item", 1, 1) }) }.isFailure)
        check(runCatching { ledger.createSale("bad-name", 1, "USD", 77, listOf(SaleItem("tea", " ", 1, 1))) }.isFailure)
        check(runCatching { ledger.createSale("bad-unit", 1, "USD", 78, listOf(SaleItem("tea", "Tea", 0, 1))) }.isFailure)

        for (version in listOf(2, 3)) {
            val legacyStorage = MemoryStorage().also { it.bytes = legacySaleBytes(version, "legacy-v$version") }
            val migrated = CheckoutLedger(legacyStorage)
            check(migrated.getSale("legacy-v$version").let { it.items.isEmpty() && it.paid && it.attempts.single().state == PaymentState.CONFIRMED })
            migrated.createSale("after-migration-v$version", 25, "USD", 79, listOf(SaleItem("water", "Water", 25, 1)))
            val recovered = CheckoutLedger(legacyStorage)
            check(recovered.getSale("legacy-v$version").let { it.items.isEmpty() && it.paid && it.attempts.single().state == PaymentState.CONFIRMED })
            check(recovered.getSale("after-migration-v$version").items.single().productId == "water")
        }
        for ((case, chainState, confirmations) in listOf(
            Triple("unconfirmed", BitcoinChainState.ACTIVE, 0),
            Triple("reorged", BitcoinChainState.REORGED, 0),
        )) {
            val legacyStorage = MemoryStorage().also { it.bytes = legacySaleBytes(2, "legacy-v2-$case", chainState, confirmations) }
            val migrated = CheckoutLedger(legacyStorage)
            val oldSale = migrated.getSale("legacy-v2-$case")
            check(oldSale.attempts.single().bitcoinEvidence.single().historicalConfirmedSats == 0L)
            check(runCatching { migrated.requestRefund("legacy-v2-$case", "unproven-refund", "legacy-btc", 0, 500, 80) }.isFailure)
        }
        val unverifiableRefund = MemoryStorage().also {
            it.bytes = legacySaleBytes(2, "legacy-v2-refund", BitcoinChainState.REORGED, 0, refundRequest = true)
        }
        val originalBytes = unverifiableRefund.bytes!!.copyOf()
        val loadFailure = runCatching { CheckoutLedger(unverifiableRefund) }.exceptionOrNull()
        check(loadFailure?.message?.contains("legacy v2 refund cannot be verified") == true)
        check(unverifiableRefund.bytes!!.contentEquals(originalBytes)) // fail closed without rewriting the legacy file
    }

    private fun pricedSaleRoundTripAndV6Upgrade() {
        val lineInputs = listOf(
            CheckoutLineInput("latte-1", "latte", "Latte", 550, 2,
                listOf(SelectedModifier("oat", "Oat milk", 75))),
            CheckoutLineInput("bagel-1", "bagel", "Bagel", 450, 1),
            CheckoutLineInput("sample-1", "sample", "Free sample", 0, 1),
        )
        val pricing = CheckoutPricing.freeze(lineInputs, discountMinor = 125,
            tax = CheckoutTaxRule("sales-tax", "Sales tax", 825, TaxTreatment.EXCLUSIVE), tipMinor = 200)
        val storage = MemoryStorage()
        var ledger = CheckoutLedger(storage)
        ledger.initializeEvents()
        val sale = ledger.createPricedSale("priced", "USD", 90, pricing)
        check(sale.amountMinor == pricing.totalMinor && sale.pricing?.totalMinor == pricing.totalMinor)
        val initialPage = ledger.readEvents(ledger.eventSnapshot().epoch!!, 0, 10)
        check(initialPage.events.single().type == CheckoutEventType.SALE_CREATED)
        check(initialPage.events.single().saleVersion == 1L)
        val savedPricing = sale.pricing!!
        check(runCatching { (savedPricing.lines as MutableList).clear() }.isFailure)
        ledger = CheckoutLedger(storage)
        val recovered = ledger.getSale("priced")
        check(recovered.amountMinor == pricing.totalMinor && recovered.pricing?.lines == pricing.lines)
        check(recovered.pricing?.taxes == pricing.taxes)
        check(recovered.items == listOf(SaleItem("latte", "Latte", 625, 2), SaleItem("bagel", "Bagel", 450, 1), SaleItem("sample", "Free sample", 0, 1)))
        check(ledger.eventSnapshot().highWater == 1L) // opening/reloading a snapshot adds no event
        check(runCatching { ledger.createPricedSale("bad-currency", "SAT", 91, pricing) }.isFailure)
        val inconsistent = CheckoutPricingSnapshot(pricing.lines, pricing.subtotalMinor, pricing.discountMinor,
            pricing.taxableAfterDiscountMinor, pricing.taxes, pricing.taxMinor, pricing.tipMinor,
            Math.addExact(pricing.totalMinor, 1))
        check(runCatching { ledger.createPricedSale("bad-snapshot", "USD", 92, inconsistent) }.isFailure)
        val allFree = CheckoutPricing.freeze(listOf(CheckoutLineInput("free-line", "free", "Free", 0, 1)))
        check(runCatching { ledger.createPricedSale("all-free", "USD", 93, allFree) }.isFailure)
        check(runCatching { ledger.getSale("all-free") }.isFailure) // zero-total sale was never persisted

        val legacy = MemoryStorage().also { it.bytes = legacyV6WithCash() }
        val rawBefore = legacy.bytes!!.copyOf()
        val oldLedger = CheckoutLedger(legacy)
        val oldSale = oldLedger.getSale("legacy-v6")
        check(oldSale.amountMinor == 100L && oldSale.pricing == null && oldSale.cashPayments.single().changeMinor == 20L)
        check(legacy.bytes!!.contentEquals(rawBefore)) // v6 read stays read-only
        check(oldLedger.eventSnapshot().legacyHistoryUnattributed)
        oldLedger.createPricedSale("new-after-v6", "USD", 3, CheckoutPricing.freeze(
            listOf(CheckoutLineInput("cookie-line", "cookie", "Cookie", 125, 1))))
        val upgraded = CheckoutLedger(legacy)
        check(upgraded.getSale("legacy-v6").let { it.amountMinor == 100L && it.pricing == null && it.cashPayments.single().paymentId == "cash-v6" })
        check(upgraded.getSale("new-after-v6").pricing?.totalMinor == 125L)
        check(upgraded.getSale("legacy-v6").attempts.isEmpty())
        check(runCatching {
            upgraded.createPricedSale("bad-snapshot", "USD", 4,
                CheckoutPricing.freeze(listOf(CheckoutLineInput("overflow", "p", "Huge", Long.MAX_VALUE, 1)), tipMinor = 1))
        }.isFailure)
        check(ledger.eventSnapshot().legacyHistoryUnattributed)
    }

    private fun replaySurvivesAuthorizedPolicyChanges() {
        val scope = CheckoutAuthorityScope("merchant-replay", "store-replay", "location-replay", "register-replay", "device-replay")
        val authority = CheckoutAuthority(LedgerAuthorityStorage(), LedgerAuthorityClock(), defaultIterations = 1_000)
        val setup = authority.bootstrapManager(authority.prepareBootstrap(), scope, "Primary Manager", "123456".toCharArray())
        var primary = authority.authenticate(setup.managerActorId, "123456".toCharArray(), scope)
        val cashier = authority.createStaff(primary, scope, "Cashier", CheckoutAuthorityRole.CASHIER, "234567".toCharArray())
        primary = authority.authenticate(setup.managerActorId, "123456".toCharArray(), scope)
        val storage = MemoryStorage()
        val ledger = CheckoutLedger(storage, authority, scope)
        ledger.createSale(primary, "generation-replay", "generation-sale", 100, "USD", 20)
        val original = ledger.commandEvents().single().attribution!!

        authority.renameStaff(primary, scope, cashier.actorId, "Renamed Cashier") // unrelated policy generation change
        val refreshedPrimary = authority.authenticate(setup.managerActorId, "123456".toCharArray(), scope)
        ledger.createSale(refreshedPrimary, "generation-replay", "generation-sale", 100, "USD", 20)
        check(ledger.commandEvents().single().attribution == original)

        val secondary = authority.createStaff(refreshedPrimary, scope, "Secondary Manager", CheckoutAuthorityRole.MANAGER, "345678".toCharArray())
        val currentPrimary = authority.authenticate(setup.managerActorId, "123456".toCharArray(), scope)
        val secondaryManager = authority.authenticate(secondary.actorId, "345678".toCharArray(), scope)
        val secondStorage = MemoryStorage()
        val secondLedger = CheckoutLedger(secondStorage, authority, scope)
        secondLedger.createSale(secondaryManager, "role-replay", "role-sale", 200, "USD", 21)
        val committedAttribution = secondLedger.commandEvents().single().attribution!!
        authority.changeRole(currentPrimary, scope, secondary.actorId, CheckoutAuthorityRole.CASHIER)
        val secondaryCashier = authority.authenticate(secondary.actorId, "345678".toCharArray(), scope)
        secondLedger.createSale(secondaryCashier, "role-replay", "role-sale", 200, "USD", 21)
        check(secondLedger.commandEvents().single().attribution == committedAttribution)
        check(committedAttribution.role == CheckoutAuthorityRole.MANAGER)
        val refreshedPrimaryAfterRoleChange = authority.authenticate(setup.managerActorId, "123456".toCharArray(), scope)
        check(runCatching {
            secondLedger.createSale(refreshedPrimaryAfterRoleChange, "role-replay", "role-sale", 200, "USD", 21)
        }.isFailure) // same request and operation, wrong currently authorized actor
        check(runCatching {
            secondLedger.createSale(secondaryManager, "role-replay", "role-sale", 200, "USD", 21)
        }.isFailure) // role change invalidated the old session
    }

    private fun ledgerCompareAndSwapRejectsStaleInstances() {
        val directory = Files.createTempDirectory("checkout-ledger-cas-")
        try {
            val path = directory.resolve("ledger.obcl")
            val first = CheckoutLedger(NioLedgerStorage(path))
            val staleAbsent = CheckoutLedger(NioLedgerStorage(path))
            first.createSale("first-writer", 100, "USD", 1)
            val absentConflict = runCatching { staleAbsent.createSale("absent-racer", 200, "USD", 2) }.exceptionOrNull()
            check(absentConflict is CheckoutLedgerConcurrentWriteException)
            check(runCatching { staleAbsent.listSales() }.isFailure)
            check(CheckoutLedger(NioLedgerStorage(path)).listSales().map { it.saleId } == listOf("first-writer"))

            val current = CheckoutLedger(NioLedgerStorage(path))
            val stale = CheckoutLedger(NioLedgerStorage(path))
            current.createSale("second-writer", 300, "USD", 3)
            val staleConflict = runCatching { stale.createSale("stale-writer", 400, "USD", 4) }.exceptionOrNull()
            check(staleConflict is CheckoutLedgerConcurrentWriteException)
            val reopened = CheckoutLedger(NioLedgerStorage(path))
            check(reopened.listSales().map { it.saleId } == listOf("first-writer", "second-writer"))

            val initPath = directory.resolve("init-race.obcl")
            val initFirst = CheckoutLedger(NioLedgerStorage(initPath))
            val initStale = CheckoutLedger(NioLedgerStorage(initPath))
            check(initFirst.initializeEvents().initialized)
            check(runCatching { initStale.initializeEvents() }.exceptionOrNull() is CheckoutLedgerConcurrentWriteException)

            val migrationPath = directory.resolve("legacy.obcl")
            val legacyBytes = legacyV7WithCash()
            Files.write(migrationPath, legacyBytes)
            val migrating = CheckoutLedger(NioLedgerStorage(migrationPath))
            migrating.createSale("post-migration", 50, "USD", 5)
            check(readInt(Files.readAllBytes(migrationPath), 4) == 10)
            check(CheckoutLedger(NioLedgerStorage(migrationPath)).listSales().map { it.saleId } ==
                listOf("legacy-v7", "post-migration"))
        } finally {
            Files.walk(directory).use { paths ->
                paths.sorted(java.util.Comparator.reverseOrder<java.nio.file.Path>()).forEach { Files.deleteIfExists(it) }
            }
        }
    }

    private fun authenticatedSaleCommandIsAtomicAndIdempotent() {
        val scope = CheckoutAuthorityScope("merchant-ledger", "store-ledger", "location-ledger", "register-ledger", "device-ledger")
        val authorityClock = LedgerAuthorityClock()
        val authority = CheckoutAuthority(LedgerAuthorityStorage(), authorityClock, defaultIterations = 1_000)
        val draft = authority.prepareBootstrap()
        val manager = authority.bootstrapManager(draft, scope, "Ledger Manager", "123456".toCharArray())
        val session = authority.authenticate(manager.managerActorId, "123456".toCharArray(), scope)
        val storage = MemoryStorage()
        var ledger = CheckoutLedger(storage)
        ledger.bindCommandAuthority(authority, scope)
        check(runCatching { ledger.bindCommandAuthority(authority, scope.copy(registerId = "other-register")) }.isFailure)
        val items = listOf(SaleItem("tea", "Tea", 225, 2))
        val saved = ledger.createSale(session, "sale-op-1", "sale-authenticated", 450, "USD", 50, items)
        check(saved.items == items)
        val command = ledger.commandEvents().single()
        check(command.operationId == "sale-op-1" && command.eventType == "sale.create")
        check(command.origin == CheckoutLedgerEventOrigin.HUMAN && command.attribution != null)
        check(command.attribution!!.actorId == manager.managerActorId)
        check(command.attribution.capabilityId == CheckoutAuthorityCapability.SALE_CREATE.id)
        check(command.attribution.scope == scope && command.attribution.policyGeneration > 0)
        check(command.payloadDigest.size == 32 && command.commitIdentity == "sale-authenticated@1")
        check(!ledger.eventSnapshot().legacyHistoryUnattributed)
        val mismatchedScopeLedger = CheckoutLedger(storage)
        check(runCatching {
            mismatchedScopeLedger.bindCommandAuthority(authority, scope.copy(storeId = "other-store"))
        }.isFailure)

        val initializedLedger = CheckoutLedger(MemoryStorage(), authority, scope)
        initializedLedger.initializeEvents()
        initializedLedger.createSale(session, "sale-op-initialized", "sale-initialized", 25, "USD", 52)
        check(initializedLedger.commandEvents().single().commitIdentity == "sale-initialized@1")

        ledger = CheckoutLedger(storage, authority, scope)
        check(ledger.createSale(session, "sale-op-1", "sale-authenticated", 450, "USD", 50, items) == saved)
        check(ledger.commandEvents().size == 1) // replay finds the co-committed event and does not write again
        val committedBytes = storage.bytes!!.copyOf()
        check(runCatching {
            ledger.createSale(session, "sale-op-1", "sale-authenticated", 451, "USD", 50, items)
        }.isFailure)
        check(storage.bytes!!.contentEquals(committedBytes))

        val staleStorage = MemoryStorage()
        val staleLedger = CheckoutLedger(staleStorage, authority, scope)
        authority.lock()
        check(runCatching {
            staleLedger.createSale(session, "sale-op-stale", "sale-stale", 100, "USD", 50)
        }.isFailure)
        check(staleLedger.commandEvents().isEmpty() && staleLedger.listSales().isEmpty())
        val recoveredSession = authority.authenticate(manager.managerActorId, "123456".toCharArray(), scope)

        val uncertainStorage = MemoryStorage().also { it.failNextWrite = true }
        val first = CheckoutLedger(uncertainStorage, authority, scope)
        val uncertain = runCatching {
            first.createSale(recoveredSession, "sale-op-uncertain", "sale-uncertain", 100, "USD", 51)
        }.exceptionOrNull()
        check(uncertain is CheckoutLedgerWriteUncertainException)
        check(runCatching { first.getSale("sale-uncertain") }.isFailure)
        val recovered = CheckoutLedger(uncertainStorage, authority, scope)
        check(recovered.getSale("sale-uncertain").saleId == "sale-uncertain") // committed despite callback loss
        check(recovered.commandEvents().single().operationId == "sale-op-uncertain")
        check(recovered.createSale(recoveredSession, "sale-op-uncertain", "sale-uncertain", 100, "USD", 51).saleId == "sale-uncertain")
        check(recovered.commandEvents().single().operationId == "sale-op-uncertain")
        check(runCatching {
            recovered.createSale(recoveredSession, "sale-op-uncertain", "sale-uncertain", 101, "USD", 51)
        }.isFailure)

        val maximumSaleId = "s".repeat(160)
        val maximumIdStorage = MemoryStorage()
        val maximumIdLedger = CheckoutLedger(maximumIdStorage, authority, scope)
        maximumIdLedger.createSale(recoveredSession, "sale-op-max-id", maximumSaleId, 100, "USD", 51)
        val maximumIdReopened = CheckoutLedger(maximumIdStorage, authority, scope)
        check(maximumIdReopened.getSale(maximumSaleId).saleId == maximumSaleId)
        check(maximumIdReopened.commandEvents().single().commitIdentity == "$maximumSaleId@1")

        val beforeWrite = object : DurableLedgerStorage {
            private var bytes: ByteArray? = null
            override fun read(): ByteArray? = bytes?.copyOf()
            override fun writeAtomically(bytes: ByteArray) {
                check(bytes.isNotEmpty())
                error("simulated pre-write failure")
            }
        }
        val prewriteLedger = CheckoutLedger(beforeWrite, authority, scope)
        check(runCatching {
            prewriteLedger.createSale(recoveredSession, "sale-op-prewrite", "sale-prewrite", 100, "USD", 52)
        }.exceptionOrNull() is CheckoutLedgerWriteUncertainException)
        val prewriteReopened = CheckoutLedger(beforeWrite, authority, scope)
        check(prewriteReopened.listSales().isEmpty() && prewriteReopened.commandEvents().isEmpty())

        val legacyV7Storage = MemoryStorage().also { it.bytes = legacyV7WithCash() }
        val originalV7 = legacyV7Storage.bytes!!.copyOf()
        val legacyV7 = CheckoutLedger(legacyV7Storage)
        check(legacyV7.getSale("legacy-v7").cashPayments.single().changeMinor == 20L)
        check(legacyV7.eventSnapshot().legacyHistoryUnattributed && legacyV7.commandEvents().isEmpty())
        check(legacyV7Storage.bytes!!.contentEquals(originalV7)) // reading v7 does not rewrite it
        legacyV7.createSale("legacy-v7-followup", 50, "USD", 53)
        val migratedV7 = CheckoutLedger(legacyV7Storage)
        check(migratedV7.getSale("legacy-v7").cashPayments.single().paymentId == "cash-v7")
        check(migratedV7.getSale("legacy-v7-followup").saleId == "legacy-v7-followup")
        check(migratedV7.eventSnapshot().legacyHistoryUnattributed)
        check(migratedV7.commandEvents().isEmpty()) // no actor is inferred for v7 or compatibility writes
    }

    private fun strictSaleCommandDecodeRejectsCorruption() {
        val valid = twoSaleCommandLedgerBytes()
        fun rejected(bytes: ByteArray) = check(runCatching {
            CheckoutLedger(MemoryStorage().also { it.bytes = bytes })
        }.isFailure)

        val unsupportedType = valid.copyOf()
        replaceEncodedString(unsupportedType, "sale.create", "sale.broken")
        rejected(unsupportedType)

        val systemEvent = valid.copyOf()
        val firstOffsets = commandOffsets(systemEvent, "sale-op-1")
        writeInt(systemEvent, firstOffsets.origin, CheckoutLedgerEventOrigin.SYSTEM.ordinal)
        rejected(systemEvent)

        val missingTarget = valid.copyOf()
        val targetOffset = commandOffsets(missingTarget, "sale-op-1").target
        replaceEncodedStringAt(missingTarget, targetOffset, "sale-xxx")
        rejected(missingTarget)

        val badDigest = valid.copyOf()
        badDigest[commandOffsets(badDigest, "sale-op-1").digest] = (badDigest[commandOffsets(badDigest, "sale-op-1").digest].toInt() xor 1).toByte()
        rejected(badDigest)

        val forgedCompleteBaseline = valid.copyOf()
        // Two rows: count, then sale-one's encoded ID, version, and legacy flag.
        writeLong(forgedCompleteBaseline, forgedCompleteBaseline.size - 50 + 4 + encodeString("sale-one").size, 1L)
        rejected(forgedCompleteBaseline)

        val nonCanonicalCashCommit = run {
            val scope = CheckoutAuthorityScope("merchant-corrupt", "store-corrupt", "location-corrupt", "register-corrupt", "device-corrupt")
            val authority = CheckoutAuthority(LedgerAuthorityStorage(), LedgerAuthorityClock(), defaultIterations = 1_000)
            val manager = authority.bootstrapManager(authority.prepareBootstrap(), scope, "Corruption Manager", "123456".toCharArray())
            val session = authority.authenticate(manager.managerActorId, "123456".toCharArray(), scope)
            val storage = MemoryStorage()
            val ledger = CheckoutLedger(storage, authority, scope)
            ledger.createSale(session, "corrupt-sale-create", "commit-sale", 100, "USD", 50)
            recordCashWithIntent(ledger, session, "corrupt-cash-prepare", "commit-sale", "commit-cash", 100, 51, 52, 1, 100)
            val committed = checkNotNull(storage.bytes).copyOf()
            val commitOffset = commandOffsets(committed, checkNotNull(ledger.getCashPaymentIntent("commit-cash")).completeOperationId).commit
            replaceEncodedStringResizing(committed, commitOffset, "commit-sale@junk@3")
        }
        rejected(nonCanonicalCashCommit)

        val uninitializedBaseline = run {
            val storage = MemoryStorage()
            CheckoutLedger(storage).createSale("baseline-uninitialized", 100, "USD", 50)
            checkNotNull(storage.bytes).copyOf()
        }
        writeLong(uninitializedBaseline,
            uninitializedBaseline.size - (4 + encodeString("baseline-uninitialized").size + 8 + 1 + 4) + 4 + encodeString("baseline-uninitialized").size,
            1L)
        rejected(uninitializedBaseline)

        val duplicateOperation = valid.copyOf()
        replaceEncodedStringAt(duplicateOperation, findEncodedString(duplicateOperation, "sale-op-2"), "sale-op-1")
        rejected(duplicateOperation)

        val crossScope = valid.copyOf()
        val storeScope = encodeString("store-ledger")
        val firstStore = findBytes(crossScope, storeScope, 0)
        val secondStore = findBytes(crossScope, storeScope, firstStore + storeScope.size)
        val replacement = "store-fraud1".toByteArray(Charsets.UTF_8)
        check(replacement.size == storeScope.size - Int.SIZE_BYTES)
        replacement.copyInto(crossScope, secondStore + 4)
        rejected(crossScope)
    }

    private fun migratesV8SaleCommandDigestsWithoutChangingOnRead() {
        val encodedV10 = twoSaleCommandLedgerBytes()
        val v8Bytes = encodedV10.copyOfRange(0, encodedV10.size - 50)
        writeInt(v8Bytes, 4, 8)
        listOf(
            Triple("sale-op-1", "sale-one", 100L),
            Triple("sale-op-2", "sale-two", 200L),
        ).forEach { (operationId, saleId, amount) ->
            legacySaleDigest(saleId, amount, "USD", if (saleId == "sale-one") 50L else 51L)
                .copyInto(v8Bytes, commandOffsets(v8Bytes, operationId).digest)
        }
        val original = v8Bytes.copyOf()
        val storage = MemoryStorage().also { it.bytes = v8Bytes }
        val reopened = CheckoutLedger(storage)
        check(reopened.listSales().map { it.saleId } == listOf("sale-one", "sale-two"))
        check(reopened.commandEvents().map { it.operationId } == listOf("sale-op-1", "sale-op-2"))
        check(storage.bytes!!.contentEquals(original)) // v8 read is read-only

        val scope = CheckoutAuthorityScope("merchant-ledger", "store-ledger", "location-ledger", "register-ledger", "device-ledger")
        val authority = CheckoutAuthority(LedgerAuthorityStorage(), LedgerAuthorityClock(), defaultIterations = 1_000)
        val manager = authority.bootstrapManager(authority.prepareBootstrap(), scope, "Ledger Manager", "123456".toCharArray())
        val session = authority.authenticate(manager.managerActorId, "123456".toCharArray(), scope)
        reopened.bindCommandAuthority(authority, scope)
        reopened.createSale(session, "sale-op-3", "sale-three", 300, "USD", 52)
        check(readInt(checkNotNull(storage.bytes), 4) == 10)
        val migrated = CheckoutLedger(storage)
        check(migrated.commandEvents().map { it.operationId } == listOf("sale-op-1", "sale-op-2", "sale-op-3"))
        check(migrated.listSales().map { it.saleId } == listOf("sale-one", "sale-two", "sale-three"))
    }

    private fun migratesV9CoverageReadOnlyAndTracksTypedCash() {
        val encodedV10 = twoSaleCommandLedgerBytes()
        val v9 = encodedV10.copyOfRange(0, encodedV10.size - 50)
        writeInt(v9, 4, 9)
        val storage = MemoryStorage().also { it.bytes = v9 }
        val original = v9.copyOf()
        val ledger = CheckoutLedger(storage)
        check(ledger.listSales().map { it.saleId } == listOf("sale-one", "sale-two"))
        check(storage.bytes!!.contentEquals(original)) // v9 baseline migration is read-only

        val scope = CheckoutAuthorityScope("merchant-ledger", "store-ledger", "location-ledger", "register-ledger", "device-ledger")
        val authority = CheckoutAuthority(LedgerAuthorityStorage(), LedgerAuthorityClock(), defaultIterations = 1_000)
        val manager = authority.bootstrapManager(authority.prepareBootstrap(), scope, "Ledger Manager", "123456".toCharArray())
        val session = authority.authenticate(manager.managerActorId, "123456".toCharArray(), scope)
        ledger.bindCommandAuthority(authority, scope)
        val result = recordCashWithIntent(ledger, session, "cash-op-1", "sale-one", "cash-payment-1",
            120, 59, 60, expectedSaleVersion = 1, expectedRemainingMinor = 100)
        check(result.disposition == CheckoutLedgerCommandDisposition.NEW_COMMIT)
        check(readInt(checkNotNull(storage.bytes), 4) == 10)

        val reopened = CheckoutLedger(storage)
        check(reopened.commandEvents().size == 4)
        val replayLedger = CheckoutLedger(storage, authority, scope)
        val replay = recordCashWithIntent(replayLedger, session, "cash-op-1", "sale-one", "cash-payment-1",
            120, 59, 60, expectedSaleVersion = 1, expectedRemainingMinor = 100)
        check(replay.disposition == CheckoutLedgerCommandDisposition.REPLAY)
        check(replay.value == result.value)
        check(replayLedger.commandEvents().size == 4)
    }

    private fun authenticatedCashCommandIsReplaySafeAndVersionBound() {
        val scope = CheckoutAuthorityScope("merchant-cash", "store-cash", "location-cash", "register-cash", "device-cash")
        val authority = CheckoutAuthority(LedgerAuthorityStorage(), LedgerAuthorityClock(), defaultIterations = 1_000)
        val manager = authority.bootstrapManager(authority.prepareBootstrap(), scope, "Cash Manager", "123456".toCharArray())
        val session = authority.authenticate(manager.managerActorId, "123456".toCharArray(), scope)
        val storage = MemoryStorage()
        var ledger = CheckoutLedger(storage, authority, scope)
        ledger.createSale(session, "cash-sale-create", "cash-sale", 100, "USD", 50)
        check(ledger.eventSnapshot().saleVersions["cash-sale"] == 1L)

        val prepared = ledger.prepareCashPayment(session, "cash-op-1", "cash-sale", "cash-payment-1",
            120, 59, expectedSaleVersion = 1, expectedRemainingMinor = 100)
        check(prepared.disposition == CheckoutLedgerCommandDisposition.NEW_COMMIT)
        check(ledger.getSale("cash-sale").pendingCashPayment == prepared.value)
        val result = ledger.completePreparedCashPayment(session, prepared.value.completeOperationId, "cash-sale", "cash-payment-1",
            60, expectedSaleVersion = 2, expectedRemainingMinor = 100)
        check(result.disposition == CheckoutLedgerCommandDisposition.NEW_COMMIT)
        check(result.value.amountMinor == 100L && result.value.tenderedMinor == 120L && result.value.changeMinor == 20L)
        val command = ledger.commandEvents().last()
        check(command.operationId == prepared.value.completeOperationId && command.eventType == "tender.cash.complete")
        check(command.attribution?.capabilityId == CheckoutAuthorityCapability.TENDER_BEGIN.id)
        check(command.attribution?.scope == scope && command.commitIdentity == "cash-sale@3")
        check(!ledger.eventSnapshot().legacyHistoryUnattributed)

        val committed = checkNotNull(storage.bytes).copyOf()
        ledger = CheckoutLedger(storage, authority, scope)
        val replay = ledger.completePreparedCashPayment(session, prepared.value.completeOperationId, "cash-sale", "cash-payment-1",
            60, expectedSaleVersion = 2, expectedRemainingMinor = 100)
        check(replay.disposition == CheckoutLedgerCommandDisposition.REPLAY && replay.value == result.value)
        check(storage.bytes!!.contentEquals(committed) && ledger.commandEvents().size == 3)
        check(runCatching {
            ledger.completePreparedCashPayment(session, prepared.value.completeOperationId, "cash-sale", "cash-payment-1",
                61, expectedSaleVersion = 2, expectedRemainingMinor = 100)
        }.isFailure)
        check(storage.bytes!!.contentEquals(committed))
        check(runCatching { CheckoutLedger(storage, authority, scope.copy(storeId = "other-store")) }.isFailure)
        check(storage.bytes!!.contentEquals(committed))

        val staleStorage = MemoryStorage()
        val staleLedger = CheckoutLedger(staleStorage, authority, scope)
        staleLedger.createSale(session, "stale-sale-create", "stale-cash-sale", 50, "USD", 70)
        val beforeStale = checkNotNull(staleStorage.bytes).copyOf()
        authority.lock()
        check(runCatching {
            staleLedger.prepareCashPayment(session, "stale-cash-op", "stale-cash-sale", "stale-cash-payment",
                50, 71, expectedSaleVersion = 1, expectedRemainingMinor = 50)
        }.isFailure)
        check(staleStorage.bytes!!.contentEquals(beforeStale))
        val freshSession = authority.authenticate(manager.managerActorId, "123456".toCharArray(), scope)

        val prewriteStorage = MemoryStorage()
        val prewriteLedger = CheckoutLedger(prewriteStorage, authority, scope)
        prewriteLedger.createSale(freshSession, "prewrite-sale-create", "prewrite-sale", 40, "USD", 80)
        val beforePrewrite = checkNotNull(prewriteStorage.bytes).copyOf()
        prewriteStorage.failBeforeWrite = true
        check(runCatching {
            prewriteLedger.prepareCashPayment(freshSession, "prewrite-cash-op", "prewrite-sale", "prewrite-cash",
                50, 81, expectedSaleVersion = 1, expectedRemainingMinor = 40)
        }.exceptionOrNull() is CheckoutLedgerWriteUncertainException)
        check(prewriteStorage.bytes!!.contentEquals(beforePrewrite))
        check(CheckoutLedger(prewriteStorage).getSale("prewrite-sale").cashPayments.isEmpty())

        val uncertainStorage = MemoryStorage()
        val uncertainLedger = CheckoutLedger(uncertainStorage, authority, scope)
        uncertainLedger.createSale(freshSession, "uncertain-sale-create", "uncertain-sale", 30, "USD", 90)
        uncertainStorage.failNextWrite = true
        check(runCatching {
            uncertainLedger.prepareCashPayment(freshSession, "uncertain-cash-op", "uncertain-sale", "uncertain-cash",
                35, 91, expectedSaleVersion = 1, expectedRemainingMinor = 30)
        }.exceptionOrNull() is CheckoutLedgerWriteUncertainException)
        val recovered = CheckoutLedger(uncertainStorage, authority, scope)
        check(recovered.getSale("uncertain-sale").cashPayments.isEmpty())
        check(recovered.getSale("uncertain-sale").pendingCashPayment?.paymentId == "uncertain-cash")
        check(recovered.commandEvents().last().operationId == "uncertain-cash-op")

        val damaged = checkNotNull(uncertainStorage.bytes).copyOf()
        damaged[commandOffsets(damaged, "uncertain-cash-op").digest] =
            (damaged[commandOffsets(damaged, "uncertain-cash-op").digest].toInt() xor 1).toByte()
        check(runCatching { CheckoutLedger(MemoryStorage().also { it.bytes = damaged }) }.isFailure)
    }

    private fun cashIntentSurvivesRestartAndFinalizesOnce() {
        val scope = CheckoutAuthorityScope("merchant-intent", "store-intent", "location-intent", "register-intent", "device-intent")
        val authority = CheckoutAuthority(LedgerAuthorityStorage(), LedgerAuthorityClock(), defaultIterations = 1_000)
        val manager = authority.bootstrapManager(authority.prepareBootstrap(), scope, "Intent Manager", "123456".toCharArray())
        val session = authority.authenticate(manager.managerActorId, "123456".toCharArray(), scope)
        val storage = MemoryStorage()
        var ledger = CheckoutLedger(storage, authority, scope)
        ledger.createSale(session, "intent-sale-create", "intent-sale", 100, "USD", 10)
        val initialVersion = ledger.eventSnapshot().saleVersions.getValue("intent-sale")
        val prepared = ledger.prepareCashPayment(session, "cash-prepare-1", "intent-sale", "cash-intent-1",
            125, 20, initialVersion, 100)
        check(prepared.disposition == CheckoutLedgerCommandDisposition.NEW_COMMIT)
        check(prepared.value.status == CashPaymentIntentStatus.PREPARED)
        check(prepared.value.completeOperationId.startsWith("cash-complete-"))
        check(prepared.value.cancelOperationId.startsWith("cash-cancel-"))
        check(ledger.getSale("intent-sale").pendingCashPayment == prepared.value)
        check(!ledger.canRecordCashPayment("intent-sale"))
        check(runCatching { ledger.beginCardAttempt("intent-sale", "blocked-card", "square", "blocked-stable", 100) }.isFailure)
        check(runCatching { ledger.recordCashPayment("intent-sale", "blocked-cash", 100, 21) }.isFailure)
        check(runCatching {
            ledger.prepareCashPayment(session, "cash-prepare-2", "intent-sale", "cash-intent-2", 100, 21,
                initialVersion + 1, 100)
        }.isFailure)

        ledger = CheckoutLedger(storage, authority, scope)
        val reopenedIntent = checkNotNull(ledger.getCashPaymentIntent("cash-intent-1"))
        check(reopenedIntent == prepared.value && ledger.getSale("intent-sale").pendingCashPayment == reopenedIntent)
        val prepareReplay = ledger.prepareCashPayment(session, "cash-prepare-1", "intent-sale", "cash-intent-1",
            125, 20, initialVersion, 100)
        check(prepareReplay.disposition == CheckoutLedgerCommandDisposition.REPLAY && prepareReplay.value == reopenedIntent)
        check(runCatching {
            ledger.prepareCashPayment(session, "cash-prepare-1", "intent-sale", "cash-intent-1", 126, 20,
                initialVersion, 100)
        }.isFailure)

        val afterPrepareVersion = ledger.eventSnapshot().saleVersions.getValue("intent-sale")
        val completeOp = reopenedIntent.completeOperationId
        val completed = ledger.completePreparedCashPayment(session, completeOp, "intent-sale", "cash-intent-1",
            30, afterPrepareVersion, 100)
        check(completed.disposition == CheckoutLedgerCommandDisposition.NEW_COMMIT)
        check(completed.value == CashPayment("cash-intent-1", 100, 125, 25, 30))
        check(ledger.getSale("intent-sale").let { it.cashPayments.single() == completed.value && it.pendingCashPayment == null })
        check(ledger.getCashPaymentIntent("cash-intent-1")?.status == CashPaymentIntentStatus.COMPLETED)
        ledger = CheckoutLedger(storage, authority, scope)
        val completedReplay = ledger.completePreparedCashPayment(session, completeOp, "intent-sale", "cash-intent-1",
            30, afterPrepareVersion, 100)
        check(completedReplay.disposition == CheckoutLedgerCommandDisposition.REPLAY && completedReplay.value == completed.value)
        check(ledger.getSale("intent-sale").cashPayments.size == 1)
        check(runCatching {
            ledger.completePreparedCashPayment(session, completeOp, "intent-sale", "cash-intent-1", 31, afterPrepareVersion, 100)
        }.isFailure)

        ledger.createSale(session, "cancel-sale-create", "cancel-sale", 100, "USD", 40)
        val cancelInitialVersion = ledger.eventSnapshot().saleVersions.getValue("cancel-sale")
        val cancelPrepared = ledger.prepareCashPayment(session, "cash-prepare-cancel", "cancel-sale", "cash-intent-cancel",
            100, 41, cancelInitialVersion, 100).value
        ledger = CheckoutLedger(storage, authority, scope)
        val cancelPreparedVersion = ledger.eventSnapshot().saleVersions.getValue("cancel-sale")
        val cancelOp = cancelPrepared.cancelOperationId
        val canceled = ledger.cancelPreparedCashPayment(session, cancelOp, "cancel-sale", "cash-intent-cancel",
            42, cancelPreparedVersion)
        check(canceled.disposition == CheckoutLedgerCommandDisposition.NEW_COMMIT)
        check(canceled.value.status == CashPaymentIntentStatus.CANCELED && canceled.value.canceledAt == 42L)
        check(ledger.getSale("cancel-sale").let { it.pendingCashPayment == null && it.cashPayments.isEmpty() })
        val cancelReplay = ledger.cancelPreparedCashPayment(session, cancelOp, "cancel-sale", "cash-intent-cancel",
            42, cancelPreparedVersion)
        check(cancelReplay.disposition == CheckoutLedgerCommandDisposition.REPLAY && cancelReplay.value == canceled.value)
        check(runCatching {
            ledger.cancelPreparedCashPayment(session, cancelOp, "cancel-sale", "cash-intent-cancel", 43, cancelPreparedVersion)
        }.isFailure)

        val afterCancelVersion = ledger.eventSnapshot().saleVersions.getValue("cancel-sale")
        val retry = ledger.prepareCashPayment(session, "cash-prepare-after-cancel", "cancel-sale", "cash-intent-retry",
            100, 44, afterCancelVersion, 100)
        check(retry.disposition == CheckoutLedgerCommandDisposition.NEW_COMMIT)
        check(ledger.getSale("cancel-sale").pendingCashPayment?.paymentId == "cash-intent-retry")
        val retryVersion = ledger.eventSnapshot().saleVersions.getValue("cancel-sale")
        val retryPaid = ledger.completePreparedCashPayment(session, retry.value.completeOperationId,
            "cancel-sale", "cash-intent-retry", 45, retryVersion, 100)
        check(retryPaid.value.amountMinor == 100L && ledger.getSale("cancel-sale").cashPayments.single() == retryPaid.value)
        val reopened = CheckoutLedger(storage)
        check(reopened.getCashPaymentIntent("cash-intent-cancel")?.status == CashPaymentIntentStatus.CANCELED)
        check(reopened.getCashPaymentIntent("cash-intent-retry")?.status == CashPaymentIntentStatus.COMPLETED)

        val uncertainStorage = MemoryStorage()
        val uncertainLedger = CheckoutLedger(uncertainStorage, authority, scope)
        uncertainLedger.createSale(session, "uncertain-intent-sale", "uncertain-intent-sale", 60, "USD", 50)
        val uncertainStartVersion = uncertainLedger.eventSnapshot().saleVersions.getValue("uncertain-intent-sale")
        val uncertainPrepared = uncertainLedger.prepareCashPayment(session, "uncertain-intent-prepare", "uncertain-intent-sale",
            "uncertain-intent-payment", 60, 51, uncertainStartVersion, 60).value
        val uncertainPrepareVersion = uncertainLedger.eventSnapshot().saleVersions.getValue("uncertain-intent-sale")
        uncertainStorage.failNextWrite = true
        check(runCatching {
            uncertainLedger.completePreparedCashPayment(session, uncertainPrepared.completeOperationId, "uncertain-intent-sale",
                "uncertain-intent-payment", 52, uncertainPrepareVersion, 60)
        }.exceptionOrNull() is CheckoutLedgerWriteUncertainException)
        val uncertainReopened = CheckoutLedger(uncertainStorage, authority, scope)
        check(uncertainReopened.getSale("uncertain-intent-sale").cashPayments.single().paymentId == "uncertain-intent-payment")
        val uncertainReplay = uncertainReopened.completePreparedCashPayment(session, uncertainPrepared.completeOperationId,
            "uncertain-intent-sale", "uncertain-intent-payment", 52, uncertainPrepareVersion, 60)
        check(uncertainReplay.disposition == CheckoutLedgerCommandDisposition.REPLAY)
        check(uncertainReopened.getSale("uncertain-intent-sale").cashPayments.size == 1)
    }

    private fun cashIntentReconcilesLateTenderEvidence() {
        val scope = CheckoutAuthorityScope("merchant-late-cash", "store-late-cash", "location-late-cash", "register-late-cash", "device-late-cash")
        val authority = CheckoutAuthority(LedgerAuthorityStorage(), LedgerAuthorityClock(), defaultIterations = 1_000)
        val manager = authority.bootstrapManager(authority.prepareBootstrap(), scope, "Late Cash Manager", "123456".toCharArray())
        val session = authority.authenticate(manager.managerActorId, "123456".toCharArray(), scope)

        val partialStorage = MemoryStorage()
        var partialLedger = CheckoutLedger(partialStorage, authority, scope)
        partialLedger.createSale(session, "late-partial-sale", "late-partial-sale", 100, "USD", 10)
        partialLedger.beginBitcoinAttempt("late-partial-sale", "late-partial-btc", 7001, 100, 1000, "late-partial-quote", 11)
        partialLedger.requestCancellation("late-partial-sale", "late-partial-btc")
        partialLedger.acknowledgeBitcoinCancellation("late-partial-sale", "late-partial-btc")
        val beforePrepareVersion = partialLedger.eventSnapshot().saleVersions.getValue("late-partial-sale")
        val prepared = partialLedger.prepareCashPayment(session, "late-partial-prepare", "late-partial-sale", "late-partial-cash",
            100, 20, beforePrepareVersion, 100)
        val staleVersion = partialLedger.eventSnapshot().saleVersions.getValue("late-partial-sale")
        partialLedger.recordBitcoinEvidence("late-partial-sale", "late-partial-btc", "late-partial-output", "late-partial-tx:0",
            400, 1, 1, 21)
        check(partialLedger.getSale("late-partial-sale").remainingMinor == 60L)
        check(runCatching {
            partialLedger.completePreparedCashPayment(session, prepared.value.completeOperationId, "late-partial-sale",
                "late-partial-cash", 22, staleVersion, 100)
        }.isFailure) // stale version and balance are rejected before commit
        val freshVersion = partialLedger.eventSnapshot().saleVersions.getValue("late-partial-sale")
        val partialComplete = partialLedger.completePreparedCashPayment(session, prepared.value.completeOperationId,
            "late-partial-sale", "late-partial-cash", 23, freshVersion, 60)
        check(partialComplete.disposition == CheckoutLedgerCommandDisposition.NEW_COMMIT)
        check(partialComplete.value == CashPayment("late-partial-cash", 60, 100, 40, 23))
        partialLedger = CheckoutLedger(partialStorage, authority, scope)
        check(partialLedger.getSale("late-partial-sale").let {
            it.cashPayments.single() == partialComplete.value && it.receivedMinor == 100L && it.remainingMinor == 0L
        })
        check(partialLedger.getCashPaymentIntent("late-partial-cash")?.completedExpectedRemainingMinor == 60L)

        val fullStorage = MemoryStorage()
        var fullLedger = CheckoutLedger(fullStorage, authority, scope)
        fullLedger.createSale(session, "late-full-sale", "late-full-sale", 100, "USD", 30)
        fullLedger.beginBitcoinAttempt("late-full-sale", "late-full-btc", 7002, 100, 1000, "late-full-quote", 31)
        fullLedger.requestCancellation("late-full-sale", "late-full-btc")
        fullLedger.acknowledgeBitcoinCancellation("late-full-sale", "late-full-btc")
        val fullPrepareVersion = fullLedger.eventSnapshot().saleVersions.getValue("late-full-sale")
        val fullPrepared = fullLedger.prepareCashPayment(session, "late-full-prepare", "late-full-sale", "late-full-cash",
            100, 40, fullPrepareVersion, 100)
        fullLedger.recordBitcoinEvidence("late-full-sale", "late-full-btc", "late-full-output", "late-full-tx:0",
            1000, 1, 1, 41)
        val fullCompletionVersion = fullLedger.eventSnapshot().saleVersions.getValue("late-full-sale")
        val fullyReturned = fullLedger.completePreparedCashPayment(session, fullPrepared.value.completeOperationId,
            "late-full-sale", "late-full-cash", 42, fullCompletionVersion, 0)
        check(fullyReturned.value == CashPayment("late-full-cash", 0, 100, 100, 42))
        fullLedger = CheckoutLedger(fullStorage, authority, scope)
        check(fullLedger.getSale("late-full-sale").cashPayments.single() == fullyReturned.value)
        check(fullLedger.getCashPaymentIntent("late-full-cash")?.completedExpectedRemainingMinor == 0L)

        val risingStorage = MemoryStorage()
        var risingLedger = CheckoutLedger(risingStorage, authority, scope)
        risingLedger.createSale(session, "late-rising-sale", "late-rising-sale", 100, "USD", 50)
        risingLedger.beginCardAttempt("late-rising-sale", "late-rising-card", "square-sandbox", "late-rising-stable", 100)
        risingLedger.recordCardEvidence("late-rising-sale", "late-rising-card", "late-rising-card-event",
            "late-rising-stable", CardResult.COMPLETED, 40, 51)
        val risingPrepareVersion = risingLedger.eventSnapshot().saleVersions.getValue("late-rising-sale")
        val risingPrepared = risingLedger.prepareCashPayment(session, "late-rising-prepare", "late-rising-sale",
            "late-rising-cash", 60, 52, risingPrepareVersion, 60)
        val staleRisingVersion = risingLedger.eventSnapshot().saleVersions.getValue("late-rising-sale")
        risingLedger.recordCardEvidence("late-rising-sale", "late-rising-card", "late-rising-card-event",
            "late-rising-stable", CardResult.FAILED, 0, 53)
        check(risingLedger.getSale("late-rising-sale").remainingMinor == 100L)
        check(runCatching {
            risingLedger.completePreparedCashPayment(session, risingPrepared.value.completeOperationId,
                "late-rising-sale", "late-rising-cash", 54, staleRisingVersion, 60)
        }.isFailure) // stale prepared-balance snapshot cannot finalize
        val freshRisingVersion = risingLedger.eventSnapshot().saleVersions.getValue("late-rising-sale")
        val risingComplete = risingLedger.completePreparedCashPayment(session, risingPrepared.value.completeOperationId,
            "late-rising-sale", "late-rising-cash", 55, freshRisingVersion, 100)
        check(risingComplete.value == CashPayment("late-rising-cash", 60, 60, 0, 55))
        risingLedger.recordCardEvidence("late-rising-sale", "late-rising-card", "late-rising-card-event",
            "late-rising-stable", CardResult.COMPLETED, 20, 56)
        check(risingLedger.getSale("late-rising-sale").let { it.receivedMinor == 80L && it.remainingMinor == 20L })
        val completedBytes = checkNotNull(risingStorage.bytes).copyOf()
        val badTerminalRemaining = completedBytes.copyOf().also { bytes ->
            var cursor = findLastEncodedString(bytes, "late-rising-cash")
            cursor = skipEncodedString(bytes, cursor) // payment id
            cursor = skipEncodedString(bytes, cursor) // sale id
            cursor += 8 + 8 + 8 + 8 // tender, prepare time, prepare version, prepare balance
            cursor = skipEncodedString(bytes, cursor) // prepare operation
            cursor += 4 // status
            cursor += 9 // completion expected version
            cursor += 9 // completion time
            check(bytes[cursor].toInt() == 1)
            writeLong(bytes, cursor + 1, 30L)
        }
        check(runCatching { CheckoutLedger(MemoryStorage().also { it.bytes = badTerminalRemaining }) }.isFailure)
        val badTerminalPayment = completedBytes.copyOf().also { bytes ->
            val amountOffset = findEncodedString(bytes, "late-rising-cash") + encodeString("late-rising-cash").size
            writeLong(bytes, amountOffset, 59L)
            writeLong(bytes, amountOffset + 16, 1L)
        }
        check(runCatching { CheckoutLedger(MemoryStorage().also { it.bytes = badTerminalPayment }) }.isFailure)
        val badCompletionDigest = completedBytes.copyOf().also { bytes ->
            val digestOffset = commandOffsets(bytes, risingPrepared.value.completeOperationId).digest
            bytes[digestOffset] = (bytes[digestOffset].toInt() xor 1).toByte()
        }
        check(runCatching { CheckoutLedger(MemoryStorage().also { it.bytes = badCompletionDigest }) }.isFailure)
        risingLedger = CheckoutLedger(risingStorage, authority, scope)
        check(risingLedger.getSale("late-rising-sale").let {
            it.cashPayments.single() == risingComplete.value && it.receivedMinor == 80L && it.remainingMinor == 20L
        })
        check(risingLedger.getCashPaymentIntent("late-rising-cash")?.completedExpectedRemainingMinor == 100L)

        val forgedStorage = MemoryStorage()
        val forgedLedger = CheckoutLedger(forgedStorage)
        forgedLedger.createSale("forged-zero-sale", 100, "USD", 50)
        forgedLedger.recordCashPayment("forged-zero-sale", "forged-zero-cash", 100, 51)
        val forged = checkNotNull(forgedStorage.bytes).copyOf()
        val amountOffset = findEncodedString(forged, "forged-zero-cash") + encodeString("forged-zero-cash").size
        writeLong(forged, amountOffset, 0L)
        writeLong(forged, amountOffset + 16, 100L)
        check(runCatching { CheckoutLedger(MemoryStorage().also { it.bytes = forged }) }.isFailure)
    }

    private fun recordCashWithIntent(
        ledger: CheckoutLedger,
        session: CheckoutAuthoritySession,
        prepareOperationId: String,
        saleId: String,
        paymentId: String,
        tenderedMinor: Long,
        preparedAt: Long,
        completedAt: Long,
        expectedSaleVersion: Long,
        expectedRemainingMinor: Long,
    ): CheckoutLedgerCommandResult<CashPayment> {
        val prepared = ledger.prepareCashPayment(session, prepareOperationId, saleId, paymentId,
            tenderedMinor, preparedAt, expectedSaleVersion, expectedRemainingMinor)
        return ledger.completePreparedCashPayment(session, prepared.value.completeOperationId, saleId,
            paymentId, completedAt, Math.addExact(expectedSaleVersion, 1L), expectedRemainingMinor)
    }

    private fun twoSaleCommandLedgerBytes(): ByteArray {
        val scope = CheckoutAuthorityScope("merchant-ledger", "store-ledger", "location-ledger", "register-ledger", "device-ledger")
        val authority = CheckoutAuthority(LedgerAuthorityStorage(), LedgerAuthorityClock(), defaultIterations = 1_000)
        val manager = authority.bootstrapManager(authority.prepareBootstrap(), scope, "Ledger Manager", "123456".toCharArray())
        val session = authority.authenticate(manager.managerActorId, "123456".toCharArray(), scope)
        val storage = MemoryStorage()
        val ledger = CheckoutLedger(storage, authority, scope)
        ledger.createSale(session, "sale-op-1", "sale-one", 100, "USD", 50)
        ledger.createSale(session, "sale-op-2", "sale-two", 200, "USD", 51)
        return checkNotNull(storage.bytes).copyOf()
    }

    private fun legacySaleDigest(saleId: String, amount: Long, currency: String, createdAt: Long): ByteArray {
        val bytes = ByteArrayOutputStream().also { buffer ->
            DataOutputStream(buffer).use { out ->
                writeString(out, "sale.create"); writeString(out, saleId); out.writeLong(amount)
                writeString(out, currency); out.writeLong(createdAt); out.writeInt(0); out.writeBoolean(false)
            }
        }.toByteArray()
        return MessageDigest.getInstance("SHA-256").digest(bytes)
    }

    private data class CommandOffsets(val origin: Int, val target: Int, val digest: Int, val commit: Int)

    private fun commandOffsets(bytes: ByteArray, operationId: String): CommandOffsets {
        var cursor = findEncodedString(bytes, operationId) + encodeString(operationId).size
        cursor = skipEncodedString(bytes, cursor) // event type
        val originOffset = cursor
        cursor += 4
        check(bytes[cursor].toInt() == 1)
        cursor++ // actor attribution present
        cursor = skipEncodedString(bytes, cursor) // actor ID
        cursor = skipEncodedString(bytes, cursor) // display name
        cursor += 4 // role
        cursor = skipEncodedString(bytes, cursor) // capability
        cursor += 8 // policy generation
        repeat(5) { cursor = skipEncodedString(bytes, cursor) } // exact scope
        cursor += 8 // actor event time
        val targetOffset = cursor
        cursor = skipEncodedString(bytes, cursor)
        check(readInt(bytes, cursor) == 32)
        val digestOffset = cursor + 4
        cursor = skipNullableString(bytes, digestOffset + 32)
        cursor = skipNullableString(bytes, cursor)
        cursor = if (bytes[cursor].toInt() == 0) cursor + 1 else cursor + 9 // source observed time
        cursor += 8 // recordedAtMillis
        return CommandOffsets(originOffset, targetOffset, digestOffset, cursor)
    }

    private fun skipNullableString(bytes: ByteArray, offset: Int): Int =
        if (bytes[offset].toInt() == 0) offset + 1 else skipEncodedString(bytes, offset + 1)

    private fun skipEncodedString(bytes: ByteArray, offset: Int): Int = offset + 4 + readInt(bytes, offset)

    private fun findEncodedString(bytes: ByteArray, value: String): Int = findBytes(bytes, encodeString(value), 0)

    private fun findLastEncodedString(bytes: ByteArray, value: String): Int {
        val encoded = encodeString(value)
        var found = -1
        for (start in 0..bytes.size - encoded.size) {
            if (encoded.indices.all { bytes[start + it] == encoded[it] }) found = start
        }
        return checkNotNull(found.takeIf { it >= 0 }) { "encoded test value was not found" }
    }

    private fun replaceEncodedString(bytes: ByteArray, old: String, replacement: String) {
        require(old.toByteArray(Charsets.UTF_8).size == replacement.toByteArray(Charsets.UTF_8).size)
        replaceEncodedStringAt(bytes, findEncodedString(bytes, old), replacement)
    }

    private fun replaceEncodedStringAt(bytes: ByteArray, offset: Int, replacement: String) {
        val encoded = encodeString(replacement)
        require(readInt(bytes, offset) == encoded.size - 4)
        encoded.copyInto(bytes, offset)
    }

    private fun replaceEncodedStringResizing(bytes: ByteArray, offset: Int, replacement: String): ByteArray {
        val oldSize = 4 + readInt(bytes, offset)
        val encoded = encodeString(replacement)
        return ByteArray(bytes.size - oldSize + encoded.size).also { result ->
            bytes.copyInto(result, 0, 0, offset)
            encoded.copyInto(result, offset)
            bytes.copyInto(result, offset + encoded.size, offset + oldSize, bytes.size)
        }
    }

    private fun encodeString(value: String): ByteArray {
        val text = value.toByteArray(Charsets.UTF_8)
        return byteArrayOf((text.size ushr 24).toByte(), (text.size ushr 16).toByte(),
            (text.size ushr 8).toByte(), text.size.toByte()) + text
    }

    private fun readInt(bytes: ByteArray, offset: Int): Int =
        ((bytes[offset].toInt() and 0xff) shl 24) or ((bytes[offset + 1].toInt() and 0xff) shl 16) or
            ((bytes[offset + 2].toInt() and 0xff) shl 8) or (bytes[offset + 3].toInt() and 0xff)

    private fun writeInt(bytes: ByteArray, offset: Int, value: Int) {
        bytes[offset] = (value ushr 24).toByte(); bytes[offset + 1] = (value ushr 16).toByte()
        bytes[offset + 2] = (value ushr 8).toByte(); bytes[offset + 3] = value.toByte()
    }

    private fun writeLong(bytes: ByteArray, offset: Int, value: Long) {
        repeat(8) { index -> bytes[offset + index] = (value ushr ((7 - index) * 8)).toByte() }
    }

    private fun findBytes(bytes: ByteArray, needle: ByteArray, fromIndex: Int): Int {
        val index = (fromIndex..bytes.size - needle.size).firstOrNull { start ->
            needle.indices.all { bytes[start + it] == needle[it] }
        }
        return checkNotNull(index) { "encoded test value was not found" }
    }

    /** v6 uninitialized event ledger with one cash-only manual sale. */
    private fun legacyV6WithCash(): ByteArray = ByteArrayOutputStream().also { bytes ->
        DataOutputStream(bytes).use { out ->
            out.writeInt(0x4f42434c); out.writeInt(6); out.writeInt(1)
            writeString(out, "legacy-v6"); out.writeLong(100); writeString(out, "USD"); out.writeLong(1)
            out.writeInt(0); out.writeInt(0); out.writeInt(0) // items, attempts, refunds
            out.writeInt(1); writeString(out, "cash-v6"); out.writeLong(100); out.writeLong(120); out.writeLong(20); out.writeLong(2)
            out.writeBoolean(false); out.writeBoolean(false); out.writeLong(0); out.writeLong(0)
            out.writeInt(0); out.writeInt(0) // sale versions, events
        }
    }.toByteArray()

    /** v7 file contains the v6 cash record plus the v7 pricing marker. */
    private fun legacyV7WithCash(): ByteArray = ByteArrayOutputStream().also { bytes ->
        DataOutputStream(bytes).use { out ->
            out.writeInt(0x4f42434c); out.writeInt(7); out.writeInt(1)
            writeString(out, "legacy-v7"); out.writeLong(100); writeString(out, "USD"); out.writeLong(1)
            out.writeInt(0); out.writeInt(0); out.writeInt(0)
            out.writeInt(1); writeString(out, "cash-v7"); out.writeLong(100); out.writeLong(120); out.writeLong(20); out.writeLong(2)
            out.writeBoolean(false) // no pricing snapshot
            out.writeBoolean(false); out.writeBoolean(false); out.writeLong(0); out.writeLong(0)
            out.writeInt(0); out.writeInt(0)
        }
    }.toByteArray()

    private fun stableSaleSerializesLocalCommit() {
        val ledger = CheckoutLedger(MemoryStorage())
        ledger.createSale("stable", 100, "USD", 1)
        ledger.beginCardAttempt("stable", "card-stable", "processor", "stable-id", 100)
        val initial = ledger.initializeEvents()
        val version = initial.saleVersions.getValue("stable")
        val entered = CountDownLatch(1)
        val release = CountDownLatch(1)
        val mutationStarted = CountDownLatch(1)
        val mutationDone = CountDownLatch(1)
        val failure = AtomicReference<Throwable?>(null)

        val inventoryCommit = Thread {
            try {
                val total = ledger.withStableSale("stable", version) { snapshot ->
                    check(snapshot.amountMinor == 100L)
                    check(ledger.getSale("stable").amountMinor == 100L) // reads are allowed
                    check(runCatching { ledger.createSale("reentrant", 1, "USD", 2) }.isFailure)
                    entered.countDown()
                    check(release.await(5, TimeUnit.SECONDS))
                    snapshot.amountMinor
                }
                check(total == 100L)
            } catch (e: Throwable) { failure.set(e) }
        }
        val paymentUpdate = Thread {
            try {
                check(entered.await(5, TimeUnit.SECONDS))
                mutationStarted.countDown()
                ledger.recordCardEvidence("stable", "card-stable", "payment-stable", "stable-id", CardResult.COMPLETED, 100, 3)
            } catch (e: Throwable) { failure.compareAndSet(null, e) }
            finally { mutationDone.countDown() }
        }
        inventoryCommit.start(); paymentUpdate.start()
        check(mutationStarted.await(5, TimeUnit.SECONDS))
        check(!mutationDone.await(150, TimeUnit.MILLISECONDS)) // payment evidence cannot interleave local commit
        release.countDown()
        inventoryCommit.join(5000); paymentUpdate.join(5000)
        check(!inventoryCommit.isAlive && !paymentUpdate.isAlive)
        failure.get()?.let { throw AssertionError("stable-sale concurrency failed", it) }
        check(mutationDone.count == 0L && ledger.getSale("stable").paid)
        check(runCatching { ledger.withStableSale("stable", version) { it.amountMinor } }.isFailure)
        check(runCatching { ledger.withStableSale("stable", version + 1) { throw IllegalStateException("callback failure") } }.isFailure)
        ledger.createSale("after-callback-error", 50, "USD", 4) // finally released the mutation guard
        check(ledger.getSale("after-callback-error").amountMinor == 50L)
    }

    /** A v2/v3/v4 on-disk paid sale. */
    private fun legacySaleBytes(version: Int, saleId: String, chainState: BitcoinChainState = BitcoinChainState.ACTIVE,
                                confirmations: Int = 1, refundRequest: Boolean = false): ByteArray = ByteArrayOutputStream().also { bytes ->
        DataOutputStream(bytes).use { out ->
            out.writeInt(0x4f42434c); out.writeInt(version); out.writeInt(1)
            writeString(out, saleId); out.writeLong(500); writeString(out, "USD"); out.writeLong(69)
            if (version >= 4) out.writeInt(0) // v4 item snapshots
            out.writeInt(1) // attempts
            writeString(out, "legacy-btc"); out.writeInt(Tender.BITCOIN.ordinal); out.writeLong(500)
            out.writeBoolean(true); out.writeLong(1000) // expected sats
            writeNullableString(out, "legacy-quote"); out.writeBoolean(true); out.writeLong(68) // quote id / time
            out.writeBoolean(true); out.writeLong(900) // counter index
            out.writeBoolean(false); out.writeBoolean(false) // card provider / stable id
            out.writeBoolean(false); out.writeBoolean(false); out.writeBoolean(false) // cancel / ack / unknown
            out.writeInt(1) // Bitcoin evidence
            writeString(out, "legacy-output"); writeString(out, "legacy-tx:0"); out.writeLong(1000)
            out.writeInt(confirmations); out.writeInt(1); out.writeLong(67); out.writeInt(chainState.ordinal)
            if (version >= 3) out.writeLong(1000) // v3 historical confirmed sats
            out.writeInt(0) // card evidence
            out.writeInt(if (refundRequest) 1 else 0)
            if (refundRequest) {
                writeString(out, "legacy-refund"); writeString(out, "legacy-btc")
                out.writeLong(0); out.writeLong(500); out.writeLong(68); out.writeInt(0)
            }
        }
    }.toByteArray()

    private fun writeNullableString(out: DataOutputStream, value: String?) {
        out.writeBoolean(value != null)
        if (value != null) writeString(out, value)
    }

    private fun writeString(out: DataOutputStream, value: String) {
        val bytes = value.toByteArray(Charsets.UTF_8)
        out.writeInt(bytes.size); out.write(bytes)
    }

    private class MemoryStorage : DurableLedgerStorage {
        var bytes: ByteArray? = null
        var failNextWrite = false
        var failBeforeWrite = false
        override fun read(): ByteArray? = bytes?.copyOf()
        override fun writeAtomically(bytes: ByteArray) {
            if (failBeforeWrite) { failBeforeWrite = false; error("simulated pre-write failure") }
            if (failNextWrite) { failNextWrite = false; this.bytes = bytes.copyOf(); error("simulated post-commit write failure") }
            this.bytes = bytes.copyOf()
        }
    }

    private class LedgerAuthorityClock : CheckoutAuthorityClock {
        var elapsed = 10_000L
        var wall = 1_800_000_000_000L
        override fun elapsedRealtimeMillis() = elapsed
        override fun wallTimeMillis() = wall++
        override fun bootIdentity() = "ledger-test-boot"
    }

    private class LedgerAuthorityStorage : CheckoutAuthorityStorage {
        private var bytes: ByteArray? = null
        override fun read() = bytes?.copyOf()
        override fun compareAndSwap(expectedBytes: ByteArray?, nextBytes: ByteArray): Boolean {
            val current = bytes
            val matches = if (current == null || expectedBytes == null) current == null && expectedBytes == null
            else current.contentEquals(expectedBytes)
            if (!matches) return false
            bytes = nextBytes.copyOf()
            return true
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
