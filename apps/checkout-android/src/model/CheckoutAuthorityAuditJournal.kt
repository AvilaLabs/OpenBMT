package org.openbmt.checkout

import java.io.ByteArrayInputStream
import java.io.ByteArrayOutputStream
import java.io.DataInputStream
import java.io.DataOutputStream
import java.nio.ByteBuffer
import java.nio.charset.CodingErrorAction
import java.security.MessageDigest
import java.util.Collections
import java.util.UUID

private const val STAFF_OPERATION_PROTOCOL_VERSION = 2

enum class CheckoutAuthorityAuditAction {
    BOOTSTRAP, STAFF_CREATED, STAFF_RENAMED, STAFF_ROLE_CHANGED, STAFF_PIN_CHANGED,
    STAFF_DISABLED, MANAGER_RECOVERED, LIFECYCLE_LOCKED,
    STAFF_CREATE_PREPARED, STAFF_RENAME_PREPARED, STAFF_ROLE_PREPARED, STAFF_PIN_PREPARED,
    STAFF_DISABLE_PREPARED, STAFF_OPERATION_REJECTED, STAFF_OPERATION_RECONCILED_NO_CHANGE,
    STAFF_CREATE_PREPARED_WITH_RECEIPT, STAFF_RENAME_PREPARED_WITH_RECEIPT,
    STAFF_ROLE_PREPARED_WITH_RECEIPT, STAFF_PIN_PREPARED_WITH_RECEIPT,
    STAFF_DISABLE_PREPARED_WITH_RECEIPT,
}

/** Immutable snapshot. Construction is internal; only the authenticated command service creates events. */
class CheckoutAuthorityAuditEvent internal constructor(
    val sequence: Long,
    val operationId: String,
    val action: CheckoutAuthorityAuditAction,
    val actorId: String?,
    val actorDisplayName: String?,
    val actorRole: CheckoutAuthorityRole?,
    val capabilityId: String,
    val policyGeneration: Long,
    val scope: CheckoutAuthorityScope,
    val occurredAtMillis: Long,
    val subjectActorId: String?,
    val subjectDisplayNameBefore: String?,
    val subjectDisplayNameAfter: String?,
    val subjectRoleBefore: CheckoutAuthorityRole?,
    val subjectRoleAfter: CheckoutAuthorityRole?,
    val operationProtocolVersion: Int = 0,
    val ownerEpoch: String? = null,
) {
    override fun toString() = "CheckoutAuthorityAuditEvent(sequence=$sequence, action=$action, operationId=redacted)"
    internal fun numbered(n: Long) = CheckoutAuthorityAuditEvent(n, operationId, action, actorId,
        actorDisplayName, actorRole, capabilityId, policyGeneration, scope, occurredAtMillis,
        subjectActorId, subjectDisplayNameBefore, subjectDisplayNameAfter, subjectRoleBefore, subjectRoleAfter,
        operationProtocolVersion, ownerEpoch)
    internal fun sameCommand(b: CheckoutAuthorityAuditEvent) = operationId == b.operationId && action == b.action &&
        actorId == b.actorId && actorDisplayName == b.actorDisplayName && actorRole == b.actorRole &&
        capabilityId == b.capabilityId && policyGeneration == b.policyGeneration && scope == b.scope &&
        occurredAtMillis == b.occurredAtMillis && subjectActorId == b.subjectActorId &&
        subjectDisplayNameBefore == b.subjectDisplayNameBefore && subjectDisplayNameAfter == b.subjectDisplayNameAfter &&
        subjectRoleBefore == b.subjectRoleBefore && subjectRoleAfter == b.subjectRoleAfter &&
        operationProtocolVersion == b.operationProtocolVersion && ownerEpoch == b.ownerEpoch
}

/** Process epoch plus a live-operation set proves restart abandonment without wall-clock expiry. */
internal object CheckoutAuthorityStaffOperationOwnership {
    val processEpoch: String = UUID.randomUUID().toString()
    private val monitor = Any()
    private val active = HashSet<String>()

    fun claim(operationId: String): Boolean = synchronized(monitor) { active.add(operationId) }
    fun release(operationId: String) = synchronized(monitor) { active.remove(operationId); Unit }

    fun <T> withAbandonedOwner(operationId: String, ownerEpoch: String?, block: () -> T): T = synchronized(monitor) {
        if (ownerEpoch.isNullOrBlank() || operationId in active) {
            throw CheckoutAuthorityAuditReconciliationRequiredException()
        }
        block()
    }
}

private fun isReservedStaffOperationId(value: String) = value.startsWith("p-") || value.startsWith("c-") ||
    value.startsWith("r-") || value.startsWith("n-")

internal data class CheckoutAuthorityAuditStampedSnapshot(val events: List<CheckoutAuthorityAuditEvent>, val revision: ByteArray)

open class CheckoutAuthorityAuditUnavailableException : IllegalStateException("Checkout authority audit unavailable")
class CheckoutAuthorityAuditOperationConflictException : IllegalStateException("Checkout authority audit operation ID conflict")
class CheckoutAuthorityAuditReconciliationRequiredException : CheckoutAuthorityAuditUnavailableException()

/** Versioned bounded journal. Android storage must be Keystore-sealed; NIO storage is for JVM/local use. */
class CheckoutAuthorityAuditJournal(private val storage: CheckoutAuthorityStorage) {
    private val monitor = Any()
    private var poisoned = false

    internal fun snapshot(): List<CheckoutAuthorityAuditEvent> = synchronized(monitor) {
        ensureAvailable()
        Collections.unmodifiableList(ArrayList(readEvents()))
    }

    internal fun snapshotStamped(): CheckoutAuthorityAuditStampedSnapshot = synchronized(monitor) {
        ensureAvailable()
        val bytes = readBytes()
        val events = decodeOrEmpty(bytes)
        CheckoutAuthorityAuditStampedSnapshot(
            Collections.unmodifiableList(ArrayList(events)),
            MessageDigest.getInstance("SHA-256").digest(bytes ?: ByteArray(0)),
        )
    }

    internal fun findOperation(operationId: String, requiredProtocolVersion: Int? = null): CheckoutAuthorityAuditEvent? = synchronized(monitor) {
        ensureAvailable(); checkOperationId(operationId)
        readEvents().firstOrNull { it.operationId == operationId &&
            (requiredProtocolVersion == null || it.operationProtocolVersion == requiredProtocolVersion) }
    }

    internal fun append(event: CheckoutAuthorityAuditEvent): CheckoutAuthorityAuditEvent = appendChecked(event, false)

    /** Prepare-row predicate is evaluated inside each CAS attempt to serialize competing managers. */
    internal fun appendStaffPrepare(event: CheckoutAuthorityAuditEvent): CheckoutAuthorityAuditEvent =
        appendChecked(event, true)

    private fun appendChecked(event: CheckoutAuthorityAuditEvent, requireNoUnresolvedPrepare: Boolean): CheckoutAuthorityAuditEvent = synchronized(monitor) {
        ensureAvailable(); validate(event, allowZero = true)
        if (isReservedStaffOperationId(event.operationId) && event.operationProtocolVersion != STRICT_PROTOCOL_VERSION) {
            throw CheckoutAuthorityAuditOperationConflictException()
        }
        repeat(MAX_CAS_ATTEMPTS) {
            val oldBytes = readBytes()
            val old = decodeOrEmpty(oldBytes)
            if (requireNoUnresolvedPrepare) {
                val base = event.operationId.removePrefix("p-")
                val sameId = old.firstOrNull { it.operationId == event.operationId }
                if (sameId != null && sameId.operationProtocolVersion != STRICT_PROTOCOL_VERSION) {
                    throw CheckoutAuthorityAuditOperationConflictException()
                }
                if (old.any { it.operationId in reservedStaffOperationIds(base) && it.operationId != event.operationId }) {
                    throw CheckoutAuthorityAuditOperationConflictException()
                }
            }
            if (requireNoUnresolvedPrepare && hasUnresolvedStaffPrepare(old)) {
                throw CheckoutAuthorityAuditReconciliationRequiredException()
            }
            old.firstOrNull { it.operationId == event.operationId }?.let {
                if (requireNoUnresolvedPrepare) throw CheckoutAuthorityAuditReconciliationRequiredException()
                if (!it.sameCommand(event)) throw CheckoutAuthorityAuditOperationConflictException()
                return@synchronized it
            }
            if (old.size >= MAX_EVENTS) unavailable()
            val number = try { Math.addExact(old.lastOrNull()?.sequence ?: 0L, 1L) }
                catch (_: ArithmeticException) { unavailable() }
            val numbered = event.numbered(number)
            val bytes = try { encode(old + numbered) } catch (_: Exception) { unavailable() }
            try { if (storage.compareAndSwap(oldBytes, bytes)) return@synchronized numbered }
            catch (_: Exception) { unavailable() } // commit outcome may be uncertain
        }
        unavailable()
    }

    private fun hasUnresolvedStaffPrepare(events: List<CheckoutAuthorityAuditEvent>): Boolean {
        val strict = events.filter { it.operationProtocolVersion == STRICT_PROTOCOL_VERSION }
        val ids = strict.mapTo(HashSet()) { it.operationId }
        return strict.any { isReceiptRequiredPreparedAction(it.action) && it.operationId.startsWith("p-") &&
            terminalIds(it.operationId.removePrefix("p-")).none { terminal -> terminal in ids } }
    }

    private fun readEvents() = decodeOrEmpty(readBytes())
    private fun readBytes(): ByteArray? = try {
        storage.read()?.also { if (it.size !in MIN_BYTES..MAX_BYTES) unavailable() }
    } catch (_: Exception) { unavailable() }
    private fun decodeOrEmpty(bytes: ByteArray?) = if (bytes == null) emptyList() else try { decode(bytes) } catch (_: Exception) { unavailable() }
    private fun ensureAvailable() { if (poisoned) throw CheckoutAuthorityAuditUnavailableException() }
    private fun unavailable(): Nothing { poisoned = true; throw CheckoutAuthorityAuditUnavailableException() }

    private fun encode(events: List<CheckoutAuthorityAuditEvent>): ByteArray {
        require(events.size <= MAX_EVENTS)
        val buffer = ByteArrayOutputStream()
        DataOutputStream(buffer).use { out ->
            out.writeInt(MAGIC); out.writeInt(VERSION); out.writeInt(events.size)
            events.forEachIndexed { i, e -> validate(e, false); require(e.sequence == i + 1L); writeEvent(out, e) }
            validateHistory(events)
        }
        val payload = buffer.toByteArray()
        return (payload + MessageDigest.getInstance("SHA-256").digest(payload)).also { require(it.size in MIN_BYTES..MAX_BYTES) }
    }

    private fun decode(bytes: ByteArray): List<CheckoutAuthorityAuditEvent> {
        require(bytes.size in MIN_BYTES..MAX_BYTES)
        val payload = bytes.copyOfRange(0, bytes.size - HASH_BYTES)
        require(MessageDigest.isEqual(bytes.copyOfRange(bytes.size - HASH_BYTES, bytes.size), MessageDigest.getInstance("SHA-256").digest(payload)))
        val input = DataInputStream(ByteArrayInputStream(payload))
        require(input.readInt() == MAGIC)
        val version = input.readInt()
        require(version in LEGACY_VERSION..VERSION)
        val count = input.readInt(); require(count in 0..MAX_EVENTS)
        val events = ArrayList<CheckoutAuthorityAuditEvent>(count); val opIds = HashSet<String>()
        repeat(count) { i ->
            val e = readEvent(input, version); validate(e, false); require(e.sequence == i + 1L && opIds.add(e.operationId)); events.add(e)
        }
        require(input.available() == 0)
        if (version >= STRICT_PROTOCOL_VERSION) validateHistory(events)
        return events
    }

    private fun writeEvent(o: DataOutputStream, e: CheckoutAuthorityAuditEvent) {
        o.writeLong(e.sequence); text(o, e.operationId, MAX_OP); o.writeByte(e.action.ordinal)
        optional(o, e.actorId, MAX_FIELD); optional(o, e.actorDisplayName, MAX_NAME); o.writeByte(role(e.actorRole))
        text(o, e.capabilityId, MAX_CAP); o.writeLong(e.policyGeneration)
        text(o, e.scope.merchantId, MAX_SCOPE); text(o, e.scope.storeId, MAX_SCOPE); text(o, e.scope.locationId, MAX_SCOPE)
        text(o, e.scope.registerId, MAX_SCOPE); text(o, e.scope.deviceId, MAX_SCOPE); o.writeLong(e.occurredAtMillis)
        optional(o, e.subjectActorId, MAX_FIELD); optional(o, e.subjectDisplayNameBefore, MAX_NAME)
        optional(o, e.subjectDisplayNameAfter, MAX_NAME); o.writeByte(role(e.subjectRoleBefore)); o.writeByte(role(e.subjectRoleAfter))
        o.writeInt(e.operationProtocolVersion); optional(o, e.ownerEpoch, MAX_OP)
    }

    private fun readEvent(i: DataInputStream, version: Int): CheckoutAuthorityAuditEvent {
        val seq=i.readLong(); val op=readText(i,MAX_OP)
        val action=CheckoutAuthorityAuditAction.entries.getOrNull(i.readUnsignedByte()) ?: error("action")
        val actor=readOptional(i,MAX_FIELD); val actorName=readOptional(i,MAX_NAME); val actorRole=readRole(i.readByte().toInt())
        val capability=readText(i,MAX_CAP); val generation=i.readLong()
        val scope=CheckoutAuthorityScope(readText(i,MAX_SCOPE),readText(i,MAX_SCOPE),readText(i,MAX_SCOPE),readText(i,MAX_SCOPE),readText(i,MAX_SCOPE))
        val at=i.readLong(); val subject=readOptional(i,MAX_FIELD); val before=readOptional(i,MAX_NAME); val after=readOptional(i,MAX_NAME)
        val beforeRole = readRole(i.readByte().toInt()); val afterRole = readRole(i.readByte().toInt())
        val protocol = if (version >= STRICT_PROTOCOL_VERSION) i.readInt() else 0
        val owner = if (version >= STRICT_PROTOCOL_VERSION) readOptional(i, MAX_OP) else null
        return CheckoutAuthorityAuditEvent(seq,op,action,actor,actorName,actorRole,capability,generation,scope,at,
            subject,before,after,beforeRole,afterRole,protocol,owner)
    }

    private fun validate(e: CheckoutAuthorityAuditEvent, allowZero: Boolean) {
        require(if (allowZero) e.sequence >= 0 else e.sequence > 0)
        checkOperationId(e.operationId); require(e.policyGeneration >= 0 && e.occurredAtMillis >= 0)
        require(e.capabilityId.isNotBlank() && e.capabilityId.length <= MAX_CAP && e.capabilityId.none { it == '\u0000' })
        listOf(e.actorId to MAX_FIELD,e.actorDisplayName to MAX_NAME,e.subjectActorId to MAX_FIELD,
            e.subjectDisplayNameBefore to MAX_NAME,e.subjectDisplayNameAfter to MAX_NAME).forEach { (v,n) ->
            require(v == null || (v.isNotBlank() && v.none { it == '\u0000' } && v.toByteArray(Charsets.UTF_8).size <= n))
        }
        val systemAction = e.action in setOf(
            CheckoutAuthorityAuditAction.BOOTSTRAP,
            CheckoutAuthorityAuditAction.MANAGER_RECOVERED,
            CheckoutAuthorityAuditAction.LIFECYCLE_LOCKED,
        )
        require(systemAction == (e.actorId == null))
        require((e.actorId == null) == (e.actorRole == null))
        require(e.operationProtocolVersion in 0..STRICT_PROTOCOL_VERSION)
        if (e.operationProtocolVersion == 0) require(e.ownerEpoch == null)
        else require(e.ownerEpoch != null && e.ownerEpoch.length == 36 && e.ownerEpoch.all { it in '0'..'9' || it in 'a'..'f' || it == '-' })
        when (e.action) {
            CheckoutAuthorityAuditAction.BOOTSTRAP -> require(
                e.subjectActorId != null && e.subjectDisplayNameAfter != null && e.capabilityId == "authority.bootstrap",
            )
            CheckoutAuthorityAuditAction.STAFF_CREATED -> require(e.subjectActorId != null && e.subjectDisplayNameAfter != null && e.subjectRoleAfter != null)
            CheckoutAuthorityAuditAction.STAFF_RENAMED -> require(e.subjectActorId != null && e.subjectDisplayNameBefore != null && e.subjectDisplayNameAfter != null)
            CheckoutAuthorityAuditAction.STAFF_ROLE_CHANGED -> require(e.subjectActorId != null && e.subjectRoleBefore != null && e.subjectRoleAfter != null)
            CheckoutAuthorityAuditAction.STAFF_PIN_CHANGED, CheckoutAuthorityAuditAction.STAFF_DISABLED -> require(e.subjectActorId != null)
            CheckoutAuthorityAuditAction.MANAGER_RECOVERED -> require(e.subjectActorId != null && e.capabilityId == "authority.recovery")
            CheckoutAuthorityAuditAction.LIFECYCLE_LOCKED -> require(e.capabilityId == "authority.lifecycle.lock")
            CheckoutAuthorityAuditAction.STAFF_CREATE_PREPARED -> require(
                e.subjectActorId == null && e.subjectDisplayNameAfter != null && e.subjectRoleAfter != null && e.capabilityId == "staff.manage",
            )
            CheckoutAuthorityAuditAction.STAFF_CREATE_PREPARED_WITH_RECEIPT -> require(
                e.subjectActorId != null && e.subjectDisplayNameAfter != null && e.subjectRoleAfter != null && e.capabilityId == "staff.manage",
            )
            CheckoutAuthorityAuditAction.STAFF_RENAME_PREPARED, CheckoutAuthorityAuditAction.STAFF_RENAME_PREPARED_WITH_RECEIPT -> require(
                e.subjectActorId != null && e.subjectDisplayNameBefore != null && e.subjectDisplayNameAfter != null && e.capabilityId == "staff.manage",
            )
            CheckoutAuthorityAuditAction.STAFF_ROLE_PREPARED, CheckoutAuthorityAuditAction.STAFF_ROLE_PREPARED_WITH_RECEIPT -> require(
                e.subjectActorId != null && e.subjectRoleBefore != null && e.subjectRoleAfter != null && e.capabilityId == "staff.manage",
            )
            CheckoutAuthorityAuditAction.STAFF_PIN_PREPARED, CheckoutAuthorityAuditAction.STAFF_DISABLE_PREPARED,
            CheckoutAuthorityAuditAction.STAFF_PIN_PREPARED_WITH_RECEIPT,
            CheckoutAuthorityAuditAction.STAFF_DISABLE_PREPARED_WITH_RECEIPT -> require(
                e.subjectActorId != null && e.capabilityId == "staff.manage",
            )
            CheckoutAuthorityAuditAction.STAFF_OPERATION_REJECTED -> require(e.capabilityId == "staff.manage")
            CheckoutAuthorityAuditAction.STAFF_OPERATION_RECONCILED_NO_CHANGE -> require(e.capabilityId == "staff.manage")
        }
    }

    private fun checkOperationId(v: String) { require(v.length in 8..MAX_OP && v.all { it.isLetterOrDigit() && it.code < 128 || it == '-' || it == '_' }) }

    private fun isReceiptRequiredPreparedAction(action: CheckoutAuthorityAuditAction) = action in setOf(
        CheckoutAuthorityAuditAction.STAFF_CREATE_PREPARED_WITH_RECEIPT,
        CheckoutAuthorityAuditAction.STAFF_RENAME_PREPARED_WITH_RECEIPT,
        CheckoutAuthorityAuditAction.STAFF_ROLE_PREPARED_WITH_RECEIPT,
        CheckoutAuthorityAuditAction.STAFF_PIN_PREPARED_WITH_RECEIPT,
        CheckoutAuthorityAuditAction.STAFF_DISABLE_PREPARED_WITH_RECEIPT,
    )

    private fun reservedStaffOperationIds(base: String) = setOf("p-$base", "c-$base", "r-$base", "n-$base")

    private fun validateHistory(events: List<CheckoutAuthorityAuditEvent>) {
        val strictEvents = events.filter { it.operationProtocolVersion == STAFF_OPERATION_PROTOCOL_VERSION }
        val byId = strictEvents.associateBy { it.operationId }
        strictEvents.forEach { event ->
            val isPrepare = event.operationId.startsWith("p-")
            val isCommit = event.operationId.startsWith("c-")
            val isReject = event.operationId.startsWith("r-")
            val isNoChange = event.operationId.startsWith("n-")
            if (isPrepare) require(isReceiptRequiredPreparedAction(event.action))
            if (isCommit) require(event.action in setOf(
                CheckoutAuthorityAuditAction.STAFF_CREATED, CheckoutAuthorityAuditAction.STAFF_RENAMED,
                CheckoutAuthorityAuditAction.STAFF_ROLE_CHANGED, CheckoutAuthorityAuditAction.STAFF_PIN_CHANGED,
                CheckoutAuthorityAuditAction.STAFF_DISABLED,
            ))
            if (isReject) require(event.action == CheckoutAuthorityAuditAction.STAFF_OPERATION_REJECTED)
            if (isNoChange) require(event.action == CheckoutAuthorityAuditAction.STAFF_OPERATION_RECONCILED_NO_CHANGE)
            if (event.action == CheckoutAuthorityAuditAction.STAFF_OPERATION_REJECTED) require(isReject)
            if (event.action == CheckoutAuthorityAuditAction.STAFF_OPERATION_RECONCILED_NO_CHANGE) require(isNoChange)
            if (isPrepare || isCommit || isReject || isNoChange) {
                val prefix = event.operationId.substringBefore('-')
                val base = event.operationId.substringAfter('-', "")
                require(base.length in 8..126)
                val prepare = if (prefix == "p") event else byId["p-$base"] ?: error("terminal lacks prepare")
                if (prefix != "p") require(sameAuditCommand(
                    prepare, event,
                    allowLegacyCreateResultId = prepare.action == CheckoutAuthorityAuditAction.STAFF_CREATE_PREPARED &&
                        event.action == CheckoutAuthorityAuditAction.STAFF_CREATED,
                ))
            }
        }
        strictEvents.filter { isReceiptRequiredPreparedAction(it.action) }.forEach { prepare ->
            require(prepare.operationId.startsWith("p-"))
            val base = prepare.operationId.removePrefix("p-")
            val terminals = listOf("c-$base", "r-$base", "n-$base").mapNotNull(byId::get)
            require(terminals.size <= 1)
            terminals.singleOrNull()?.let { terminal ->
                require(prepare.sequence < terminal.sequence)
                val expectedAction = when (terminal.operationId.first()) {
                    'c' -> when (prepare.action) {
                        CheckoutAuthorityAuditAction.STAFF_CREATE_PREPARED,
                        CheckoutAuthorityAuditAction.STAFF_CREATE_PREPARED_WITH_RECEIPT -> CheckoutAuthorityAuditAction.STAFF_CREATED
                        CheckoutAuthorityAuditAction.STAFF_RENAME_PREPARED,
                        CheckoutAuthorityAuditAction.STAFF_RENAME_PREPARED_WITH_RECEIPT -> CheckoutAuthorityAuditAction.STAFF_RENAMED
                        CheckoutAuthorityAuditAction.STAFF_ROLE_PREPARED,
                        CheckoutAuthorityAuditAction.STAFF_ROLE_PREPARED_WITH_RECEIPT -> CheckoutAuthorityAuditAction.STAFF_ROLE_CHANGED
                        CheckoutAuthorityAuditAction.STAFF_PIN_PREPARED,
                        CheckoutAuthorityAuditAction.STAFF_PIN_PREPARED_WITH_RECEIPT -> CheckoutAuthorityAuditAction.STAFF_PIN_CHANGED
                        CheckoutAuthorityAuditAction.STAFF_DISABLE_PREPARED,
                        CheckoutAuthorityAuditAction.STAFF_DISABLE_PREPARED_WITH_RECEIPT -> CheckoutAuthorityAuditAction.STAFF_DISABLED
                        else -> error("invalid prepared action")
                    }
                    'r' -> CheckoutAuthorityAuditAction.STAFF_OPERATION_REJECTED
                    else -> CheckoutAuthorityAuditAction.STAFF_OPERATION_RECONCILED_NO_CHANGE
                }
                require(terminal.action == expectedAction)
                require(sameAuditCommand(
                    prepare, terminal,
                    allowLegacyCreateResultId = prepare.action == CheckoutAuthorityAuditAction.STAFF_CREATE_PREPARED &&
                        terminal.action == CheckoutAuthorityAuditAction.STAFF_CREATED,
                ))
            }
        }
    }

    private fun sameAuditCommand(
        a: CheckoutAuthorityAuditEvent,
        b: CheckoutAuthorityAuditEvent,
        allowLegacyCreateResultId: Boolean = false,
    ) =
        a.actorId == b.actorId && a.actorDisplayName == b.actorDisplayName && a.actorRole == b.actorRole &&
            a.capabilityId == b.capabilityId && a.policyGeneration == b.policyGeneration && a.scope == b.scope &&
            a.occurredAtMillis == b.occurredAtMillis &&
            (allowLegacyCreateResultId || a.subjectActorId == b.subjectActorId) &&
            a.subjectDisplayNameBefore == b.subjectDisplayNameBefore && a.subjectDisplayNameAfter == b.subjectDisplayNameAfter &&
            a.subjectRoleBefore == b.subjectRoleBefore && a.subjectRoleAfter == b.subjectRoleAfter &&
            a.operationProtocolVersion == b.operationProtocolVersion && a.ownerEpoch == b.ownerEpoch

    private fun isPreparedAction(action: CheckoutAuthorityAuditAction) = action in setOf(
        CheckoutAuthorityAuditAction.STAFF_CREATE_PREPARED, CheckoutAuthorityAuditAction.STAFF_RENAME_PREPARED,
        CheckoutAuthorityAuditAction.STAFF_ROLE_PREPARED, CheckoutAuthorityAuditAction.STAFF_PIN_PREPARED,
        CheckoutAuthorityAuditAction.STAFF_DISABLE_PREPARED, CheckoutAuthorityAuditAction.STAFF_CREATE_PREPARED_WITH_RECEIPT,
        CheckoutAuthorityAuditAction.STAFF_RENAME_PREPARED_WITH_RECEIPT, CheckoutAuthorityAuditAction.STAFF_ROLE_PREPARED_WITH_RECEIPT,
        CheckoutAuthorityAuditAction.STAFF_PIN_PREPARED_WITH_RECEIPT, CheckoutAuthorityAuditAction.STAFF_DISABLE_PREPARED_WITH_RECEIPT,
    )

    private fun terminalIds(operationId: String) = listOf("c-$operationId", "r-$operationId", "n-$operationId")
    private fun text(o: DataOutputStream, v: String, max: Int) { val b=v.toByteArray(Charsets.UTF_8); require(b.isNotEmpty() && b.size<=max); o.writeShort(b.size); o.write(b) }
    private fun optional(o: DataOutputStream, v: String?, max: Int) { o.writeBoolean(v!=null); if(v!=null) text(o,v,max) }
    private fun readText(i: DataInputStream,max: Int): String { val n=i.readUnsignedShort(); require(n in 1..max && n<=i.available()); val b=ByteArray(n); i.readFully(b); return Charsets.UTF_8.newDecoder().onMalformedInput(CodingErrorAction.REPORT).onUnmappableCharacter(CodingErrorAction.REPORT).decode(ByteBuffer.wrap(b)).toString() }
    private fun readOptional(i: DataInputStream,max: Int): String? = when (i.readUnsignedByte()) {
        0 -> null
        1 -> readText(i, max)
        else -> error("invalid optional field flag")
    }
    private fun role(r: CheckoutAuthorityRole?)=r?.ordinal ?: -1
    private fun readRole(r: Int): CheckoutAuthorityRole?=if(r==-1) null else CheckoutAuthorityRole.entries.getOrNull(r) ?: error("role")

    companion object {
        private const val MAGIC=0x4F42414A; private const val LEGACY_VERSION=1; private const val VERSION=2
        private const val STRICT_PROTOCOL_VERSION=STAFF_OPERATION_PROTOCOL_VERSION; private const val MAX_BYTES=1024*1024
        private const val MIN_BYTES=44; private const val HASH_BYTES=32; private const val MAX_EVENTS=4096
        private const val MAX_CAS_ATTEMPTS=8; private const val MAX_OP=128; private const val MAX_FIELD=128
        private const val MAX_NAME=256; private const val MAX_CAP=64; private const val MAX_SCOPE=64
    }
}

/** Authenticated command boundary: attribution always comes from CheckoutAuthority.authorize(). */
class CheckoutAuthorityCommandService(
    private val authority: CheckoutAuthority,
    private val journal: CheckoutAuthorityAuditJournal,
    private val clock: CheckoutAuthorityClock,
) {
    private var poisoned = false

    /** Manager-authorized audit view; returned rows are immutable snapshots. */
    fun auditSnapshot(session: CheckoutAuthoritySession, scope: CheckoutAuthorityScope): List<CheckoutAuthorityAuditEvent> {
        ensureAvailable()
        principal(session, scope)
        return journal.snapshot()
    }

    fun signIn(actorId: String, pin: CharArray, scope: CheckoutAuthorityScope): CheckoutAuthoritySession = try {
        ensureAvailable()
        // Failed sign-ins are intentionally omitted: authority persists throttling, and this journal
        // should not retain PIN attempts, unknown actor IDs, or a guessable login history.
        authority.authenticate(actorId, pin, scope)
    } finally {
        pin.fill('\u0000')
    }

    fun bootstrap(op: String, draft: CheckoutAuthorityBootstrapDraft, scope: CheckoutAuthorityScope,
                  displayName: String, pin: CharArray): CheckoutAuthorityAuditEvent = try {
        execute(op, CheckoutAuthorityAuditAction.BOOTSTRAP, draft.managerActorId,
            expectedScope = scope, expectedName = displayName.trim(), expectedRole = CheckoutAuthorityRole.MANAGER) {
        val result=authority.bootstrapManager(draft,scope,displayName,pin); val name=displayName.trim()
        event(op,CheckoutAuthorityAuditAction.BOOTSTRAP,null,null,null,
            "authority.bootstrap",1,scope,wallTime(),result.managerActorId,afterName=name,afterRole=CheckoutAuthorityRole.MANAGER)
    } } finally { pin.fill('\u0000') }

    /** Legacy post-write staff paths retained for compatibility tests; production UI uses the prepared variants below. */
    fun createStaff(op:String,session:CheckoutAuthoritySession,scope:CheckoutAuthorityScope,name:String,role:CheckoutAuthorityRole,pin:CharArray)=try {
        execute(op,CheckoutAuthorityAuditAction.STAFF_CREATED,
            expectedScope = scope, expectedName = name.trim(), expectedRole = role) {
            val p=principal(session,scope); val s=authority.createStaff(session,scope,name,role,pin)
            event(op,CheckoutAuthorityAuditAction.STAFF_CREATED,p,s.actorId,afterName=s.displayName,afterRole=s.role)
        }
    } finally { pin.fill('\u0000') }

    fun renameStaff(op:String,session:CheckoutAuthoritySession,scope:CheckoutAuthorityScope,actorId:String,name:String)=
        execute(op,CheckoutAuthorityAuditAction.STAFF_RENAMED,actorId, expectedScope = scope, expectedName = name.trim()) {
            val p=principal(session,scope); val old=staff(session,scope,actorId); val next=authority.renameStaff(session,scope,actorId,name)
            event(op,CheckoutAuthorityAuditAction.STAFF_RENAMED,p,actorId,beforeName=old.displayName,afterName=next.displayName,beforeRole=old.role,afterRole=next.role)
        }

    fun changeRole(op:String,session:CheckoutAuthoritySession,scope:CheckoutAuthorityScope,actorId:String,role:CheckoutAuthorityRole)=
        execute(op,CheckoutAuthorityAuditAction.STAFF_ROLE_CHANGED,actorId, expectedScope = scope, expectedRole = role) {
            val p=principal(session,scope); val old=staff(session,scope,actorId); val next=authority.changeRole(session,scope,actorId,role)
            event(op,CheckoutAuthorityAuditAction.STAFF_ROLE_CHANGED,p,actorId,beforeName=old.displayName,afterName=next.displayName,beforeRole=old.role,afterRole=next.role)
        }

    fun changePin(op:String,session:CheckoutAuthoritySession,scope:CheckoutAuthorityScope,actorId:String,pin:CharArray)=try {
        execute(op,CheckoutAuthorityAuditAction.STAFF_PIN_CHANGED,actorId, expectedScope = scope) {
            val p=principal(session,scope); authority.changePin(session,scope,actorId,pin)
            event(op,CheckoutAuthorityAuditAction.STAFF_PIN_CHANGED,p,actorId)
        }
    } finally { pin.fill('\u0000') }

    fun disableStaff(op:String,session:CheckoutAuthoritySession,scope:CheckoutAuthorityScope,actorId:String)=
        execute(op,CheckoutAuthorityAuditAction.STAFF_DISABLED,actorId, expectedScope = scope) {
            val p=principal(session,scope); val old=staff(session,scope,actorId); val next=authority.disableStaff(session,scope,actorId)
            event(op,CheckoutAuthorityAuditAction.STAFF_DISABLED,p,actorId,beforeName=old.displayName,afterName=next.displayName,beforeRole=old.role,afterRole=next.role)
        }

    fun recoverManager(op:String,scope:CheckoutAuthorityScope,recovery:CharArray,pin:CharArray,next:CheckoutAuthorityRecoveryDraft)=try {
        execute(op,CheckoutAuthorityAuditAction.MANAGER_RECOVERED, expectedScope = scope) {
            val r=authority.recoverManager(scope,recovery,pin,next)
            event(op,CheckoutAuthorityAuditAction.MANAGER_RECOVERED,null,null,null,
                "authority.recovery",r.policyGeneration,scope,wallTime(),r.managerActorId)
        }
    } finally { recovery.fill('\u0000'); pin.fill('\u0000') }

    fun lockForLifecycle(op:String,scope:CheckoutAuthorityScope)=execute(op,CheckoutAuthorityAuditAction.LIFECYCLE_LOCKED, expectedScope = scope) {
        authority.lock()
        event(op,CheckoutAuthorityAuditAction.LIFECYCLE_LOCKED,null,null,null,"authority.lifecycle.lock",0,scope,wallTime())
    }

    /** Staff mutation with a durable intent record written before the authority commit. */
    fun createStaffPrepared(op: String, session: CheckoutAuthoritySession, scope: CheckoutAuthorityScope,
                            name: String, role: CheckoutAuthorityRole, pin: CharArray): CheckoutAuthorityAuditEvent = try {
        require(op.length in 8..126 && op.all { it.isLetterOrDigit() && it.code < 128 || it == '-' || it == '_' })
        val actorId = try { journal.findOperation("p-$op", STAFF_OPERATION_PROTOCOL_VERSION)?.subjectActorId } catch (_: Exception) { poison() }
            ?: authority.prepareStaffActorId()
        staffWrite(
            op, session, scope, CheckoutAuthorityAuditAction.STAFF_CREATE_PREPARED_WITH_RECEIPT,
            commit = { actor, commitId, requestDigest, ownerEpoch ->
                val staff = authority.createStaffForOperation(session, scope, name, role, pin, actorId, op, requestDigest)
                event(commitId, CheckoutAuthorityAuditAction.STAFF_CREATED, actor, staff.actorId,
                    afterName = staff.displayName, afterRole = staff.role, ownerEpoch = ownerEpoch)
            },
            prepare = { actor, prepareId, ownerEpoch ->
                event(prepareId, CheckoutAuthorityAuditAction.STAFF_CREATE_PREPARED_WITH_RECEIPT, actor, actorId,
                    afterName = name.trim(), afterRole = role, ownerEpoch = ownerEpoch)
            },
        )
    } finally { pin.fill('\u0000') }

    fun renameStaffPrepared(op: String, session: CheckoutAuthoritySession, scope: CheckoutAuthorityScope,
                            actorId: String, name: String): CheckoutAuthorityAuditEvent =
        staffWrite(
            op, session, scope, CheckoutAuthorityAuditAction.STAFF_RENAME_PREPARED_WITH_RECEIPT,
            commit = { actor, commitId, requestDigest, ownerEpoch ->
                val before = staff(session, scope, actorId)
                val after = authority.renameStaffForOperation(session, scope, actorId, name, op, requestDigest)
                event(commitId, CheckoutAuthorityAuditAction.STAFF_RENAMED, actor, actorId,
                    beforeName = before.displayName, afterName = after.displayName,
                    beforeRole = before.role, afterRole = after.role, ownerEpoch = ownerEpoch)
            },
            prepare = { actor, prepareId, ownerEpoch ->
                val before = staff(session, scope, actorId)
                event(prepareId, CheckoutAuthorityAuditAction.STAFF_RENAME_PREPARED_WITH_RECEIPT, actor, actorId,
                    beforeName = before.displayName, afterName = name.trim(),
                    beforeRole = before.role, afterRole = before.role, ownerEpoch = ownerEpoch)
            },
        )

    fun changeRolePrepared(op: String, session: CheckoutAuthoritySession, scope: CheckoutAuthorityScope,
                           actorId: String, role: CheckoutAuthorityRole): CheckoutAuthorityAuditEvent =
        staffWrite(
            op, session, scope, CheckoutAuthorityAuditAction.STAFF_ROLE_PREPARED_WITH_RECEIPT,
            commit = { actor, commitId, requestDigest, ownerEpoch ->
                val before = staff(session, scope, actorId)
                val after = authority.changeRoleForOperation(session, scope, actorId, role, op, requestDigest)
                event(commitId, CheckoutAuthorityAuditAction.STAFF_ROLE_CHANGED, actor, actorId,
                    beforeName = before.displayName, afterName = after.displayName,
                    beforeRole = before.role, afterRole = after.role, ownerEpoch = ownerEpoch)
            },
            prepare = { actor, prepareId, ownerEpoch ->
                val before = staff(session, scope, actorId)
                event(prepareId, CheckoutAuthorityAuditAction.STAFF_ROLE_PREPARED_WITH_RECEIPT, actor, actorId,
                    beforeName = before.displayName, afterName = before.displayName,
                    beforeRole = before.role, afterRole = role, ownerEpoch = ownerEpoch)
            },
        )

    fun changePinPrepared(op: String, session: CheckoutAuthoritySession, scope: CheckoutAuthorityScope,
                          actorId: String, pin: CharArray): CheckoutAuthorityAuditEvent = try {
        staffWrite(
            op, session, scope, CheckoutAuthorityAuditAction.STAFF_PIN_PREPARED_WITH_RECEIPT,
            commit = { actor, commitId, requestDigest, ownerEpoch ->
                val before = staff(session, scope, actorId)
                authority.changePinForOperation(session, scope, actorId, pin, op, requestDigest)
                event(commitId, CheckoutAuthorityAuditAction.STAFF_PIN_CHANGED, actor, actorId,
                    beforeName = before.displayName, afterName = before.displayName,
                    beforeRole = before.role, afterRole = before.role, ownerEpoch = ownerEpoch)
            },
            prepare = { actor, prepareId, ownerEpoch ->
                val before = staff(session, scope, actorId)
                event(prepareId, CheckoutAuthorityAuditAction.STAFF_PIN_PREPARED_WITH_RECEIPT, actor, actorId,
                    beforeName = before.displayName, afterName = before.displayName,
                    beforeRole = before.role, afterRole = before.role, ownerEpoch = ownerEpoch)
            },
        )
    } finally { pin.fill('\u0000') }

    fun disableStaffPrepared(op: String, session: CheckoutAuthoritySession, scope: CheckoutAuthorityScope,
                             actorId: String): CheckoutAuthorityAuditEvent =
        staffWrite(
            op, session, scope, CheckoutAuthorityAuditAction.STAFF_DISABLE_PREPARED_WITH_RECEIPT,
            commit = { actor, commitId, requestDigest, ownerEpoch ->
                val before = staff(session, scope, actorId)
                val after = authority.disableStaffForOperation(session, scope, actorId, op, requestDigest)
                event(commitId, CheckoutAuthorityAuditAction.STAFF_DISABLED, actor, actorId,
                    beforeName = before.displayName, afterName = after.displayName,
                    beforeRole = before.role, afterRole = after.role, ownerEpoch = ownerEpoch)
            },
            prepare = { actor, prepareId, ownerEpoch ->
                val before = staff(session, scope, actorId)
                event(prepareId, CheckoutAuthorityAuditAction.STAFF_DISABLE_PREPARED_WITH_RECEIPT, actor, actorId,
                    beforeName = before.displayName, afterName = before.displayName,
                    beforeRole = before.role, afterRole = before.role, ownerEpoch = ownerEpoch)
            },
        )

    /** Resolve an uncertain prepare row from an authority-side mutation receipt or exact pre-state. */
    fun reconcileStaffIntents(session: CheckoutAuthoritySession, scope: CheckoutAuthorityScope): Int = synchronized(this) {
        ensureAvailable()
        principal(session, scope)
        var stableAudit: CheckoutAuthorityAuditStampedSnapshot? = null
        var state: CheckoutAuthorityStaffRecoveryState? = null
        for (attempt in 0 until MAX_STABLE_READ_ATTEMPTS) {
            val before = try { journal.snapshotStamped() } catch (_: Exception) { poison() }
            val authorityState = try { authority.staffRecoveryState(session, scope) } catch (_: Exception) { poison() }
            val after = try { journal.snapshotStamped() } catch (_: Exception) { poison() }
            if (MessageDigest.isEqual(before.revision, after.revision)) {
                stableAudit = after
                state = authorityState
                break
            }
        }
        val auditSnapshot = stableAudit ?: throw CheckoutAuthorityAuditReconciliationRequiredException()
        val events = auditSnapshot.events
        val authoritySnapshot = state ?: throw CheckoutAuthorityAuditReconciliationRequiredException()
        validateReceiptAnchors(authoritySnapshot.receipts, events, scope)
        val unresolved = unresolvedStaffIntents(events)
        if (unresolved.isEmpty()) return@synchronized 0
        if (unresolved.size != 1) throw CheckoutAuthorityAuditReconciliationRequiredException()
        val intent = unresolved.single()
        if (intent.scope != scope) throw CheckoutAuthorityAuditReconciliationRequiredException()
        val operationId = intent.operationId.removePrefix("p-")
        val receipt = authoritySnapshot.receipts.firstOrNull { it.operationId == operationId }
        val terminal = if (receipt != null) {
            if (!receiptMatches(receipt, intent)) {
                throw CheckoutAuthorityAuditReconciliationRequiredException()
            }
            terminalFromIntent(intent, committedAction(intent.action), "c-$operationId")
        } else {
            CheckoutAuthorityStaffOperationOwnership.withAbandonedOwner(operationId, intent.ownerEpoch) {
                val latestState = try { authority.staffRecoveryState(session, scope) } catch (_: Exception) { poison() }
                if (!MessageDigest.isEqual(authoritySnapshot.revision, latestState.revision) ||
                    !isReceiptRequiredPreparedAction(intent.action) || latestState.policyGeneration != intent.policyGeneration ||
                    !intentPreconditionHolds(intent, latestState)) {
                    throw CheckoutAuthorityAuditReconciliationRequiredException()
                }
                val unchangedEvents = try { journal.snapshotStamped() } catch (_: Exception) { poison() }
                if (!MessageDigest.isEqual(auditSnapshot.revision, unchangedEvents.revision)) {
                    throw CheckoutAuthorityAuditReconciliationRequiredException()
                }
                terminalFromIntent(intent, CheckoutAuthorityAuditAction.STAFF_OPERATION_RECONCILED_NO_CHANGE, "n-$operationId")
                    .also { try { journal.append(it) } catch (_: Exception) { poison() } }
            }
        }
        if (receipt != null) try { journal.append(terminal) } catch (_: Exception) { poison() }
        1
    }

    private fun staffWrite(
        operationId: String,
        session: CheckoutAuthoritySession,
        scope: CheckoutAuthorityScope,
        preparedAction: CheckoutAuthorityAuditAction,
        commit: (ActorAttribution, String, ByteArray, String) -> CheckoutAuthorityAuditEvent,
        prepare: (ActorAttribution, String, String) -> CheckoutAuthorityAuditEvent,
    ): CheckoutAuthorityAuditEvent = synchronized(this) {
        ensureAvailable()
        require(operationId.length in 8..126 && operationId.all { it.isLetterOrDigit() && it.code < 128 || it == '-' || it == '_' })
        val actor = principal(session, scope)
        val prepareId = "p-$operationId"
        val commitId = "c-$operationId"
        val rejectId = "r-$operationId"
        val noChangeId = "n-$operationId"
        val ownerEpoch = CheckoutAuthorityStaffOperationOwnership.processEpoch
        val intent = prepare(actor, prepareId, ownerEpoch)
        require(intent.action == preparedAction && intent.capabilityId == CheckoutAuthorityCapability.STAFF_MANAGE.id)
        val previousIntent = try { journal.findOperation(prepareId, STAFF_OPERATION_PROTOCOL_VERSION) } catch (_: Exception) { poison() }
        if (previousIntent != null) {
            if (!sameStaffIntent(previousIntent, intent)) throw CheckoutAuthorityAuditOperationConflictException()
            val previousCommit = try { journal.findOperation(commitId, STAFF_OPERATION_PROTOCOL_VERSION) } catch (_: Exception) { poison() }
            if (previousCommit != null) {
                ensureNoUnresolvedStaffIntent(session, scope)
                return@synchronized previousCommit
            }
            val previousRejection = try { journal.findOperation(rejectId, STAFF_OPERATION_PROTOCOL_VERSION) } catch (_: Exception) { poison() }
            if (previousRejection != null) {
                ensureNoUnresolvedStaffIntent(session, scope)
                throw CheckoutAuthorityDeniedException()
            }
            val previousNoChange = try { journal.findOperation(noChangeId, STAFF_OPERATION_PROTOCOL_VERSION) } catch (_: Exception) { poison() }
            if (previousNoChange != null) throw CheckoutAuthorityAuditReconciliationRequiredException()
            throw CheckoutAuthorityAuditReconciliationRequiredException()
        }
        ensureNoUnresolvedStaffIntent(session, scope)
        if (!CheckoutAuthorityStaffOperationOwnership.claim(operationId)) throw CheckoutAuthorityAuditReconciliationRequiredException()
        try {
            try { journal.appendStaffPrepare(intent) } catch (error: CheckoutAuthorityAuditReconciliationRequiredException) {
                throw error
            } catch (error: CheckoutAuthorityAuditOperationConflictException) {
                throw error
            } catch (_: Exception) { poison() }
            val committed = try {
                commit(actor, commitId, requestDigest(intent), ownerEpoch)
            } catch (denied: CheckoutAuthorityDeniedException) {
                val rejected = event(rejectId, CheckoutAuthorityAuditAction.STAFF_OPERATION_REJECTED, actor,
                    intent.subjectActorId, beforeName = intent.subjectDisplayNameBefore,
                    afterName = intent.subjectDisplayNameAfter, beforeRole = intent.subjectRoleBefore,
                    afterRole = intent.subjectRoleAfter, ownerEpoch = ownerEpoch)
                try { journal.append(rejected) } catch (_: Exception) { poison() }
                throw denied
            } catch (_: Exception) {
                // The mutation may have committed. Keep the durable prepare row unresolved and block
                // further admin writes until an explicit recovery/reconciliation path exists.
                poison()
            }
            try { journal.append(committed) } catch (_: Exception) { poison() }
        } finally {
            CheckoutAuthorityStaffOperationOwnership.release(operationId)
        }
    }

    private fun sameStaffIntent(a: CheckoutAuthorityAuditEvent, b: CheckoutAuthorityAuditEvent): Boolean =
        a.action == b.action && a.actorId == b.actorId &&
            a.capabilityId == b.capabilityId && a.scope == b.scope && a.subjectActorId == b.subjectActorId &&
            a.subjectDisplayNameBefore == b.subjectDisplayNameBefore && a.subjectRoleBefore == b.subjectRoleBefore &&
            a.subjectDisplayNameAfter == b.subjectDisplayNameAfter && a.subjectRoleAfter == b.subjectRoleAfter

    private fun ensureNoUnresolvedStaffIntent(session: CheckoutAuthoritySession, scope: CheckoutAuthorityScope) {
        for (attempt in 0 until MAX_STABLE_READ_ATTEMPTS) {
            val before = try { journal.snapshotStamped() } catch (_: Exception) { poison() }
            val state = try { authority.staffRecoveryState(session, scope) } catch (_: Exception) { poison() }
            val after = try { journal.snapshotStamped() } catch (_: Exception) { poison() }
            if (!MessageDigest.isEqual(before.revision, after.revision)) continue
            validateReceiptAnchors(state.receipts, after.events, scope)
            if (unresolvedStaffIntents(after.events).isNotEmpty()) {
                throw CheckoutAuthorityAuditReconciliationRequiredException()
            }
            return
        }
        throw CheckoutAuthorityAuditReconciliationRequiredException()
    }

    private fun unresolvedStaffIntents(events: List<CheckoutAuthorityAuditEvent>): List<CheckoutAuthorityAuditEvent> {
        val strictEvents = events.filter { it.operationProtocolVersion == STAFF_OPERATION_PROTOCOL_VERSION }
        val strictIds = strictEvents.mapTo(HashSet()) { it.operationId }
        return strictEvents.filter { isReceiptRequiredPreparedAction(it.action) && it.operationId.startsWith("p-") &&
            terminalIds(it.operationId.removePrefix("p-")).none { terminal -> terminal in strictIds } }
    }

    private fun validateReceiptAnchors(
        receipts: List<CheckoutAuthorityStaffOperationReceipt>,
        events: List<CheckoutAuthorityAuditEvent>,
        scope: CheckoutAuthorityScope?,
    ) {
        val receiptById = receipts.associateBy { it.operationId }
        for (receipt in receipts) {
            val intent = events.firstOrNull { it.operationId == "p-${receipt.operationId}" &&
                it.operationProtocolVersion == STAFF_OPERATION_PROTOCOL_VERSION } ?: poison()
            if ((scope != null && intent.scope != scope) || !receiptMatches(receipt, intent)) poison()
        }
        val strictEvents = events.filter { it.operationProtocolVersion == STAFF_OPERATION_PROTOCOL_VERSION }
        for (intent in strictEvents.filter { isReceiptRequiredPreparedAction(it.action) }) {
            val operationId = intent.operationId.removePrefix("p-")
            val commit = strictEvents.any { it.operationId == "c-$operationId" }
            val nonCommitTerminal = strictEvents.any { it.operationId == "r-$operationId" || it.operationId == "n-$operationId" }
            if (commit && receiptById[operationId] == null) poison()
            if (nonCommitTerminal && receiptById[operationId] != null) poison()
        }
    }

    private fun receiptMatches(receipt: CheckoutAuthorityStaffOperationReceipt, intent: CheckoutAuthorityAuditEvent): Boolean {
        val expectedKind = when (intent.action) {
            CheckoutAuthorityAuditAction.STAFF_CREATE_PREPARED,
            CheckoutAuthorityAuditAction.STAFF_CREATE_PREPARED_WITH_RECEIPT -> CheckoutAuthorityStaffOperationKind.CREATE
            CheckoutAuthorityAuditAction.STAFF_RENAME_PREPARED,
            CheckoutAuthorityAuditAction.STAFF_RENAME_PREPARED_WITH_RECEIPT -> CheckoutAuthorityStaffOperationKind.RENAME
            CheckoutAuthorityAuditAction.STAFF_ROLE_PREPARED,
            CheckoutAuthorityAuditAction.STAFF_ROLE_PREPARED_WITH_RECEIPT -> CheckoutAuthorityStaffOperationKind.ROLE
            CheckoutAuthorityAuditAction.STAFF_PIN_PREPARED,
            CheckoutAuthorityAuditAction.STAFF_PIN_PREPARED_WITH_RECEIPT -> CheckoutAuthorityStaffOperationKind.PIN
            CheckoutAuthorityAuditAction.STAFF_DISABLE_PREPARED,
            CheckoutAuthorityAuditAction.STAFF_DISABLE_PREPARED_WITH_RECEIPT -> CheckoutAuthorityStaffOperationKind.DISABLE
            else -> return false
        }
        val expectedResultGeneration = try { Math.addExact(intent.policyGeneration, 1L) } catch (_: ArithmeticException) { return false }
        return receipt.operationId == intent.operationId.removePrefix("p-") && receipt.kind == expectedKind &&
            receipt.subjectActorId == intent.subjectActorId && receipt.baseGeneration == intent.policyGeneration &&
            receipt.resultGeneration == expectedResultGeneration &&
            MessageDigest.isEqual(receipt.requestDigest, requestDigest(intent))
    }

    private fun intentPreconditionHolds(
        intent: CheckoutAuthorityAuditEvent,
        state: CheckoutAuthorityStaffRecoveryState,
    ): Boolean {
        val target = intent.subjectActorId?.let { id -> state.staff.firstOrNull { it.actorId == id } }
        return when (intent.action) {
            CheckoutAuthorityAuditAction.STAFF_CREATE_PREPARED, CheckoutAuthorityAuditAction.STAFF_CREATE_PREPARED_WITH_RECEIPT -> target == null
            CheckoutAuthorityAuditAction.STAFF_RENAME_PREPARED, CheckoutAuthorityAuditAction.STAFF_RENAME_PREPARED_WITH_RECEIPT -> target != null &&
                target.displayName == intent.subjectDisplayNameBefore && target.role == intent.subjectRoleBefore
            CheckoutAuthorityAuditAction.STAFF_ROLE_PREPARED, CheckoutAuthorityAuditAction.STAFF_ROLE_PREPARED_WITH_RECEIPT -> target != null &&
                target.displayName == intent.subjectDisplayNameBefore && target.role == intent.subjectRoleBefore
            CheckoutAuthorityAuditAction.STAFF_PIN_PREPARED, CheckoutAuthorityAuditAction.STAFF_PIN_PREPARED_WITH_RECEIPT -> target != null &&
                target.displayName == intent.subjectDisplayNameBefore && target.role == intent.subjectRoleBefore
            CheckoutAuthorityAuditAction.STAFF_DISABLE_PREPARED, CheckoutAuthorityAuditAction.STAFF_DISABLE_PREPARED_WITH_RECEIPT -> target != null &&
                target.displayName == intent.subjectDisplayNameBefore && target.role == intent.subjectRoleBefore
            else -> false
        }
    }

    private fun committedAction(prepared: CheckoutAuthorityAuditAction) = when (prepared) {
        CheckoutAuthorityAuditAction.STAFF_CREATE_PREPARED, CheckoutAuthorityAuditAction.STAFF_CREATE_PREPARED_WITH_RECEIPT -> CheckoutAuthorityAuditAction.STAFF_CREATED
        CheckoutAuthorityAuditAction.STAFF_RENAME_PREPARED, CheckoutAuthorityAuditAction.STAFF_RENAME_PREPARED_WITH_RECEIPT -> CheckoutAuthorityAuditAction.STAFF_RENAMED
        CheckoutAuthorityAuditAction.STAFF_ROLE_PREPARED, CheckoutAuthorityAuditAction.STAFF_ROLE_PREPARED_WITH_RECEIPT -> CheckoutAuthorityAuditAction.STAFF_ROLE_CHANGED
        CheckoutAuthorityAuditAction.STAFF_PIN_PREPARED, CheckoutAuthorityAuditAction.STAFF_PIN_PREPARED_WITH_RECEIPT -> CheckoutAuthorityAuditAction.STAFF_PIN_CHANGED
        CheckoutAuthorityAuditAction.STAFF_DISABLE_PREPARED, CheckoutAuthorityAuditAction.STAFF_DISABLE_PREPARED_WITH_RECEIPT -> CheckoutAuthorityAuditAction.STAFF_DISABLED
        else -> throw CheckoutAuthorityAuditReconciliationRequiredException()
    }

    private fun intentPostconditionHolds(
        intent: CheckoutAuthorityAuditEvent,
        state: CheckoutAuthorityStaffRecoveryState,
        resultGeneration: Long,
    ): Boolean {
        val pinOperation = intent.action == CheckoutAuthorityAuditAction.STAFF_PIN_PREPARED ||
            intent.action == CheckoutAuthorityAuditAction.STAFF_PIN_PREPARED_WITH_RECEIPT
        if (!pinOperation && state.policyGeneration != resultGeneration) return false
        val target = intent.subjectActorId?.let { id -> state.staff.firstOrNull { it.actorId == id } }
        return when (intent.action) {
            CheckoutAuthorityAuditAction.STAFF_CREATE_PREPARED, CheckoutAuthorityAuditAction.STAFF_CREATE_PREPARED_WITH_RECEIPT -> target != null && target.enabled &&
                target.displayName == intent.subjectDisplayNameAfter && target.role == intent.subjectRoleAfter
            CheckoutAuthorityAuditAction.STAFF_RENAME_PREPARED, CheckoutAuthorityAuditAction.STAFF_RENAME_PREPARED_WITH_RECEIPT -> target != null &&
                target.displayName == intent.subjectDisplayNameAfter && target.role == intent.subjectRoleAfter
            CheckoutAuthorityAuditAction.STAFF_ROLE_PREPARED, CheckoutAuthorityAuditAction.STAFF_ROLE_PREPARED_WITH_RECEIPT -> target != null &&
                target.displayName == intent.subjectDisplayNameBefore && target.role == intent.subjectRoleAfter
            CheckoutAuthorityAuditAction.STAFF_PIN_PREPARED, CheckoutAuthorityAuditAction.STAFF_PIN_PREPARED_WITH_RECEIPT -> target != null
            CheckoutAuthorityAuditAction.STAFF_DISABLE_PREPARED, CheckoutAuthorityAuditAction.STAFF_DISABLE_PREPARED_WITH_RECEIPT -> target != null && !target.enabled &&
                target.displayName == intent.subjectDisplayNameBefore && target.role == intent.subjectRoleBefore
            else -> false
        }
    }

    /** Digest contains only non-secret command shape; a PIN verifier or PIN-derived hash is never stored. */
    private fun requestDigest(intent: CheckoutAuthorityAuditEvent): ByteArray {
        val bytes = ByteArrayOutputStream()
        DataOutputStream(bytes).use { out ->
            out.writeUTF("openbmt.checkout.staff-operation")
            out.writeInt(2)
            out.writeUTF(intent.actorId ?: "")
            out.writeUTF(intent.capabilityId)
            out.writeUTF(intent.action.name)
            out.writeUTF(intent.operationId.removePrefix("p-"))
            out.writeUTF(intent.subjectActorId ?: "")
            out.writeUTF(intent.subjectDisplayNameBefore ?: "")
            out.writeUTF(intent.subjectDisplayNameAfter ?: "")
            out.writeInt(intent.subjectRoleBefore?.ordinal ?: -1)
            out.writeInt(intent.subjectRoleAfter?.ordinal ?: -1)
            out.writeUTF(intent.scope.merchantId); out.writeUTF(intent.scope.storeId)
            out.writeUTF(intent.scope.locationId); out.writeUTF(intent.scope.registerId); out.writeUTF(intent.scope.deviceId)
        }
        return MessageDigest.getInstance("SHA-256").digest(bytes.toByteArray())
    }

    private fun terminalFromIntent(
        intent: CheckoutAuthorityAuditEvent,
        action: CheckoutAuthorityAuditAction,
        operationId: String,
    ) = CheckoutAuthorityAuditEvent(
        0, operationId, action, intent.actorId, intent.actorDisplayName, intent.actorRole,
        intent.capabilityId, intent.policyGeneration, intent.scope, intent.occurredAtMillis,
        intent.subjectActorId, intent.subjectDisplayNameBefore, intent.subjectDisplayNameAfter,
        intent.subjectRoleBefore, intent.subjectRoleAfter, intent.operationProtocolVersion, intent.ownerEpoch,
    )

    private fun isPreparedAction(action: CheckoutAuthorityAuditAction) = action in setOf(
        CheckoutAuthorityAuditAction.STAFF_CREATE_PREPARED,
        CheckoutAuthorityAuditAction.STAFF_CREATE_PREPARED_WITH_RECEIPT,
        CheckoutAuthorityAuditAction.STAFF_RENAME_PREPARED,
        CheckoutAuthorityAuditAction.STAFF_RENAME_PREPARED_WITH_RECEIPT,
        CheckoutAuthorityAuditAction.STAFF_ROLE_PREPARED,
        CheckoutAuthorityAuditAction.STAFF_ROLE_PREPARED_WITH_RECEIPT,
        CheckoutAuthorityAuditAction.STAFF_PIN_PREPARED,
        CheckoutAuthorityAuditAction.STAFF_PIN_PREPARED_WITH_RECEIPT,
        CheckoutAuthorityAuditAction.STAFF_DISABLE_PREPARED,
        CheckoutAuthorityAuditAction.STAFF_DISABLE_PREPARED_WITH_RECEIPT,
    )

    private fun isReceiptRequiredPreparedAction(action: CheckoutAuthorityAuditAction) = action in setOf(
        CheckoutAuthorityAuditAction.STAFF_CREATE_PREPARED_WITH_RECEIPT,
        CheckoutAuthorityAuditAction.STAFF_RENAME_PREPARED_WITH_RECEIPT,
        CheckoutAuthorityAuditAction.STAFF_ROLE_PREPARED_WITH_RECEIPT,
        CheckoutAuthorityAuditAction.STAFF_PIN_PREPARED_WITH_RECEIPT,
        CheckoutAuthorityAuditAction.STAFF_DISABLE_PREPARED_WITH_RECEIPT,
    )

    private fun terminalIds(operationId: String) = listOf("c-$operationId", "r-$operationId", "n-$operationId")

    private fun principal(s:CheckoutAuthoritySession,scope:CheckoutAuthorityScope)=authority.authorize(s,CheckoutAuthorityCapability.STAFF_MANAGE,scope)
    private fun staff(s:CheckoutAuthoritySession,scope:CheckoutAuthorityScope,id:String)=authority.listStaff(s,scope).firstOrNull{it.actorId==id} ?: throw CheckoutAuthorityDeniedException()

    private fun event(op:String,action:CheckoutAuthorityAuditAction,p:ActorAttribution,subject:String?=null,
                      beforeName:String?=null,afterName:String?=null,beforeRole:CheckoutAuthorityRole?=null,afterRole:CheckoutAuthorityRole?=null,
                      ownerEpoch:String?=null)=
        event(op,action,p.actorId,p.displayNameAtEvent,p.role,p.capabilityId,p.policyGeneration,p.scope,p.eventTimeMillis,subject,beforeName,afterName,beforeRole,afterRole,ownerEpoch)

    private fun event(op:String,action:CheckoutAuthorityAuditAction,actor:String?,name:String?,role:CheckoutAuthorityRole?,capability:String,
                      generation:Long,scope:CheckoutAuthorityScope,at:Long,subject:String?=null,beforeName:String?=null,afterName:String?=null,
                      beforeRole:CheckoutAuthorityRole?=null,afterRole:CheckoutAuthorityRole?=null,ownerEpoch:String?=null)=
        CheckoutAuthorityAuditEvent(0,op,action,actor,name,role,capability,generation,scope,at,subject,beforeName,afterName,beforeRole,afterRole,
            if(ownerEpoch == null) 0 else STAFF_OPERATION_PROTOCOL_VERSION, ownerEpoch)

    private fun execute(op:String,action:CheckoutAuthorityAuditAction,subject:String?=null,
                        expectedScope:CheckoutAuthorityScope?=null,expectedName:String?=null,
                        expectedRole:CheckoutAuthorityRole?=null,command:()->CheckoutAuthorityAuditEvent)=synchronized(this){
        ensureAvailable(); require(op.length in 8..128 && op.all{it.isLetterOrDigit()&&it.code<128||it=='-'||it=='_'})
        if (isReservedStaffOperationId(op)) {
            throw IllegalArgumentException("reserved staff operation ID prefix")
        }
        val previous=try{journal.findOperation(op, 0)}catch(_:Exception){poison()}
        if(previous!=null){if (previous.action != action || (subject != null && previous.subjectActorId != subject) ||
                (expectedScope != null && previous.scope != expectedScope) ||
                (expectedName != null && previous.subjectDisplayNameAfter != expectedName) ||
                (expectedRole != null && previous.subjectRoleAfter != expectedRole)) {
                throw CheckoutAuthorityAuditOperationConflictException()
            }
            return@synchronized previous}
        val pending=command()
        try{journal.append(pending)}catch(_:Exception){poison()}
    }
    private fun wallTime()=clock.wallTimeMillis().also{if(it<0)throw CheckoutAuthorityAuditUnavailableException()}
    private fun ensureAvailable(){if(poisoned)throw CheckoutAuthorityAuditUnavailableException()}
    private fun poison():Nothing{poisoned=true;throw CheckoutAuthorityAuditUnavailableException()}

    private companion object { const val MAX_STABLE_READ_ATTEMPTS = 4 }
}
