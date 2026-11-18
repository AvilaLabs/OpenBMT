package org.openbmt.checkout

import java.io.ByteArrayInputStream
import java.io.ByteArrayOutputStream
import java.io.DataInputStream
import java.io.DataOutputStream
import java.nio.ByteBuffer
import java.nio.charset.CodingErrorAction
import java.security.MessageDigest
import java.util.Collections

enum class CheckoutAuthorityAuditAction {
    BOOTSTRAP, STAFF_CREATED, STAFF_RENAMED, STAFF_ROLE_CHANGED, STAFF_PIN_CHANGED,
    STAFF_DISABLED, MANAGER_RECOVERED, LIFECYCLE_LOCKED,
    STAFF_CREATE_PREPARED, STAFF_RENAME_PREPARED, STAFF_ROLE_PREPARED, STAFF_PIN_PREPARED,
    STAFF_DISABLE_PREPARED, STAFF_OPERATION_REJECTED,
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
) {
    override fun toString() = "CheckoutAuthorityAuditEvent(sequence=$sequence, action=$action, operationId=redacted)"
    internal fun numbered(n: Long) = CheckoutAuthorityAuditEvent(n, operationId, action, actorId,
        actorDisplayName, actorRole, capabilityId, policyGeneration, scope, occurredAtMillis,
        subjectActorId, subjectDisplayNameBefore, subjectDisplayNameAfter, subjectRoleBefore, subjectRoleAfter)
    internal fun sameCommand(b: CheckoutAuthorityAuditEvent) = operationId == b.operationId && action == b.action &&
        actorId == b.actorId && actorDisplayName == b.actorDisplayName && actorRole == b.actorRole &&
        capabilityId == b.capabilityId && policyGeneration == b.policyGeneration && scope == b.scope &&
        occurredAtMillis == b.occurredAtMillis && subjectActorId == b.subjectActorId &&
        subjectDisplayNameBefore == b.subjectDisplayNameBefore && subjectDisplayNameAfter == b.subjectDisplayNameAfter &&
        subjectRoleBefore == b.subjectRoleBefore && subjectRoleAfter == b.subjectRoleAfter
}

class CheckoutAuthorityAuditUnavailableException : IllegalStateException("Checkout authority audit unavailable")
class CheckoutAuthorityAuditOperationConflictException : IllegalStateException("Checkout authority audit operation ID conflict")

/** Versioned bounded journal. Android storage must be Keystore-sealed; NIO storage is for JVM/local use. */
class CheckoutAuthorityAuditJournal(private val storage: CheckoutAuthorityStorage) {
    private val monitor = Any()
    private var poisoned = false

    internal fun snapshot(): List<CheckoutAuthorityAuditEvent> = synchronized(monitor) {
        ensureAvailable()
        Collections.unmodifiableList(ArrayList(readEvents()))
    }

    internal fun findOperation(operationId: String): CheckoutAuthorityAuditEvent? = synchronized(monitor) {
        ensureAvailable(); checkOperationId(operationId)
        readEvents().firstOrNull { it.operationId == operationId }
    }

    internal fun append(event: CheckoutAuthorityAuditEvent): CheckoutAuthorityAuditEvent = synchronized(monitor) {
        ensureAvailable(); validate(event, allowZero = true)
        repeat(MAX_CAS_ATTEMPTS) {
            val oldBytes = readBytes()
            val old = decodeOrEmpty(oldBytes)
            old.firstOrNull { it.operationId == event.operationId }?.let {
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
        }
        val payload = buffer.toByteArray()
        return (payload + MessageDigest.getInstance("SHA-256").digest(payload)).also { require(it.size in MIN_BYTES..MAX_BYTES) }
    }

    private fun decode(bytes: ByteArray): List<CheckoutAuthorityAuditEvent> {
        require(bytes.size in MIN_BYTES..MAX_BYTES)
        val payload = bytes.copyOfRange(0, bytes.size - HASH_BYTES)
        require(MessageDigest.isEqual(bytes.copyOfRange(bytes.size - HASH_BYTES, bytes.size), MessageDigest.getInstance("SHA-256").digest(payload)))
        val input = DataInputStream(ByteArrayInputStream(payload))
        require(input.readInt() == MAGIC && input.readInt() == VERSION)
        val count = input.readInt(); require(count in 0..MAX_EVENTS)
        val events = ArrayList<CheckoutAuthorityAuditEvent>(count); val opIds = HashSet<String>()
        repeat(count) { i ->
            val e = readEvent(input); validate(e, false); require(e.sequence == i + 1L && opIds.add(e.operationId)); events.add(e)
        }
        require(input.available() == 0)
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
    }

    private fun readEvent(i: DataInputStream): CheckoutAuthorityAuditEvent {
        val seq=i.readLong(); val op=readText(i,MAX_OP)
        val action=CheckoutAuthorityAuditAction.entries.getOrNull(i.readUnsignedByte()) ?: error("action")
        val actor=readOptional(i,MAX_FIELD); val actorName=readOptional(i,MAX_NAME); val actorRole=readRole(i.readByte().toInt())
        val capability=readText(i,MAX_CAP); val generation=i.readLong()
        val scope=CheckoutAuthorityScope(readText(i,MAX_SCOPE),readText(i,MAX_SCOPE),readText(i,MAX_SCOPE),readText(i,MAX_SCOPE),readText(i,MAX_SCOPE))
        val at=i.readLong(); val subject=readOptional(i,MAX_FIELD); val before=readOptional(i,MAX_NAME); val after=readOptional(i,MAX_NAME)
        return CheckoutAuthorityAuditEvent(seq,op,action,actor,actorName,actorRole,capability,generation,scope,at,subject,before,after,readRole(i.readByte().toInt()),readRole(i.readByte().toInt()))
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
            CheckoutAuthorityAuditAction.STAFF_RENAME_PREPARED -> require(
                e.subjectActorId != null && e.subjectDisplayNameBefore != null && e.subjectDisplayNameAfter != null && e.capabilityId == "staff.manage",
            )
            CheckoutAuthorityAuditAction.STAFF_ROLE_PREPARED -> require(
                e.subjectActorId != null && e.subjectRoleBefore != null && e.subjectRoleAfter != null && e.capabilityId == "staff.manage",
            )
            CheckoutAuthorityAuditAction.STAFF_PIN_PREPARED, CheckoutAuthorityAuditAction.STAFF_DISABLE_PREPARED -> require(
                e.subjectActorId != null && e.capabilityId == "staff.manage",
            )
            CheckoutAuthorityAuditAction.STAFF_OPERATION_REJECTED -> require(e.capabilityId == "staff.manage")
        }
    }

    private fun checkOperationId(v: String) { require(v.length in 8..MAX_OP && v.all { it.isLetterOrDigit() && it.code < 128 || it == '-' || it == '_' }) }
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
        private const val MAGIC=0x4F42414A; private const val VERSION=1; private const val MAX_BYTES=1024*1024
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
        staffWrite(
            op, session, scope, CheckoutAuthorityAuditAction.STAFF_CREATE_PREPARED,
            commit = { actor, commitId ->
                val staff = authority.createStaff(session, scope, name, role, pin)
                event(commitId, CheckoutAuthorityAuditAction.STAFF_CREATED, actor, staff.actorId,
                    afterName = staff.displayName, afterRole = staff.role)
            },
            prepare = { actor, prepareId ->
                event(prepareId, CheckoutAuthorityAuditAction.STAFF_CREATE_PREPARED, actor,
                    afterName = name.trim(), afterRole = role)
            },
        )
    } finally { pin.fill('\u0000') }

    fun renameStaffPrepared(op: String, session: CheckoutAuthoritySession, scope: CheckoutAuthorityScope,
                            actorId: String, name: String): CheckoutAuthorityAuditEvent =
        staffWrite(
            op, session, scope, CheckoutAuthorityAuditAction.STAFF_RENAME_PREPARED,
            commit = { actor, commitId ->
                val before = staff(session, scope, actorId)
                val after = authority.renameStaff(session, scope, actorId, name)
                event(commitId, CheckoutAuthorityAuditAction.STAFF_RENAMED, actor, actorId,
                    beforeName = before.displayName, afterName = after.displayName,
                    beforeRole = before.role, afterRole = after.role)
            },
            prepare = { actor, prepareId ->
                val before = staff(session, scope, actorId)
                event(prepareId, CheckoutAuthorityAuditAction.STAFF_RENAME_PREPARED, actor, actorId,
                    beforeName = before.displayName, afterName = name.trim(),
                    beforeRole = before.role, afterRole = before.role)
            },
        )

    fun changeRolePrepared(op: String, session: CheckoutAuthoritySession, scope: CheckoutAuthorityScope,
                           actorId: String, role: CheckoutAuthorityRole): CheckoutAuthorityAuditEvent =
        staffWrite(
            op, session, scope, CheckoutAuthorityAuditAction.STAFF_ROLE_PREPARED,
            commit = { actor, commitId ->
                val before = staff(session, scope, actorId)
                val after = authority.changeRole(session, scope, actorId, role)
                event(commitId, CheckoutAuthorityAuditAction.STAFF_ROLE_CHANGED, actor, actorId,
                    beforeName = before.displayName, afterName = after.displayName,
                    beforeRole = before.role, afterRole = after.role)
            },
            prepare = { actor, prepareId ->
                val before = staff(session, scope, actorId)
                event(prepareId, CheckoutAuthorityAuditAction.STAFF_ROLE_PREPARED, actor, actorId,
                    beforeName = before.displayName, afterName = before.displayName,
                    beforeRole = before.role, afterRole = role)
            },
        )

    fun changePinPrepared(op: String, session: CheckoutAuthoritySession, scope: CheckoutAuthorityScope,
                          actorId: String, pin: CharArray): CheckoutAuthorityAuditEvent = try {
        staffWrite(
            op, session, scope, CheckoutAuthorityAuditAction.STAFF_PIN_PREPARED,
            commit = { actor, commitId ->
                authority.changePin(session, scope, actorId, pin)
                event(commitId, CheckoutAuthorityAuditAction.STAFF_PIN_CHANGED, actor, actorId)
            },
            prepare = { actor, prepareId ->
                staff(session, scope, actorId)
                event(prepareId, CheckoutAuthorityAuditAction.STAFF_PIN_PREPARED, actor, actorId)
            },
        )
    } finally { pin.fill('\u0000') }

    fun disableStaffPrepared(op: String, session: CheckoutAuthoritySession, scope: CheckoutAuthorityScope,
                             actorId: String): CheckoutAuthorityAuditEvent =
        staffWrite(
            op, session, scope, CheckoutAuthorityAuditAction.STAFF_DISABLE_PREPARED,
            commit = { actor, commitId ->
                val before = staff(session, scope, actorId)
                val after = authority.disableStaff(session, scope, actorId)
                event(commitId, CheckoutAuthorityAuditAction.STAFF_DISABLED, actor, actorId,
                    beforeName = before.displayName, afterName = after.displayName,
                    beforeRole = before.role, afterRole = after.role)
            },
            prepare = { actor, prepareId ->
                val before = staff(session, scope, actorId)
                event(prepareId, CheckoutAuthorityAuditAction.STAFF_DISABLE_PREPARED, actor, actorId,
                    beforeName = before.displayName, afterName = before.displayName,
                    beforeRole = before.role, afterRole = before.role)
            },
        )

    private fun staffWrite(
        operationId: String,
        session: CheckoutAuthoritySession,
        scope: CheckoutAuthorityScope,
        preparedAction: CheckoutAuthorityAuditAction,
        commit: (ActorAttribution, String) -> CheckoutAuthorityAuditEvent,
        prepare: (ActorAttribution, String) -> CheckoutAuthorityAuditEvent,
    ): CheckoutAuthorityAuditEvent = synchronized(this) {
        ensureAvailable()
        require(operationId.length in 8..126 && operationId.all { it.isLetterOrDigit() && it.code < 128 || it == '-' || it == '_' })
        ensureNoUnresolvedStaffIntent()
        val actor = principal(session, scope)
        val prepareId = "p-$operationId"
        val commitId = "c-$operationId"
        val rejectId = "r-$operationId"
        val intent = prepare(actor, prepareId)
        require(intent.action == preparedAction && intent.capabilityId == CheckoutAuthorityCapability.STAFF_MANAGE.id)
        val previousIntent = try { journal.findOperation(prepareId) } catch (_: Exception) { poison() }
        if (previousIntent != null) {
            if (!sameStaffIntent(previousIntent, intent)) throw CheckoutAuthorityAuditOperationConflictException()
            val previousCommit = try { journal.findOperation(commitId) } catch (_: Exception) { poison() }
            if (previousCommit != null) return@synchronized previousCommit
            val previousRejection = try { journal.findOperation(rejectId) } catch (_: Exception) { poison() }
            if (previousRejection != null) throw CheckoutAuthorityDeniedException()
            poison()
        }
        try { journal.append(intent) } catch (_: Exception) { poison() }
        val committed = try {
            commit(actor, commitId)
        } catch (denied: CheckoutAuthorityDeniedException) {
            val rejected = event(rejectId, CheckoutAuthorityAuditAction.STAFF_OPERATION_REJECTED, actor,
                intent.subjectActorId, beforeName = intent.subjectDisplayNameBefore,
                afterName = intent.subjectDisplayNameAfter, beforeRole = intent.subjectRoleBefore,
                afterRole = intent.subjectRoleAfter)
            try { journal.append(rejected) } catch (_: Exception) { poison() }
            throw denied
        } catch (_: Exception) {
            // The mutation may have committed. Keep the durable prepare row unresolved and block
            // further admin writes until an explicit recovery/reconciliation path exists.
            poison()
        }
        try { journal.append(committed) } catch (_: Exception) { poison() }
        committed
    }

    private fun sameStaffIntent(a: CheckoutAuthorityAuditEvent, b: CheckoutAuthorityAuditEvent): Boolean =
        a.action == b.action && a.actorId == b.actorId && a.actorRole == b.actorRole &&
            a.capabilityId == b.capabilityId && a.scope == b.scope && a.subjectActorId == b.subjectActorId &&
            a.subjectDisplayNameAfter == b.subjectDisplayNameAfter && a.subjectRoleAfter == b.subjectRoleAfter

    private fun ensureNoUnresolvedStaffIntent() {
        val events = try { journal.snapshot() } catch (_: Exception) { poison() }
        val operationIds = events.mapTo(HashSet()) { it.operationId }
        val preparedActions = setOf(
            CheckoutAuthorityAuditAction.STAFF_CREATE_PREPARED,
            CheckoutAuthorityAuditAction.STAFF_RENAME_PREPARED,
            CheckoutAuthorityAuditAction.STAFF_ROLE_PREPARED,
            CheckoutAuthorityAuditAction.STAFF_PIN_PREPARED,
            CheckoutAuthorityAuditAction.STAFF_DISABLE_PREPARED,
        )
        val unresolved = events.any { event ->
            event.action in preparedActions && event.operationId.startsWith("p-") &&
                "c-${event.operationId.removePrefix("p-")}" !in operationIds &&
                "r-${event.operationId.removePrefix("p-")}" !in operationIds
        }
        if (unresolved) poison()
    }

    private fun principal(s:CheckoutAuthoritySession,scope:CheckoutAuthorityScope)=authority.authorize(s,CheckoutAuthorityCapability.STAFF_MANAGE,scope)
    private fun staff(s:CheckoutAuthoritySession,scope:CheckoutAuthorityScope,id:String)=authority.listStaff(s,scope).firstOrNull{it.actorId==id} ?: throw CheckoutAuthorityDeniedException()

    private fun event(op:String,action:CheckoutAuthorityAuditAction,p:ActorAttribution,subject:String?=null,
                      beforeName:String?=null,afterName:String?=null,beforeRole:CheckoutAuthorityRole?=null,afterRole:CheckoutAuthorityRole?=null)=
        event(op,action,p.actorId,p.displayNameAtEvent,p.role,p.capabilityId,p.policyGeneration,p.scope,p.eventTimeMillis,subject,beforeName,afterName,beforeRole,afterRole)

    private fun event(op:String,action:CheckoutAuthorityAuditAction,actor:String?,name:String?,role:CheckoutAuthorityRole?,capability:String,
                      generation:Long,scope:CheckoutAuthorityScope,at:Long,subject:String?=null,beforeName:String?=null,afterName:String?=null,
                      beforeRole:CheckoutAuthorityRole?=null,afterRole:CheckoutAuthorityRole?=null)=
        CheckoutAuthorityAuditEvent(0,op,action,actor,name,role,capability,generation,scope,at,subject,beforeName,afterName,beforeRole,afterRole)

    private fun execute(op:String,action:CheckoutAuthorityAuditAction,subject:String?=null,
                        expectedScope:CheckoutAuthorityScope?=null,expectedName:String?=null,
                        expectedRole:CheckoutAuthorityRole?=null,command:()->CheckoutAuthorityAuditEvent)=synchronized(this){
        ensureAvailable(); require(op.length in 8..128 && op.all{it.isLetterOrDigit()&&it.code<128||it=='-'||it=='_'})
        val previous=try{journal.findOperation(op)}catch(_:Exception){poison()}
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
}
