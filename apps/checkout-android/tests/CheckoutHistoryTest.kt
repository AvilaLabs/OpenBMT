package org.openbmt.checkout

object CheckoutHistoryTest {
    fun run() {
        val bitcoin = sale("btc", 300, attempts = listOf(attempt(Tender.BITCOIN, PaymentState.UNKNOWN)))
        val card = sale("Card-Case", 200, items = listOf(SaleItem("mug", "Blue Mug", 200, 1)),
            attempts = listOf(attempt(Tender.CARD, PaymentState.PARTIAL)), received = 100, remaining = 100)
        val cash = sale("cash", 200, cashPayments = listOf(CashPayment("cash-1", 100, 100, 0, 201)),
            paid = false, received = 100, remaining = 100)
        val mixed = sale("mixed", 250, attempts = listOf(attempt(Tender.BITCOIN, PaymentState.PARTIAL),
            attempt(Tender.CARD, PaymentState.PAID)), cashPayments = listOf(CashPayment("cash-2", 100, 100, 0, 251)))
        val review = sale("review-sale", 150, 100, attempts = listOf(attempt(Tender.CARD, PaymentState.PAID)),
            paid = false, review = true, received = 150, remaining = 0)
        val sameTimeZ = sale("z-tie", 300)
        val sameTimeA = sale("a-tie", 300)
        val all = listOf(bitcoin, card, cash, mixed, review, sameTimeZ, sameTimeA)

        check(searchSales(all, SaleHistoryQuery(tender = HistoryTender.BITCOIN)).map { it.saleId } == listOf("btc", "mixed"))
        check(searchSales(all, SaleHistoryQuery(tender = HistoryTender.CARD)).map { it.saleId } == listOf("mixed", "Card-Case", "review-sale"))
        check(searchSales(all, SaleHistoryQuery(tender = HistoryTender.CASH)).map { it.saleId } == listOf("mixed", "cash"))
        // Unknown and partial attempts still count as recorded tender history.
        check(searchSales(all, SaleHistoryQuery(tender = HistoryTender.BITCOIN)).contains(bitcoin))

        check(searchSales(all, SaleHistoryQuery(status = HistoryStatus.REVIEW)).map { it.saleId } == listOf("review-sale"))
        check(searchSales(all, SaleHistoryQuery(status = HistoryStatus.PAID)).none { it.requiresReview })
        check(searchSales(all, SaleHistoryQuery(status = HistoryStatus.UNPAID)).none { it.requiresReview })
        check(searchSales(all, SaleHistoryQuery(text = " bLuE mUG ")).single() == card)
        check(searchSales(all, SaleHistoryQuery(text = "CARD-CASE")).single() == card)
        check(searchSales(all, SaleHistoryQuery(fromInclusive = 200, untilExclusive = 300)).all { it.createdAt in 200L..299L })
        check(searchSales(listOf(sameTimeZ, sameTimeA), SaleHistoryQuery()).map { it.saleId } == listOf("a-tie", "z-tie"))
        check(runCatching { searchSales(all, SaleHistoryQuery(text = "x".repeat(129))) }.isFailure)
        check(runCatching { searchSales(all, SaleHistoryQuery(fromInclusive = 20, untilExclusive = 20)) }.isFailure)
    }

    private fun sale(
        id: String,
        created: Long,
        amount: Long = 100,
        attempts: List<PaymentAttempt> = emptyList(),
        cashPayments: List<CashPayment> = emptyList(),
        items: List<SaleItem> = emptyList(),
        paid: Boolean = true,
        review: Boolean = false,
        received: Long = if (paid) amount else 0,
        remaining: Long = amount - received,
    ) = Sale(id, amount, "USD", created, attempts, emptyList(), received, remaining, paid, review,
        false, 0, 0, items, cashPayments)

    private fun attempt(tender: Tender, state: PaymentState) = PaymentAttempt(
        "${tender.name}-${state.name}", tender, 100, null, null, null, null, null, null,
        false, false, emptyList(), emptyList(), state, 100, 0,
    )
}
