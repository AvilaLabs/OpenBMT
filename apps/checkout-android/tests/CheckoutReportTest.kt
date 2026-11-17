package org.openbmt.checkout

/** Exact, bounded reporting projection and CSV safety checks. */
object CheckoutReportTest {
    fun run() {
        rangeSortAndMixedTenderEvidence()
        pricingAndLegacyBreakdown()
        permitsParentScopedIdsFromLedger()
        rejectsDuplicateIdsBoundsAndOverflow()
        csvEscapesAndGuardsUntrustedText()
    }

    private fun rangeSortAndMixedTenderEvidence() {
        val card = PaymentAttempt("private-card", Tender.CARD, 500, null, null, null, null,
            "private-provider", "private-stable", false, false, emptyList(),
            listOf(CardEvidence("private-card-evidence", "private-stable", CardResult.COMPLETED, 300, 2)),
            PaymentState.PARTIAL, 300, 0)
        val btc = PaymentAttempt("private-btc", Tender.BITCOIN, 500, 2500, "private-quote", 1, 2,
            null, null, false, false,
            listOf(BitcoinEvidence("private-btc-ev", "private-source", 1000, 0, 1, 3, BitcoinChainState.ACTIVE)),
            emptyList(), PaymentState.PARTIAL, 200, 1000)
        val reorg = PaymentAttempt("private-reorg", Tender.BITCOIN, 200, 1000, "q", 1, 3,
            null, null, false, false,
            listOf(BitcoinEvidence("e", "s", 1000, 2, 1, 4, BitcoinChainState.REORGED)),
            emptyList(), PaymentState.REORGED, 0, 0)
        val requested = Refund("private-refund-1", "private-card", 100, 0, 4, emptyList(), RefundState.REQUESTED, 0, 0)
        val paid = Refund("private-refund-2", "private-card", 200, 0, 5,
            listOf(RefundEvidence("private-refund-ev", 120, 0, false, 6)), RefundState.PARTIAL, 120, 0)
        val sale = sale("z", 10, 1000, attempts = listOf(card, btc, reorg),
            cash = listOf(CashPayment("private-cash", 100, 200, 100, 7)), refunds = listOf(requested, paid),
            received = 600, remaining = 400, review = true)
        val edgeStart = sale("start", 10, 100, received = 0)
        val edgeEnd = sale("end", 20, 100, received = 0)
        val earlierId = sale("a", 10, 200, received = 0)
        val report = CheckoutReporting.build(listOf(edgeEnd, sale, earlierId, edgeStart), CheckoutReportRange(10, 20))
        check(report.rows.map { it.saleId } == listOf("a", "start", "z"))
        check(report.usdSaleCount == 3L && report.usdSalesMinor == 1300L)
        val row = report.rows.last()
        check(row.status == "REVIEW")
        check(row.cardObservedMinor == 300L && row.cardAppliedMinor == 300L)
        check(row.bitcoinObservedMinor == 200L && row.bitcoinObservedSats == 1000L)
        check(row.bitcoinAppliedMinor == 0L && row.bitcoinAppliedSats == 0L) // partial remains unconfirmed; reorg contributes no fiat
        check(row.cashAppliedMinor == 100L && row.observedMinor == 600L && row.appliedMinor == 400L)
        check(row.refundRequestedMinor == 300L && row.refundPaidMinor == 120L)
        check(report.rows.none { it.saleId == "end" })
    }

    private fun pricingAndLegacyBreakdown() {
        val pricing = CheckoutPricing.freeze(listOf(CheckoutLineInput("l", "p", "Item", 1000, 1)),
            discountMinor = 100, tax = CheckoutTaxRule("t", "Tax", 1000, TaxTreatment.EXCLUSIVE), tipMinor = 50)
        val priced = sale("priced", 1, pricing.totalMinor, pricing = pricing)
        val legacy = sale("legacy", 2, 900)
        val report = CheckoutReporting.build(listOf(legacy, priced), CheckoutReportRange(0, 3))
        check(report.usdSubtotalMinor == 1000L && report.usdDiscountMinor == 100L) // legacy breakdown is unknown
        check(report.usdSalesMinor == 1940L) // legacy sale total is still included
        check(report.usdTaxMinor == 90L && report.usdTipMinor == 50L)
        check(report.usdBreakdownUnknownCount == 1L)
        val old = report.rows.single { it.saleId == "legacy" }
        check(!old.breakdownKnown && old.taxMinor == null)
    }

    private fun rejectsDuplicateIdsBoundsAndOverflow() {
        val one = sale("duplicate", 1, 1)
        check(runCatching { CheckoutReporting.build(listOf(one, one), CheckoutReportRange(0, 2)) }.isFailure)
        val repeatedAttempt = PaymentAttempt("same-attempt", Tender.CARD, 1, null, null, null, null,
            "p", "s", false, false, emptyList(), emptyList(), PaymentState.UNKNOWN, 0, 0)
        val repeatedAttemptOnSale = repeatedAttempt.copy(stableId = "s2")
        check(runCatching { CheckoutReporting.build(listOf(sale("a", 1, 1,
            attempts = listOf(repeatedAttempt, repeatedAttemptOnSale))), CheckoutReportRange(0, 2)) }.isFailure)
        val repeatedRefund = Refund("same-refund", "a", 1, 0, 1, emptyList(), RefundState.REQUESTED, 0, 0)
        check(runCatching { CheckoutReporting.build(listOf(sale("a", 1, 1,
            refunds = listOf(repeatedRefund, repeatedRefund.copy(paymentAttemptId = "other")))), CheckoutReportRange(0, 2)) }.isFailure)
        val repeatedCash = CashPayment("same-cash", 1, 1, 0, 1)
        check(runCatching { CheckoutReporting.build(listOf(
            sale("a", 1, 1, cash = listOf(repeatedCash)), sale("b", 1, 1, cash = listOf(repeatedCash))),
            CheckoutReportRange(0, 2)) }.isFailure)
        val ev1 = PaymentAttempt("a1", Tender.CARD, 1, null, null, null, null, "p", "s1", false, false,
            emptyList(), listOf(CardEvidence("same-evidence", "s1", CardResult.PENDING, 0, 1)), PaymentState.UNKNOWN, 0, 0)
        val evDuplicate = ev1.copy(cardEvidence = listOf(
            CardEvidence("same-evidence", "s1", CardResult.PENDING, 0, 1),
            CardEvidence("same-evidence", "s1", CardResult.PENDING, 0, 2)))
        check(runCatching { CheckoutReporting.build(listOf(sale("a", 1, 1, attempts = listOf(evDuplicate))), CheckoutReportRange(0, 2)) }.isFailure)
        check(runCatching { CheckoutReporting.build(List(2001) { sale("sale-$it", 1, 1) }, CheckoutReportRange(0, 2)) }.isFailure)
        val attemptsOverLimit = List(101) { index -> repeatedAttempt.copy(attemptId = "attempt-$index") }
        check(runCatching { CheckoutReporting.build(listOf(sale("many-attempts", 1, 1, attempts = attemptsOverLimit)), CheckoutReportRange(0, 2)) }.isFailure)
        check(runCatching {
            CheckoutReporting.build(listOf(sale("a", 1, Long.MAX_VALUE), sale("b", 1, 1)), CheckoutReportRange(0, 2))
        }.isFailure)
        check(runCatching { CheckoutReportRange(2, 2) }.isFailure)
    }

    private fun permitsParentScopedIdsFromLedger() {
        val storage = MemoryStorage()
        val ledger = CheckoutLedger(storage)
        ledger.initializeEvents()
        for ((saleId, stableId) in listOf("sale-a" to "card-a", "sale-b" to "card-b")) {
            ledger.createSale(saleId, 100, "USD", 10)
            ledger.beginCardAttempt(saleId, "payment", "test", stableId, 100)
            ledger.recordCardEvidence(saleId, "payment", "status", stableId, CardResult.COMPLETED, 100, 11)
            ledger.requestRefund(saleId, "refund", "payment", 10, 0, 12)
            ledger.recordRefundEvidence(saleId, "refund", "status", 5, 0, false, 13)
        }
        val report = CheckoutReporting.build(ledger.eventSnapshot().sales, CheckoutReportRange(0, 20))
        check(report.usdSaleCount == 2L)
        check(report.usdRefundRequestedMinor == 20L && report.usdRefundPaidMinor == 10L)
    }

    private fun csvEscapesAndGuardsUntrustedText() {
        val report = CheckoutReporting.build(listOf(
            sale("=SUM(1,2)\"\nX", 1, 5), sale("  =1+1", 1, 5), sale("\t=1+1", 1, 5),
        ), CheckoutReportRange(0, 2))
        val csv = report.toCsv()
        check("\"'=SUM(1,2)\"\" X\"" in csv)
        check("\"'  =1+1\"" in csv)
        check("\"' =1+1\"" in csv)
        check("private" !in csv)
        check("\nX" !in csv)
        check(csv.endsWith("\n"))
        check(csv == report.toCsv())
    }

    private fun sale(
        id: String, created: Long, amount: Long,
        attempts: List<PaymentAttempt> = emptyList(), cash: List<CashPayment> = emptyList(),
        refunds: List<Refund> = emptyList(), received: Long = 0, remaining: Long = amount,
        paid: Boolean = false, review: Boolean = false, pricing: CheckoutPricingSnapshot? = null,
        currency: String = "USD",
    ) = Sale(id, amount, currency, created, attempts, refunds, received, remaining, paid,
        review, false, 0, 0, emptyList(), cash, pricing)

    private class MemoryStorage : DurableLedgerStorage {
        var bytes: ByteArray? = null
        override fun read(): ByteArray? = bytes?.copyOf()
        override fun writeAtomically(bytes: ByteArray) { this.bytes = bytes.copyOf() }
    }
}
