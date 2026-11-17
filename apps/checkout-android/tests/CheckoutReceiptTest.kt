package org.openbmt.checkout

/** Focused JVM cases for the public receipt projection/export. */
object CheckoutReceiptTest {
    fun run() {
        cashChangeAndSnapshot()
        mixedUncertainAndRefundEvidence()
        unsupportedCurrencyAndTextEscaping()
    }

    private fun cashChangeAndSnapshot() {
        val sale = sale(
            items = listOf(SaleItem("coffee", "Coffee", 325, 2)),
            cash = listOf(CashPayment("cash-1", 650, 1000, 350, 1)),
            amount = 650, received = 650, remaining = 0, paid = true,
        )
        val text = CheckoutReceipt.receiptText(sale)
        check("2 × Coffee — USD 3.25 each" in text)
        check("CASH — ACKNOWLEDGED; observed USD 6.50; settled USD 6.50; tendered USD 10.00; change USD 3.50" in text)
        check("Total: USD 6.50" in text)
    }

    private fun mixedUncertainAndRefundEvidence() {
        val bitcoin = PaymentAttempt("secret-attempt", Tender.BITCOIN, 500, 2500, "private-quote", 1, 99,
            null, null, false, false,
            listOf(BitcoinEvidence("private-evidence", "private-source", 1000, 0, 1, 2, BitcoinChainState.ACTIVE)),
            emptyList(), PaymentState.PARTIAL, 200, 1000)
        val card = PaymentAttempt("secret-card", Tender.CARD, 300, null, null, null, null, "private-provider", "private-stable",
            false, false, emptyList(), listOf(CardEvidence("private-card-ev", "private-stable", CardResult.APPROVED, 300, 3)),
            PaymentState.UNKNOWN, 0, 0)
        val reorganized = PaymentAttempt("secret-reorg", Tender.BITCOIN, 200, 1000, "private-q", 1, 100,
            null, null, false, false,
            listOf(BitcoinEvidence("ev", "source", 1000, 0, 1, 4, BitcoinChainState.REORGED)),
            emptyList(), PaymentState.REORGED, 0, 0)
        val requestedRefund = Refund("secret-refund", "secret-card", 100, 0, 5, emptyList(), RefundState.REQUESTED, 0, 0)
        val sale = sale(items = emptyList(), attempts = listOf(bitcoin, card, reorganized), refunds = listOf(requestedRefund),
            received = 200, remaining = 800, paid = false, review = true)
        val projected = CheckoutReceipt.project(sale)
        check(projected.tenders.map { it.state } == listOf("PARTIAL", "UNKNOWN", "REORGED"))
        val text = CheckoutReceipt.format(projected)
        check("Status: OWNER REVIEW REQUIRED" in text)
        check("Observation: UNCONFIRMED — 1000 sats" in text)
        check("Settled / applied: USD 0.00" in text)
        check("Observation: APPROVED — USD 3.00" in text)
        check("Observation: REORGED — 1000 sats" in text)
        check("REQUESTED — requested USD 1.00; paid out USD 0.00" in text)
        for (privateValue in listOf("secret-attempt", "private-quote", "private-source", "private-provider", "private-stable", "secret-refund", "testnet")) {
            check(privateValue !in text)
        }
    }

    private fun unsupportedCurrencyAndTextEscaping() {
        check(runCatching { CheckoutReceipt.receiptText(sale(currency = "EUR")) }.isFailure)
        val sale = sale(items = listOf(SaleItem("item", "Cookie\nINJECTED\u0001", 99, 1)), amount = 99,
            received = 0, remaining = 99)
        val text = CheckoutReceipt.receiptText(sale)
        check("Cookie INJECTED" in text)
        check("\nINJECTED" !in text)
        check("\u0001" !in text)
    }

    private fun sale(
        items: List<SaleItem> = emptyList(), attempts: List<PaymentAttempt> = emptyList(),
        refunds: List<Refund> = emptyList(), cash: List<CashPayment> = emptyList(), currency: String = "USD",
        amount: Long = 1000, received: Long = 0, remaining: Long = amount, paid: Boolean = false,
        review: Boolean = false,
    ) = Sale("sale-opaque-1", amount, currency, 0, attempts, refunds, received, remaining, paid,
        review, false, 0, 0, items, cash)
}
