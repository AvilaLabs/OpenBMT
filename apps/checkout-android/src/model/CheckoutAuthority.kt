package org.openbmt.checkout

import java.io.ByteArrayInputStream
import java.io.ByteArrayOutputStream
import java.io.DataInputStream
import java.io.DataOutputStream
import java.nio.ByteBuffer
import java.nio.charset.CodingErrorAction
import java.nio.channels.FileChannel
import java.nio.file.Files
import java.nio.file.Path
import java.nio.file.StandardCopyOption
import java.nio.file.StandardOpenOption
import java.security.MessageDigest
import java.security.SecureRandom
import java.util.IdentityHashMap
import java.util.concurrent.ConcurrentHashMap
import javax.crypto.SecretKeyFactory
import javax.crypto.spec.PBEKeySpec

/** Monotonic time is used for expiry and lockouts; wall time is only for event attribution. */
interface CheckoutAuthorityClock {
    fun elapsedRealtimeMillis(): Long
    fun wallTimeMillis(): Long
    /** Stable for one operating-system boot, so a persisted hold survives process recreation. */
    fun bootIdentity(): String
}

/** Injected to make IDs, salts and recovery credentials deterministic in JVM tests. */
interface CheckoutAuthorityRandom {
    fun bytes(count: Int): ByteArray
}

/**
 * Compare-and-swap storage for authority records. Implementations must serialize CAS operations
 * across instances/processes and must distinguish an absent record (expectedBytes == null).
 * Android production implementations must seal records with Keystore-backed authenticated
 * encryption and bind the installation scope as authenticated data.
 */
interface CheckoutAuthorityStorage {
    fun read(): ByteArray?
    fun compareAndSwap(expectedBytes: ByteArray?, nextBytes: ByteArray): Boolean
}

/** Desktop/JVM storage. Android must provide a Keystore-sealed [CheckoutAuthorityStorage]. */
class NioCheckoutAuthorityStorage(file: Path) : CheckoutAuthorityStorage {
    private val authorityFile = file.toAbsolutePath()
    private val lockFile = authorityFile.resolveSibling("${authorityFile.fileName}.lock")
    private val processLock = PROCESS_LOCKS.computeIfAbsent(lockFile.normalize()) { Any() }

    override fun read(): ByteArray? = withLock { readUnlocked() }

    override fun compareAndSwap(expectedBytes: ByteArray?, nextBytes: ByteArray): Boolean {
        require(nextBytes.size <= MAX_AUTHORITY_FILE_BYTES)
        return withLock {
            val current = readUnlocked()
            if (!sameBytes(current, expectedBytes)) return@withLock false
            atomicReplace(nextBytes)
            true
        }
    }

    private fun <T> withLock(block: () -> T): T {
        return synchronized(processLock) {
            val parent = authorityFile.parent ?: throw IllegalStateException("Authority storage needs a parent directory")
            Files.createDirectories(parent)
            val lockPath = parent.resolve(lockFile.fileName)
            FileChannel.open(lockPath, StandardOpenOption.CREATE, StandardOpenOption.WRITE).use { channel ->
                channel.lock().use { block() }
            }
        }
    }

    private fun readUnlocked(): ByteArray? {
        if (!Files.exists(authorityFile)) return null
        val size = Files.size(authorityFile)
        require(size in 1..MAX_AUTHORITY_FILE_BYTES.toLong()) { "Invalid authority record size" }
        return Files.readAllBytes(authorityFile).also { require(it.size.toLong() == size) }
    }

    private fun atomicReplace(bytes: ByteArray) {
        val parent = authorityFile.parent ?: throw IllegalStateException("Authority storage needs a parent directory")
        val temp = Files.createTempFile(parent, "${authorityFile.fileName}.", ".tmp")
        try {
            FileChannel.open(temp, StandardOpenOption.WRITE, StandardOpenOption.TRUNCATE_EXISTING).use { channel ->
                val buffer = ByteBuffer.wrap(bytes)
                while (buffer.hasRemaining()) channel.write(buffer)
                channel.force(true)
            }
            Files.move(temp, authorityFile, StandardCopyOption.ATOMIC_MOVE, StandardCopyOption.REPLACE_EXISTING)
            FileChannel.open(parent, StandardOpenOption.READ).use { it.force(true) }
        } finally {
            Files.deleteIfExists(temp)
        }
    }

    private fun sameBytes(left: ByteArray?, right: ByteArray?): Boolean = when {
        left == null || right == null -> left == null && right == null
        else -> MessageDigest.isEqual(left, right)
    }

    private companion object {
        const val MAX_AUTHORITY_FILE_BYTES = 1024 * 1024
        val PROCESS_LOCKS = ConcurrentHashMap<Path, Any>()
    }
}

private object SecureCheckoutAuthorityRandom : CheckoutAuthorityRandom {
    private val secure = SecureRandom()
    override fun bytes(count: Int): ByteArray = ByteArray(count).also(secure::nextBytes)
}

private fun validScopeId(value: String): Boolean = value.length in 1..64 &&
    (value.first() in 'a'..'z' || value.first() in '0'..'9') && value.all {
        it in 'a'..'z' || it in '0'..'9' || it == '-' || it == '_' || it == '.'
    }

/** Fixed, installation-local authority scope. Callers must present this exact tuple at every command. */
data class CheckoutAuthorityScope(
    val merchantId: String,
    val storeId: String,
    val locationId: String,
    val registerId: String,
    val deviceId: String,
) {
    init {
        require(validScopeId(merchantId) && validScopeId(storeId) && validScopeId(locationId) &&
            validScopeId(registerId) && validScopeId(deviceId)) { "Invalid checkout authority scope" }
    }
}

enum class CheckoutAuthorityRole { CASHIER, MANAGER }

/** Static allowlist. Capabilities without a grant in the role matrix remain denied. */
enum class CheckoutAuthorityCapability(val id: String) {
    SALE_CREATE("sale.create"),
    SALE_EDIT("sale.edit"),
    TENDER_BEGIN("tender.begin"),
    RECEIPT_READ("receipt.read"),
    SALES_READ("sales.read"),
    CASH_SESSION_OPEN("cash_session.open"),
    CASH_SESSION_MOVEMENT_RECORD("cash_session.movement.record"),
    CASH_SESSION_SALE_LINK("cash_session.sale.link"),
    CASH_SESSION_CLOSE("cash_session.close"),
    REFUND_REQUEST("refund.request"),
    STAFF_MANAGE("staff.manage"),
    REPORT_READ("report.read"),
    CATALOG_MANAGE("catalog.manage"),
    INVENTORY_MANAGE("inventory.manage"),
    REPORT_EXPORT("report.export"),
    WALLET_CONFIGURE("wallet.configure");

    companion object {
        fun fromId(id: String): CheckoutAuthorityCapability? = entries.firstOrNull { it.id == id }
    }
}

/** Stable principal plus event-time display/role/policy/scope attribution. Contains no credential. */
class ActorAttribution private constructor(
    val actorId: String,
    val displayNameAtEvent: String,
    val role: CheckoutAuthorityRole,
    val capabilityId: String,
    val policyGeneration: Long,
    val scope: CheckoutAuthorityScope,
    val eventTimeMillis: Long,
) {
    override fun equals(other: Any?): Boolean = other is ActorAttribution &&
        actorId == other.actorId && displayNameAtEvent == other.displayNameAtEvent &&
        role == other.role && capabilityId == other.capabilityId &&
        policyGeneration == other.policyGeneration && scope == other.scope &&
        eventTimeMillis == other.eventTimeMillis

    override fun hashCode(): Int {
        var result = actorId.hashCode()
        result = 31 * result + displayNameAtEvent.hashCode()
        result = 31 * result + role.hashCode()
        result = 31 * result + capabilityId.hashCode()
        result = 31 * result + policyGeneration.hashCode()
        result = 31 * result + scope.hashCode()
        result = 31 * result + eventTimeMillis.hashCode()
        return result
    }

    companion object {
        internal fun create(
            actorId: String, displayNameAtEvent: String, role: CheckoutAuthorityRole,
            capabilityId: String, policyGeneration: Long, scope: CheckoutAuthorityScope,
            eventTimeMillis: Long,
        ) = ActorAttribution(actorId, displayNameAtEvent, role, capabilityId, policyGeneration, scope, eventTimeMillis)
    }
}

data class CheckoutStaffSummary(
    val actorId: String,
    val displayName: String,
    val role: CheckoutAuthorityRole,
    val enabled: Boolean,
)

/** Non-secret, local sign-in directory. It exposes no verifier, recovery material or session. */
data class CheckoutAuthorityLoginProfile(
    val scope: CheckoutAuthorityScope,
    val enabledStaff: List<CheckoutStaffSummary>,
)

/** Non-secret state projection used only to resolve an uncertain staff-audit intent. */
internal data class CheckoutAuthorityStaffRecoveryState(
    val policyGeneration: Long,
    val staff: List<CheckoutStaffSummary>,
    val receipts: List<CheckoutAuthorityStaffOperationReceipt>,
    val revision: ByteArray,
)

internal enum class CheckoutAuthorityStaffOperationKind { CREATE, RENAME, ROLE, PIN, DISABLE }
internal data class CheckoutAuthorityStaffOperationReceipt(
    val operationId: String,
    val kind: CheckoutAuthorityStaffOperationKind,
    val subjectActorId: String,
    val baseGeneration: Long,
    val resultGeneration: Long,
    val requestDigest: ByteArray,
)

/** Opaque, process-local bearer. It is deliberately not serializable and its string form is redacted. */
class CheckoutAuthoritySession internal constructor() {
    override fun toString(): String = "CheckoutAuthoritySession(redacted)"
}

/** Drafts are generated before persistence so callers retain the one-time credential on uncertain writes. */
class CheckoutAuthorityBootstrapDraft internal constructor(
    val managerActorId: String,
    val recoveryCode: String,
) {
    override fun toString(): String = "CheckoutAuthorityBootstrapDraft(managerActorId=redacted, recoveryCode=redacted)"
}

class CheckoutAuthorityRecoveryDraft internal constructor(val recoveryCode: String) {
    override fun toString(): String = "CheckoutAuthorityRecoveryDraft(recoveryCode=redacted)"
}

class CheckoutAuthorityBootstrap internal constructor(
    val managerActorId: String,
    val recoveryCode: String,
) {
    override fun toString(): String = "CheckoutAuthorityBootstrap(managerActorId=redacted, recoveryCode=redacted)"
}

class CheckoutAuthorityRecoveryResult internal constructor(
    val managerActorId: String,
    val recoveryCode: String,
    val policyGeneration: Long,
) {
    override fun toString(): String = "CheckoutAuthorityRecoveryResult(managerActorId=redacted, recoveryCode=redacted)"
}

class CheckoutAuthorityDeniedException : IllegalStateException("Checkout authority denied")
class CheckoutAuthorityUnavailableException : IllegalStateException("Checkout authority unavailable")

/**
 * Local staff identity and authorization policy for the experimental checkout.
 *
 * Storage must provide serialized compare-and-swap. Android production storage must additionally
 * provide Keystore-backed authenticated sealing. Authentication state is intentionally separate
 * from business ledgers, so resetting staff access never rewrites sale/cash data.
 */
class CheckoutAuthority(
    private val storage: CheckoutAuthorityStorage,
    private val clock: CheckoutAuthorityClock,
    private val random: CheckoutAuthorityRandom = SecureCheckoutAuthorityRandom,
    private val defaultIterations: Int = PBKDF2_ITERATIONS,
    private val sessionLifetimeMillis: Long = DEFAULT_SESSION_LIFETIME_MILLIS,
) {
    private val monitor = Any()
    private var poisoned = false
    private var persistedBytes: ByteArray? = null
    private var state: AuthorityState? = loadInitialState()
    private val sessions = IdentityHashMap<CheckoutAuthoritySession, SessionRecord>()

    init {
        require(defaultIterations in MIN_ITERATIONS..MAX_ITERATIONS) { "Invalid checkout authority configuration" }
        require(sessionLifetimeMillis in 1_000L..MAX_SESSION_LIFETIME_MILLIS) { "Invalid checkout authority configuration" }
    }

    /** Null only on a genuinely unconfigured store; corrupt or stale state fails unavailable. */
    fun loginProfile(): CheckoutAuthorityLoginProfile? = synchronized(monitor) {
        ensureAvailable()
        ensureCurrentRevision()
        state?.let { current ->
            CheckoutAuthorityLoginProfile(
                current.scope,
                current.actors.filter { it.enabled }
                    .map { CheckoutStaffSummary(it.actorId, it.displayName, it.role, true) },
            )
        }
    }

    /** Generate and retain this draft before attempting first-install persistence. */
    fun prepareBootstrap(): CheckoutAuthorityBootstrapDraft = synchronized(monitor) {
        ensureAvailable()
        CheckoutAuthorityBootstrapDraft(newId(), newRecoveryCredential().code)
    }

    /** Fresh-install bootstrap is explicit and does not create a signed-in session. */
    fun bootstrapManager(
        draft: CheckoutAuthorityBootstrapDraft,
        scope: CheckoutAuthorityScope,
        displayName: String,
        pin: CharArray,
    ): CheckoutAuthorityBootstrap {
        try {
            validateDisplayName(displayName)
            validatePin(pin)
            synchronized(monitor) {
                ensureAvailable()
                if (state != null || persistedBytes != null) throw CheckoutAuthorityDeniedException()
                validateActorId(draft.managerActorId)
                val actorId = draft.managerActorId
                val actorSalt = randomBytes(SALT_BYTES)
                val actorVerifier = derive(pin, actorSalt, defaultIterations)
                val recovery = recoveryCredential(draft.recoveryCode.toCharArray(), defaultIterations)
                val manager = ActorRecord(
                    actorId, displayName.trim(), CheckoutAuthorityRole.MANAGER, true,
                    actorSalt, actorVerifier, 0, 0, "",
                )
                val next = AuthorityState(
                    scope, 1, defaultIterations, actorId, listOf(manager), recovery.salt, recovery.verifier,
                    0, 0, "",
                )
                persist(next)
                return CheckoutAuthorityBootstrap(actorId, draft.recoveryCode)
            }
        } finally {
            pin.fill('\u0000')
        }
    }

    /**
     * Verifies a PIN after persisting the next failure/hold state. Any write error poisons this
     * instance and returns no session. PIN contents are cleared before returning.
     */
    fun authenticate(
        actorId: String,
        pin: CharArray,
        requestedScope: CheckoutAuthorityScope,
    ): CheckoutAuthoritySession {
        try {
            validateActorId(actorId)
            synchronized(monitor) {
                ensureAvailable()
                val current = requireState()
                requireScope(current.scope, requestedScope)
                val actor = current.actors.firstOrNull { it.actorId == actorId }
                if (actor == null || !actor.enabled) {
                    val safePin = safePinForHash(pin)
                    val dummy = try { derive(safePin, DUMMY_SALT, current.iterations) }
                    finally { if (safePin !== pin) safePin.fill('\u0000') }
                    MessageDigest.isEqual(dummy, DUMMY_VERIFIER)
                    throw CheckoutAuthorityDeniedException()
                }
                val now = checkedElapsedTime()
                val boot = checkedBootIdentity()
                if (actor.holdUntilElapsed > now && actor.holdBootIdentity == boot) {
                    throw CheckoutAuthorityDeniedException()
                }
                if (actor.holdUntilElapsed > 0 && actor.holdBootIdentity != boot) {
                    val nextHold = safeAdd(now, holdDuration(actor.failedAttempts.coerceAtLeast(MAX_FAILURES)))
                    persist(current.replace(actor.copy(holdUntilElapsed = nextHold, holdBootIdentity = boot)))
                    throw CheckoutAuthorityDeniedException()
                }

                val priorFailures = if (actor.holdUntilElapsed > 0) 0 else actor.failedAttempts
                val failures = if (priorFailures == Int.MAX_VALUE) Int.MAX_VALUE else priorFailures + 1
                val holdUntil = if (failures >= MAX_FAILURES) safeAdd(now, holdDuration(failures)) else 0
                val attempted = actor.copy(
                    failedAttempts = failures,
                    holdUntilElapsed = holdUntil,
                    holdBootIdentity = if (holdUntil > 0) boot else "",
                )
                // Persist before computing/comparing the submitted PIN, including a correct attempt.
                val committed = current.replace(attempted)
                persist(committed)

                val validSyntax = isValidPin(pin)
                val safePin = safePinForHash(pin)
                val candidate = try { derive(safePin, actor.salt, committed.iterations) }
                finally { if (safePin !== pin) safePin.fill('\u0000') }
                val matched = MessageDigest.isEqual(candidate, actor.verifier) && validSyntax
                candidate.fill(0)
                if (!matched) throw CheckoutAuthorityDeniedException()

                val reset = committed.replace(attempted.copy(
                    failedAttempts = 0, holdUntilElapsed = 0, holdBootIdentity = "",
                ))
                persist(reset)
                val expiresAt = safeAdd(now, sessionLifetimeMillis)
                return CheckoutAuthoritySession().also { session ->
                    sessions[session] = SessionRecord(actor.actorId, reset.policyGeneration, reset.scope, expiresAt)
                }
            }
        } finally {
            pin.fill('\u0000')
        }
    }

    /** Resolve the current principal and role for each command; never trust a caller-supplied role. */
    fun authorize(
        session: CheckoutAuthoritySession,
        capability: CheckoutAuthorityCapability,
        requestedScope: CheckoutAuthorityScope,
    ): ActorAttribution = synchronized(monitor) {
        ensureAvailable()
        ensureCurrentRevision()
        val current = requireState()
        requireScope(current.scope, requestedScope)
        val record = sessions[session] ?: throw CheckoutAuthorityDeniedException()
        val now = checkedElapsedTime()
        if (now >= record.expiresAtElapsed || record.policyGeneration != current.policyGeneration ||
            record.scope != current.scope) {
            sessions.remove(session)
            throw CheckoutAuthorityDeniedException()
        }
        val actor = current.actors.firstOrNull { it.actorId == record.actorId && it.enabled }
            ?: throw CheckoutAuthorityDeniedException()
        if (!grants(actor.role).contains(capability)) throw CheckoutAuthorityDeniedException()
        ActorAttribution.create(
            actor.actorId, actor.displayName, actor.role, capability.id,
            current.policyGeneration, current.scope, checkedWallTime(),
        )
    }

    /** String entry point is intentionally fail-closed for unknown or future capability IDs. */
    fun authorize(
        session: CheckoutAuthoritySession,
        capabilityId: String,
        requestedScope: CheckoutAuthorityScope,
    ): ActorAttribution {
        val capability = CheckoutAuthorityCapability.fromId(capabilityId) ?: throw CheckoutAuthorityDeniedException()
        return authorize(session, capability, requestedScope)
    }

    /** Manager-only staff enrollment; a policy change invalidates all previously issued sessions. */
    fun createStaff(
        session: CheckoutAuthoritySession,
        requestedScope: CheckoutAuthorityScope,
        displayName: String,
        role: CheckoutAuthorityRole,
        pin: CharArray,
    ): CheckoutStaffSummary = try {
        createStaffInternal(session, requestedScope, displayName, role, pin, prepareStaffActorId(), null)
    } catch (error: Exception) {
        pin.fill('\u0000')
        throw error
    }

    /** Stable ID is generated before an audit prepare row so uncertain creation can be inspected. */
    internal fun prepareStaffActorId(): String = synchronized(monitor) {
        ensureAvailable()
        ensureCurrentRevision()
        val current = requireState()
        var actorId: String
        do { actorId = newId() } while (current.actors.any { it.actorId == actorId })
        actorId
    }

    internal fun createStaffForOperation(
        session: CheckoutAuthoritySession, requestedScope: CheckoutAuthorityScope, displayName: String,
        role: CheckoutAuthorityRole, pin: CharArray, actorId: String, operationId: String, requestDigest: ByteArray,
    ): CheckoutStaffSummary = createStaffInternal(session, requestedScope, displayName, role, pin, actorId,
        staffReceipt(operationId, CheckoutAuthorityStaffOperationKind.CREATE, actorId, 0, 0, requestDigest))

    private fun createStaffInternal(
        session: CheckoutAuthoritySession,
        requestedScope: CheckoutAuthorityScope,
        displayName: String,
        role: CheckoutAuthorityRole,
        pin: CharArray,
        actorId: String,
        receiptTemplate: CheckoutAuthorityStaffOperationReceipt?,
    ): CheckoutStaffSummary {
        try {
            validateDisplayName(displayName)
            validatePin(pin)
            validateActorId(actorId)
            synchronized(monitor) {
                authorize(session, CheckoutAuthorityCapability.STAFF_MANAGE, requestedScope)
                val current = requireState()
                if (pinMatchesEnabledActor(pin, current, excludeActorId = null)) throw CheckoutAuthorityDeniedException()
                if (current.actors.any { it.actorId == actorId }) throw CheckoutAuthorityDeniedException()
                val salt = randomBytes(SALT_BYTES)
                val verifier = derive(pin, salt, current.iterations)
                val actor = ActorRecord(actorId, displayName.trim(), role, true, salt, verifier, 0, 0, "")
                val nextGeneration = incrementGeneration(current.policyGeneration)
                val next = current.copy(
                    policyGeneration = nextGeneration,
                    actors = current.actors + actor,
                    staffReceipts = receiptTemplate?.let { addStaffReceipt(current.staffReceipts, it, current.policyGeneration, nextGeneration) } ?: current.staffReceipts,
                )
                persist(next)
                sessions.clear()
                return CheckoutStaffSummary(actor.actorId, actor.displayName, actor.role, actor.enabled)
            }
        } finally {
            pin.fill('\u0000')
        }
    }

    fun listStaff(
        session: CheckoutAuthoritySession,
        requestedScope: CheckoutAuthorityScope,
    ): List<CheckoutStaffSummary> = synchronized(monitor) {
        authorize(session, CheckoutAuthorityCapability.STAFF_MANAGE, requestedScope)
        requireState().actors.map { CheckoutStaffSummary(it.actorId, it.displayName, it.role, it.enabled) }
    }

    /** Read the exact non-secret staff projection and generation under the authority lock. */
    internal fun staffRecoveryState(
        session: CheckoutAuthoritySession,
        requestedScope: CheckoutAuthorityScope,
    ): CheckoutAuthorityStaffRecoveryState = synchronized(monitor) {
        authorize(session, CheckoutAuthorityCapability.STAFF_MANAGE, requestedScope)
        val current = requireState()
        CheckoutAuthorityStaffRecoveryState(
            current.policyGeneration,
            current.actors.map { CheckoutStaffSummary(it.actorId, it.displayName, it.role, it.enabled) },
            current.staffReceipts.map { it.copy(requestDigest = it.requestDigest.copyOf()) },
            MessageDigest.getInstance("SHA-256").digest(persistedBytes ?: error("authority state bytes unavailable")),
        )
    }

    internal fun staffOperationReceipts(
        session: CheckoutAuthoritySession,
        requestedScope: CheckoutAuthorityScope,
    ): List<CheckoutAuthorityStaffOperationReceipt> = staffRecoveryState(session, requestedScope).receipts

    /** Renaming changes future event snapshots, never the stable actor ID or prior attribution. */
    fun renameStaff(
        session: CheckoutAuthoritySession,
        requestedScope: CheckoutAuthorityScope,
        actorId: String,
        displayName: String,
    ): CheckoutStaffSummary = renameStaffInternal(session, requestedScope, actorId, displayName, null)

    internal fun renameStaffForOperation(
        session: CheckoutAuthoritySession, requestedScope: CheckoutAuthorityScope, actorId: String,
        displayName: String, operationId: String, requestDigest: ByteArray,
    ): CheckoutStaffSummary = renameStaffInternal(session, requestedScope, actorId, displayName,
        staffReceipt(operationId, CheckoutAuthorityStaffOperationKind.RENAME, actorId, 0, 0, requestDigest))

    private fun renameStaffInternal(
        session: CheckoutAuthoritySession, requestedScope: CheckoutAuthorityScope, actorId: String,
        displayName: String, receipt: CheckoutAuthorityStaffOperationReceipt?,
    ): CheckoutStaffSummary = synchronized(monitor) {
        validateActorId(actorId)
        validateDisplayName(displayName)
        authorize(session, CheckoutAuthorityCapability.STAFF_MANAGE, requestedScope)
        val current = requireState()
        val actor = current.actors.firstOrNull { it.actorId == actorId } ?: throw CheckoutAuthorityDeniedException()
        val changed = actor.copy(displayName = displayName.trim())
        val nextGeneration = incrementGeneration(current.policyGeneration)
        persist(current.replace(changed).copy(policyGeneration = nextGeneration,
            staffReceipts = receipt?.let { addStaffReceipt(current.staffReceipts, it, current.policyGeneration, nextGeneration) } ?: current.staffReceipts))
        sessions.clear()
        CheckoutStaffSummary(changed.actorId, changed.displayName, changed.role, changed.enabled)
    }

    fun changeRole(
        session: CheckoutAuthoritySession,
        requestedScope: CheckoutAuthorityScope,
        actorId: String,
        role: CheckoutAuthorityRole,
    ): CheckoutStaffSummary = changeRoleInternal(session, requestedScope, actorId, role, null)

    internal fun changeRoleForOperation(
        session: CheckoutAuthoritySession, requestedScope: CheckoutAuthorityScope, actorId: String,
        role: CheckoutAuthorityRole, operationId: String, requestDigest: ByteArray,
    ): CheckoutStaffSummary = changeRoleInternal(session, requestedScope, actorId, role,
        staffReceipt(operationId, CheckoutAuthorityStaffOperationKind.ROLE, actorId, 0, 0, requestDigest))

    private fun changeRoleInternal(
        session: CheckoutAuthoritySession, requestedScope: CheckoutAuthorityScope, actorId: String,
        role: CheckoutAuthorityRole, receipt: CheckoutAuthorityStaffOperationReceipt?,
    ): CheckoutStaffSummary = synchronized(monitor) {
        validateActorId(actorId)
        authorize(session, CheckoutAuthorityCapability.STAFF_MANAGE, requestedScope)
        val current = requireState()
        val actor = current.actors.firstOrNull { it.actorId == actorId } ?: throw CheckoutAuthorityDeniedException()
        if (actorId == current.recoveryManagerActorId && role != CheckoutAuthorityRole.MANAGER) {
            throw CheckoutAuthorityDeniedException()
        }
        if (actor.role == CheckoutAuthorityRole.MANAGER && role != CheckoutAuthorityRole.MANAGER &&
            current.actors.none { it.actorId != actorId && it.enabled && it.role == CheckoutAuthorityRole.MANAGER }) {
            throw CheckoutAuthorityDeniedException()
        }
        val changed = actor.copy(role = role)
        val nextGeneration = incrementGeneration(current.policyGeneration)
        persist(current.replace(changed).copy(policyGeneration = nextGeneration,
            staffReceipts = receipt?.let { addStaffReceipt(current.staffReceipts, it, current.policyGeneration, nextGeneration) } ?: current.staffReceipts))
        sessions.clear()
        CheckoutStaffSummary(changed.actorId, changed.displayName, changed.role, changed.enabled)
    }

    fun changePin(
        session: CheckoutAuthoritySession,
        requestedScope: CheckoutAuthorityScope,
        actorId: String,
        newPin: CharArray,
    ) = changePinInternal(session, requestedScope, actorId, newPin, null)

    internal fun changePinForOperation(
        session: CheckoutAuthoritySession, requestedScope: CheckoutAuthorityScope, actorId: String,
        newPin: CharArray, operationId: String, requestDigest: ByteArray,
    ) = changePinInternal(session, requestedScope, actorId, newPin,
        staffReceipt(operationId, CheckoutAuthorityStaffOperationKind.PIN, actorId, 0, 0, requestDigest))

    private fun changePinInternal(
        session: CheckoutAuthoritySession, requestedScope: CheckoutAuthorityScope, actorId: String,
        newPin: CharArray, receipt: CheckoutAuthorityStaffOperationReceipt?,
    ) {
        try {
            validateActorId(actorId)
            validatePin(newPin)
            synchronized(monitor) {
                authorize(session, CheckoutAuthorityCapability.STAFF_MANAGE, requestedScope)
                val current = requireState()
                val actor = current.actors.firstOrNull { it.actorId == actorId } ?: throw CheckoutAuthorityDeniedException()
                if (pinMatchesEnabledActor(newPin, current, excludeActorId = actorId)) throw CheckoutAuthorityDeniedException()
                val salt = randomBytes(SALT_BYTES)
                val changed = actor.copy(
                    salt = salt,
                    verifier = derive(newPin, salt, current.iterations),
                    failedAttempts = 0, holdUntilElapsed = 0, holdBootIdentity = "",
                )
                val nextGeneration = incrementGeneration(current.policyGeneration)
                persist(current.replace(changed).copy(policyGeneration = nextGeneration,
                    staffReceipts = receipt?.let { addStaffReceipt(current.staffReceipts, it, current.policyGeneration, nextGeneration) } ?: current.staffReceipts))
                sessions.clear()
            }
        } finally {
            newPin.fill('\u0000')
        }
    }

    fun disableStaff(
        session: CheckoutAuthoritySession,
        requestedScope: CheckoutAuthorityScope,
        actorId: String,
    ): CheckoutStaffSummary = disableStaffInternal(session, requestedScope, actorId, null)

    internal fun disableStaffForOperation(
        session: CheckoutAuthoritySession, requestedScope: CheckoutAuthorityScope, actorId: String,
        operationId: String, requestDigest: ByteArray,
    ): CheckoutStaffSummary = disableStaffInternal(session, requestedScope, actorId,
        staffReceipt(operationId, CheckoutAuthorityStaffOperationKind.DISABLE, actorId, 0, 0, requestDigest))

    private fun disableStaffInternal(
        session: CheckoutAuthoritySession, requestedScope: CheckoutAuthorityScope, actorId: String,
        receipt: CheckoutAuthorityStaffOperationReceipt?,
    ): CheckoutStaffSummary = synchronized(monitor) {
        validateActorId(actorId)
        authorize(session, CheckoutAuthorityCapability.STAFF_MANAGE, requestedScope)
        val current = requireState()
        val actor = current.actors.firstOrNull { it.actorId == actorId } ?: throw CheckoutAuthorityDeniedException()
        if (actorId == current.recoveryManagerActorId && actor.enabled) throw CheckoutAuthorityDeniedException()
        if (actor.enabled && actor.role == CheckoutAuthorityRole.MANAGER &&
            current.actors.none { it.actorId != actorId && it.enabled && it.role == CheckoutAuthorityRole.MANAGER }) {
            throw CheckoutAuthorityDeniedException()
        }
        val changed = actor.copy(enabled = false)
        val nextGeneration = incrementGeneration(current.policyGeneration)
        persist(current.replace(changed).copy(policyGeneration = nextGeneration,
            staffReceipts = receipt?.let { addStaffReceipt(current.staffReceipts, it, current.policyGeneration, nextGeneration) } ?: current.staffReceipts))
        sessions.clear()
        CheckoutStaffSummary(changed.actorId, changed.displayName, changed.role, changed.enabled)
    }

    /** Prepare a replacement recovery code before attempting the durable rotation. */
    fun prepareRecovery(): CheckoutAuthorityRecoveryDraft = synchronized(monitor) {
        ensureAvailable()
        CheckoutAuthorityRecoveryDraft(newRecoveryCredential().code)
    }

    /** One-time code reset changes only this authority store and rotates the recovery verifier. */
    fun recoverManager(
        requestedScope: CheckoutAuthorityScope,
        recoveryCode: CharArray,
        newPin: CharArray,
        nextRecovery: CheckoutAuthorityRecoveryDraft,
    ): CheckoutAuthorityRecoveryResult {
        try {
            validatePin(newPin)
            if (sameSecret(nextRecovery.recoveryCode, recoveryCode)) {
                throw CheckoutAuthorityDeniedException()
            }
            synchronized(monitor) {
                ensureAvailable()
                val current = requireState()
                requireScope(current.scope, requestedScope)
                val target = current.actors.firstOrNull {
                    it.actorId == current.recoveryManagerActorId && it.role == CheckoutAuthorityRole.MANAGER
                } ?: throw CheckoutAuthorityDeniedException()
                val now = checkedElapsedTime()
                val boot = checkedBootIdentity()
                if (current.recoveryHoldUntilElapsed > now && current.recoveryHoldBootIdentity == boot) {
                    throw CheckoutAuthorityDeniedException()
                }
                if (current.recoveryHoldUntilElapsed > 0 && current.recoveryHoldBootIdentity != boot) {
                    val nextHold = safeAdd(now, holdDuration(current.recoveryFailedAttempts.coerceAtLeast(MAX_FAILURES)))
                    persist(current.copy(recoveryHoldUntilElapsed = nextHold, recoveryHoldBootIdentity = boot))
                    throw CheckoutAuthorityDeniedException()
                }
                val prior = if (current.recoveryHoldUntilElapsed > 0) 0 else current.recoveryFailedAttempts
                val failures = if (prior == Int.MAX_VALUE) Int.MAX_VALUE else prior + 1
                val hold = if (failures >= MAX_FAILURES) safeAdd(now, holdDuration(failures)) else 0
                // Recovery attempts are rate-limited durably before hashing/comparison too.
                val attempted = current.copy(
                    recoveryFailedAttempts = failures,
                    recoveryHoldUntilElapsed = hold,
                    recoveryHoldBootIdentity = if (hold > 0) boot else "",
                )
                persist(attempted)
                val safeCode = safeRecoveryCodeForHash(recoveryCode)
                val candidate = try { derive(safeCode, attempted.recoverySalt, attempted.iterations) }
                finally { if (safeCode !== recoveryCode) safeCode.fill('\u0000') }
                val matches = MessageDigest.isEqual(candidate, attempted.recoveryVerifier) && isValidRecoveryCode(recoveryCode)
                candidate.fill(0)
                if (!matches) throw CheckoutAuthorityDeniedException()

                if (pinMatchesEnabledActor(newPin, attempted, excludeActorId = target.actorId)) {
                    throw CheckoutAuthorityDeniedException()
                }
                val recovery = recoveryCredential(nextRecovery.recoveryCode.toCharArray(), attempted.iterations)
                val managerSalt = randomBytes(SALT_BYTES)
                val managerVerifier = derive(newPin, managerSalt, attempted.iterations)
                val changed = target.copy(
                    enabled = true, salt = managerSalt, verifier = managerVerifier,
                    failedAttempts = 0, holdUntilElapsed = 0, holdBootIdentity = "",
                )
                val next = attempted.replace(changed).copy(
                    policyGeneration = incrementGeneration(attempted.policyGeneration),
                    recoverySalt = recovery.salt,
                    recoveryVerifier = recovery.verifier,
                    recoveryFailedAttempts = 0,
                    recoveryHoldUntilElapsed = 0,
                    recoveryHoldBootIdentity = "",
                )
                persist(next)
                sessions.clear()
                return CheckoutAuthorityRecoveryResult(target.actorId, nextRecovery.recoveryCode, next.policyGeneration)
            }
        } finally {
            recoveryCode.fill('\u0000')
            newPin.fill('\u0000')
        }
    }

    /** Process-local lock; every previously issued token is revoked. */
    fun lock() = synchronized(monitor) { sessions.clear() }

    private fun loadInitialState(): AuthorityState? {
        return try {
            val bytes = storage.read() ?: return null
            persistedBytes = bytes.copyOf()
            decode(bytes)
        } catch (_: Exception) {
            throw CheckoutAuthorityUnavailableException()
        }
    }

    private fun persist(next: AuthorityState) {
        ensureAvailable()
        val bytes = try { encode(next) } catch (_: Exception) {
            poisoned = true
            throw CheckoutAuthorityUnavailableException()
        }
        try {
            if (!storage.compareAndSwap(persistedBytes?.copyOf(), bytes.copyOf())) {
                poisoned = true
                throw CheckoutAuthorityUnavailableException()
            }
            state = next
            persistedBytes = bytes.copyOf()
        } catch (_: CheckoutAuthorityUnavailableException) {
            poisoned = true
            throw CheckoutAuthorityUnavailableException()
        } catch (_: Exception) {
            // A conflict or thrown CAS may have an uncertain commit. Never continue on stale bytes.
            poisoned = true
            throw CheckoutAuthorityUnavailableException()
        }
    }

    private fun ensureAvailable() {
        if (poisoned) throw CheckoutAuthorityUnavailableException()
    }

    /** Reject cached policy/session state as soon as another instance commits any authority change. */
    private fun ensureCurrentRevision() {
        val expected = persistedBytes
        val actual = try { storage.read() } catch (_: Exception) {
            poisoned = true
            sessions.clear()
            throw CheckoutAuthorityUnavailableException()
        }
        val same = when {
            expected == null || actual == null -> expected == null && actual == null
            else -> MessageDigest.isEqual(expected, actual)
        }
        if (!same) {
            poisoned = true
            sessions.clear()
            throw CheckoutAuthorityUnavailableException()
        }
    }

    private fun requireState(): AuthorityState = state ?: throw CheckoutAuthorityUnavailableException()

    private fun requireScope(stored: CheckoutAuthorityScope, requested: CheckoutAuthorityScope) {
        if (stored != requested) throw CheckoutAuthorityDeniedException()
    }

    private fun checkedElapsedTime(): Long = clock.elapsedRealtimeMillis().also {
        if (it < 0) throw CheckoutAuthorityUnavailableException()
    }

    private fun checkedWallTime(): Long = clock.wallTimeMillis().also {
        if (it < 0) throw CheckoutAuthorityUnavailableException()
    }

    private fun checkedBootIdentity(): String = clock.bootIdentity().also {
        if (!validScopeId(it)) throw CheckoutAuthorityUnavailableException()
    }

    private fun newId(): String = "actor-" + hex(randomBytes(ID_BYTES))

    private fun newRecoveryCredential(iterations: Int = defaultIterations): RecoveryCredential {
        val code = hex(randomBytes(RECOVERY_BYTES))
        return recoveryCredential(code.toCharArray(), iterations, code)
    }

    private fun recoveryCredential(codeChars: CharArray, iterations: Int, retainedCode: String = String(codeChars)): RecoveryCredential {
        try {
            if (!isValidRecoveryCode(codeChars)) throw CheckoutAuthorityDeniedException()
            val salt = randomBytes(SALT_BYTES)
            return RecoveryCredential(retainedCode, salt, derive(codeChars, salt, iterations))
        } finally {
            codeChars.fill('\u0000')
        }
    }

    /** Checks every enabled verifier, without early exit; work is capped by MAX_ACTORS. */
    private fun pinMatchesEnabledActor(pin: CharArray, state: AuthorityState, excludeActorId: String?): Boolean {
        var matched = 0
        for (actor in state.actors) {
            if (actor.enabled && actor.actorId != excludeActorId) {
                val candidate = derive(pin, actor.salt, state.iterations)
                val same = MessageDigest.isEqual(candidate, actor.verifier)
                candidate.fill(0)
                matched = matched or if (same) 1 else 0
            }
        }
        return matched != 0
    }

    private fun randomBytes(count: Int): ByteArray {
        val bytes = try { random.bytes(count) } catch (_: Exception) {
            throw CheckoutAuthorityUnavailableException()
        }
        if (bytes.size != count) throw CheckoutAuthorityUnavailableException()
        return bytes.copyOf()
    }

    private fun derive(secret: CharArray, salt: ByteArray, iterations: Int): ByteArray {
        val spec = PBEKeySpec(secret, salt, iterations, VERIFIER_BITS)
        return try {
            SecretKeyFactory.getInstance("PBKDF2WithHmacSHA256").generateSecret(spec).encoded
        } catch (_: Exception) {
            throw CheckoutAuthorityUnavailableException()
        } finally {
            spec.clearPassword()
        }
    }

    private fun validatePin(pin: CharArray) {
        if (!isValidPin(pin)) throw CheckoutAuthorityDeniedException()
    }

    private fun isValidPin(pin: CharArray): Boolean = pin.size in MIN_PIN_LENGTH..MAX_PIN_LENGTH &&
        pin.all { it in '0'..'9' }

    private fun isValidRecoveryCode(code: CharArray): Boolean = code.size == RECOVERY_BYTES * 2 &&
        code.all { it in '0'..'9' || it in 'a'..'f' }

    private fun sameSecret(expected: String, supplied: CharArray): Boolean {
        var difference = expected.length xor supplied.size
        for (index in expected.indices) {
            val suppliedCode = if (index < supplied.size) supplied[index].code else 0
            difference = difference or (expected[index].code xor suppliedCode)
        }
        return difference == 0
    }

    private fun safePinForHash(pin: CharArray): CharArray =
        if (pin.size in MIN_PIN_LENGTH..MAX_PIN_LENGTH) pin else DUMMY_PIN.copyOf()

    private fun safeRecoveryCodeForHash(code: CharArray): CharArray =
        if (code.size == RECOVERY_BYTES * 2) code else DUMMY_RECOVERY_CODE.copyOf()

    private fun validateDisplayName(value: String) {
        if (value.trim().isEmpty() || value.length > MAX_DISPLAY_NAME_CHARS ||
            value.any { it.isISOControl() } || value.toByteArray(Charsets.UTF_8).size > MAX_DISPLAY_NAME_BYTES) {
            throw CheckoutAuthorityDeniedException()
        }
    }

    private fun validateActorId(value: String) {
        if (value.length !in 8..80 || value.any { !(it in 'a'..'z' || it in '0'..'9' || it == '-') }) {
            throw CheckoutAuthorityDeniedException()
        }
    }

    private fun holdDuration(failures: Int): Long {
        val exponent = (failures - MAX_FAILURES).coerceIn(0, 8)
        val multiplier = 1L shl exponent
        return minOf(MAX_HOLD_MILLIS, BASE_HOLD_MILLIS * multiplier)
    }

    private fun safeAdd(left: Long, right: Long): Long = try { Math.addExact(left, right) }
    catch (_: ArithmeticException) { Long.MAX_VALUE }

    private fun grants(role: CheckoutAuthorityRole): Set<CheckoutAuthorityCapability> = when (role) {
        CheckoutAuthorityRole.CASHIER -> CASHIER_GRANTS
        CheckoutAuthorityRole.MANAGER -> MANAGER_GRANTS
    }

    private data class RecoveryCredential(val code: String, val salt: ByteArray, val verifier: ByteArray)
    private data class SessionRecord(
        val actorId: String,
        val policyGeneration: Long,
        val scope: CheckoutAuthorityScope,
        val expiresAtElapsed: Long,
    )
    private data class ActorRecord(
        val actorId: String,
        val displayName: String,
        val role: CheckoutAuthorityRole,
        val enabled: Boolean,
        val salt: ByteArray,
        val verifier: ByteArray,
        val failedAttempts: Int,
        val holdUntilElapsed: Long,
        val holdBootIdentity: String,
    )
    private data class AuthorityState(
        val scope: CheckoutAuthorityScope,
        val policyGeneration: Long,
        val iterations: Int,
        val recoveryManagerActorId: String,
        val actors: List<ActorRecord>,
        val recoverySalt: ByteArray,
        val recoveryVerifier: ByteArray,
        val recoveryFailedAttempts: Int,
        val recoveryHoldUntilElapsed: Long,
        val recoveryHoldBootIdentity: String,
        val staffReceipts: List<CheckoutAuthorityStaffOperationReceipt> = emptyList(),
    ) {
        fun replace(actor: ActorRecord): AuthorityState = copy(actors = actors.map { if (it.actorId == actor.actorId) actor else it })
    }

    private fun encode(state: AuthorityState): ByteArray {
        require(state.actors.size in 1..MAX_ACTORS && state.iterations in MIN_ITERATIONS..MAX_ITERATIONS)
        require(state.policyGeneration > 0 && state.recoverySalt.size == SALT_BYTES && state.recoveryVerifier.size == VERIFIER_BYTES)
        require(state.actors.map { it.actorId }.toSet().size == state.actors.size)
        require(state.staffReceipts.size <= MAX_STAFF_RECEIPTS)
        require(state.staffReceipts.map { it.operationId }.toSet().size == state.staffReceipts.size)
        state.staffReceipts.forEachIndexed { index, receipt ->
            validateStaffReceipt(receipt)
            if (receipt.resultGeneration > state.policyGeneration ||
                (index > 0 && state.staffReceipts[index - 1].resultGeneration >= receipt.resultGeneration)) {
                throw CheckoutAuthorityUnavailableException()
            }
        }
        validateActorId(state.recoveryManagerActorId)
        require(state.actors.any { it.actorId == state.recoveryManagerActorId && it.role == CheckoutAuthorityRole.MANAGER && it.enabled })
        val out = ByteArrayOutputStream()
        DataOutputStream(out).use { data ->
            data.writeInt(MAGIC)
            data.writeInt(VERSION)
            writeScope(data, state.scope)
            data.writeText(state.recoveryManagerActorId)
            data.writeLong(state.policyGeneration)
            data.writeInt(state.iterations)
            data.writeInt(state.actors.size)
            for (actor in state.actors) {
                validateActorId(actor.actorId)
                validateDisplayName(actor.displayName)
                require(actor.salt.size == SALT_BYTES && actor.verifier.size == VERIFIER_BYTES)
                require(actor.failedAttempts >= 0 && actor.holdUntilElapsed >= 0)
                require((actor.holdUntilElapsed == 0L) == actor.holdBootIdentity.isEmpty())
                require(actor.holdBootIdentity.isEmpty() || validScopeId(actor.holdBootIdentity))
                data.writeText(actor.actorId)
                data.writeText(actor.displayName)
                data.writeByte(actor.role.ordinal)
                data.writeByte(if (actor.enabled) 1 else 0)
                data.write(actor.salt)
                data.write(actor.verifier)
                data.writeInt(actor.failedAttempts)
                data.writeLong(actor.holdUntilElapsed)
                data.writeText(actor.holdBootIdentity)
            }
            data.write(state.recoverySalt)
            data.write(state.recoveryVerifier)
            data.writeInt(state.recoveryFailedAttempts)
            data.writeLong(state.recoveryHoldUntilElapsed)
            data.writeText(state.recoveryHoldBootIdentity)
            require(state.staffReceipts.size <= MAX_STAFF_RECEIPTS)
            data.writeInt(state.staffReceipts.size)
            state.staffReceipts.forEach { receipt ->
                validateStaffReceipt(receipt)
                data.writeText(receipt.operationId)
                data.writeByte(receipt.kind.ordinal)
                data.writeText(receipt.subjectActorId)
                data.writeLong(receipt.baseGeneration)
                data.writeLong(receipt.resultGeneration)
                data.write(receipt.requestDigest)
            }
        }
        val bytes = out.toByteArray()
        require(bytes.size <= MAX_FILE_BYTES)
        return bytes
    }

    private fun decode(bytes: ByteArray): AuthorityState {
        if (bytes.size !in 16..MAX_FILE_BYTES) throw CheckoutAuthorityUnavailableException()
        try {
            DataInputStream(ByteArrayInputStream(bytes)).use { data ->
                if (data.readInt() != MAGIC) throw CheckoutAuthorityUnavailableException()
                val version = data.readInt()
                if (version !in 1..VERSION) throw CheckoutAuthorityUnavailableException()
                val scope = readScope(data)
                val storedRecoveryManagerId = if (version >= 2) data.readText(MAX_ACTOR_ID_BYTES) else null
                val generation = data.readLong()
                val iterations = data.readInt()
                val actorCount = data.readInt()
                if (generation <= 0 || iterations !in MIN_ITERATIONS..MAX_ITERATIONS || actorCount !in 1..MAX_ACTORS) {
                    throw CheckoutAuthorityUnavailableException()
                }
                val actors = ArrayList<ActorRecord>(actorCount)
                val ids = HashSet<String>()
                repeat(actorCount) {
                    val actorId = data.readText(MAX_ACTOR_ID_BYTES)
                    validateActorId(actorId)
                    if (!ids.add(actorId)) throw CheckoutAuthorityUnavailableException()
                    val name = data.readText(MAX_DISPLAY_NAME_BYTES)
                    validateDisplayName(name)
                    val roleIndex = data.readUnsignedByte()
                    val role = CheckoutAuthorityRole.entries.getOrNull(roleIndex) ?: throw CheckoutAuthorityUnavailableException()
                    val enabled = readStrictBoolean(data)
                    val salt = ByteArray(SALT_BYTES).also(data::readFully)
                    val verifier = ByteArray(VERIFIER_BYTES).also(data::readFully)
                    val failures = data.readInt()
                    val holdUntil = data.readLong()
                    val holdBoot = data.readText(MAX_BOOT_ID_BYTES)
                    if (failures < 0 || holdUntil < 0 || (holdUntil == 0L) != holdBoot.isEmpty()) {
                        throw CheckoutAuthorityUnavailableException()
                    }
                    if (holdBoot.isNotEmpty() && !validScopeId(holdBoot)) throw CheckoutAuthorityUnavailableException()
                    actors.add(ActorRecord(actorId, name, role, enabled, salt, verifier, failures, holdUntil, holdBoot))
                }
                if (actors.none { it.role == CheckoutAuthorityRole.MANAGER && it.enabled }) throw CheckoutAuthorityUnavailableException()
                val recoveryManagerId = storedRecoveryManagerId ?: actors.first {
                    it.role == CheckoutAuthorityRole.MANAGER && it.enabled
                }.actorId
                validateActorId(recoveryManagerId)
                if (actors.none { it.actorId == recoveryManagerId && it.role == CheckoutAuthorityRole.MANAGER && it.enabled }) {
                    throw CheckoutAuthorityUnavailableException()
                }
                val recoverySalt = ByteArray(SALT_BYTES).also(data::readFully)
                val recoveryVerifier = ByteArray(VERIFIER_BYTES).also(data::readFully)
                val recoveryFailures = data.readInt()
                val recoveryHold = data.readLong()
                val recoveryBoot = data.readText(MAX_BOOT_ID_BYTES)
                if (recoveryFailures < 0 || recoveryHold < 0 || (recoveryHold == 0L) != recoveryBoot.isEmpty()) {
                    throw CheckoutAuthorityUnavailableException()
                }
                if (recoveryBoot.isNotEmpty() && !validScopeId(recoveryBoot)) throw CheckoutAuthorityUnavailableException()
                val receipts = ArrayList<CheckoutAuthorityStaffOperationReceipt>()
                if (version >= 3) {
                    val receiptCount = data.readInt()
                    if (receiptCount !in 0..MAX_STAFF_RECEIPTS) throw CheckoutAuthorityUnavailableException()
                    repeat(receiptCount) {
                        val op = data.readText(MAX_OPERATION_ID_BYTES)
                        val kind = CheckoutAuthorityStaffOperationKind.entries.getOrNull(data.readUnsignedByte())
                            ?: throw CheckoutAuthorityUnavailableException()
                        val subject = data.readText(MAX_ACTOR_ID_BYTES)
                        val base = data.readLong(); val result = data.readLong()
                        val requestDigest = if (version >= 4) ByteArray(32).also(data::readFully) else ByteArray(32)
                        val receipt = CheckoutAuthorityStaffOperationReceipt(op, kind, subject, base, result, requestDigest)
                        validateStaffReceipt(receipt)
                        if (receipts.any { it.operationId == op }) throw CheckoutAuthorityUnavailableException()
                        receipts.add(receipt)
                    }
                }
                if (receipts.any { it.resultGeneration > generation } ||
                    receipts.zipWithNext().any { (previous, next) -> previous.resultGeneration >= next.resultGeneration }) {
                    throw CheckoutAuthorityUnavailableException()
                }
                if (data.available() != 0) throw CheckoutAuthorityUnavailableException()
                return AuthorityState(scope, generation, iterations, recoveryManagerId, actors, recoverySalt, recoveryVerifier,
                    recoveryFailures, recoveryHold, recoveryBoot, receipts)
            }
        } catch (e: CheckoutAuthorityUnavailableException) {
            throw e
        } catch (_: Exception) {
            throw CheckoutAuthorityUnavailableException()
        }
    }

    private fun writeScope(data: DataOutputStream, scope: CheckoutAuthorityScope) {
        data.writeText(scope.merchantId); data.writeText(scope.storeId); data.writeText(scope.locationId)
        data.writeText(scope.registerId); data.writeText(scope.deviceId)
    }

    private fun readScope(data: DataInputStream): CheckoutAuthorityScope = CheckoutAuthorityScope(
        data.readText(MAX_SCOPE_ID_BYTES), data.readText(MAX_SCOPE_ID_BYTES), data.readText(MAX_SCOPE_ID_BYTES),
        data.readText(MAX_SCOPE_ID_BYTES), data.readText(MAX_SCOPE_ID_BYTES),
    )

    private fun readStrictBoolean(data: DataInputStream): Boolean = when (data.readUnsignedByte()) {
        0 -> false
        1 -> true
        else -> throw CheckoutAuthorityUnavailableException()
    }

    private fun incrementGeneration(value: Long): Long = try { Math.addExact(value, 1) }
    catch (_: ArithmeticException) { throw CheckoutAuthorityUnavailableException() }

    private fun staffReceipt(
        operationId: String, kind: CheckoutAuthorityStaffOperationKind, subject: String,
        baseGeneration: Long, resultGeneration: Long, requestDigest: ByteArray,
    ): CheckoutAuthorityStaffOperationReceipt {
        if (requestDigest.size != 32) throw CheckoutAuthorityUnavailableException()
        return CheckoutAuthorityStaffOperationReceipt(operationId, kind, subject, baseGeneration, resultGeneration, requestDigest.copyOf())
    }

    private fun addStaffReceipt(
        existing: List<CheckoutAuthorityStaffOperationReceipt>,
        template: CheckoutAuthorityStaffOperationReceipt,
        baseGeneration: Long,
        resultGeneration: Long,
    ): List<CheckoutAuthorityStaffOperationReceipt> {
        if (existing.size >= MAX_STAFF_RECEIPTS || existing.any { it.operationId == template.operationId }) {
            throw CheckoutAuthorityUnavailableException()
        }
        val receipt = template.copy(baseGeneration = baseGeneration, resultGeneration = resultGeneration)
        validateStaffReceipt(receipt)
        return existing + receipt
    }

    private fun validateStaffReceipt(receipt: CheckoutAuthorityStaffOperationReceipt) {
        if (receipt.operationId.length !in 8..MAX_OPERATION_ID_BYTES ||
            receipt.operationId.any { !(it in 'a'..'z' || it in 'A'..'Z' || it in '0'..'9' || it == '-' || it == '_') }) {
            throw CheckoutAuthorityUnavailableException()
        }
        validateActorId(receipt.subjectActorId)
        if (receipt.baseGeneration <= 0 || receipt.resultGeneration != incrementGeneration(receipt.baseGeneration) ||
            receipt.requestDigest.size != 32) {
            throw CheckoutAuthorityUnavailableException()
        }
    }

    override fun toString(): String = "CheckoutAuthority(redacted)"

    companion object {
        private const val MAGIC = 0x43415554 // CAUT
        private const val VERSION = 4
        private const val MAX_FILE_BYTES = 1024 * 1024
        private const val MAX_ACTORS = 500
        private const val SALT_BYTES = 16
        private const val VERIFIER_BYTES = 32
        private const val VERIFIER_BITS = VERIFIER_BYTES * 8
        private const val ID_BYTES = 16
        private const val RECOVERY_BYTES = 32
        private const val MIN_PIN_LENGTH = 6
        private const val MAX_PIN_LENGTH = 12
        private const val MAX_SCOPE_ID_BYTES = 64
        private const val MAX_ACTOR_ID_BYTES = 80
        private const val MAX_OPERATION_ID_BYTES = 128
        private const val MAX_STAFF_RECEIPTS = 4096
        private const val MAX_DISPLAY_NAME_CHARS = 80
        private const val MAX_DISPLAY_NAME_BYTES = 256
        private const val MAX_BOOT_ID_BYTES = 128
        private const val PBKDF2_ITERATIONS = 600_000
        private const val MIN_ITERATIONS = 1_000
        private const val MAX_ITERATIONS = 10_000_000
        private const val DEFAULT_SESSION_LIFETIME_MILLIS = 5 * 60 * 1000L
        private const val MAX_SESSION_LIFETIME_MILLIS = 60 * 60 * 1000L
        private const val MAX_FAILURES = 5
        private const val BASE_HOLD_MILLIS = 30_000L
        private const val MAX_HOLD_MILLIS = 15 * 60 * 1000L
        private val DUMMY_SALT = ByteArray(SALT_BYTES) { (it + 17).toByte() }
        private val DUMMY_VERIFIER = ByteArray(VERIFIER_BYTES) { (it + 31).toByte() }
        private val DUMMY_PIN = charArrayOf('0', '0', '0', '0', '0', '0')
        private val DUMMY_RECOVERY_CODE = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".toCharArray()
        private val CASHIER_GRANTS = setOf(
            CheckoutAuthorityCapability.SALE_CREATE, CheckoutAuthorityCapability.SALE_EDIT,
            CheckoutAuthorityCapability.TENDER_BEGIN, CheckoutAuthorityCapability.RECEIPT_READ,
            CheckoutAuthorityCapability.SALES_READ, CheckoutAuthorityCapability.CASH_SESSION_OPEN,
            CheckoutAuthorityCapability.CASH_SESSION_MOVEMENT_RECORD, CheckoutAuthorityCapability.CASH_SESSION_SALE_LINK,
            CheckoutAuthorityCapability.CASH_SESSION_CLOSE, CheckoutAuthorityCapability.REFUND_REQUEST,
        )
        private val MANAGER_GRANTS = CASHIER_GRANTS + setOf(
            CheckoutAuthorityCapability.STAFF_MANAGE, CheckoutAuthorityCapability.REPORT_READ,
            CheckoutAuthorityCapability.CATALOG_MANAGE, CheckoutAuthorityCapability.INVENTORY_MANAGE,
            CheckoutAuthorityCapability.REPORT_EXPORT, CheckoutAuthorityCapability.WALLET_CONFIGURE,
        )

        private fun hex(bytes: ByteArray): String {
            val chars = "0123456789abcdef"
            val out = CharArray(bytes.size * 2)
            for (i in bytes.indices) {
                val b = bytes[i].toInt() and 0xff
                out[i * 2] = chars[b ushr 4]
                out[i * 2 + 1] = chars[b and 15]
            }
            return String(out)
        }

        private fun DataOutputStream.writeText(value: String) {
            val bytes = value.toByteArray(Charsets.UTF_8)
            writeInt(bytes.size); write(bytes)
        }

        private fun DataInputStream.readText(maxBytes: Int): String {
            val size = readInt()
            if (size < 0 || size > maxBytes || size > available()) throw CheckoutAuthorityUnavailableException()
            val bytes = ByteArray(size).also(::readFully)
            return try {
                Charsets.UTF_8.newDecoder().onMalformedInput(CodingErrorAction.REPORT)
                    .onUnmappableCharacter(CodingErrorAction.REPORT).decode(java.nio.ByteBuffer.wrap(bytes)).toString()
            } catch (_: Exception) { throw CheckoutAuthorityUnavailableException() }
        }
    }
}
