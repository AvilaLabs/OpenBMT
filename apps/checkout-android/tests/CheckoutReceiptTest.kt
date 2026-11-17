package org.openbmt.checkout

/** Focused JVM cases for the public receipt projection/export. */
object CheckoutReceiptTest {
    fun run() {
        cashChangeAndSnapshot()
        mixedUncertainAndRefundEvidence()
        unsupportedCurrencyAndTextEscaping()
        frozenPricingAndLegacyReceipt()
        modifiersAndInclusiveTaxRenderWithoutDoubleCounting()
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

    private fun frozenPricingAndLegacyReceipt() {
        val pricing = CheckoutPricing.freeze(
            listOf(
                CheckoutLineInput("latte-line", "latte", "Latte", 550, 1),
                CheckoutLineInput("bagel-line", "bagel", "Bagel", 450, 1),
            ),
            discountMinor = 100,
            tax = CheckoutTaxRule("merchant-tax", "Merchant configured tax", 825, TaxTreatment.EXCLUSIVE),
            tipMinor = 150,
        )
        check(pricing.subtotalMinor == 1000L)
        check(pricing.discountMinor == 100L)
        check(pricing.taxMinor == 74L)
        check(pricing.totalMinor == 1124L)
        val priced = sale(items = listOf(SaleItem("latte", "Latte", 550, 1), SaleItem("bagel", "Bagel", 450, 1)),
            amount = pricing.totalMinor, remaining = pricing.totalMinor, pricing = pricing)
        val text = CheckoutReceipt.receiptText(priced)
        check("Subtotal: USD 10.00" in text)
        check("Order discount: −USD 1.00" in text)
        check("Merchant configured tax (exclusive, 8.25%): +USD 0.74" in text)
        check("Total: USD 11.24" in text)
        val legacy = CheckoutReceipt.receiptText(sale(items = listOf(SaleItem("latte", "Latte", 550, 1)), amount = 550, remaining = 550))
        check("Total: USD 5.50" in legacy)
        check("Price details" !in legacy)
        check("Tax:" !in legacy)
    }

    private fun modifiersAndInclusiveTaxRenderWithoutDoubleCounting() {
        val modifierPricing = CheckoutPricing.freeze(listOf(
            CheckoutLineInput("cookie-line", "cookie", "Cookie", 499, 2,
                listOf(SelectedModifier("topping", "Chocolate topping", 75))),
        ))
        check(modifierPricing.subtotalMinor == 1148L)
        val modifierText = CheckoutReceipt.receiptText(sale(
            items = listOf(SaleItem("cookie", "Cookie", 574, 2)), amount = 1148,
            remaining = 1148, pricing = modifierPricing,
        ))
        check("2 × Cookie — USD 4.99 each" in modifierText)
        check("Chocolate topping modifier — +USD 0.75 each × 2 = +USD 1.50" in modifierText)
        check("Subtotal: USD 11.48" in modifierText)
        check("Total: USD 11.48" in modifierText)
        check("Total: USD 12.98" !in modifierText)

        val inclusive = CheckoutPricing.freeze(
            listOf(CheckoutLineInput("drink-line", "drink", "Drink", 105, 1)),
            tax = CheckoutTaxRule("inclusive-tax", "Included tax", 500, TaxTreatment.INCLUSIVE),
        )
        check(inclusive.taxMinor == 5L && inclusive.totalMinor == 105L)
        val inclusiveText = CheckoutReceipt.receiptText(sale(
            items = listOf(SaleItem("drink", "Drink", 105, 1)), amount = 105,
            remaining = 105, pricing = inclusive,
        ))
        check("Included tax (inclusive, 5.00%): included/extracted USD 0.05" in inclusiveText)
        check("Included tax (inclusive, 5.00%): +USD 0.05" !in inclusiveText)
        check("Total: USD 1.05" in inclusiveText)
    }

    private fun sale(
        items: List<SaleItem> = emptyList(), attempts: List<PaymentAttempt> = emptyList(),
        refunds: List<Refund> = emptyList(), cash: List<CashPayment> = emptyList(), currency: String = "USD",
        amount: Long = 1000, received: Long = 0, remaining: Long = amount, paid: Boolean = false,
        review: Boolean = false,
        pricing: CheckoutPricingSnapshot? = null,
    ) = Sale("sale-opaque-1", amount, currency, 0, attempts, refunds, received, remaining, paid,
        review, false, 0, 0, items, cash, pricing)
}
