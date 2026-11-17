package org.openbmt.checkout

import java.util.Locale

enum class HistoryTender { BITCOIN, CARD, CASH }
enum class HistoryStatus { PAID, UNPAID, REVIEW }

/** A pure filter over caller-provided sale snapshots. Times are Unix epoch seconds. */
data class SaleHistoryQuery(
    val text: String = "",
    val tender: HistoryTender? = null,
    val status: HistoryStatus? = null,
    val fromInclusive: Long? = null,
    val untilExclusive: Long? = null,
)

/**
 * Searches stable sale IDs and immutable basket names. Tender matches mean that a payment
 * attempt was recorded, even if its final result is unknown; this is history, not income reporting.
 * Review status has priority over paid state so owner-review sales remain discoverable.
 */
fun searchSales(sales: List<Sale>, query: SaleHistoryQuery): List<Sale> {
    require(query.text.length <= MAX_HISTORY_QUERY_LENGTH) { "search text is too long" }
    require(query.fromInclusive == null || query.fromInclusive >= 0) { "invalid start time" }
    require(query.untilExclusive == null || query.untilExclusive >= 0) { "invalid end time" }
    require(query.fromInclusive == null || query.untilExclusive == null || query.fromInclusive < query.untilExclusive) {
        "invalid time range"
    }
    val needle = query.text.trim().lowercase(Locale.ROOT)
    return sales.asSequence()
        .filter { sale ->
            (needle.isEmpty() || sale.saleId.lowercase(Locale.ROOT).contains(needle) ||
                sale.items.any { it.name.lowercase(Locale.ROOT).contains(needle) }) &&
                (query.fromInclusive == null || sale.createdAt >= query.fromInclusive) &&
                (query.untilExclusive == null || sale.createdAt < query.untilExclusive) &&
                matchesTender(sale, query.tender) && matchesStatus(sale, query.status)
        }
        .sortedWith(compareByDescending<Sale> { it.createdAt }.thenBy { it.saleId })
        .toList()
}

private const val MAX_HISTORY_QUERY_LENGTH = 128

private fun matchesTender(sale: Sale, tender: HistoryTender?): Boolean = when (tender) {
    null -> true
    HistoryTender.BITCOIN -> sale.attempts.any { it.tender == Tender.BITCOIN }
    HistoryTender.CARD -> sale.attempts.any { it.tender == Tender.CARD }
    HistoryTender.CASH -> sale.cashPayments.isNotEmpty()
}

private fun matchesStatus(sale: Sale, status: HistoryStatus?): Boolean = when (status) {
    null -> true
    HistoryStatus.REVIEW -> sale.requiresReview
    HistoryStatus.PAID -> sale.paid && !sale.requiresReview
    HistoryStatus.UNPAID -> !sale.paid && !sale.requiresReview
}
