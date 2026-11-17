package org.openbmt.checkout

/** Half-open range on Sale.createdAt, expressed only as Unix epoch seconds. */
data class CheckoutReportRange(val fromInclusiveEpochSeconds: Long, val untilExclusiveEpochSeconds: Long) {
    init {
        require(fromInclusiveEpochSeconds >= 0 && untilExclusiveEpochSeconds > fromInclusiveEpochSeconds) {
            "invalid report range"
        }
    }
}

data class CheckoutSaleReportRow(
    val saleId: String,
    val createdAtEpochSeconds: Long,
    val currency: String,
    val totalMinor: Long,
    val observedMinor: Long,
    val appliedMinor: Long,
    val status: String,
    /** Distinct attempt states, retained verbatim so UNKNOWN/CANCELED/REORGED are not hidden by a sale status. */
    val paymentStates: String,
    val lateBitcoinReceipt: Boolean,
    val cashAppliedMinor: Long,
    val cardObservedMinor: Long,
    val cardAppliedMinor: Long,
    val bitcoinObservedMinor: Long,
    val bitcoinObservedSats: Long,
    val bitcoinAppliedMinor: Long,
    val bitcoinAppliedSats: Long,
    val refundRequestedMinor: Long,
    val refundPaidMinor: Long,
    val refundRequestedSats: Long,
    val refundPaidSats: Long,
    val subtotalMinor: Long?,
    val discountMinor: Long?,
    val taxMinor: Long?,
    val tipMinor: Long?,
    val breakdownKnown: Boolean,
)

data class CheckoutReport(
    val range: CheckoutReportRange,
    /** Sorted, minimal per-sale rows. Non-USD sales remain in their native currency bucket. */
    val rows: List<CheckoutSaleReportRow>,
    val usdSaleCount: Long,
    val usdSalesMinor: Long,
    val usdObservedMinor: Long,
    val usdAppliedMinor: Long,
    val usdCashAppliedMinor: Long,
    val usdCardObservedMinor: Long,
    val usdCardAppliedMinor: Long,
    val usdBitcoinObservedMinor: Long,
    val usdBitcoinObservedSats: Long,
    val usdBitcoinAppliedMinor: Long,
    val usdBitcoinAppliedSats: Long,
    val usdRefundRequestedMinor: Long,
    val usdRefundPaidMinor: Long,
    val usdRefundRequestedSats: Long,
    val usdRefundPaidSats: Long,
    val usdSubtotalMinor: Long,
    val usdDiscountMinor: Long,
    val usdTaxMinor: Long,
    val usdTipMinor: Long,
    val usdBreakdownUnknownCount: Long,
    val nonUsdCountsByCurrency: Map<String, Long>,
) {
    /** Deterministic UTF-8-friendly CSV text; callers decide where or whether to save it. */
    fun toCsv(): String = CheckoutReportCsv.encode(this)
}

/** Pure, bounded report over caller-supplied sale snapshots. No storage, network, FX, or local-time use. */
object CheckoutReporting {
    fun build(sales: List<Sale>, range: CheckoutReportRange): CheckoutReport {
        require(sales.size <= MAX_SALES) { "too many sales for one report input" }
        val ids = HashSet<String>()
        val cashPaymentIds = HashSet<String>()
        for (sale in sales) {
            require(ids.add(sale.saleId)) { "duplicate sale IDs are not reportable" }
            require(sale.attempts.size <= MAX_ATTEMPTS) { "too many payment attempts on sale" }
            require(sale.refunds.size <= MAX_REFUNDS) { "too many refunds on sale" }
            require(sale.cashPayments.size <= MAX_CASH_PAYMENTS) { "too many cash payments on sale" }
            require(sale.items.size <= MAX_ITEMS_PER_SALE) { "too many item snapshots on sale" }
            val attemptIds = HashSet<String>()
            require(sale.attempts.all { attemptIds.add(it.attemptId) }) { "duplicate payment attempt ID on sale" }
            val refundIds = HashSet<String>()
            require(sale.refunds.all { refundIds.add(it.refundId) }) { "duplicate refund ID on sale" }
            require(sale.cashPayments.all { cashPaymentIds.add(it.paymentId) }) { "duplicate cash payment ID" }
            for (attempt in sale.attempts) {
                require(attempt.bitcoinEvidence.size <= MAX_EVIDENCE && attempt.cardEvidence.size <= MAX_EVIDENCE) {
                    "too many payment observations on attempt"
                }
                val evidenceIds = HashSet<String>()
                require(attempt.bitcoinEvidence.all { evidenceIds.add(it.evidenceId) } &&
                    attempt.cardEvidence.all { evidenceIds.add(it.evidenceId) }) { "duplicate payment evidence ID on attempt" }
            }
            for (refund in sale.refunds) {
                require(refund.evidence.size <= MAX_EVIDENCE) { "too many refund observations" }
                val evidenceIds = HashSet<String>()
                require(refund.evidence.all { evidenceIds.add(it.evidenceId) }) { "duplicate refund evidence ID on refund" }
            }
        }
        val selected = sales.asSequence()
            .filter { it.createdAt >= range.fromInclusiveEpochSeconds && it.createdAt < range.untilExclusiveEpochSeconds }
            .sortedWith(compareBy<Sale> { it.createdAt }.thenBy { it.saleId })
            .map(::row)
            .toList()
        val usd = selected.filter { it.currency == "USD" }
        val nonUsdCounts = selected.filter { it.currency != "USD" }.groupingBy { it.currency }.eachCount()
            .toSortedMap().mapValues { it.value.toLong() }
        fun sum(f: (CheckoutSaleReportRow) -> Long) = usd.fold(0L) { acc, item -> Math.addExact(acc, f(item)) }
        return CheckoutReport(
            range, selected, usd.size.toLong(), sum { it.totalMinor }, sum { it.observedMinor }, sum { it.appliedMinor },
            sum { it.cashAppliedMinor }, sum { it.cardObservedMinor }, sum { it.cardAppliedMinor },
            sum { it.bitcoinObservedMinor }, sum { it.bitcoinObservedSats }, sum { it.bitcoinAppliedMinor }, sum { it.bitcoinAppliedSats },
            sum { it.refundRequestedMinor }, sum { it.refundPaidMinor }, sum { it.refundRequestedSats }, sum { it.refundPaidSats },
            sum { it.subtotalMinor ?: 0L }, sum { it.discountMinor ?: 0L }, sum { it.taxMinor ?: 0L }, sum { it.tipMinor ?: 0L },
            usd.count { !it.breakdownKnown }.toLong(), nonUsdCounts,
        )
    }

    private fun row(sale: Sale): CheckoutSaleReportRow {
        require(sale.createdAt >= 0) { "sale creation time must be Unix epoch seconds" }
        val cash = sum(sale.cashPayments.map { it.amountMinor })
        val cardAttempts = sale.attempts.filter { it.tender == Tender.CARD }
        val bitcoinAttempts = sale.attempts.filter { it.tender == Tender.BITCOIN }
        val cardObserved = sum(cardAttempts.map { it.receivedMinor })
        val cardApplied = sum(cardAttempts.filter(::isApplied).map { it.receivedMinor })
        val bitcoinObserved = sum(bitcoinAttempts.map { it.receivedMinor })
        val bitcoinSats = sum(bitcoinAttempts.map { it.receivedSats })
        val bitcoinAppliedAttempts = bitcoinAttempts.filter(::isApplied)
        val bitcoinApplied = sum(bitcoinAppliedAttempts.map { it.receivedMinor })
        val bitcoinAppliedSats = sum(bitcoinAppliedAttempts.map { it.receivedSats })
        val observed = sum(listOf(cash) + sale.attempts.map { it.receivedMinor })
        val applied = sum(listOf(cash) + sale.attempts.filter(::isApplied).map { it.receivedMinor })
        val requestedMinor = sum(sale.refunds.map { it.amountMinor })
        val paidMinor = sum(sale.refunds.map { it.actuallyPaidMinor })
        val requestedSats = sum(sale.refunds.map { it.amountSats })
        val paidSats = sum(sale.refunds.map { it.actuallyPaidSats })
        val pricing = sale.pricing
        return CheckoutSaleReportRow(
            sale.saleId, sale.createdAt, sale.currency, sale.amountMinor, observed, applied,
            when { sale.requiresReview -> "REVIEW"; sale.paid -> "PAID"; else -> "INCOMPLETE" },
            sale.attempts.map { it.state.name }.distinct().sorted().joinToString("|"), sale.lateBitcoinReceipt,
            cash, cardObserved, cardApplied, bitcoinObserved, bitcoinSats, bitcoinApplied, bitcoinAppliedSats,
            requestedMinor, paidMinor, requestedSats, paidSats,
            pricing?.subtotalMinor, pricing?.discountMinor, pricing?.taxMinor, pricing?.tipMinor, pricing != null,
        )
    }

    private fun isApplied(attempt: PaymentAttempt): Boolean = when (attempt.tender) {
        Tender.CARD -> attempt.state == PaymentState.PAID || attempt.state == PaymentState.PARTIAL
        Tender.BITCOIN -> attempt.state == PaymentState.CONFIRMED ||
            (attempt.state == PaymentState.PARTIAL && attempt.bitcoinEvidence
                .filter { it.chainState == BitcoinChainState.ACTIVE && it.receivedSats > 0 }
                .let { it.isNotEmpty() && it.all { ev -> ev.confirmations >= ev.requiredConfirmations } })
    }

    private fun sum(values: List<Long>): Long = values.fold(0L, Math::addExact)

    private const val MAX_SALES = 2000
    private const val MAX_ATTEMPTS = 100
    private const val MAX_EVIDENCE = 1000
    private const val MAX_REFUNDS = 1000
    private const val MAX_CASH_PAYMENTS = 100
    private const val MAX_ITEMS_PER_SALE = 100
}

/** Stable export deliberately excludes attempt, provider, quote, evidence, refund, and payment IDs. */
object CheckoutReportCsv {
    private val header = listOf(
        "sale_id", "created_at_unix_seconds", "currency", "total_minor", "observed_minor", "applied_minor", "status", "payment_states", "late_bitcoin_receipt",
        "cash_applied_minor", "card_observed_minor", "card_applied_minor", "bitcoin_observed_minor", "bitcoin_observed_sats",
        "bitcoin_applied_minor", "bitcoin_applied_sats", "refund_requested_minor", "refund_paid_minor",
        "refund_requested_sats", "refund_paid_sats", "subtotal_minor", "discount_minor", "tax_minor", "tip_minor", "breakdown_known",
    )

    fun encode(report: CheckoutReport): String = buildString {
        append(header.joinToString(",")).append('\n')
        for (r in report.rows) {
            val fields: List<Any?> = listOf(r.saleId, r.createdAtEpochSeconds, r.currency, r.totalMinor, r.observedMinor,
                r.appliedMinor, r.status, r.paymentStates, r.lateBitcoinReceipt, r.cashAppliedMinor, r.cardObservedMinor, r.cardAppliedMinor,
                r.bitcoinObservedMinor, r.bitcoinObservedSats, r.bitcoinAppliedMinor, r.bitcoinAppliedSats,
                r.refundRequestedMinor, r.refundPaidMinor, r.refundRequestedSats, r.refundPaidSats,
                r.subtotalMinor, r.discountMinor, r.taxMinor, r.tipMinor, r.breakdownKnown)
            append(fields.joinToString(",") { field ->
                when (field) {
                    null -> ""
                    is String -> csvString(field)
                    else -> field.toString()
                }
            }).append('\n')
        }
    }

    private fun csvString(value: String): String {
        val first = value.firstOrNull()
        val firstMeaningful = value.firstOrNull { !it.isWhitespace() && !it.isISOControl() }
        val needsGuard = first != null && (first.isISOControl() ||
            (firstMeaningful != null && firstMeaningful in listOf('=', '+', '@', '-')))
        val clean = buildString(value.length) {
            value.forEach { ch -> append(if (ch.isISOControl()) ' ' else ch) }
        }
        val guarded = if (needsGuard) "'$clean" else clean
        return "\"${guarded.replace("\"", "\"\"")}\""
    }
}
