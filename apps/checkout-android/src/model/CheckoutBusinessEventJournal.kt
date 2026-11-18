package org.openbmt.checkout

import java.io.ByteArrayInputStream
import java.io.ByteArrayOutputStream
import java.io.DataInputStream
import java.io.DataOutputStream
import java.nio.ByteBuffer
import java.nio.charset.CodingErrorAction
import java.security.MessageDigest
import java.util.Collections

enum class CheckoutBusinessEventOutcome { PREPARED, COMMITTED, ABORTED, RECONCILE_REQUIRED }
enum class CheckoutBusinessPrepareDisposition { READY_TO_EXECUTE, EXISTING_RECONCILE_FIRST }

/** Closed event vocabulary. Each event is bound to one capability at the command boundary. */
enum class CheckoutBusinessEventType(val key: String, val capability: CheckoutAuthorityCapability) {
    SALE_CREATED("sale.created", CheckoutAuthorityCapability.SALE_CREATE),
    SALE_EDITED("sale.edited", CheckoutAuthorityCapability.SALE_EDIT),
    TENDER_STARTED("tender.started", CheckoutAuthorityCapability.TENDER_BEGIN),
    REFUND_REQUESTED("refund.requested", CheckoutAuthorityCapability.REFUND_REQUEST),
    CATALOG_ITEM_CREATED("catalog.item_created", CheckoutAuthorityCapability.CATALOG_MANAGE),
    CATALOG_ITEM_EDITED("catalog.item_edited", CheckoutAuthorityCapability.CATALOG_MANAGE),
    CATALOG_ITEM_ARCHIVED("catalog.item_archived", CheckoutAuthorityCapability.CATALOG_MANAGE),
    INVENTORY_STOCK_ADJUSTED("inventory.stock_adjusted", CheckoutAuthorityCapability.INVENTORY_MANAGE),
    INVENTORY_SALE_DECREMENTED("inventory.sale_decremented", CheckoutAuthorityCapability.INVENTORY_MANAGE),
    CASH_SESSION_OPENED("cash.opened", CheckoutAuthorityCapability.CASH_SESSION_OPEN),
    CASH_MOVEMENT_RECORDED("cash.movement_recorded", CheckoutAuthorityCapability.CASH_SESSION_MOVEMENT_RECORD),
    CASH_SALE_LINKED("cash.sale_linked", CheckoutAuthorityCapability.CASH_SESSION_SALE_LINK),
    CASH_SESSION_CLOSED("cash.closed", CheckoutAuthorityCapability.CASH_SESSION_CLOSE);

    companion object {
        internal fun fromKey(value: String): CheckoutBusinessEventType? = entries.firstOrNull { it.key == value }
    }
}

/** Immutable row. Attribution can only originate from CheckoutAuthority's private-constructor type. */
class CheckoutBusinessJournalEvent internal constructor(
    val sequence: Long,
    val operationId: String,
    val eventType: CheckoutBusinessEventType,
    val attribution: ActorAttribution,
    val targetIdentity: String,
    val businessIdentity: String,
    digest: ByteArray,
    val outcome: CheckoutBusinessEventOutcome,
    val recordedAtMillis: Long,
    val outcomeCode: String,
    val businessCommitIdentity: String?,
    previousHashData: ByteArray,
    eventHashData: ByteArray,
) {
    private val digestData = digest.copyOf()
    private val previousHashData = previousHashData.copyOf()
    private val eventHashData = eventHashData.copyOf()
    val commandDigest: ByteArray get() = digestData.copyOf()
    val previousHash: ByteArray get() = previousHashData.copyOf()
    val eventHash: ByteArray get() = eventHashData.copyOf()

    internal fun digestCopy() = digestData.copyOf()
    internal fun previousCopy() = previousHashData.copyOf()
    internal fun hashCopy() = eventHashData.copyOf()
    internal fun withIntegrity(seq: Long, previous: ByteArray, hash: ByteArray) = CheckoutBusinessJournalEvent(
        seq, operationId, eventType, attribution, targetIdentity, businessIdentity, digestData,
        outcome, recordedAtMillis, outcomeCode, businessCommitIdentity, previous, hash,
    )

    override fun toString() = "CheckoutBusinessJournalEvent(sequence=$sequence, eventType=$eventType, outcome=$outcome, operationId=redacted)"
}

class CheckoutBusinessJournalUnavailableException : IllegalStateException("Checkout business journal unavailable")
class CheckoutBusinessJournalAuthorizationException : IllegalStateException("Checkout business journal authorization refused")
class CheckoutBusinessOperationConflictException : IllegalStateException("Checkout business operation ID conflict")
class CheckoutBusinessJournalTransitionException : IllegalStateException("Checkout business journal transition refused")

data class CheckoutBusinessPrepareResult(
    val disposition: CheckoutBusinessPrepareDisposition,
    val event: CheckoutBusinessJournalEvent,
)

/**
 * Durable preparation and reconciliation protocol for business commands.
 *
 * 1. Persist PREPARED before touching a domain store. Only READY_TO_EXECUTE permits the initial
 *    attempt. A retry returns EXISTING_RECONCILE_FIRST; inspect the domain store by operationId.
 * 2. The owning domain store must atomically persist operationId, event identity, exact attribution,
 *    and its mutation in its own envelope. This separate journal cannot make two files atomic.
 * 3. Append COMMITTED only after the domain store confirms its durable commit. Append ABORTED only
 *    after it definitively confirms no commit. Unknown write/result appends RECONCILE_REQUIRED.
 * 4. After a crash, PREPARED/RECONCILE_REQUIRED must be resolved against the owning store before a
 *    retry. A missing domain event is not by itself safe to replay if an old writer may still run.
 *
 * Android backing storage must authenticate/encrypt data with Keystore. The SHA-256 chain detects
 * accidental changes and reordering; without authenticated storage, a file editor can rewrite it.
 */
class CheckoutBusinessEventJournal(
    private val storage: CheckoutAuthorityStorage,
    private val authority: CheckoutAuthority,
    private val scope: CheckoutAuthorityScope,
    private val clock: CheckoutAuthorityClock,
) {
    private val monitor = Any()
    private var poisoned = false

    /**
     * Bind an authorized actor and safe request fingerprint before any domain mutation. The digest
     * should cover canonical, non-secret command fields; never pass raw payloads, PINs, recovery
     * codes, payment credentials, or reversible secret material.
     */
    fun prepare(
        operationId: String,
        eventType: String,
        session: CheckoutAuthoritySession,
        targetIdentity: String,
        businessIdentity: String,
        commandDigest: ByteArray,
    ): CheckoutBusinessPrepareResult = synchronized(monitor) {
        ensureAvailable()
        validateOperationId(operationId)
        val type = CheckoutBusinessEventType.fromKey(eventType)
            ?: throw CheckoutBusinessJournalAuthorizationException()
        validateIdentity(targetIdentity); validateIdentity(businessIdentity)
        require(commandDigest.size == DIGEST_BYTES)
        repeat(MAX_CAS_ATTEMPTS) {
            val oldBytes = readBytes()
            val events = decodeOrEmpty(oldBytes)
            val prior = events.lastOrNull { it.operationId == operationId }
            if (prior != null) {
                // Retries are authorized against current policy before exposing the prior result.
                val attribution = authority.authorize(session, type.capability, scope)
                val binding = Binding(operationId, type, attribution, targetIdentity, businessIdentity, commandDigest.copyOf())
                if (!prior.sameBinding(binding)) throw CheckoutBusinessOperationConflictException()
                return@synchronized CheckoutBusinessPrepareResult(
                    CheckoutBusinessPrepareDisposition.EXISTING_RECONCILE_FIRST, prior,
                )
            }
            if (events.size >= MAX_EVENTS) unavailable()
            val now = wallTime()
            // Resolve the opaque session at the storage commit boundary, after read/decode and
            // immediately before the prepare CAS. Callers cannot manufacture attribution.
            val attribution = authority.authorize(session, type.capability, scope)
            val prepared = CheckoutBusinessJournalEvent(
                0, operationId, type, attribution, targetIdentity, businessIdentity,
                commandDigest, CheckoutBusinessEventOutcome.PREPARED, now, "prepared", null,
                ByteArray(HASH_BYTES), ByteArray(HASH_BYTES),
            )
            val next = append(events, prepared)
            try {
                if (storage.compareAndSwap(oldBytes, encode(next))) {
                    return@synchronized CheckoutBusinessPrepareResult(
                        CheckoutBusinessPrepareDisposition.READY_TO_EXECUTE, next.last(),
                    )
                }
            } catch (_: Exception) { unavailable() }
        }
        unavailable()
    }

    /** Record a known durable domain-store commit and stable commit/record identity. */
    internal fun commit(operationId: String, outcomeCode: String, businessCommitIdentity: String): CheckoutBusinessJournalEvent =
        transition(operationId, CheckoutBusinessEventOutcome.COMMITTED, outcomeCode, businessCommitIdentity)

    /** Use only after the domain store definitively confirms that this operation did not commit. */
    internal fun abort(operationId: String, outcomeCode: String): CheckoutBusinessJournalEvent =
        transition(operationId, CheckoutBusinessEventOutcome.ABORTED, outcomeCode, null)

    /** Unknown store write/result: require lookup by operationId before retrying the business action. */
    internal fun requireReconciliation(operationId: String, reasonCode: String): CheckoutBusinessJournalEvent =
        transition(operationId, CheckoutBusinessEventOutcome.RECONCILE_REQUIRED, reasonCode, null)

    /** Immutable transition history for one operation. A caller must authorize access to this data. */
    internal fun history(operationId: String): List<CheckoutBusinessJournalEvent> = synchronized(monitor) {
        ensureAvailable(); validateOperationId(operationId)
        immutable(decodeOrEmpty(readBytes()).filter { it.operationId == operationId })
    }

    /** Internal full snapshot for domain-specific reconcilers; not an access-control API. */
    internal fun snapshot(): List<CheckoutBusinessJournalEvent> = synchronized(monitor) {
        ensureAvailable(); immutable(decodeOrEmpty(readBytes()))
    }

    private data class Binding(
        val operationId: String,
        val eventType: CheckoutBusinessEventType,
        val attribution: ActorAttribution,
        val targetIdentity: String,
        val businessIdentity: String,
        val digest: ByteArray,
    )

    private fun CheckoutBusinessJournalEvent.sameBinding(b: Binding): Boolean =
        operationId == b.operationId && eventType == b.eventType && samePrincipal(attribution, b.attribution) &&
            targetIdentity == b.targetIdentity && businessIdentity == b.businessIdentity &&
            MessageDigest.isEqual(digestCopy(), b.digest)

    // A retry is reauthorized now, so its event clock can differ. The persisted row keeps the exact
    // original ActorAttribution; idempotency requires the same principal, grant, generation and scope.
    private fun samePrincipal(a: ActorAttribution, b: ActorAttribution): Boolean =
        a.actorId == b.actorId && a.displayNameAtEvent == b.displayNameAtEvent && a.role == b.role &&
            a.capabilityId == b.capabilityId && a.policyGeneration == b.policyGeneration && a.scope == b.scope

    private fun transition(
        operationId: String,
        desired: CheckoutBusinessEventOutcome,
        code: String,
        commitIdentity: String?,
    ): CheckoutBusinessJournalEvent = synchronized(monitor) {
        ensureAvailable(); validateOperationId(operationId); validateCode(code)
        if (commitIdentity != null) validateIdentity(commitIdentity)
        repeat(MAX_CAS_ATTEMPTS) {
            val oldBytes = readBytes()
            val events = decodeOrEmpty(oldBytes)
            val current = events.lastOrNull { it.operationId == operationId }
                ?: throw CheckoutBusinessJournalTransitionException()
            if (current.outcome == desired) {
                if (current.outcomeCode == code && current.businessCommitIdentity == commitIdentity) return@synchronized current
                throw CheckoutBusinessOperationConflictException()
            }
            if (!validTransition(current.outcome, desired)) throw CheckoutBusinessJournalTransitionException()
            if ((desired == CheckoutBusinessEventOutcome.COMMITTED) != (commitIdentity != null)) {
                throw CheckoutBusinessJournalTransitionException()
            }
            if (events.size >= MAX_EVENTS) unavailable()
            val now = wallTime()
            val draft = CheckoutBusinessJournalEvent(
                0, current.operationId, current.eventType, current.attribution, current.targetIdentity,
                current.businessIdentity, current.commandDigest, desired, now, code, commitIdentity,
                ByteArray(HASH_BYTES), ByteArray(HASH_BYTES),
            )
            val next = append(events, draft)
            try {
                if (storage.compareAndSwap(oldBytes, encode(next))) return@synchronized next.last()
            } catch (_: Exception) { unavailable() }
        }
        unavailable()
    }

    private fun append(events: List<CheckoutBusinessJournalEvent>, draft: CheckoutBusinessJournalEvent): List<CheckoutBusinessJournalEvent> {
        val sequence = try { Math.addExact(events.lastOrNull()?.sequence ?: 0L, 1L) }
            catch (_: ArithmeticException) { unavailable() }
        val previous = events.lastOrNull()?.hashCopy() ?: ByteArray(HASH_BYTES)
        val provisional = draft.withIntegrity(sequence, previous, ByteArray(HASH_BYTES))
        return events + provisional.withIntegrity(sequence, previous, hashEvent(previous, eventPayload(provisional)))
    }

    private fun encode(events: List<CheckoutBusinessJournalEvent>): ByteArray {
        require(events.size <= MAX_EVENTS)
        val buffer = ByteArrayOutputStream()
        DataOutputStream(buffer).use { out ->
            out.writeInt(MAGIC); out.writeInt(VERSION); out.writeInt(events.size)
            events.forEach { event ->
                val payload = eventPayload(event)
                out.writeInt(payload.size); out.write(payload); out.write(event.previousCopy()); out.write(event.hashCopy())
            }
        }
        return buffer.toByteArray().also { require(it.size in MIN_FILE_BYTES..MAX_FILE_BYTES) }
    }

    private fun decode(bytes: ByteArray): List<CheckoutBusinessJournalEvent> {
        require(bytes.size in MIN_FILE_BYTES..MAX_FILE_BYTES)
        val input = DataInputStream(ByteArrayInputStream(bytes))
        require(input.readInt() == MAGIC && input.readInt() == VERSION)
        val count = input.readInt(); require(count in 0..MAX_EVENTS)
        val events = ArrayList<CheckoutBusinessJournalEvent>(count)
        val lastByOperation = HashMap<String, CheckoutBusinessJournalEvent>()
        var previousHash = ByteArray(HASH_BYTES)
        repeat(count) { index ->
            val size = input.readInt(); require(size in 1..MAX_EVENT_BYTES && size <= input.available() - 2 * HASH_BYTES)
            val payload = ByteArray(size); input.readFully(payload)
            val storedPrevious = ByteArray(HASH_BYTES); input.readFully(storedPrevious)
            val storedHash = ByteArray(HASH_BYTES); input.readFully(storedHash)
            require(MessageDigest.isEqual(storedPrevious, previousHash))
            require(MessageDigest.isEqual(storedHash, hashEvent(previousHash, payload)))
            val event = readPayload(payload, storedPrevious, storedHash)
            require(event.sequence == index + 1L)
            val prior = lastByOperation[event.operationId]
            if (prior == null) require(event.outcome == CheckoutBusinessEventOutcome.PREPARED && event.outcomeCode == "prepared")
            else {
                require(prior.sameBinding(Binding(event.operationId, event.eventType, event.attribution,
                    event.targetIdentity, event.businessIdentity, event.digestCopy())))
                require(validTransition(prior.outcome, event.outcome))
                if (prior.outcome == event.outcome) require(
                    prior.outcomeCode == event.outcomeCode && prior.businessCommitIdentity == event.businessCommitIdentity,
                )
            }
            lastByOperation[event.operationId] = event
            events.add(event); previousHash = storedHash
        }
        require(input.available() == 0)
        return events
    }

    private fun validTransition(from: CheckoutBusinessEventOutcome, to: CheckoutBusinessEventOutcome): Boolean = when (from) {
        CheckoutBusinessEventOutcome.PREPARED -> to in setOf(
            CheckoutBusinessEventOutcome.COMMITTED, CheckoutBusinessEventOutcome.ABORTED,
            CheckoutBusinessEventOutcome.RECONCILE_REQUIRED,
        )
        CheckoutBusinessEventOutcome.RECONCILE_REQUIRED -> to in setOf(
            CheckoutBusinessEventOutcome.COMMITTED, CheckoutBusinessEventOutcome.ABORTED,
            CheckoutBusinessEventOutcome.RECONCILE_REQUIRED,
        )
        CheckoutBusinessEventOutcome.COMMITTED, CheckoutBusinessEventOutcome.ABORTED -> false
    }

    private fun eventPayload(e: CheckoutBusinessJournalEvent): ByteArray {
        val buffer = ByteArrayOutputStream()
        DataOutputStream(buffer).use { out ->
            out.writeLong(e.sequence); writeText(out, e.operationId, MAX_ID); writeText(out, e.eventType.key, MAX_TYPE)
            writeText(out, e.attribution.actorId, MAX_ID); writeText(out, e.attribution.displayNameAtEvent, MAX_NAME)
            out.writeByte(e.attribution.role.ordinal); writeText(out, e.attribution.capabilityId, MAX_TYPE)
            out.writeLong(e.attribution.policyGeneration); writeScope(out, e.attribution.scope)
            out.writeLong(e.attribution.eventTimeMillis)
            writeText(out, e.targetIdentity, MAX_ID); writeText(out, e.businessIdentity, MAX_ID)
            out.write(e.digestCopy()); out.writeByte(e.outcome.ordinal); out.writeLong(e.recordedAtMillis)
            writeText(out, e.outcomeCode, MAX_CODE); writeOptionalText(out, e.businessCommitIdentity, MAX_ID)
        }
        return buffer.toByteArray()
    }

    private fun readPayload(payload: ByteArray, previous: ByteArray, hash: ByteArray): CheckoutBusinessJournalEvent {
        val input = DataInputStream(ByteArrayInputStream(payload))
        val sequence = input.readLong(); val operationId = readText(input, MAX_ID)
        val type = CheckoutBusinessEventType.fromKey(readText(input, MAX_TYPE)) ?: error("unknown event type")
        val actor = readText(input, MAX_ID); val name = readText(input, MAX_NAME)
        val role = CheckoutAuthorityRole.entries.getOrNull(input.readUnsignedByte()) ?: error("role")
        val capability = readText(input, MAX_TYPE); val generation = input.readLong(); val scope = readScope(input)
        val actorTime = input.readLong()
        val attribution = ActorAttribution.create(actor, name, role, capability, generation, scope, actorTime)
        val target = readText(input, MAX_ID); val business = readText(input, MAX_ID)
        val digest = ByteArray(DIGEST_BYTES); input.readFully(digest)
        val outcome = CheckoutBusinessEventOutcome.entries.getOrNull(input.readUnsignedByte()) ?: error("outcome")
        val recordedAt = input.readLong(); val code = readText(input, MAX_CODE); val commit = readOptionalText(input, MAX_ID)
        require(input.available() == 0)
        val event = CheckoutBusinessJournalEvent(sequence, operationId, type, attribution, target, business,
            digest, outcome, recordedAt, code, commit, previous, hash)
        validateEvent(event)
        require(MessageDigest.isEqual(hash, hashEvent(previous, eventPayload(event))))
        return event
    }

    private fun validateEvent(e: CheckoutBusinessJournalEvent) {
        require(e.sequence > 0 && e.recordedAtMillis >= 0 && e.commandDigest.size == DIGEST_BYTES)
        validateOperationId(e.operationId); validateIdentity(e.targetIdentity); validateIdentity(e.businessIdentity)
        require(e.attribution.scope == scope && e.attribution.capabilityId == e.eventType.capability.id &&
            e.attribution.actorId.isNotBlank() && e.attribution.displayNameAtEvent.isNotBlank() &&
            e.attribution.policyGeneration >= 0 && e.attribution.eventTimeMillis >= 0)
        validateCode(e.outcomeCode)
        if (e.outcome == CheckoutBusinessEventOutcome.COMMITTED) validateIdentity(e.businessCommitIdentity ?: error("commit identity"))
        else require(e.businessCommitIdentity == null)
        require(e.previousHash.size == HASH_BYTES && e.eventHash.size == HASH_BYTES)
    }

    private fun validateOperationId(value: String) = validateToken(value, MAX_ID)
    private fun validateIdentity(value: String) = validateToken(value, MAX_ID)
    private fun validateCode(value: String) = validateToken(value, MAX_CODE)
    private fun validateToken(value: String, max: Int) {
        require(value.length in 1..max && value.all { it in 'a'..'z' || it in 'A'..'Z' || it in '0'..'9' || it in "/._:-" })
    }
    private fun hashEvent(previous: ByteArray, payload: ByteArray) = MessageDigest.getInstance("SHA-256").digest(previous + payload)
    private fun writeScope(out: DataOutputStream, scope: CheckoutAuthorityScope) {
        writeText(out, scope.merchantId, MAX_SCOPE); writeText(out, scope.storeId, MAX_SCOPE)
        writeText(out, scope.locationId, MAX_SCOPE); writeText(out, scope.registerId, MAX_SCOPE)
        writeText(out, scope.deviceId, MAX_SCOPE)
    }
    private fun readScope(input: DataInputStream) = CheckoutAuthorityScope(
        readText(input, MAX_SCOPE), readText(input, MAX_SCOPE), readText(input, MAX_SCOPE),
        readText(input, MAX_SCOPE), readText(input, MAX_SCOPE),
    )
    private fun writeText(out: DataOutputStream, value: String, max: Int) {
        val bytes = value.toByteArray(Charsets.UTF_8); require(bytes.isNotEmpty() && bytes.size <= max)
        out.writeShort(bytes.size); out.write(bytes)
    }
    private fun writeOptionalText(out: DataOutputStream, value: String?, max: Int) {
        out.writeByte(if (value == null) 0 else 1); if (value != null) writeText(out, value, max)
    }
    private fun readText(input: DataInputStream, max: Int): String {
        val size = input.readUnsignedShort(); require(size in 1..max && size <= input.available())
        val bytes = ByteArray(size); input.readFully(bytes)
        return Charsets.UTF_8.newDecoder().onMalformedInput(CodingErrorAction.REPORT)
            .onUnmappableCharacter(CodingErrorAction.REPORT).decode(ByteBuffer.wrap(bytes)).toString()
    }
    private fun readOptionalText(input: DataInputStream, max: Int): String? = when (input.readUnsignedByte()) {
        0 -> null
        1 -> readText(input, max)
        else -> error("optional flag")
    }
    private fun immutable(events: List<CheckoutBusinessJournalEvent>): List<CheckoutBusinessJournalEvent> =
        Collections.unmodifiableList(ArrayList(events))
    private fun readBytes(): ByteArray? = try {
        storage.read()?.also { if (it.size !in MIN_FILE_BYTES..MAX_FILE_BYTES) unavailable() }
    } catch (_: Exception) { unavailable() }
    private fun decodeOrEmpty(bytes: ByteArray?): List<CheckoutBusinessJournalEvent> =
        if (bytes == null) emptyList() else try { decode(bytes) } catch (_: Exception) { unavailable() }
    private fun wallTime(): Long = clock.wallTimeMillis().also { if (it < 0) unavailable() }
    private fun ensureAvailable() { if (poisoned) throw CheckoutBusinessJournalUnavailableException() }
    private fun unavailable(): Nothing { poisoned = true; throw CheckoutBusinessJournalUnavailableException() }

    companion object {
        private const val MAGIC = 0x4F424A4C
        private const val VERSION = 1
        private const val MAX_FILE_BYTES = 1024 * 1024
        private const val MIN_FILE_BYTES = 44
        private const val MAX_EVENTS = 4096
        private const val MAX_EVENT_BYTES = 4096
        private const val MAX_CAS_ATTEMPTS = 8
        private const val HASH_BYTES = 32
        private const val DIGEST_BYTES = 32
        private const val MAX_ID = 128
        private const val MAX_TYPE = 64
        private const val MAX_NAME = 256
        private const val MAX_CODE = 64
        private const val MAX_SCOPE = 64
    }
}
