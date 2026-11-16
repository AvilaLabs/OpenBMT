package org.openbmt.checkout

import java.io.ByteArrayInputStream
import java.io.ByteArrayOutputStream
import java.io.DataInputStream
import java.io.DataOutputStream
import java.nio.charset.CodingErrorAction
import java.nio.ByteBuffer
import java.nio.channels.FileChannel
import java.nio.file.Files
import java.nio.file.Path
import java.nio.file.StandardCopyOption
import java.nio.file.StandardOpenOption

private const val MAX_LEDGER_FILE_BYTES = 4 * 1024 * 1024

/** Writes may throw after an uncertain/committed rename; callers must reload and never reuse stale state. */
interface DurableLedgerStorage {
    fun read(): ByteArray?
    fun writeAtomically(bytes: ByteArray)
}

/** JVM/desktop implementation. Android should provide an adapter using its app-private files dir. */
class NioLedgerStorage(private val file: Path) : DurableLedgerStorage {
    override fun read(): ByteArray? {
        if (!Files.exists(file)) return null
        Files.newInputStream(file).use { input ->
            val out = ByteArrayOutputStream()
            val chunk = ByteArray(8192)
            while (true) {
                val n = input.read(chunk)
                if (n < 0) break
                require(out.size().toLong() + n <= MAX_LEDGER_FILE_BYTES) { "ledger file exceeds size limit" }
                out.write(chunk, 0, n)
            }
            return out.toByteArray()
        }
    }

    override fun writeAtomically(bytes: ByteArray) {
        require(bytes.size <= MAX_LEDGER_FILE_BYTES) { "ledger exceeds storage limit" }
        val parent = file.toAbsolutePath().parent
        Files.createDirectories(parent)
        val temp = Files.createTempFile(parent, ".checkout-ledger-", ".tmp")
        try {
            FileChannel.open(temp, StandardOpenOption.WRITE, StandardOpenOption.TRUNCATE_EXISTING).use { ch ->
                var buffer = ByteBuffer.wrap(bytes)
                while (buffer.hasRemaining()) ch.write(buffer)
                ch.force(true)
            }
            Files.move(temp, file, StandardCopyOption.ATOMIC_MOVE, StandardCopyOption.REPLACE_EXISTING)
            // Persist the directory entry; report failure as an uncertain write outcome.
            FileChannel.open(parent, StandardOpenOption.READ).use { it.force(true) }
        } finally { Files.deleteIfExists(temp) }
    }

}

enum class Tender { BITCOIN, CARD }
enum class BitcoinChainState { ACTIVE, REORGED }
enum class CardResult { PENDING, APPROVED, COMPLETED, CANCELED, FAILED }
enum class PaymentState { AWAITING, UNKNOWN, UNCONFIRMED, PARTIAL, CONFIRMED, REORGED, PAID, CANCELED }
enum class RefundState { REQUESTED, PENDING, PARTIAL, PAID, FAILED }

data class BitcoinEvidence(
    val evidenceId: String,
    val sourceId: String,
    val receivedSats: Long,
    val confirmations: Int,
    val requiredConfirmations: Int,
    val observedAt: Long,
    val chainState: BitcoinChainState,
    val historicalConfirmedSats: Long = 0,
)

data class CardEvidence(
    val evidenceId: String,
    val stableId: String,
    val result: CardResult,
    val amountMinor: Long,
    val observedAt: Long,
    val historicalCompletedMinor: Long = 0,
)

data class RefundEvidence(
    val evidenceId: String,
    val paidMinor: Long,
    val paidSats: Long,
    val pending: Boolean,
    val observedAt: Long,
)

data class PaymentAttempt(
    val attemptId: String,
    val tender: Tender,
    val amountMinor: Long,
    val expectedSats: Long?,
    val quoteId: String?,
    val quotedAt: Long?,
    val counterIndex: Long?,
    val provider: String?,
    val stableId: String?,
    val cancellationRequested: Boolean,
    val cancellationAcknowledged: Boolean,
    val bitcoinEvidence: List<BitcoinEvidence>,
    val cardEvidence: List<CardEvidence>,
    val state: PaymentState,
    val receivedMinor: Long,
    val receivedSats: Long,
)

data class Refund(
    val refundId: String,
    val paymentAttemptId: String,
    val amountMinor: Long,
    val amountSats: Long,
    val requestedAt: Long,
    val evidence: List<RefundEvidence>,
    val state: RefundState,
    val actuallyPaidMinor: Long,
    val actuallyPaidSats: Long,
)

data class Sale(
    val saleId: String,
    val amountMinor: Long,
    val currency: String,
    val createdAt: Long,
    val attempts: List<PaymentAttempt>,
    val refunds: List<Refund>,
    val receivedMinor: Long,
    val remainingMinor: Long,
    val paid: Boolean,
    val requiresReview: Boolean,
    val lateBitcoinReceipt: Boolean,
    val overpaidMinor: Long,
    val overpaidSats: Long,
)

/** Append-only observations and stable attempt identifiers. Every successful mutation is durable on return. */
class CheckoutLedger(private val storage: DurableLedgerStorage) {
    private val lock = Any()
    private var state: State = decode(storage.read())
    private var poisoned = false

    fun createSale(saleId: String, amountMinor: Long, currency: String, createdAt: Long): Sale = mutate {
        validId(saleId, "sale id"); positive(amountMinor, "sale amount"); validCurrency(currency)
        require(createdAt >= 0) { "invalid creation time" }
        require(!sales.containsKey(saleId)) { "sale already exists" }
        require(sales.size < MAX_SALES) { "sale limit reached" }
        sales[saleId] = SaleData(saleId, amountMinor, currency, createdAt)
        view(sales.getValue(saleId))
    }

    /** Must complete before invoking the external receiver/payment SDK. counterIndex is the existing payment request index. */
    fun beginBitcoinAttempt(saleId: String, attemptId: String, counterIndex: Long, expectedMinor: Long,
                            expectedSats: Long, quoteId: String, quotedAt: Long): PaymentAttempt = mutate {
        val sale = sale(saleId); validId(attemptId, "attempt id"); positive(expectedMinor, "attempt amount")
        positive(expectedSats, "quoted sats"); validId(quoteId, "quote id"); require(counterIndex >= 0 && quotedAt >= 0) { "invalid quote/index" }
        require(expectedMinor <= view(sale).remainingMinor) { "attempt exceeds unpaid balance" }
        ensureNoUncertainCard(sale)
        require(sale.attempts.none { it.attemptId == attemptId }) { "attempt id already exists" }
        require(state.sales.values.none { other -> other.attempts.any { it.tender == Tender.BITCOIN && it.counterIndex == counterIndex } }) { "counter index already used" }
        require(sale.attempts.size < MAX_ATTEMPTS) { "attempt limit reached" }
        require(canStartTender(sale)) { "another payment attempt must be resolved first" }
        val a = AttemptData(attemptId, Tender.BITCOIN, expectedMinor, expectedSats, quoteId, quotedAt, counterIndex, null, null)
        sale.attempts.add(a); attemptView(a)
    }

    /** Persist the stable provider id before calling the card SDK. */
    fun beginCardAttempt(saleId: String, attemptId: String, provider: String, stableId: String, amountMinor: Long): PaymentAttempt = mutate {
        val sale = sale(saleId); validId(attemptId, "attempt id"); validId(stableId, "provider idempotency id")
        validText(provider, "provider"); positive(amountMinor, "attempt amount")
        require(amountMinor <= view(sale).remainingMinor) { "attempt exceeds unpaid balance" }
        require(sale.attempts.none { it.attemptId == attemptId }) { "attempt id already exists" }
        require(state.sales.values.none { other -> other.attempts.any { it.tender == Tender.CARD && it.provider == provider && it.stableId == stableId } }) { "provider id already used" }
        require(sale.attempts.none { it.tender == Tender.CARD && cardBlocksRetry(it) }) { "card attempt must be reconciled first" }
        require(canStartTender(sale)) { "another payment attempt must be resolved first" }
        require(sale.attempts.size < MAX_ATTEMPTS) { "attempt limit reached" }
        val a = AttemptData(attemptId, Tender.CARD, amountMinor, null, null, null, null, provider, stableId)
        sale.attempts.add(a); attemptView(a)
    }

    fun requestCancellation(saleId: String, attemptId: String): PaymentAttempt = mutate {
        val a = attempt(saleId, attemptId); a.cancellationRequested = true; a.cancellationAcknowledged = false; a.cancellationUnknown = false; attemptView(a)
    }

    /** Call only after the native CounterNode.cancel operation returns success. Late evidence can reopen the derived state. */
    fun acknowledgeBitcoinCancellation(saleId: String, attemptId: String): PaymentAttempt = mutate {
        val a = attempt(saleId, attemptId); require(a.tender == Tender.BITCOIN) { "not a Bitcoin attempt" }
        require(a.cancellationRequested) { "cancellation was not requested" }
        a.cancellationAcknowledged = true
        attemptView(a)
    }

    /** Reconciliation response that says the cancel outcome is still unknown. */
    fun markCancellationUnknown(saleId: String, attemptId: String): PaymentAttempt = mutate {
        val a = attempt(saleId, attemptId); require(a.tender == Tender.CARD) { "cancellation reconciliation applies to card attempts" }
        a.cancellationRequested = true; a.cancellationUnknown = true; attemptView(a)
    }

    /** receivedSats is the current cumulative amount for evidenceId, not a fiat conversion or a delta. */
    fun recordBitcoinEvidence(saleId: String, attemptId: String, evidenceId: String, sourceId: String, receivedSats: Long,
                              confirmations: Int, requiredConfirmations: Int, observedAt: Long,
                              chainState: BitcoinChainState = BitcoinChainState.ACTIVE): PaymentAttempt = mutate {
        val a = attempt(saleId, attemptId); require(a.tender == Tender.BITCOIN) { "not a Bitcoin attempt" }
        validId(evidenceId, "evidence id"); validId(sourceId, "evidence source"); nonnegative(receivedSats, "received sats")
        require(confirmations >= 0 && requiredConfirmations >= 0 && observedAt >= 0) { "invalid evidence values" }
        require(requiredConfirmations >= 1) { "confirmation threshold must be positive" }
        require(a.bitcoinEvidence.none { it.sourceId == sourceId && it.evidenceId != evidenceId }) { "Bitcoin output source already recorded" }
        require(a.bitcoinEvidence.size < MAX_EVIDENCE || a.bitcoinEvidence.any { it.evidenceId == evidenceId }) { "evidence limit reached" }
        val priorConfirmed = a.bitcoinEvidence.firstOrNull { it.evidenceId == evidenceId }?.historicalConfirmedSats ?: 0L
        val historicalConfirmed = maxOf(priorConfirmed, if (chainState == BitcoinChainState.ACTIVE && confirmations >= requiredConfirmations) receivedSats else 0L)
        replaceSameId(a.bitcoinEvidence, evidenceId, BitcoinEvidence(evidenceId, sourceId, receivedSats, confirmations, requiredConfirmations, observedAt, chainState, historicalConfirmed))
        attemptView(a)
    }

    fun recordCardEvidence(saleId: String, attemptId: String, evidenceId: String, stableId: String,
                           result: CardResult, amountMinor: Long, observedAt: Long): PaymentAttempt = mutate {
        val a = attempt(saleId, attemptId); require(a.tender == Tender.CARD) { "not a card attempt" }
        validId(evidenceId, "evidence id"); require(stableId == a.stableId) { "provider id does not match attempt" }
        nonnegative(amountMinor, "card amount"); require(amountMinor <= a.amountMinor) { "card amount exceeds attempt" }
        require(observedAt >= 0) { "invalid observation time" }
        require(a.cardEvidence.size < MAX_EVIDENCE || a.cardEvidence.any { it.evidenceId == evidenceId }) { "evidence limit reached" }
        val priorCompleted = a.cardEvidence.firstOrNull { it.evidenceId == evidenceId }?.historicalCompletedMinor ?: 0L
        val historicalCompleted = maxOf(priorCompleted, if (result == CardResult.COMPLETED) amountMinor else 0L)
        replaceSameId(a.cardEvidence, evidenceId, CardEvidence(evidenceId, stableId, result, amountMinor, observedAt, historicalCompleted))
        if (result == CardResult.COMPLETED || result == CardResult.CANCELED || result == CardResult.FAILED) a.cancellationUnknown = false
        attemptView(a)
    }

    fun requestRefund(saleId: String, refundId: String, paymentAttemptId: String, amountMinor: Long, amountSats: Long, requestedAt: Long): Refund = mutate {
        val sale = sale(saleId); validId(refundId, "refund id"); nonnegative(amountMinor, "refund amount"); nonnegative(amountSats, "refund sats")
        require(requestedAt >= 0) { "invalid request time" }
        val payment = sale.attempts.firstOrNull { it.attemptId == paymentAttemptId } ?: error("payment attempt not found")
        require(paymentState(payment).let { it == PaymentState.PAID || it == PaymentState.CONFIRMED || it == PaymentState.PARTIAL }) { "payment is not settled" }
        val prior = sale.refunds.filter { it.paymentAttemptId == paymentAttemptId }
        if (payment.tender == Tender.BITCOIN) {
            val remainingSats = paidSats(payment) - safeSum(prior.map { it.amountSats })
            require(amountMinor == 0L && amountSats in 1..remainingSats) { "invalid Bitcoin refund amount" }
        } else {
            val remainingMinor = paidMinor(payment) - safeSum(prior.map { it.amountMinor })
            require(amountSats == 0L && amountMinor in 1..remainingMinor) { "invalid card refund amount" }
        }
        require(sale.refunds.none { it.refundId == refundId }) { "refund id already exists" }
        require(sale.refunds.size < MAX_REFUNDS) { "refund limit reached" }
        val r = RefundData(refundId, paymentAttemptId, amountMinor, amountSats, requestedAt)
        sale.refunds.add(r); refundView(r)
    }

    /** pending evidence means no funds are claimed returned; paid evidence accumulates to the requested amount. */
    fun recordRefundEvidence(saleId: String, refundId: String, evidenceId: String, actualPaidMinor: Long, actualPaidSats: Long,
                             pending: Boolean, observedAt: Long): Refund = mutate {
        val r = refund(saleId, refundId); validId(evidenceId, "refund evidence id")
        nonnegative(actualPaidMinor, "refund paid amount"); nonnegative(actualPaidSats, "refund paid sats")
        require(actualPaidMinor <= r.amountMinor && actualPaidSats <= r.amountSats) { "refund evidence exceeds request" }
        require(observedAt >= 0) { "invalid observation time" }
        require(!pending || actualPaidMinor == 0L && actualPaidSats == 0L) { "pending refund cannot be counted as paid" }
        require(r.evidence.size < MAX_EVIDENCE || r.evidence.any { it.evidenceId == evidenceId }) { "refund evidence limit reached" }
        replaceSameId(r.evidence, evidenceId, RefundEvidence(evidenceId, actualPaidMinor, actualPaidSats, pending, observedAt))
        val associated = sale(saleId).attempts.first { it.attemptId == r.paymentAttemptId }
        require(refundView(r).actuallyPaidMinor <= r.amountMinor && refundView(r).actuallyPaidSats <= r.amountSats)
        require((associated.tender == Tender.CARD && actualPaidSats == 0L) || (associated.tender == Tender.BITCOIN && actualPaidMinor == 0L)) { "refund units do not match tender" }
        refundView(r)
    }

    fun getSale(saleId: String): Sale = synchronized(lock) { ensureHealthy(); view(sale(saleId)) }
    fun listSales(): List<Sale> = synchronized(lock) { ensureHealthy(); state.sales.values.map(::view) }

    private inline fun <T> mutate(block: State.() -> T): T = synchronized(lock) {
        ensureHealthy()
        val before = encode(state)
        val result = try { state.block() } catch (e: Throwable) { state = decode(before); throw e }
        val next = try { encode(state) } catch (e: Throwable) { state = decode(before); throw e }
        try { storage.writeAtomically(next) } catch (e: Throwable) {
            poisoned = true
            try { state = decode(storage.read()) } catch (_: Throwable) { }
            throw IllegalStateException("ledger write outcome is uncertain; reopen before continuing", e)
        }
        result
    }

    private fun ensureHealthy() { check(!poisoned) { "ledger write outcome is uncertain; reopen before continuing" } }

    private fun sale(id: String) = state.sales[id] ?: error("sale not found")
    private fun attempt(saleId: String, attemptId: String) = sale(saleId).attempts.firstOrNull { it.attemptId == attemptId } ?: error("attempt not found")
    private fun refund(saleId: String, refundId: String) = sale(saleId).refunds.firstOrNull { it.refundId == refundId } ?: error("refund not found")
    private fun ensureNoUncertainCard(s: SaleData) { require(s.attempts.none { it.tender == Tender.CARD && cardBlocksRetry(it) }) { "card attempt must be reconciled first" } }
    private fun canStartTender(s: SaleData) = s.attempts.none { a ->
        a.tender == Tender.CARD && cardBlocksRetry(a) || a.tender == Tender.BITCOIN && paymentState(a) != PaymentState.CANCELED
    }
    private fun cardBlocksRetry(a: AttemptData) = a.cancellationUnknown || when (a.cardEvidence.maxByOrNull { it.observedAt }?.result) {
        null, CardResult.PENDING, CardResult.APPROVED -> true
        else -> false
    }

    private fun paymentState(a: AttemptData): PaymentState = if (a.tender == Tender.CARD) {
        when (a.cardEvidence.maxByOrNull { it.observedAt }?.result) {
            null, CardResult.PENDING, CardResult.APPROVED -> PaymentState.UNKNOWN
            CardResult.CANCELED, CardResult.FAILED -> PaymentState.CANCELED
            CardResult.COMPLETED -> if ((a.cardEvidence.maxByOrNull { it.observedAt }?.amountMinor ?: 0) >= a.amountMinor) PaymentState.PAID else PaymentState.PARTIAL
        }
    } else {
        val active = a.bitcoinEvidence.filter { it.chainState == BitcoinChainState.ACTIVE }
        val amount = safeSum(active.map { it.receivedSats })
        val confirmed = active.filter { it.receivedSats > 0 }.all { it.confirmations >= it.requiredConfirmations }
        when {
            a.bitcoinEvidence.any { it.receivedSats > 0 } && active.none { it.receivedSats > 0 } -> PaymentState.REORGED
            amount == 0L -> if (a.cancellationAcknowledged) PaymentState.CANCELED else PaymentState.AWAITING
            confirmed -> if (amount >= a.expectedSats!!) PaymentState.CONFIRMED else PaymentState.PARTIAL
            !confirmed -> PaymentState.UNCONFIRMED
            else -> PaymentState.PARTIAL
        }
    }
    private fun received(a: AttemptData): Long = when (a.tender) {
        Tender.BITCOIN -> fiatForSats(a)
        Tender.CARD -> if (paymentState(a) == PaymentState.PAID || paymentState(a) == PaymentState.PARTIAL) a.cardEvidence.maxBy { it.observedAt }.amountMinor else 0L
    }
    private fun paidMinor(a: AttemptData) = if (a.tender == Tender.CARD) received(a) else 0L
    private fun paidSats(a: AttemptData) = if (a.tender == Tender.BITCOIN) safeSum(a.bitcoinEvidence.filter { it.chainState == BitcoinChainState.ACTIVE }.map { it.receivedSats }) else 0L
    private fun fiatForSats(a: AttemptData): Long {
        if (a.tender != Tender.BITCOIN) return received(a)
        val sats = paidSats(a)
        return java.math.BigInteger.valueOf(a.amountMinor).multiply(java.math.BigInteger.valueOf(sats))
            .divide(java.math.BigInteger.valueOf(a.expectedSats!!)).min(java.math.BigInteger.valueOf(a.amountMinor)).toLong()
    }
    private fun attemptView(a: AttemptData) = PaymentAttempt(a.attemptId, a.tender, a.amountMinor, a.expectedSats, a.quoteId, a.quotedAt, a.counterIndex, a.provider, a.stableId,
        a.cancellationRequested, a.cancellationAcknowledged, a.bitcoinEvidence.toList(), a.cardEvidence.toList(), paymentState(a), received(a), paidSats(a))
    private fun refundView(r: RefundData): Refund {
        val actual = safeSum(r.evidence.filterNot { it.pending }.map { it.paidMinor })
        val actualSats = safeSum(r.evidence.filterNot { it.pending }.map { it.paidSats })
        val state = when {
            actual >= r.amountMinor && actualSats >= r.amountSats -> RefundState.PAID
            actual > 0 || actualSats > 0 -> RefundState.PARTIAL
            r.evidence.any { it.pending } -> RefundState.PENDING
            else -> RefundState.REQUESTED
        }
        return Refund(r.refundId, r.paymentAttemptId, r.amountMinor, r.amountSats, r.requestedAt, r.evidence.toList(), state, actual, actualSats)
    }
    private fun view(s: SaleData): Sale {
        val attempts = s.attempts.map(::attemptView)
        val refunds = s.refunds.map(::refundView)
        val total = safeSum(attempts.map { it.receivedMinor })
        val overpaidMinor = (total - s.amountMinor).coerceAtLeast(0L)
        val overpaidSats = safeSum(saleOverpaidSats(s))
        val lateBitcoinReceipt = s.attempts.any { a -> a.tender == Tender.BITCOIN && a.cancellationAcknowledged && a.bitcoinEvidence.any { it.receivedSats > 0 } }
        val refundAtRisk = s.refunds.groupBy { it.paymentAttemptId }.any { (attemptId, requests) ->
            val payment = s.attempts.first { it.attemptId == attemptId }
            if (payment.tender == Tender.BITCOIN) safeSum(requests.map { it.amountSats }) > paidSats(payment)
            else safeSum(requests.map { it.amountMinor }) > paidMinor(payment)
        }
        val requiresReview = lateBitcoinReceipt || overpaidMinor > 0 || overpaidSats > 0 || refundAtRisk
        val settled = safeSum(attempts.filter { it.state == PaymentState.PAID || it.state == PaymentState.CONFIRMED ||
            (it.state == PaymentState.PARTIAL && (it.tender == Tender.CARD || it.bitcoinEvidence.filter { ev -> ev.chainState == BitcoinChainState.ACTIVE && ev.receivedSats > 0 }.all { ev -> ev.confirmations >= ev.requiredConfirmations }))
        }.map { it.receivedMinor })
        return Sale(s.id, s.amountMinor, s.currency, s.createdAt, attempts, refunds, total,
            (s.amountMinor - total).coerceAtLeast(0), settled >= s.amountMinor && settled > 0 && !requiresReview,
            requiresReview, lateBitcoinReceipt, overpaidMinor, overpaidSats)
    }

    private fun saleOverpaidSats(s: SaleData): List<Long> = s.attempts.filter { it.tender == Tender.BITCOIN }.map { a ->
        (paidSats(a) - a.expectedSats!!).coerceAtLeast(0L)
    }

    private class State(val sales: LinkedHashMap<String, SaleData> = LinkedHashMap())
    private data class SaleData(val id: String, val amountMinor: Long, val currency: String, val createdAt: Long,
                                val attempts: MutableList<AttemptData> = ArrayList(), val refunds: MutableList<RefundData> = ArrayList())
    private data class AttemptData(val attemptId: String, val tender: Tender, val amountMinor: Long, val expectedSats: Long?, val quoteId: String?, val quotedAt: Long?, val counterIndex: Long?,
                                   val provider: String?, val stableId: String?, var cancellationRequested: Boolean = false,
                                   var cancellationAcknowledged: Boolean = false,
                                   var cancellationUnknown: Boolean = false, val bitcoinEvidence: MutableList<BitcoinEvidence> = ArrayList(),
                                   val cardEvidence: MutableList<CardEvidence> = ArrayList())
    private data class RefundData(val refundId: String, val paymentAttemptId: String, val amountMinor: Long, val amountSats: Long, val requestedAt: Long,
                                  val evidence: MutableList<RefundEvidence> = ArrayList())

    private fun <T> replaceSameId(list: MutableList<T>, id: String, value: T) {
        val index = when (value) {
            is BitcoinEvidence -> list.indexOfFirst { (it as BitcoinEvidence).evidenceId == id }
            is CardEvidence -> list.indexOfFirst { (it as CardEvidence).evidenceId == id }
            is RefundEvidence -> list.indexOfFirst { (it as RefundEvidence).evidenceId == id }
            else -> -1
        }
        if (index < 0) { list.add(value); return }
        val old = list[index]
        val oldAt = when (old) { is BitcoinEvidence -> old.observedAt; is CardEvidence -> old.observedAt; is RefundEvidence -> old.observedAt; else -> -1 }
        val newAt = when (value) { is BitcoinEvidence -> value.observedAt; is CardEvidence -> value.observedAt; is RefundEvidence -> value.observedAt; else -> -1 }
        require(newAt >= oldAt) { "stale evidence" }
        if (newAt == oldAt) require(old == value) { "conflicting duplicate evidence" }
        else {
            if (old is BitcoinEvidence && value is BitcoinEvidence) require(old.sourceId == value.sourceId) { "evidence source changed" }
            if (old is CardEvidence && value is CardEvidence) require(old.stableId == value.stableId) { "provider evidence id changed" }
            list[index] = value
        }
    }

    private fun validId(v: String, label: String) { require(v.isNotBlank() && v.length <= 160 && v.all { it.code in 0x21..0x7e }) { "invalid $label" } }
    private fun validText(v: String, label: String) { require(v.isNotBlank() && v.length <= 100 && v.none { it.isISOControl() }) { "invalid $label" } }
    private fun validCurrency(v: String) { require(v.length == 3 && v.all { it in 'A'..'Z' }) { "invalid currency" } }
    private fun positive(v: Long, label: String) { require(v > 0) { "invalid $label" } }
    private fun nonnegative(v: Long, label: String) { require(v >= 0) { "invalid $label" } }
    private fun safeSum(values: Iterable<Long>): Long = values.fold(0L) { sum, value -> Math.addExact(sum, value) }

    private fun validateDecoded(s: State) {
        val indexes = HashSet<Long>()
        val cardIds = HashSet<Pair<String, String>>()
        for (sale in s.sales.values) {
            require(sale.attempts.size <= MAX_ATTEMPTS && sale.refunds.size <= MAX_REFUNDS)
            for (a in sale.attempts) {
                require(a.bitcoinEvidence.size <= MAX_EVIDENCE && a.cardEvidence.size <= MAX_EVIDENCE)
                if (a.tender == Tender.BITCOIN) require(indexes.add(a.counterIndex!!)) { "Bitcoin counter index is duplicated" }
                if (a.tender == Tender.CARD) require(cardIds.add(a.provider!! to a.stableId!!)) { "card provider id is duplicated" }
                safeSum(a.bitcoinEvidence.map { it.receivedSats })
            }
            val reserved = sale.refunds.groupBy { it.paymentAttemptId }
            for ((attemptId, requests) in reserved) {
                val a = sale.attempts.first { it.attemptId == attemptId }
                val minor = safeSum(requests.map { it.amountMinor })
                val sats = safeSum(requests.map { it.amountSats })
                val historicalMinor = if (a.tender == Tender.CARD) a.cardEvidence.maxOfOrNull { it.historicalCompletedMinor } ?: 0L else 0L
                val historicalSats = if (a.tender == Tender.BITCOIN) safeSum(a.bitcoinEvidence.map { it.historicalConfirmedSats }) else 0L
                require(minor <= historicalMinor && sats <= historicalSats) { "refund requests exceed historically settled payment" }
                for (refund in requests) {
                    require(refund.evidence.size <= MAX_EVIDENCE)
                    val view = refundView(refund)
                    require(view.actuallyPaidMinor <= refund.amountMinor && view.actuallyPaidSats <= refund.amountSats) { "refund evidence exceeds request" }
                }
            }
            safeSum(sale.attempts.map { if (it.tender == Tender.BITCOIN) fiatForSats(it) else received(it) })
        }
    }

    private fun encode(s: State): ByteArray {
        require(s.sales.size <= MAX_SALES) { "sale limit reached" }
        val bytes = ByteArrayOutputStream()
        DataOutputStream(bytes).use { out ->
            out.writeInt(MAGIC); out.writeInt(VERSION); out.writeInt(s.sales.size)
            for (sale in s.sales.values) {
                out.str(sale.id); out.writeLong(sale.amountMinor); out.str(sale.currency); out.writeLong(sale.createdAt)
                out.writeInt(sale.attempts.size)
                for (a in sale.attempts) {
                    out.str(a.attemptId); out.writeInt(a.tender.ordinal); out.writeLong(a.amountMinor); out.nullLong(a.expectedSats); out.nullStr(a.quoteId); out.nullLong(a.quotedAt); out.nullLong(a.counterIndex)
                    out.nullStr(a.provider); out.nullStr(a.stableId); out.writeBoolean(a.cancellationRequested); out.writeBoolean(a.cancellationAcknowledged); out.writeBoolean(a.cancellationUnknown)
                    out.writeInt(a.bitcoinEvidence.size)
                    for (e in a.bitcoinEvidence) { out.str(e.evidenceId); out.str(e.sourceId); out.writeLong(e.receivedSats); out.writeInt(e.confirmations); out.writeInt(e.requiredConfirmations); out.writeLong(e.observedAt); out.writeInt(e.chainState.ordinal); out.writeLong(e.historicalConfirmedSats) }
                    out.writeInt(a.cardEvidence.size)
                    for (e in a.cardEvidence) { out.str(e.evidenceId); out.str(e.stableId); out.writeInt(e.result.ordinal); out.writeLong(e.amountMinor); out.writeLong(e.observedAt); out.writeLong(e.historicalCompletedMinor) }
                }
                out.writeInt(sale.refunds.size)
                for (r in sale.refunds) {
                    out.str(r.refundId); out.str(r.paymentAttemptId); out.writeLong(r.amountMinor); out.writeLong(r.amountSats); out.writeLong(r.requestedAt); out.writeInt(r.evidence.size)
                    for (e in r.evidence) { out.str(e.evidenceId); out.writeLong(e.paidMinor); out.writeLong(e.paidSats); out.writeBoolean(e.pending); out.writeLong(e.observedAt) }
                }
            }
        }
        return bytes.toByteArray().also { require(it.size <= MAX_LEDGER_FILE_BYTES) { "ledger exceeds storage limit" } }
    }

    private fun decode(bytes: ByteArray?): State {
        if (bytes == null) return State()
        require(bytes.size <= MAX_LEDGER_FILE_BYTES) { "ledger file exceeds size limit" }
        try {
            DataInputStream(ByteArrayInputStream(bytes)).use { input ->
                require(input.readInt() == MAGIC && input.readInt() == VERSION) { "unknown or corrupt ledger format" }
                val sales = input.count(MAX_SALES); val state = State()
                repeat(sales) {
                    val id = input.str(); val amount = input.readLong(); val currency = input.str(); val created = input.readLong()
                    validId(id, "sale id"); positive(amount, "sale amount"); validCurrency(currency); nonnegative(created, "creation time")
                    require(!state.sales.containsKey(id)) { "duplicate sale" }
                    val s = SaleData(id, amount, currency, created); state.sales[id] = s
                    val attempts = input.count(MAX_ATTEMPTS)
                    repeat(attempts) {
                        val aid = input.str(); val tender = Tender.entries.getOrNull(input.readInt()) ?: error("invalid tender")
                        val aamount = input.readLong(); val expectedSats = input.nullLong(); val quoteId = input.nullStr(); val quotedAt = input.nullLong(); val counter = input.nullLong(); val provider = input.nullStr(); val stable = input.nullStr()
                        val cancel = input.readBool(); val cancelAck = input.readBool(); val cancelUnknown = input.readBool()
                        validId(aid, "attempt id"); positive(aamount, "attempt amount"); require(aamount <= amount) { "attempt exceeds sale" }
                        if (tender == Tender.BITCOIN) require(counter != null && provider == null && stable == null && counter >= 0 && expectedSats != null && expectedSats > 0 && quoteId != null && quotedAt != null && quotedAt >= 0) { "invalid Bitcoin attempt" }
                        else require(counter == null && expectedSats == null && quoteId == null && quotedAt == null && provider != null && stable != null) { "invalid card attempt" }
                        require(!cancelUnknown || tender == Tender.CARD) { "invalid cancellation uncertainty" }
                        require(!cancelAck || tender == Tender.BITCOIN && cancel) { "invalid cancellation acknowledgement" }
                        if (tender == Tender.CARD) { validText(provider!!, "provider"); validId(stable!!, "provider id") }
                        require(s.attempts.none { it.attemptId == aid }) { "duplicate attempt" }
                        val a = AttemptData(aid, tender, aamount, expectedSats, quoteId, quotedAt, counter, provider, stable, cancel, cancelAck, cancelUnknown); s.attempts.add(a)
                        repeat(input.count(MAX_EVIDENCE)) {
                            val ev = BitcoinEvidence(input.str(), input.str(), input.readLong(), input.readInt(), input.readInt(), input.readLong(), BitcoinChainState.entries.getOrNull(input.readInt()) ?: error("invalid chain state"), input.readLong())
                            validId(ev.evidenceId, "Bitcoin evidence id"); validId(ev.sourceId, "Bitcoin evidence source")
                            nonnegative(ev.receivedSats, "received sats"); require(ev.confirmations >= 0 && ev.requiredConfirmations >= 1); nonnegative(ev.observedAt, "evidence time"); nonnegative(ev.historicalConfirmedSats, "historical confirmed sats")
                            require(ev.historicalConfirmedSats >= if (ev.chainState == BitcoinChainState.ACTIVE && ev.confirmations >= ev.requiredConfirmations) ev.receivedSats else 0L)
                            require(tender == Tender.BITCOIN && a.bitcoinEvidence.none { it.evidenceId == ev.evidenceId } && a.bitcoinEvidence.none { it.sourceId == ev.sourceId }); a.bitcoinEvidence.add(ev)
                        }
                        repeat(input.count(MAX_EVIDENCE)) {
                            val ev = CardEvidence(input.str(), input.str(), CardResult.entries.getOrNull(input.readInt()) ?: error("invalid card result"), input.readLong(), input.readLong(), input.readLong())
                            validId(ev.evidenceId, "card evidence id"); require(ev.stableId == stable); require(ev.amountMinor >= 0 && ev.amountMinor <= aamount); nonnegative(ev.observedAt, "evidence time"); require(ev.historicalCompletedMinor in 0..aamount)
                            require(ev.historicalCompletedMinor >= if (ev.result == CardResult.COMPLETED) ev.amountMinor else 0L)
                            require(tender == Tender.CARD && a.cardEvidence.none { it.evidenceId == ev.evidenceId }); a.cardEvidence.add(ev)
                        }
                    }
                    repeat(input.count(MAX_REFUNDS)) {
                        val rid = input.str(); val aid = input.str(); val ramount = input.readLong(); val rsats = input.readLong(); val requested = input.readLong()
                        validId(rid, "refund id"); validId(aid, "refund payment id")
                        require(s.refunds.none { it.refundId == rid }) { "duplicate refund id" }
                        val refundedPayment = s.attempts.firstOrNull { it.attemptId == aid } ?: error("refund payment is missing")
                        require(ramount >= 0 && rsats >= 0 && (ramount > 0 || rsats > 0)) { "invalid refund amount" }
                        require((refundedPayment.tender == Tender.CARD && ramount > 0 && rsats == 0L) || (refundedPayment.tender == Tender.BITCOIN && rsats > 0 && ramount == 0L)) { "refund units do not match tender" }
                        nonnegative(requested, "refund time")
                        val r = RefundData(rid, aid, ramount, rsats, requested); s.refunds.add(r)
                        repeat(input.count(MAX_EVIDENCE)) {
                            val ev = RefundEvidence(input.str(), input.readLong(), input.readLong(), input.readBool(), input.readLong())
                            validId(ev.evidenceId, "refund evidence id"); require(ev.paidMinor >= 0 && ev.paidMinor <= ramount && ev.paidSats >= 0 && ev.paidSats <= rsats); nonnegative(ev.observedAt, "refund evidence time")
                            require(!ev.pending || ev.paidMinor == 0L && ev.paidSats == 0L) { "pending refund records paid funds" }
                            require(r.evidence.none { it.evidenceId == ev.evidenceId }); r.evidence.add(ev)
                        }
                    }
                }
                validateDecoded(state)
                require(input.available() == 0) { "trailing ledger data" }
                return state
            }
        } catch (e: Exception) { throw IllegalStateException("ledger is corrupt; refusing to reset", e) }
    }

    private fun DataOutputStream.str(v: String) { val b = v.toByteArray(Charsets.UTF_8); require(b.size <= MAX_TEXT_BYTES); writeInt(b.size); write(b) }
    private fun DataOutputStream.nullStr(v: String?) { writeBoolean(v != null); if (v != null) str(v) }
    private fun DataOutputStream.nullLong(v: Long?) { writeBoolean(v != null); if (v != null) writeLong(v) }
    private fun DataInputStream.str(): String { val size = readInt(); require(size in 0..MAX_TEXT_BYTES); val b = ByteArray(size); readFully(b); return Charsets.UTF_8.newDecoder().onMalformedInput(CodingErrorAction.REPORT).onUnmappableCharacter(CodingErrorAction.REPORT).decode(ByteBuffer.wrap(b)).toString() }
    private fun DataInputStream.readBool(): Boolean = when (val b = readUnsignedByte()) { 0 -> false; 1 -> true; else -> error("invalid boolean encoding: $b") }
    private fun DataInputStream.nullStr(): String? = if (readBool()) str() else null
    private fun DataInputStream.nullLong(): Long? = if (readBool()) readLong() else null
    private fun DataInputStream.count(max: Int): Int = readInt().also { require(it in 0..max) }

    private companion object {
        const val MAGIC = 0x4f42434c // OBCL
        const val VERSION = 3
        const val MAX_TEXT_BYTES = 1024
        const val MAX_SALES = 2000
        const val MAX_ATTEMPTS = 100
        const val MAX_EVIDENCE = 1000
        const val MAX_REFUNDS = 1000
    }
}
