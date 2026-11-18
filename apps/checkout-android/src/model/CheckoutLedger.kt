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
import java.security.MessageDigest
import java.util.Collections
import java.util.UUID

private const val MAX_LEDGER_FILE_BYTES = 4 * 1024 * 1024

private class LegacyRefundUnverifiableException(message: String) : IllegalStateException(message)

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

/** Manual cash acknowledgment. amountMinor is applied to the sale; tenderedMinor includes returned change. */
data class CashPayment(
    val paymentId: String,
    val amountMinor: Long,
    val tenderedMinor: Long,
    val changeMinor: Long,
    val recordedAt: Long,
)

/** Immutable line snapshot used to reproduce the exact basket after restart. */
data class SaleItem(
    val productId: String,
    val name: String,
    val unitMinor: Long,
    val quantity: Int,
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
    val items: List<SaleItem> = emptyList(),
    val cashPayments: List<CashPayment> = emptyList(),
    val pricing: CheckoutPricingSnapshot? = null,
)

enum class CheckoutEventType { SALE_CREATED, PAYMENT_CHANGED, REFUND_CHANGED }
data class CheckoutEvent(val eventId: String, val epoch: String, val cursor: Long, val saleId: String,
                         val saleVersion: Long, val type: CheckoutEventType)
data class EventSnapshot(val epoch: String?, val highWater: Long, val floor: Long,
                         val saleVersions: Map<String, Long>, val initialized: Boolean,
                         val sales: List<Sale>, val legacyHistoryUnattributed: Boolean = false)
data class EventPage(val events: List<CheckoutEvent>, val highWater: Long, val floor: Long,
                     val resyncRequired: Boolean, val nextCursor: Long, val hasMore: Boolean)

/** Stable attempt identifiers and sale state. Every successful mutation is durable on return. */
enum class CheckoutLedgerEventOrigin { HUMAN, SYSTEM }

/** A v8 durable command envelope. SYSTEM is reserved for a future typed observation writer. */
internal data class CheckoutLedgerCommandEvent(
    val sequence: Long,
    val operationId: String,
    val eventType: String,
    val origin: CheckoutLedgerEventOrigin,
    val attribution: ActorAttribution?,
    val targetIdentity: String,
    val payloadDigest: ByteArray,
    val linkedOperationId: String?,
    val sourceIdentity: String?,
    val sourceObservedAt: Long?,
    val recordedAtMillis: Long,
    val commitIdentity: String,
) {
    override fun equals(other: Any?): Boolean = other is CheckoutLedgerCommandEvent &&
        sequence == other.sequence && operationId == other.operationId && eventType == other.eventType &&
        origin == other.origin && attribution == other.attribution && targetIdentity == other.targetIdentity &&
        payloadDigest.contentEquals(other.payloadDigest) && linkedOperationId == other.linkedOperationId &&
        sourceIdentity == other.sourceIdentity && sourceObservedAt == other.sourceObservedAt &&
        recordedAtMillis == other.recordedAtMillis && commitIdentity == other.commitIdentity
    override fun hashCode(): Int = 31 * sequence.hashCode() + operationId.hashCode()
}

private data class PendingLedgerCommand(
    val operationId: String,
    val eventType: String,
    val origin: CheckoutLedgerEventOrigin,
    val attribution: ActorAttribution?,
    val targetIdentity: String,
    val payloadDigest: ByteArray,
    val linkedOperationId: String?,
    val sourceIdentity: String?,
    val sourceObservedAt: Long?,
)

class CheckoutLedgerWriteUncertainException(cause: Throwable) :
    IllegalStateException("ledger write outcome is uncertain; reopen before continuing", cause)

class CheckoutLedger(
    private val storage: DurableLedgerStorage,
    initialCommandAuthority: CheckoutAuthority? = null,
    initialCommandScope: CheckoutAuthorityScope? = null,
) {
    private val lock = Any()
    private var state: State = decode(storage.read())
    private var poisoned = false
    private var stableSaleCallbackActive = false
    private var activeCommand: PendingLedgerCommand? = null
    private var commandAuthority: CheckoutAuthority? = initialCommandAuthority
    private var commandScope: CheckoutAuthorityScope? = initialCommandScope

    init {
        if ((commandAuthority == null) != (commandScope == null)) throw CheckoutAuthorityUnavailableException()
        commandScope?.let(::requireRecordedCommandScope)
    }

    /** Bind Android's already-open authority without exposing its opaque session to the UI. */
    internal fun bindCommandAuthority(authority: CheckoutAuthority, scope: CheckoutAuthorityScope) = synchronized(lock) {
        ensureHealthy()
        val existingAuthority = commandAuthority
        val existingScope = commandScope
        if (existingAuthority == null && existingScope == null) {
            requireRecordedCommandScope(scope)
            commandAuthority = authority
            commandScope = scope
        } else if (existingAuthority !== authority || existingScope != scope) {
            throw CheckoutAuthorityUnavailableException()
        }
    }

    private fun requireRecordedCommandScope(scope: CheckoutAuthorityScope) {
        if (state.commandEvents.any { event ->
                event.origin == CheckoutLedgerEventOrigin.HUMAN &&
                    (event.attribution == null || event.attribution.scope != scope)
            }) throw CheckoutAuthorityUnavailableException()
    }

    /** Explicitly commits the event epoch; legacy reads alone never rewrite their source file.
     * Restoring an older raw ledger file is not detectable here; callers must rotate the epoch
     * through an authorized restore workflow before serving cursors from that restored copy.
     */
    fun initializeEvents(): EventSnapshot = synchronized(lock) {
        ensureHealthy()
        check(!stableSaleCallbackActive) { "ledger mutation is blocked during stable sale callback" }
        if (!state.eventsInitialized) {
            val before = encode(state)
            state.eventsInitialized = true
            state.eventEpoch = UUID.randomUUID().toString()
            state.events.clear(); state.eventHighWater = 0; state.eventFloor = 0
            state.saleVersions.clear()
            state.sales.keys.forEach { state.saleVersions[it] = 0L }
            val next = try { encode(state) } catch (e: Throwable) { state = decode(before); throw e }
            try { storage.writeAtomically(next) } catch (e: Throwable) {
                poisoned = true
                try { state = decode(storage.read()) } catch (_: Throwable) { }
                throw CheckoutLedgerWriteUncertainException(e)
            }
        }
        eventSnapshotLocked()
    }

    fun eventSnapshot(): EventSnapshot = synchronized(lock) { ensureHealthy(); eventSnapshotLocked() }

    /** Attributed command history. Access control for this read belongs to the caller. */
    internal fun commandEvents(): List<CheckoutLedgerCommandEvent> = synchronized(lock) {
        ensureHealthy(); Collections.unmodifiableList(state.commandEvents.map { it.copy(payloadDigest = it.payloadDigest.copyOf()) })
    }

    /** Runs one synchronous local commit against a verified sale version while ledger mutations are excluded.
     * The callback must not perform network or UI waits. Reentrant reads are allowed; reentrant mutations fail.
     */
    fun <T> withStableSale(saleId: String, expectedVersion: Long, action: (Sale) -> T): T = synchronized(lock) {
        ensureHealthy()
        check(!stableSaleCallbackActive) { "stable sale callback is already active" }
        require(state.eventsInitialized) { "sale versions are not initialized" }
        val actualVersion = state.saleVersions[saleId] ?: error("sale not found")
        require(expectedVersion >= 0 && actualVersion == expectedVersion) { "sale changed since inventory review" }
        val snapshot = view(sale(saleId))
        stableSaleCallbackActive = true
        try { action(snapshot) } finally { stableSaleCallbackActive = false }
    }

    /** A null epoch means no history is available and requires a current-state resync. */
    fun readEvents(epoch: String, afterCursor: Long, limit: Int): EventPage = synchronized(lock) {
        ensureHealthy(); require(limit in 1..MAX_EVENT_PAGE) { "invalid event page size" }
        require(afterCursor >= 0) { "invalid event cursor" }
        val snapshot = eventSnapshotLocked()
        if (!snapshot.initialized || epoch != snapshot.epoch || afterCursor < snapshot.floor)
            return@synchronized EventPage(emptyList(), snapshot.highWater, snapshot.floor, true, afterCursor, false)
        require(afterCursor <= snapshot.highWater) { "event cursor is ahead of the ledger" }
        val available = state.events.filter { it.cursor > afterCursor }
        val events = available.take(limit)
        EventPage(events, snapshot.highWater, snapshot.floor, false,
            events.lastOrNull()?.cursor ?: afterCursor, available.size > events.size)
    }

    private fun eventSnapshotLocked() = EventSnapshot(state.eventEpoch, state.eventHighWater, state.eventFloor,
        state.saleVersions.toMap(), state.eventsInitialized, state.sales.values.map(::view), state.legacyHistoryUnattributed)

    fun createSale(session: CheckoutAuthoritySession, operationId: String, saleId: String, amountMinor: Long,
                   currency: String, createdAt: Long, items: List<SaleItem> = emptyList()): Sale = humanCommand(
        session, operationId, CheckoutAuthorityCapability.SALE_CREATE, "sale.create", saleId,
        saleCommandDigest(saleId, amountMinor, currency, createdAt, items, null),
        replay = { getSale(saleId) },
    ) { createSale(saleId, amountMinor, currency, createdAt, items) }

    /** Compatibility API; resulting rows are v8 legacy/unattributed history. */
    fun createSale(saleId: String, amountMinor: Long, currency: String, createdAt: Long,
                   items: List<SaleItem> = emptyList()): Sale = createSaleInternal(saleId, amountMinor, currency, createdAt, items, null)

    /** Creates a sale from a frozen canonical price. Its total is the amount due for every tender. */
    fun createPricedSale(saleId: String, currency: String, createdAt: Long,
                         pricing: CheckoutPricingSnapshot): Sale {
        return createPricedSaleInternal(saleId, currency, createdAt, pricing)
    }

    fun createPricedSale(session: CheckoutAuthoritySession, operationId: String, saleId: String,
                         currency: String, createdAt: Long, pricing: CheckoutPricingSnapshot): Sale {
        val frozen = freezePricing(pricing)
        return humanCommand(
            session, operationId, CheckoutAuthorityCapability.SALE_CREATE, "sale.create", saleId,
            saleCommandDigest(saleId, frozen.totalMinor, currency, createdAt, pricingItems(frozen), frozen),
            replay = { getSale(saleId) },
        ) { createPricedSaleInternal(saleId, currency, createdAt, frozen) }
    }

    private fun createPricedSaleInternal(saleId: String, currency: String, createdAt: Long,
                         pricing: CheckoutPricingSnapshot): Sale {
        require(currency == "USD") { "priced checkout currently supports USD only" }
        val frozen = freezePricing(pricing)
        return createSaleInternal(saleId, frozen.totalMinor, currency, createdAt,
            pricingItems(frozen), frozen)
    }

    private fun createSaleInternal(saleId: String, amountMinor: Long, currency: String, createdAt: Long,
                   items: List<SaleItem>, pricing: CheckoutPricingSnapshot?): Sale = mutate {
        validId(saleId, "sale id"); positive(amountMinor, "sale amount"); validCurrency(currency)
        require(createdAt >= 0) { "invalid creation time" }
        require(!sales.containsKey(saleId)) { "sale already exists" }
        require(sales.size < MAX_SALES) { "sale limit reached" }
        val snapshot = snapshotItems(items, allowZeroUnit = pricing != null)
        if (pricing == null && snapshot.isNotEmpty()) require(itemTotal(snapshot) == amountMinor) { "basket total does not match sale amount" }
        if (pricing != null) {
            require(pricing.totalMinor == amountMinor && snapshot == pricingItems(pricing))
            require(snapshot.isNotEmpty())
        }
        sales[saleId] = SaleData(saleId, amountMinor, currency, createdAt, snapshot, pricing = pricing)
        view(sales.getValue(saleId))
    }

    /** Must complete before invoking the external receiver/payment SDK. counterIndex is the existing payment request index. */
    /** Compatibility API; resulting rows are v8 legacy/unattributed history. */
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
    /** Compatibility API; resulting rows are v8 legacy/unattributed history. */
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

    /** Records a manual cash acknowledgment. This records operator input, not processor or cash verification. */
    /** Compatibility API; resulting rows are v8 legacy/unattributed history. */
    fun recordCashPayment(saleId: String, paymentId: String, tenderedMinor: Long, recordedAt: Long): CashPayment = mutate {
        val sale = sale(saleId)
        validId(paymentId, "cash payment id"); positive(tenderedMinor, "cash tendered amount")
        require(recordedAt >= 0) { "invalid cash payment time" }
        val prior = state.sales.values.flatMap { other -> other.cashPayments.map { other.id to it } }.firstOrNull { it.second.paymentId == paymentId }
        if (prior != null) {
            require(prior.first == saleId && prior.second.tenderedMinor == tenderedMinor && prior.second.recordedAt == recordedAt) {
                "cash payment id was already used with different details"
            }
            return@mutate prior.second
        }
        require(sale.cashPayments.size < MAX_CASH_PAYMENTS) { "cash payment limit reached" }
        require(sale.currency == "USD") { "cash payments are supported only for USD sales" }
        val current = view(sale)
        require(!current.requiresReview) { "sale requires review before cash payment" }
        require(canStartTender(sale)) { "another payment attempt must be resolved first" }
        require(current.remainingMinor > 0) { "sale has no remaining balance" }
        require(tenderedMinor >= current.remainingMinor) { "cash tendered is less than remaining balance" }
        val change = Math.subtractExact(tenderedMinor, current.remainingMinor)
        val payment = CashPayment(paymentId, current.remainingMinor, tenderedMinor, change, recordedAt)
        sale.cashPayments.add(payment)
        payment
    }

    /** Eligibility hint for the cash UI. recordCashPayment rechecks every guard when recording. */
    fun canRecordCashPayment(saleId: String): Boolean = synchronized(lock) {
        ensureHealthy()
        val sale = sale(saleId)
        sale.currency == "USD" && sale.cashPayments.size < MAX_CASH_PAYMENTS &&
            view(sale).let { it.remainingMinor > 0 && !it.requiresReview } && canStartTender(sale)
    }

    /** Compatibility API; resulting rows are v8 legacy/unattributed history. */
    fun requestCancellation(saleId: String, attemptId: String): PaymentAttempt = mutate {
        val a = attempt(saleId, attemptId); a.cancellationRequested = true; a.cancellationAcknowledged = false; a.cancellationUnknown = false; attemptView(a)
    }

    /** Call only after the native CounterNode.cancel operation returns success. Late evidence can reopen the derived state. */
    fun acknowledgeBitcoinCancellation(saleId: String, attemptId: String): PaymentAttempt =
        acknowledgeBitcoinCancellationInternal(saleId, attemptId)

    private fun acknowledgeBitcoinCancellationInternal(saleId: String, attemptId: String): PaymentAttempt = mutate {
        val a = attempt(saleId, attemptId); require(a.tender == Tender.BITCOIN) { "not a Bitcoin attempt" }
        require(a.cancellationRequested) { "cancellation was not requested" }
        a.cancellationAcknowledged = true
        attemptView(a)
    }

    /** Reconciliation response that says the cancel outcome is still unknown. */
    fun markCancellationUnknown(saleId: String, attemptId: String): PaymentAttempt =
        markCancellationUnknownInternal(saleId, attemptId)

    private fun markCancellationUnknownInternal(saleId: String, attemptId: String): PaymentAttempt = mutate {
        val a = attempt(saleId, attemptId); require(a.tender == Tender.CARD) { "cancellation reconciliation applies to card attempts" }
        a.cancellationRequested = true; a.cancellationUnknown = true; attemptView(a)
    }

    /** receivedSats is the current cumulative amount for evidenceId, not a fiat conversion or a delta. */
    fun recordBitcoinEvidence(saleId: String, attemptId: String, evidenceId: String, sourceId: String, receivedSats: Long,
                              confirmations: Int, requiredConfirmations: Int, observedAt: Long,
                              chainState: BitcoinChainState = BitcoinChainState.ACTIVE): PaymentAttempt =
        recordBitcoinEvidenceInternal(saleId, attemptId, evidenceId, sourceId, receivedSats, confirmations,
            requiredConfirmations, observedAt, chainState)

    private fun recordBitcoinEvidenceInternal(saleId: String, attemptId: String, evidenceId: String, sourceId: String, receivedSats: Long,
                              confirmations: Int, requiredConfirmations: Int, observedAt: Long,
                              chainState: BitcoinChainState): PaymentAttempt = mutate {
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
                           result: CardResult, amountMinor: Long, observedAt: Long): PaymentAttempt =
        recordCardEvidenceInternal(saleId, attemptId, evidenceId, stableId, result, amountMinor, observedAt)

    private fun recordCardEvidenceInternal(saleId: String, attemptId: String, evidenceId: String, stableId: String,
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

    /** Compatibility API; resulting rows are v8 legacy/unattributed history. */
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
                             pending: Boolean, observedAt: Long): Refund =
        recordRefundEvidenceInternal(saleId, refundId, evidenceId, actualPaidMinor, actualPaidSats, pending, observedAt)

    private fun recordRefundEvidenceInternal(saleId: String, refundId: String, evidenceId: String, actualPaidMinor: Long, actualPaidSats: Long,
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

    private fun <T> humanCommand(
        session: CheckoutAuthoritySession,
        operationId: String,
        capability: CheckoutAuthorityCapability,
        eventType: String,
        targetIdentity: String,
        digest: ByteArray,
        replay: () -> T,
        operation: () -> T,
    ): T = synchronized(lock) {
        ensureHealthy()
        check(activeCommand == null) { "nested ledger commands are not allowed" }
        val authority = commandAuthority ?: throw CheckoutAuthorityUnavailableException()
        val scope = commandScope ?: throw CheckoutAuthorityUnavailableException()
        validId(operationId, "operation id")
        validId(targetIdentity, "command target")
        require(digest.size == COMMAND_DIGEST_BYTES) { "invalid ledger command digest" }
        val attribution = authority.authorize(session, capability, scope)
        val pending = PendingLedgerCommand(operationId, eventType, CheckoutLedgerEventOrigin.HUMAN,
            attribution, targetIdentity, digest.copyOf(), null, null, null)
        val prior = state.commandEvents.firstOrNull { it.operationId == operationId }
        if (prior != null) {
            if (!sameCommand(prior, pending)) throw IllegalStateException("ledger operation ID conflicts with prior command")
            return@synchronized replay()
        }
        activeCommand = pending
        try { operation() } finally { activeCommand = null }
    }

    private fun sameCommand(event: CheckoutLedgerCommandEvent, pending: PendingLedgerCommand): Boolean {
        val old = event.attribution
        val next = pending.attribution
        val actorMatches = if (old == null || next == null) old == null && next == null else
            old.actorId == next.actorId && old.role == next.role && old.capabilityId == next.capabilityId &&
                old.policyGeneration == next.policyGeneration && old.scope == next.scope
        return event.operationId == pending.operationId && event.eventType == pending.eventType &&
            event.origin == pending.origin && actorMatches && event.targetIdentity == pending.targetIdentity &&
            MessageDigest.isEqual(event.payloadDigest, pending.payloadDigest) &&
            event.linkedOperationId == pending.linkedOperationId && event.sourceIdentity == pending.sourceIdentity &&
            event.sourceObservedAt == pending.sourceObservedAt
    }

    private fun saleCommandDigest(
        saleId: String,
        amountMinor: Long,
        currency: String,
        createdAt: Long,
        items: List<SaleItem>,
        pricing: CheckoutPricingSnapshot?,
    ): ByteArray = commandDigest { out ->
        out.str("openbmt.checkout.ledger-command"); out.writeInt(1); out.str("sale.create")
        out.str(saleId); out.writeLong(amountMinor); out.str(currency); out.writeLong(createdAt)
        out.writeInt(items.size)
        items.forEach { out.str(it.productId); out.str(it.name); out.writeLong(it.unitMinor); out.writeInt(it.quantity) }
        writePricing(out, pricing)
    }

    /** Digest used by the first v8 command writer; retained only for strict v8 migration. */
    private fun saleCommandDigestV8(
        saleId: String,
        amountMinor: Long,
        currency: String,
        createdAt: Long,
        items: List<SaleItem>,
        pricing: CheckoutPricingSnapshot?,
    ): ByteArray = commandDigest { out ->
        out.str("sale.create"); out.str(saleId); out.writeLong(amountMinor); out.str(currency); out.writeLong(createdAt)
        out.writeInt(items.size)
        items.forEach { out.str(it.productId); out.str(it.name); out.writeLong(it.unitMinor); out.writeInt(it.quantity) }
        writePricing(out, pricing)
    }

    private fun commandDigest(write: (DataOutputStream) -> Unit): ByteArray {
        val bytes = ByteArrayOutputStream()
        DataOutputStream(bytes).use(write)
        return MessageDigest.getInstance("SHA-256").digest(bytes.toByteArray())
    }

    fun getSale(saleId: String): Sale = synchronized(lock) { ensureHealthy(); view(sale(saleId)) }
    fun listSales(): List<Sale> = synchronized(lock) { ensureHealthy(); state.sales.values.map(::view) }

    private inline fun <T> mutate(block: State.() -> T): T = synchronized(lock) {
        ensureHealthy()
        check(!stableSaleCallbackActive) { "ledger mutation is blocked during stable sale callback" }
        if (activeCommand != null) require(state.commandEvents.size < MAX_COMMAND_EVENTS) { "ledger command history limit reached" }
        val beforeSales = state.sales.keys.associateWith { view(state.sales.getValue(it)) }
        val before = encode(state)
        val result = try { state.block() } catch (e: Throwable) { state = decode(before); throw e }
        if (state.eventsInitialized) {
            val changed = LinkedHashSet<String>()
            (beforeSales.keys + state.sales.keys).forEach { id ->
                val old = beforeSales[id]
                val now = state.sales[id]?.let(::view)
                if (old != now) changed.add(id)
            }
            try {
                for (id in changed) {
                    check(state.eventHighWater < Long.MAX_VALUE) { "event cursor exhausted" }
                    val version = Math.addExact(state.saleVersions[id] ?: 0L, 1L)
                    state.saleVersions[id] = version
                    state.eventHighWater++
                    val type = when {
                        beforeSales[id] == null -> CheckoutEventType.SALE_CREATED
                        beforeSales[id]?.refunds != state.sales[id]?.let { sale -> view(sale).refunds } -> CheckoutEventType.REFUND_CHANGED
                        else -> CheckoutEventType.PAYMENT_CHANGED
                    }
                    state.events.add(CheckoutEvent(UUID.randomUUID().toString(), state.eventEpoch!!, state.eventHighWater, id, version, type))
                }
                while (state.events.size > MAX_EVENTS) state.events.removeAt(0)
                state.eventFloor = state.events.firstOrNull()?.cursor?.minus(1) ?: state.eventHighWater
            } catch (e: Throwable) { state = decode(before); throw e }
        }
        try {
            val command = activeCommand
            if (command == null) {
                // Compatibility writes remain available while existing Android call sites migrate.
                // Their history is never backfilled with whichever actor happens to be signed in later.
                state.legacyHistoryUnattributed = true
            } else {
                val sequence = Math.addExact(state.commandEvents.lastOrNull()?.sequence ?: 0L, 1L)
                val matchedSale = state.sales.values.firstOrNull { candidate ->
                    candidate.id == command.targetIdentity || candidate.attempts.any { it.attemptId == command.targetIdentity } ||
                        candidate.refunds.any { it.refundId == command.targetIdentity } ||
                        candidate.cashPayments.any { it.paymentId == command.targetIdentity }
                }
                val commitIdentity = matchedSale?.let { "${it.id}@${state.saleVersions[it.id] ?: 0L}" } ?: command.targetIdentity
                val recordedAt = command.attribution?.eventTimeMillis
                    ?: error("system command writer is unavailable")
                require(recordedAt >= 0) { "invalid ledger event time" }
                state.commandEvents.add(CheckoutLedgerCommandEvent(
                    sequence, command.operationId, command.eventType, command.origin, command.attribution,
                    command.targetIdentity, command.payloadDigest.copyOf(), command.linkedOperationId,
                    command.sourceIdentity, command.sourceObservedAt, recordedAt, commitIdentity,
                ))
            }
        } catch (e: Throwable) { state = decode(before); throw e }
        val next = try { encode(state) } catch (e: Throwable) { state = decode(before); throw e }
        try { storage.writeAtomically(next) } catch (e: Throwable) {
            poisoned = true
            try { state = decode(storage.read()) } catch (_: Throwable) { }
            throw CheckoutLedgerWriteUncertainException(e)
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
        val total = safeSum(attempts.map { it.receivedMinor } + s.cashPayments.map { it.amountMinor })
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
        }.map { it.receivedMinor } + s.cashPayments.map { it.amountMinor })
        return Sale(s.id, s.amountMinor, s.currency, s.createdAt, attempts, refunds, total,
            (s.amountMinor - total).coerceAtLeast(0), settled >= s.amountMinor && settled > 0 && !requiresReview,
            requiresReview, lateBitcoinReceipt, overpaidMinor, overpaidSats, s.items.map { it.copy() }, s.cashPayments.map { it.copy() }, s.pricing?.let(::freezePricing))
    }

    private fun saleOverpaidSats(s: SaleData): List<Long> = s.attempts.filter { it.tender == Tender.BITCOIN }.map { a ->
        (paidSats(a) - a.expectedSats!!).coerceAtLeast(0L)
    }

    private class State(val sales: LinkedHashMap<String, SaleData> = LinkedHashMap(),
                        var eventsInitialized: Boolean = false, var eventEpoch: String? = null,
                        var eventHighWater: Long = 0, var eventFloor: Long = 0,
                        val saleVersions: LinkedHashMap<String, Long> = LinkedHashMap(),
                        val events: MutableList<CheckoutEvent> = ArrayList(),
                        var legacyHistoryUnattributed: Boolean = false,
                        val commandEvents: MutableList<CheckoutLedgerCommandEvent> = ArrayList())
    private data class SaleData(val id: String, val amountMinor: Long, val currency: String, val createdAt: Long, val items: List<SaleItem> = emptyList(),
                                val cashPayments: MutableList<CashPayment> = ArrayList(),
                                val attempts: MutableList<AttemptData> = ArrayList(), val refunds: MutableList<RefundData> = ArrayList(),
                                var pricing: CheckoutPricingSnapshot? = null)
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

    private fun snapshotItems(items: List<SaleItem>, allowZeroUnit: Boolean = false): List<SaleItem> {
        require(items.size <= MAX_ITEMS_PER_SALE) { "too many basket items" }
        for (item in items) {
            validId(item.productId, "product id")
            require(item.name.isNotBlank() && item.name.length <= MAX_ITEM_NAME_CHARS && item.name.none { it.isISOControl() } && item.name.toByteArray(Charsets.UTF_8).size <= MAX_ITEM_NAME_BYTES) { "invalid product name" }
            if (allowZeroUnit) nonnegative(item.unitMinor, "item price") else positive(item.unitMinor, "item price")
            require(item.quantity in 1..MAX_ITEM_QUANTITY) { "invalid item quantity" }
            Math.multiplyExact(item.unitMinor, item.quantity.toLong())
        }
        return items.map { it.copy() }
    }

    private fun itemTotal(items: List<SaleItem>): Long = safeSum(items.map { Math.multiplyExact(it.unitMinor, it.quantity.toLong()) })

    private fun freezePricing(value: CheckoutPricingSnapshot): CheckoutPricingSnapshot {
        require(value.lines.size in 1..MAX_PRICED_LINES && value.taxes.size <= 1) { "priced sale exceeds limits" }
        val inputs = value.lines.map { line ->
            require(line.lineId.length <= MAX_PRICING_ID_CHARS && line.productId.length <= MAX_PRICING_ID_CHARS)
            require(line.name.length <= MAX_ITEM_NAME_CHARS && line.name.toByteArray(Charsets.UTF_8).size <= MAX_ITEM_NAME_BYTES)
            require(line.quantity in 1..MAX_ITEM_QUANTITY)
            require(line.modifiers.size <= MAX_MODIFIERS_PER_LINE)
            val modifiers = line.modifiers.map {
                require(it.modifierId.length <= MAX_PRICING_ID_CHARS && it.name.length <= MAX_ITEM_NAME_CHARS)
                require(it.name.toByteArray(Charsets.UTF_8).size <= MAX_ITEM_NAME_BYTES)
                it
            }
            CheckoutLineInput(line.lineId, line.productId, line.name, line.unitMinor, line.quantity,
                modifiers.map { SelectedModifier(it.modifierId, it.name, it.amountMinor) })
        }
        val tax = value.taxes.singleOrNull()?.let {
            require(it.taxId.length <= MAX_PRICING_ID_CHARS && it.name.length <= MAX_ITEM_NAME_CHARS)
            require(it.name.toByteArray(Charsets.UTF_8).size <= MAX_ITEM_NAME_BYTES)
            CheckoutTaxRule(it.taxId, it.name, it.rateBasisPoints, it.treatment)
        }
        val result = CheckoutPricing.freeze(inputs, value.discountMinor, tax, value.tipMinor)
        require(samePricing(result, value)) { "pricing snapshot outputs do not match its inputs" }
        return result
    }

    private fun pricingItems(pricing: CheckoutPricingSnapshot): List<SaleItem> = pricing.lines.map { line ->
        val effectiveUnit = line.modifiers.fold(line.unitMinor) { sum, modifier -> Math.addExact(sum, modifier.amountMinor) }
        SaleItem(line.productId, line.name, effectiveUnit, line.quantity)
    }

    private fun samePricing(a: CheckoutPricingSnapshot, b: CheckoutPricingSnapshot): Boolean =
        a.subtotalMinor == b.subtotalMinor && a.discountMinor == b.discountMinor &&
            a.taxableAfterDiscountMinor == b.taxableAfterDiscountMinor && a.taxMinor == b.taxMinor &&
            a.tipMinor == b.tipMinor && a.totalMinor == b.totalMinor && a.taxes == b.taxes &&
            a.lines == b.lines

    private fun writePricing(out: DataOutputStream, pricing: CheckoutPricingSnapshot?) {
        out.writeBoolean(pricing != null)
        if (pricing == null) return
        out.writeInt(pricing.lines.size)
        for (line in pricing.lines) {
            out.str(line.lineId); out.str(line.productId); out.str(line.name); out.writeLong(line.unitMinor)
            out.writeInt(line.quantity); out.writeInt(line.modifiers.size)
            for (modifier in line.modifiers) { out.str(modifier.modifierId); out.str(modifier.name); out.writeLong(modifier.amountMinor) }
            out.writeLong(line.subtotalMinor); out.writeLong(line.discountMinor); out.writeLong(line.taxMinor)
        }
        out.writeLong(pricing.subtotalMinor); out.writeLong(pricing.discountMinor); out.writeLong(pricing.taxableAfterDiscountMinor)
        out.writeInt(pricing.taxes.size)
        for (tax in pricing.taxes) {
            out.str(tax.taxId); out.str(tax.name); out.writeInt(tax.rateBasisPoints); out.writeInt(tax.treatment.ordinal); out.writeLong(tax.amountMinor)
        }
        out.writeLong(pricing.taxMinor); out.writeLong(pricing.tipMinor); out.writeLong(pricing.totalMinor)
    }

    private fun readPricing(input: DataInputStream): CheckoutPricingSnapshot? {
        if (!input.readBool()) return null
        val lines = List(input.count(MAX_PRICED_LINES)) {
            val lineId = input.str(); val productId = input.str(); val name = input.str(); val unit = input.readLong()
            val quantity = input.readInt()
            val modifiers = List(input.count(MAX_MODIFIERS_PER_LINE)) {
                SelectedModifier(input.str(), input.str(), input.readLong())
            }
            val subtotal = input.readLong(); val discount = input.readLong(); val tax = input.readLong()
            Triple(CheckoutLineInput(lineId, productId, name, unit, quantity, modifiers), Triple(subtotal, discount, tax), Unit)
        }
        val subtotal = input.readLong(); val discount = input.readLong(); val taxable = input.readLong()
        val taxes = List(input.count(1)) {
            val id = input.str(); val name = input.str(); val rate = input.readInt()
            val treatment = TaxTreatment.entries.getOrNull(input.readInt()) ?: error("invalid tax treatment")
            TaxSnapshot(id, name, rate, treatment, input.readLong())
        }
        val taxTotal = input.readLong(); val tip = input.readLong(); val total = input.readLong()
        val rule = taxes.singleOrNull()?.let { CheckoutTaxRule(it.taxId, it.name, it.rateBasisPoints, it.treatment) }
        val result = CheckoutPricing.freeze(lines.map { it.first }, discount, rule, tip)
        require(result.lines.indices.all { index ->
            val saved = lines[index].second
            result.lines[index].let { it.subtotalMinor == saved.first && it.discountMinor == saved.second && it.taxMinor == saved.third }
        }) { "stored priced lines do not match their inputs" }
        require(result.subtotalMinor == subtotal && result.discountMinor == discount &&
            result.taxableAfterDiscountMinor == taxable && result.taxes == taxes &&
            result.taxMinor == taxTotal && result.tipMinor == tip && result.totalMinor == total) {
            "stored pricing totals do not match their inputs"
        }
        return freezePricing(result)
    }

    private fun validateDecoded(s: State, version: Int) {
        val indexes = HashSet<Long>()
        val cardIds = HashSet<Pair<String, String>>()
        val cashIds = HashSet<String>()
        for (sale in s.sales.values) {
            require(sale.attempts.size <= MAX_ATTEMPTS && sale.refunds.size <= MAX_REFUNDS)
            require(sale.items.size <= MAX_ITEMS_PER_SALE)
            val copiedItems = snapshotItems(sale.items, allowZeroUnit = sale.pricing != null)
            if (sale.pricing == null && copiedItems.isNotEmpty()) require(itemTotal(copiedItems) == sale.amountMinor) { "stored basket total differs from sale amount" }
            require(sale.cashPayments.size <= MAX_CASH_PAYMENTS)
            require(safeSum(sale.cashPayments.map { it.amountMinor }) <= sale.amountMinor) { "stored cash payments exceed sale amount" }
            for (cash in sale.cashPayments) {
                validId(cash.paymentId, "cash payment id"); positive(cash.amountMinor, "cash payment amount")
                positive(cash.tenderedMinor, "cash tendered amount"); nonnegative(cash.changeMinor, "cash change"); nonnegative(cash.recordedAt, "cash payment time")
                require(cashIds.add(cash.paymentId)) { "cash payment id is duplicated" }
                require(sale.currency == "USD") { "cash payment on unsupported currency" }
                require(Math.addExact(cash.amountMinor, cash.changeMinor) == cash.tenderedMinor) { "cash amount and change do not match tendered amount" }
            }
            sale.pricing?.let { pricing ->
                require(sale.currency == "USD" && freezePricing(pricing).totalMinor == sale.amountMinor) { "stored pricing total differs from sale amount" }
                require(sale.items == pricingItems(pricing)) { "stored basket differs from pricing lines" }
            }
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
                if (minor > historicalMinor || sats > historicalSats) {
                    if (version == LEGACY_VERSION_2) throw LegacyRefundUnverifiableException(
                        "legacy v2 refund cannot be verified from stored confirmation evidence; file preserved"
                    )
                    require(false) { "refund requests exceed historically settled payment" }
                }
                for (refund in requests) {
                    require(refund.evidence.size <= MAX_EVIDENCE)
                    val view = refundView(refund)
                    require(view.actuallyPaidMinor <= refund.amountMinor && view.actuallyPaidSats <= refund.amountSats) { "refund evidence exceeds request" }
                }
            }
            safeSum(sale.attempts.map { if (it.tender == Tender.BITCOIN) fiatForSats(it) else received(it) })
            safeSum(sale.cashPayments.map { it.amountMinor })
        }
    }

    private fun validateCommandEvents(s: State, ledgerVersion: Int) {
        require(s.commandEvents.size <= MAX_COMMAND_EVENTS)
        require(s.commandEvents.asSequence()
            .filter { it.origin == CheckoutLedgerEventOrigin.HUMAN }
            .mapNotNull { it.attribution?.scope }
            .distinct()
            .take(2)
            .count() <= 1) { "ledger command events cross installation scopes" }
        val byOperation = HashMap<String, CheckoutLedgerCommandEvent>()
        val saleCreationTargets = HashSet<String>()
        s.commandEvents.forEachIndexed { index, event ->
            validId(event.operationId, "command operation id"); validText(event.eventType, "command event type")
            validId(event.targetIdentity, "command target"); validId(event.commitIdentity, "command commit identity")
            require(event.sequence == index + 1L && event.payloadDigest.size == COMMAND_DIGEST_BYTES && event.recordedAtMillis >= 0)
            require(byOperation.put(event.operationId, event) == null) { "duplicate ledger operation id" }
            when (event.origin) {
                CheckoutLedgerEventOrigin.HUMAN -> {
                    val actor = event.attribution ?: error("human command lacks attribution")
                    require(event.sourceIdentity == null && event.linkedOperationId == null && event.sourceObservedAt == null)
                    require(actor.actorId.isNotBlank() && actor.displayNameAtEvent.isNotBlank() && actor.policyGeneration > 0 && actor.eventTimeMillis >= 0)
                    require(CheckoutAuthorityCapability.fromId(actor.capabilityId) != null)
                    require(actor.eventTimeMillis == event.recordedAtMillis)
                    require(event.eventType == "sale.create") { "unsupported human ledger event type" }
                    require(actor.capabilityId == CheckoutAuthorityCapability.SALE_CREATE.id) {
                        "command capability does not match event type"
                    }
                    val sale = s.sales[event.targetIdentity] ?: error("sale command target is missing")
                    require(saleCreationTargets.add(event.targetIdentity)) { "sale has duplicate create commands" }
                    require(event.commitIdentity == "${event.targetIdentity}@0" || event.commitIdentity == "${event.targetIdentity}@1") {
                        "sale command commit identity is invalid"
                    }
                    val expectedDigest = if (ledgerVersion == LEGACY_COMMAND_DIGEST_VERSION) {
                        saleCommandDigestV8(sale.id, sale.amountMinor, sale.currency, sale.createdAt, sale.items, sale.pricing)
                    } else {
                        saleCommandDigest(sale.id, sale.amountMinor, sale.currency, sale.createdAt, sale.items, sale.pricing)
                    }
                    val digestMatches = MessageDigest.isEqual(event.payloadDigest, expectedDigest)
                    expectedDigest.fill(0)
                    require(digestMatches) { "sale command digest does not match persisted sale" }
                    if (ledgerVersion == LEGACY_COMMAND_DIGEST_VERSION) {
                        val migratedDigest = saleCommandDigest(sale.id, sale.amountMinor, sale.currency, sale.createdAt,
                            sale.items, sale.pricing)
                        s.commandEvents[index] = event.copy(payloadDigest = migratedDigest)
                    }
                }
                CheckoutLedgerEventOrigin.SYSTEM -> {
                    error("system ledger command type is not implemented")
                }
            }
        }
        if (!s.legacyHistoryUnattributed) {
            require(saleCreationTargets == s.sales.keys) { "ledger has a sale without exactly one create command" }
        }
    }

    private fun encode(s: State): ByteArray {
        require(s.sales.size <= MAX_SALES) { "sale limit reached" }
        val bytes = ByteArrayOutputStream()
        DataOutputStream(bytes).use { out ->
            out.writeInt(MAGIC); out.writeInt(VERSION); out.writeInt(s.sales.size)
            for (sale in s.sales.values) {
                out.str(sale.id); out.writeLong(sale.amountMinor); out.str(sale.currency); out.writeLong(sale.createdAt)
                require(sale.items.size <= MAX_ITEMS_PER_SALE)
                out.writeInt(sale.items.size)
                for (item in sale.items) { out.str(item.productId); out.str(item.name); out.writeLong(item.unitMinor); out.writeInt(item.quantity) }
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
                require(sale.cashPayments.size <= MAX_CASH_PAYMENTS)
                out.writeInt(sale.cashPayments.size)
                for (cash in sale.cashPayments) {
                    out.str(cash.paymentId); out.writeLong(cash.amountMinor); out.writeLong(cash.tenderedMinor)
                    out.writeLong(cash.changeMinor); out.writeLong(cash.recordedAt)
                }
                writePricing(out, sale.pricing)
            }
            out.writeBoolean(s.eventsInitialized)
            out.nullStr(s.eventEpoch)
            out.writeLong(s.eventHighWater); out.writeLong(s.eventFloor)
            out.writeInt(s.saleVersions.size)
            for ((id, version) in s.saleVersions) { out.str(id); out.writeLong(version) }
            out.writeInt(s.events.size)
            for (event in s.events) {
                out.str(event.eventId); out.str(event.epoch); out.writeLong(event.cursor); out.str(event.saleId)
                out.writeLong(event.saleVersion); out.writeInt(event.type.ordinal)
            }
            out.writeBoolean(s.legacyHistoryUnattributed)
            require(s.commandEvents.size <= MAX_COMMAND_EVENTS) { "ledger command history limit reached" }
            out.writeInt(s.commandEvents.size)
            s.commandEvents.forEach { event ->
                out.writeLong(event.sequence); out.str(event.operationId); out.str(event.eventType)
                out.writeInt(event.origin.ordinal); out.writeBoolean(event.attribution != null)
                event.attribution?.let { out.actor(it) }
                out.str(event.targetIdentity); out.writeInt(event.payloadDigest.size); out.write(event.payloadDigest)
                out.nullStr(event.linkedOperationId); out.nullStr(event.sourceIdentity); out.nullLong(event.sourceObservedAt)
                out.writeLong(event.recordedAtMillis); out.str(event.commitIdentity)
            }
        }
        return bytes.toByteArray().also { require(it.size <= MAX_LEDGER_FILE_BYTES) { "ledger exceeds storage limit" } }
    }

    private fun decode(bytes: ByteArray?): State {
        if (bytes == null) return State()
        require(bytes.size <= MAX_LEDGER_FILE_BYTES) { "ledger file exceeds size limit" }
        try {
            DataInputStream(ByteArrayInputStream(bytes)).use { input ->
                require(input.readInt() == MAGIC) { "unknown or corrupt ledger format" }
                val version = input.readInt()
                require(version in LEGACY_VERSION_2..VERSION) { "unknown or corrupt ledger format" }
                val sales = input.count(MAX_SALES); val state = State()
                repeat(sales) {
                    val id = input.str(); val amount = input.readLong(); val currency = input.str(); val created = input.readLong()
                    validId(id, "sale id"); positive(amount, "sale amount"); validCurrency(currency); nonnegative(created, "creation time")
                    require(!state.sales.containsKey(id)) { "duplicate sale" }
                    val items = if (version >= ITEM_SNAPSHOT_VERSION) {
                        val count = input.count(MAX_ITEMS_PER_SALE)
                        List(count) {
                            val item = SaleItem(input.str(), input.str(), input.readLong(), input.readInt())
                            snapshotItems(listOf(item), allowZeroUnit = version >= PRICING_SNAPSHOT_VERSION).single()
                        }
                    } else emptyList()
                    val s = SaleData(id, amount, currency, created, items); state.sales[id] = s
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
                            val evidenceId = input.str(); val sourceId = input.str(); val receivedSats = input.readLong(); val confirmations = input.readInt(); val required = input.readInt(); val observed = input.readLong(); val chain = BitcoinChainState.entries.getOrNull(input.readInt()) ?: error("invalid chain state")
                            val historical = if (version >= HISTORICAL_EVIDENCE_VERSION) input.readLong()
                                else if (chain == BitcoinChainState.ACTIVE && confirmations >= required) receivedSats else 0L
                            val ev = BitcoinEvidence(evidenceId, sourceId, receivedSats, confirmations, required, observed, chain, historical)
                            validId(ev.evidenceId, "Bitcoin evidence id"); validId(ev.sourceId, "Bitcoin evidence source")
                            nonnegative(ev.receivedSats, "received sats"); require(ev.confirmations >= 0 && ev.requiredConfirmations >= 1); nonnegative(ev.observedAt, "evidence time"); nonnegative(ev.historicalConfirmedSats, "historical confirmed sats")
                            require(ev.historicalConfirmedSats >= if (ev.chainState == BitcoinChainState.ACTIVE && ev.confirmations >= ev.requiredConfirmations) ev.receivedSats else 0L)
                            require(tender == Tender.BITCOIN && a.bitcoinEvidence.none { it.evidenceId == ev.evidenceId } && a.bitcoinEvidence.none { it.sourceId == ev.sourceId }); a.bitcoinEvidence.add(ev)
                        }
                        repeat(input.count(MAX_EVIDENCE)) {
                            val evidenceId = input.str(); val evidenceStable = input.str(); val result = CardResult.entries.getOrNull(input.readInt()) ?: error("invalid card result"); val amountMinor = input.readLong(); val observed = input.readLong()
                            val historical = if (version >= HISTORICAL_EVIDENCE_VERSION) input.readLong() else if (result == CardResult.COMPLETED) amountMinor else 0L
                            val ev = CardEvidence(evidenceId, evidenceStable, result, amountMinor, observed, historical)
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
                    if (version >= CASH_PAYMENT_VERSION) repeat(input.count(MAX_CASH_PAYMENTS)) {
                        val cash = CashPayment(input.str(), input.readLong(), input.readLong(), input.readLong(), input.readLong())
                        validId(cash.paymentId, "cash payment id"); positive(cash.amountMinor, "cash payment amount")
                        positive(cash.tenderedMinor, "cash tendered amount"); nonnegative(cash.changeMinor, "cash change"); nonnegative(cash.recordedAt, "cash payment time")
                        require(cash.tenderedMinor == Math.addExact(cash.amountMinor, cash.changeMinor)) { "cash amount and change do not match tendered amount" }
                        require(s.cashPayments.none { it.paymentId == cash.paymentId }) { "duplicate cash payment id" }
                        require(currency == "USD") { "cash payment on unsupported currency" }
                        s.cashPayments.add(cash)
                    }
                    if (version >= PRICING_SNAPSHOT_VERSION) s.pricing = readPricing(input)
                    if (s.pricing != null) {
                        require(currency == "USD" && s.pricing!!.totalMinor == amount) { "stored pricing total differs from sale amount" }
                        require(items == pricingItems(s.pricing!!)) { "stored basket differs from pricing lines" }
                    } else {
                        val validItems = snapshotItems(items)
                        if (validItems.isNotEmpty()) require(itemTotal(validItems) == amount) { "stored basket total differs from sale amount" }
                    }
                }
                if (version >= EVENT_VERSION) {
                    state.eventsInitialized = input.readBool()
                    state.eventEpoch = input.nullStr()
                    state.eventHighWater = input.readLong(); state.eventFloor = input.readLong()
                    repeat(input.count(MAX_SALES)) {
                        val id = input.str(); val saleVersion = input.readLong()
                        validId(id, "sale id"); require(saleVersion >= 0 && state.sales.containsKey(id) && !state.saleVersions.containsKey(id))
                        state.saleVersions[id] = saleVersion
                    }
                    repeat(input.count(MAX_EVENTS)) {
                        val eventId = input.str(); val eventEpoch = input.str(); val cursor = input.readLong(); val saleId = input.str()
                        val saleVersion = input.readLong(); val type = CheckoutEventType.entries.getOrNull(input.readInt()) ?: error("invalid event type")
                        validId(eventId, "event id"); validId(eventEpoch, "event epoch"); validId(saleId, "event sale id")
                        require(state.sales.containsKey(saleId) && cursor > 0 && saleVersion > 0)
                        state.events.add(CheckoutEvent(eventId, eventEpoch, cursor, saleId, saleVersion, type))
                    }
                    require(state.eventHighWater >= 0 && state.eventFloor >= 0 && state.eventFloor <= state.eventHighWater)
                    if (state.eventsInitialized) {
                        validId(state.eventEpoch ?: error("missing event epoch"), "event epoch")
                        require(state.saleVersions.keys == state.sales.keys)
                        require(state.events.all { it.epoch == state.eventEpoch && it.cursor in (state.eventFloor + 1)..state.eventHighWater })
                        require(state.events.map { it.eventId }.toSet().size == state.events.size)
                        require(state.events.zipWithNext().all { (a, b) -> b.cursor == Math.addExact(a.cursor, 1L) })
                        require(if (state.events.isEmpty()) state.eventFloor == state.eventHighWater
                            else state.events.first().cursor == state.eventFloor + 1L && state.events.last().cursor == state.eventHighWater)
                        val latestBySale = state.events.groupBy { it.saleId }.mapValues { (_, events) -> events.last().saleVersion }
                        for ((id, versions) in state.events.groupBy { it.saleId }) {
                            require(versions.zipWithNext().all { (a, b) -> b.saleVersion == Math.addExact(a.saleVersion, 1L) })
                            require(latestBySale[id] == state.saleVersions[id])
                        }
                    } else require(state.eventEpoch == null && state.events.isEmpty() && state.saleVersions.isEmpty() && state.eventHighWater == 0L && state.eventFloor == 0L)
                }
                if (version >= COMMAND_EVENT_VERSION) {
                    state.legacyHistoryUnattributed = input.readBool()
                    repeat(input.count(MAX_COMMAND_EVENTS)) {
                        val sequence = input.readLong(); val operationId = input.str(); val eventType = input.str()
                        val origin = CheckoutLedgerEventOrigin.entries.getOrNull(input.readInt()) ?: error("invalid command origin")
                        val attribution = if (input.readBool()) input.actor() else null
                        val target = input.str(); val digestSize = input.count(COMMAND_DIGEST_BYTES)
                        require(digestSize == COMMAND_DIGEST_BYTES) { "invalid command digest size" }
                        val digest = ByteArray(digestSize); input.readFully(digest)
                        val linked = input.nullStr(); val source = input.nullStr(); val sourceAt = input.nullLong()
                        val recordedAt = input.readLong(); val commit = input.str()
                        state.commandEvents.add(CheckoutLedgerCommandEvent(sequence, operationId, eventType, origin,
                            attribution, target, digest, linked, source, sourceAt, recordedAt, commit))
                    }
                    validateCommandEvents(state, version)
                } else {
                    state.legacyHistoryUnattributed = state.sales.isNotEmpty()
                }
                validateDecoded(state, version)
                require(input.available() == 0) { "trailing ledger data" }
                return state
            }
        } catch (e: LegacyRefundUnverifiableException) { throw e }
        catch (e: Exception) { throw IllegalStateException("ledger is corrupt; refusing to reset", e) }
    }

    private fun DataOutputStream.str(v: String) { val b = v.toByteArray(Charsets.UTF_8); require(b.size <= MAX_TEXT_BYTES); writeInt(b.size); write(b) }
    private fun DataOutputStream.nullStr(v: String?) { writeBoolean(v != null); if (v != null) str(v) }
    private fun DataOutputStream.nullLong(v: Long?) { writeBoolean(v != null); if (v != null) writeLong(v) }
    private fun DataInputStream.str(): String { val size = readInt(); require(size in 0..MAX_TEXT_BYTES); val b = ByteArray(size); readFully(b); return Charsets.UTF_8.newDecoder().onMalformedInput(CodingErrorAction.REPORT).onUnmappableCharacter(CodingErrorAction.REPORT).decode(ByteBuffer.wrap(b)).toString() }
    private fun DataInputStream.readBool(): Boolean = when (val b = readUnsignedByte()) { 0 -> false; 1 -> true; else -> error("invalid boolean encoding: $b") }
    private fun DataInputStream.nullStr(): String? = if (readBool()) str() else null
    private fun DataInputStream.nullLong(): Long? = if (readBool()) readLong() else null
    private fun DataInputStream.count(max: Int): Int = readInt().also { require(it in 0..max) }

    private fun DataOutputStream.actor(actor: ActorAttribution) {
        str(actor.actorId); str(actor.displayNameAtEvent); writeInt(actor.role.ordinal); str(actor.capabilityId)
        writeLong(actor.policyGeneration); str(actor.scope.merchantId); str(actor.scope.storeId); str(actor.scope.locationId)
        str(actor.scope.registerId); str(actor.scope.deviceId); writeLong(actor.eventTimeMillis)
    }

    private fun DataInputStream.actor(): ActorAttribution {
        val actorId = str(); val name = str(); val role = CheckoutAuthorityRole.entries.getOrNull(readInt()) ?: error("invalid actor role")
        val capability = str(); val generation = readLong()
        val scope = CheckoutAuthorityScope(str(), str(), str(), str(), str())
        val at = readLong()
        return ActorAttribution.create(actorId, name, role, capability, generation, scope, at)
    }

    private companion object {
        const val MAGIC = 0x4f42434c // OBCL
        const val LEGACY_VERSION_2 = 2
        const val HISTORICAL_EVIDENCE_VERSION = 3
        const val ITEM_SNAPSHOT_VERSION = 4
        const val CASH_PAYMENT_VERSION = 5
        const val EVENT_VERSION = 6
        const val PRICING_SNAPSHOT_VERSION = 7
        const val COMMAND_EVENT_VERSION = 8
        const val LEGACY_COMMAND_DIGEST_VERSION = 8
        const val VERSION = 9
        const val COMMAND_DIGEST_BYTES = 32
        const val MAX_COMMAND_EVENTS = 5_000
        const val MAX_TEXT_BYTES = 1024
        const val MAX_ITEM_NAME_CHARS = 120
        const val MAX_ITEM_NAME_BYTES = 320
        const val MAX_ITEM_QUANTITY = 999
        const val MAX_ITEMS_PER_SALE = 100
        const val MAX_PRICED_LINES = 100
        const val MAX_MODIFIERS_PER_LINE = 32
        const val MAX_PRICING_ID_CHARS = 160
        const val MAX_SALES = 2000
        const val MAX_ATTEMPTS = 100
        const val MAX_EVIDENCE = 1000
        const val MAX_REFUNDS = 1000
        const val MAX_CASH_PAYMENTS = 100
        const val MAX_EVENTS = 1000
        const val MAX_EVENT_PAGE = 500
    }
}
