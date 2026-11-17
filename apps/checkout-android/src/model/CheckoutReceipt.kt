package org.openbmt.checkout

/** A privacy-limited, immutable view of a sale suitable for a customer receipt. */
data class ReceiptProjection(
    val saleId: String,
    val currency: String,
    val totalMinor: Long,
    val observedMinor: Long,
    val appliedMinor: Long,
    val remainingMinor: Long,
    val paid: Boolean,
    val requiresReview: Boolean,
    val items: List<ReceiptItem>,
    val pricing: CheckoutPricingSnapshot? = null,
    val tenders: List<ReceiptTender>,
    val refunds: List<ReceiptRefund>,
)

data class ReceiptItem(val name: String, val unitMinor: Long, val quantity: Int, val modifiers: List<ReceiptModifier> = emptyList())
data class ReceiptModifier(val name: String, val amountMinor: Long)
data class ReceiptObservation(val state: String, val amountMinor: Long, val amountSats: Long)
data class ReceiptTender(
    val tender: String,
    val state: String,
    val requestedMinor: Long,
    val appliedMinor: Long,
    val observedSats: Long,
    val observations: List<ReceiptObservation>,
    val cashTenderedMinor: Long = 0,
    val cashChangeMinor: Long = 0,
    val observedMinor: Long = appliedMinor,
)
data class ReceiptRefund(val state: String, val requestedMinor: Long, val requestedSats: Long,
                         val paidMinor: Long, val paidSats: Long)

/** Pure projection and deterministic plain-text export; no printer, email, or network integration. */
object CheckoutReceipt {
    fun project(sale: Sale): ReceiptProjection {
        require(sale.currency == "USD" || sale.currency == "SAT") { "receipt currency is not supported: ${sale.currency}" }
        val appliedAttempts = sale.attempts.filter(::isSettled).map { it.receivedMinor }
        val appliedMinor = (appliedAttempts + sale.cashPayments.map { it.amountMinor })
            .fold(0L) { total, amount -> Math.addExact(total, amount) }
        return ReceiptProjection(
            sale.saleId, sale.currency, sale.amountMinor, sale.receivedMinor,
            appliedMinor, sale.remainingMinor,
            sale.paid, sale.requiresReview,
            if (sale.pricing == null) sale.items.map { ReceiptItem(sanitizeName(it.name), it.unitMinor, it.quantity) }
            else sale.pricing!!.lines.map { line -> ReceiptItem(sanitizeName(line.name), line.unitMinor, line.quantity,
                line.modifiers.map { ReceiptModifier(sanitizeName(it.name), it.amountMinor) }) },
            sale.pricing,
            sale.attempts.map { attempt ->
                val observations = when (attempt.tender) {
                    Tender.BITCOIN -> attempt.bitcoinEvidence.map { ev ->
                        ReceiptObservation(if (ev.chainState == BitcoinChainState.REORGED) "REORGED" else if (ev.confirmations >= ev.requiredConfirmations) "CONFIRMED" else "UNCONFIRMED", 0, ev.receivedSats)
                    }
                    Tender.CARD -> attempt.cardEvidence.map { ev ->
                        ReceiptObservation(ev.result.name, ev.amountMinor, 0)
                    }
                }
                val applied = if (isSettled(attempt)) attempt.receivedMinor else 0L
                ReceiptTender(attempt.tender.name, attempt.state.name, attempt.amountMinor,
                    applied, attempt.receivedSats, observations, observedMinor = attempt.receivedMinor)
            } + sale.cashPayments.map { cash ->
                ReceiptTender("CASH", "ACKNOWLEDGED", cash.amountMinor, cash.amountMinor, 0,
                    emptyList(), cash.tenderedMinor, cash.changeMinor, observedMinor = cash.amountMinor)
            },
            sale.refunds.map { refund -> ReceiptRefund(refund.state.name, refund.amountMinor, refund.amountSats,
                refund.actuallyPaidMinor, refund.actuallyPaidSats) },
        )
    }

    /** Stable receipt text. Only supported exact integer units are rendered; no float conversion is used. */
    fun receiptText(sale: Sale): String = format(project(sale))

    private fun isSettled(attempt: PaymentAttempt): Boolean = when (attempt.tender) {
        Tender.CARD -> attempt.state == PaymentState.PAID || attempt.state == PaymentState.PARTIAL
        Tender.BITCOIN -> attempt.state == PaymentState.CONFIRMED ||
            (attempt.state == PaymentState.PARTIAL && attempt.bitcoinEvidence
                .filter { it.chainState == BitcoinChainState.ACTIVE && it.receivedSats > 0 }
                .let { active -> active.isNotEmpty() && active.all { it.confirmations >= it.requiredConfirmations } })
    }

    fun format(receipt: ReceiptProjection): String {
        require(receipt.currency == "USD" || receipt.currency == "SAT") { "receipt currency is not supported: ${receipt.currency}" }
        val out = StringBuilder()
        out.append("SALE RECEIPT\nSale: ").append(safeLine(receipt.saleId)).append('\n')
        out.append("Status: ").append(when {
            receipt.requiresReview -> "OWNER REVIEW REQUIRED"
            receipt.paid -> "PAID"
            else -> "INCOMPLETE"
        })
        out.append("\n\nItems\n")
        if (receipt.items.isEmpty()) out.append("(item details unavailable)\n")
        else receipt.items.forEach { item ->
            out.append(item.quantity).append(" × ").append(item.name).append(" — ")
                .append(money(item.unitMinor, receipt.currency)).append(" each\n")
            item.modifiers.forEach { modifier ->
                out.append("  ").append(modifier.name).append(" modifier — +")
                    .append(money(modifier.amountMinor, receipt.currency)).append(" each × ").append(item.quantity)
                    .append(" = +").append(money(Math.multiplyExact(modifier.amountMinor, item.quantity.toLong()), receipt.currency)).append('\n')
            }
        }
        receipt.pricing?.let { pricing ->
            out.append("\nPrice details\n")
            out.append("Subtotal: ").append(money(pricing.subtotalMinor, receipt.currency)).append('\n')
            out.append("Order discount: ").append(if (pricing.discountMinor > 0) "−" else "")
                .append(money(pricing.discountMinor, receipt.currency)).append('\n')
            pricing.taxes.forEach { tax ->
                out.append(safeLine(tax.name)).append(" (").append(tax.treatment.name.lowercase()).append(", ").append(formatRate(tax.rateBasisPoints))
                    .append("%): ").append(if (tax.treatment == TaxTreatment.EXCLUSIVE) "+" else "included/extracted ")
                    .append(money(tax.amountMinor, receipt.currency)).append('\n')
            }
            if (pricing.taxes.isEmpty()) out.append("Tax: ").append(money(pricing.taxMinor, receipt.currency)).append(" (not configured)\n")
            out.append("Tip: +").append(money(pricing.tipMinor, receipt.currency)).append('\n')
        }
        out.append("Total: ").append(money(receipt.totalMinor, receipt.currency)).append('\n')
        out.append("Received / observed: ").append(money(receipt.observedMinor, receipt.currency)).append('\n')
        out.append("Settled / applied: ").append(money(receipt.appliedMinor, receipt.currency)).append('\n')
        if (receipt.remainingMinor > 0) out.append("Remaining: ").append(money(receipt.remainingMinor, receipt.currency)).append('\n')
        if (receipt.tenders.isNotEmpty()) {
            out.append("\nPayments\n")
            receipt.tenders.forEach { tender ->
                out.append(tender.tender).append(" — ").append(tender.state)
                    .append("; observed ").append(money(tender.observedMinor, receipt.currency))
                    .append("; settled ").append(money(tender.appliedMinor, receipt.currency))
                if (tender.tender == "CASH") out.append("; tendered ").append(money(tender.cashTenderedMinor, receipt.currency))
                    .append("; change ").append(money(tender.cashChangeMinor, receipt.currency))
                out.append('\n')
                tender.observations.forEach { observation ->
                    out.append("  Observation: ").append(observation.state)
                    val observed = if (observation.amountSats > 0) money(observation.amountSats, "SAT")
                        else money(observation.amountMinor, receipt.currency)
                    out.append(" — ").append(observed).append('\n')
                }
            }
        }
        if (receipt.refunds.isNotEmpty()) {
            out.append("\nRefunds\n")
            receipt.refunds.forEach { refund ->
                out.append(refund.state).append(" — requested ")
                    .append(if (refund.requestedSats > 0) money(refund.requestedSats, "SAT") else money(refund.requestedMinor, receipt.currency))
                    .append("; paid out ")
                    .append(if (refund.paidSats > 0) money(refund.paidSats, "SAT") else money(refund.paidMinor, receipt.currency))
                    .append('\n')
            }
        }
        return out.toString()
    }

    private fun money(value: Long, currency: String): String {
        require(value >= 0) { "negative receipt amount" }
        return when (currency) {
            "USD" -> "USD ${value / 100}.${(value % 100).toString().padStart(2, '0')}"
            "SAT" -> "${value} sats"
            else -> throw IllegalArgumentException("receipt currency is not supported: $currency")
        }
    }

    private fun formatRate(basisPoints: Int): String = "${basisPoints / 100}.${(basisPoints % 100).toString().padStart(2, '0')}"

    private fun sanitizeName(value: String): String = safeLine(value.take(160))
    private fun safeLine(value: String): String = buildString {
        for (ch in value) {
            when {
                ch == '\n' || ch == '\r' || ch == '\t' || ch.isISOControl() -> append(' ')
                else -> append(ch)
            }
        }
    }.trim()
}
