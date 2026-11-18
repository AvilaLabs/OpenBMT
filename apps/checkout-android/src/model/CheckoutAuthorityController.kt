package org.openbmt.checkout

import java.security.MessageDigest
import java.util.UUID

/**
 * A caller supplied, fail-closed check that every durable business store is demonstrably fresh.
 * This intentionally cannot be inferred from an absent authority record. Android must inspect
 * ledger, catalog, inventory, and cash-session records before allowing first-manager enrollment.
 */
fun interface CheckoutBusinessFreshnessCheck {
    fun isFresh(): Boolean
}

sealed interface CheckoutAccessState {
    data object Loading : CheckoutAccessState
    data object EnrollmentRequired : CheckoutAccessState
    data class RecoveryCodeConfirmation(val recoveryCode: String, val message: String? = null) : CheckoutAccessState {
        override fun toString(): String = "RecoveryCodeConfirmation(recoveryCode=redacted, message=$message)"
    }
    data class ManagerEnrollmentRequired(val message: String? = null) : CheckoutAccessState
    data class SignInRequired(val staff: List<CheckoutStaffSummary>, val message: String? = null) : CheckoutAccessState
    data class SignedIn(val actor: ActorAttribution) : CheckoutAccessState
    data class ManagerRecoveryRequired(val message: String? = null) : CheckoutAccessState
    data class ManagerRecoveryCodeAcknowledgement(
        val nextRecoveryCode: String,
        val message: String? = null,
        val commitUncertain: Boolean = false,
    ) : CheckoutAccessState {
        override fun toString(): String = "ManagerRecoveryCodeAcknowledgement(nextRecoveryCode=redacted, message=$message)"
    }
    data class Unavailable(val managerRecoveryAvailable: Boolean = false) : CheckoutAccessState
}

data class CheckoutStaffAdministrationSnapshot(
    val staff: List<CheckoutStaffSummary>,
    val message: String? = null,
)

sealed interface CheckoutStaffAdministrationResult {
    data class Ready(val snapshot: CheckoutStaffAdministrationSnapshot) : CheckoutStaffAdministrationResult
    data class Failed(val message: String) : CheckoutStaffAdministrationResult
}

/**
 * Process-local enrollment and sign-in state. It never stores PINs, sessions, or drafts in saved
 * UI state. Call [lock] on every background transition and on any stale or uncertain error.
 * Authorization still has to be checked at each business command; this controller is only the
 * entry/session gate.
 */
class CheckoutAuthorityController(
    private val authority: CheckoutAuthority,
    private val scope: CheckoutAuthorityScope,
    private val businessFreshness: CheckoutBusinessFreshnessCheck,
    private val auditCommands: CheckoutAuthorityCommandService? = null,
) {
    private val monitor = Any()
    private var state: CheckoutAccessState = CheckoutAccessState.Loading
    private var bootstrapDraft: CheckoutAuthorityBootstrapDraft? = null
    private var session: CheckoutAuthoritySession? = null
    private var recoveryDraft: CheckoutAuthorityRecoveryDraft? = null
    private var recoveryCode: CharArray? = null
    private var recoveryPin: CharArray? = null

    fun state(): CheckoutAccessState = synchronized(monitor) { state }

    /** Reconcile the process-local state with durable authority state; errors always fail closed. */
    fun refresh(): CheckoutAccessState = synchronized(monitor) {
        clearRecovery()
        try {
            val profile = authority.loginProfile()
            if (profile == null) {
                session = null
                state = if (freshBusinessState()) CheckoutAccessState.EnrollmentRequired else unavailableState()
            } else {
                bootstrapDraft = null
                session = null
                if (profile.scope != scope) unavailableState().also { state = it }
                else CheckoutAccessState.SignInRequired(profile.enabledStaff).also { state = it }
            }
        } catch (_: Exception) {
            failClosed()
        }
        state
    }

    /** Prepare only while authority is absent and business stores remain demonstrably fresh. */
    fun prepareManagerEnrollment(): CheckoutAccessState = synchronized(monitor) {
        try {
            if (authority.loginProfile() != null || !freshBusinessState()) return failClosed()
            bootstrapDraft = authority.prepareBootstrap()
            state = CheckoutAccessState.RecoveryCodeConfirmation(bootstrapDraft!!.recoveryCode)
        } catch (_: Exception) {
            failClosed()
        }
        state
    }

    /** Exact confirmation is required before the manager credential can be committed. */
    fun confirmRecoveryCode(confirmation: CharArray): CheckoutAccessState = synchronized(monitor) {
        try {
            if (state !is CheckoutAccessState.RecoveryCodeConfirmation) {
                confirmation.fill('\u0000')
                return failClosed()
            }
            val draft = bootstrapDraft ?: return failClosed().also { confirmation.fill('\u0000') }
            val expected = draft.recoveryCode.toCharArray()
            val expectedBytes = charBytes(expected)
            val confirmationBytes = charBytes(confirmation)
            val matches = try { MessageDigest.isEqual(expectedBytes, confirmationBytes) }
            finally {
                expected.fill('\u0000')
                confirmation.fill('\u0000')
                expectedBytes.fill(0)
                confirmationBytes.fill(0)
            }
            if (!matches || !freshBusinessState()) {
                state = CheckoutAccessState.RecoveryCodeConfirmation(
                    draft.recoveryCode,
                    "The confirmation did not match or checkout data is no longer fresh. The code was not accepted.",
                )
            } else {
                state = CheckoutAccessState.ManagerEnrollmentRequired()
            }
        } catch (_: Exception) {
            failClosed()
        }
        state
    }

    /** Persist the first manager only after recovery-code acknowledgement and a fresh-store recheck. */
    fun enrollManager(displayName: String, pin: CharArray): CheckoutAccessState = synchronized(monitor) {
        try {
            val draft = bootstrapDraft ?: return failClosed().also { pin.fill('\u0000') }
            if (state !is CheckoutAccessState.ManagerEnrollmentRequired || !freshBusinessState() || authority.loginProfile() != null) {
                pin.fill('\u0000')
                return failClosed()
            }
            val created = authority.bootstrapManager(draft, scope, displayName, pin)
            bootstrapDraft = null
            session = null
            state = CheckoutAccessState.SignInRequired(
                listOf(CheckoutStaffSummary(created.managerActorId, displayName.trim(), CheckoutAuthorityRole.MANAGER, true)),
                "Manager created. Sign in with the PIN you just set.",
            )
        } catch (_: CheckoutAuthorityDeniedException) {
            pin.fill('\u0000')
            state = CheckoutAccessState.ManagerEnrollmentRequired("Manager setup was rejected. Check the name and PIN, then try again.")
        } catch (_: Exception) {
            pin.fill('\u0000')
            // Preserve the draft after uncertain persistence; reopening the app will reconcile it.
            state = unavailableState()
            session = null
        }
        state
    }

    /** Sign in by stable actor ID. Credential errors are intentionally indistinguishable. */
    fun signIn(actorId: String, pin: CharArray): CheckoutAccessState = synchronized(monitor) {
        try {
            val profile = authority.loginProfile() ?: return failClosed().also { pin.fill('\u0000') }
            if (profile.scope != scope || profile.enabledStaff.none { it.actorId == actorId }) {
                pin.fill('\u0000')
                state = CheckoutAccessState.SignInRequired(profile.enabledStaff, "Sign-in failed. Check staff member and PIN.")
                return state
            }
            val authenticated = authority.authenticate(actorId, pin, scope)
            session = authenticated
            state = CheckoutAccessState.SignedIn(authority.authorize(authenticated, CheckoutAuthorityCapability.SALES_READ, scope))
        } catch (_: CheckoutAuthorityDeniedException) {
            session = null
            state = try {
                val profile = authority.loginProfile()
                if (profile == null || profile.scope != scope) unavailableState()
                else CheckoutAccessState.SignInRequired(profile.enabledStaff, "Sign-in failed. Check staff member and PIN.")
            } catch (_: Exception) { unavailableState() }
        } catch (_: Exception) {
            failClosed()
        } finally {
            pin.fill('\u0000')
        }
        state
    }

    /** Offer recovery only when the installation profile can be read for this exact scope. */
    fun beginManagerRecovery(): CheckoutAccessState = synchronized(monitor) {
        try {
            val allowed = state is CheckoutAccessState.SignInRequired ||
                (state as? CheckoutAccessState.Unavailable)?.managerRecoveryAvailable == true
            if (!allowed) return failClosed()
            val profile = authority.loginProfile()
            if (profile == null || profile.scope != scope) return failClosed()
            clearRecovery()
            state = CheckoutAccessState.ManagerRecoveryRequired()
        } catch (_: Exception) {
            failClosed()
        }
        state
    }

    /** Prepare the next one-time code before showing it and before attempting any durable recovery. */
    fun prepareManagerRecovery(currentCode: CharArray, newPin: CharArray): CheckoutAccessState = synchronized(monitor) {
        try {
            if (state !is CheckoutAccessState.ManagerRecoveryRequired) return failClosed()
            val profile = authority.loginProfile()
            if (profile == null || profile.scope != scope) return failClosed()
            clearRecovery()
            recoveryCode = currentCode.copyOf()
            recoveryPin = newPin.copyOf()
            val draft = authority.prepareRecovery()
            recoveryDraft = draft
            state = CheckoutAccessState.ManagerRecoveryCodeAcknowledgement(draft.recoveryCode)
        } catch (_: Exception) {
            clearRecovery()
            failClosed()
        } finally {
            currentCode.fill('\u0000')
            newPin.fill('\u0000')
        }
        state
    }

    /** Commit only after exact next-code acknowledgement; retain all material if the write is uncertain. */
    fun confirmNextRecoveryCode(confirmation: CharArray): CheckoutAccessState = synchronized(monitor) {
        try {
            val acknowledgement = state as? CheckoutAccessState.ManagerRecoveryCodeAcknowledgement
                ?: return failClosed().also { confirmation.fill('\u0000') }
            if (acknowledgement.commitUncertain) {
                confirmation.fill('\u0000')
                return acknowledgement
            }
            val draft = recoveryDraft ?: return failClosed().also { confirmation.fill('\u0000') }
            val savedCode = recoveryCode ?: return failClosed().also { confirmation.fill('\u0000') }
            val savedPin = recoveryPin ?: return failClosed().also { confirmation.fill('\u0000') }
            val expected = acknowledgement.nextRecoveryCode.toCharArray()
            val expectedBytes = charBytes(expected)
            val suppliedBytes = charBytes(confirmation)
            val matches = try { MessageDigest.isEqual(expectedBytes, suppliedBytes) }
            finally {
                expected.fill('\u0000')
                confirmation.fill('\u0000')
                expectedBytes.fill(0)
                suppliedBytes.fill(0)
            }
            if (!matches) {
                state = acknowledgement.copy(message = "The replacement code did not match. Nothing was changed; try again.")
                return state
            }
            val profile = authority.loginProfile()
            if (profile == null || profile.scope != scope) return failClosed()
            try {
                authority.recoverManager(scope, savedCode.copyOf(), savedPin.copyOf(), draft)
            } catch (_: CheckoutAuthorityDeniedException) {
                clearRecovery()
                state = CheckoutAccessState.SignInRequired(
                    profile.enabledStaff,
                    "Manager recovery failed. Check the recovery code and choose a new PIN.",
                )
                return state
            } catch (_: Exception) {
                state = acknowledgement.copy(
                    message = "The save outcome is uncertain. Keep this replacement code; do not discard it.",
                    commitUncertain = true,
                )
                return state
            }
            val updated = try { authority.loginProfile() } catch (_: Exception) { null }
            if (updated == null || updated.scope != scope) {
                state = acknowledgement.copy(
                    message = "The save outcome is uncertain. Keep this replacement code; do not discard it.",
                    commitUncertain = true,
                )
                return state
            }
            clearRecovery()
            session = null
            state = CheckoutAccessState.SignInRequired(
                updated.enabledStaff,
                "Manager access was recovered. Sign in with the new PIN.",
            )
        } catch (_: Exception) {
            failClosed()
        }
        state
    }

    /** Read the complete staff directory only after a fresh manager capability check. */
    fun staffAdministration(): CheckoutStaffAdministrationResult = synchronized(monitor) {
        try {
            val active = managerSession()
            val reconciled = checkNotNull(auditCommands) { "Audited staff administration is unavailable" }
                .reconcileStaffIntents(active, scope)
            CheckoutStaffAdministrationResult.Ready(CheckoutStaffAdministrationSnapshot(
                authority.listStaff(active, scope),
                if (reconciled > 0) "A previous staff operation was reconciled against its durable outcome." else null,
            ))
        } catch (_: CheckoutAuthorityAuditReconciliationRequiredException) {
            CheckoutStaffAdministrationResult.Failed(
                "A previous staff operation could not be proven safe to close. Staff changes remain blocked.",
            )
        } catch (_: CheckoutAuthorityAuditUnavailableException) {
            failClosed()
            CheckoutStaffAdministrationResult.Failed("Staff audit storage is unavailable. Staff changes remain blocked.")
        } catch (_: CheckoutAuthorityDeniedException) {
            CheckoutStaffAdministrationResult.Failed("Manager access is required to view staff administration.")
        } catch (error: CheckoutAuthorityUnavailableException) {
            failClosed()
            CheckoutStaffAdministrationResult.Failed("Staff access storage is unavailable.")
        }
    }

    fun createStaffMember(
        displayName: String,
        role: CheckoutAuthorityRole,
        pin: CharArray,
    ): CheckoutStaffAdministrationResult = synchronized(monitor) {
        try {
            val active = managerSession()
            checkNotNull(auditCommands) { "Audited staff administration is unavailable" }
                .createStaffPrepared(newOperationId(), active, scope, displayName, role, pin)
            invalidateAfterPolicyChange("Staff member created. Sign in again to continue.")
            CheckoutStaffAdministrationResult.Ready(CheckoutStaffAdministrationSnapshot(emptyList(), "Staff member created."))
        } catch (_: CheckoutAuthorityDeniedException) {
            CheckoutStaffAdministrationResult.Failed("Staff change was rejected. Check the details and PIN, then try again.")
        } catch (_: CheckoutAuthorityAuditReconciliationRequiredException) {
            CheckoutStaffAdministrationResult.Failed("A previous staff change is unresolved. Reopen staff administration to reconcile it; no new change was applied.")
        } catch (_: CheckoutAuthorityAuditUnavailableException) {
            failClosed()
            CheckoutStaffAdministrationResult.Failed("Staff audit storage is unavailable. No further staff changes can be made in this session.")
        } catch (_: CheckoutAuthorityUnavailableException) {
            failClosed()
            CheckoutStaffAdministrationResult.Failed("Staff access storage is unavailable. No further changes can be made in this session.")
        } catch (_: Exception) {
            CheckoutStaffAdministrationResult.Failed("Staff change was rejected. Check the details and try again.")
        } finally {
            pin.fill('\u0000')
        }
    }

    fun renameStaffMember(actorId: String, displayName: String): CheckoutStaffAdministrationResult = synchronized(monitor) {
        try {
            val active = managerSession()
            checkNotNull(auditCommands) { "Audited staff administration is unavailable" }
                .renameStaffPrepared(newOperationId(), active, scope, actorId, displayName)
            invalidateAfterPolicyChange("Staff name updated. Sign in again to continue.")
            CheckoutStaffAdministrationResult.Ready(CheckoutStaffAdministrationSnapshot(emptyList(), "Staff name updated."))
        } catch (_: CheckoutAuthorityDeniedException) {
            CheckoutStaffAdministrationResult.Failed("Name change was rejected. Check staff status and try again.")
        } catch (_: CheckoutAuthorityAuditReconciliationRequiredException) {
            CheckoutStaffAdministrationResult.Failed("A previous staff change is unresolved. Reopen staff administration to reconcile it; no new change was applied.")
        } catch (_: CheckoutAuthorityAuditUnavailableException) {
            failClosed()
            CheckoutStaffAdministrationResult.Failed("Staff audit storage is unavailable. No further staff changes can be made in this session.")
        } catch (_: CheckoutAuthorityUnavailableException) {
            failClosed()
            CheckoutStaffAdministrationResult.Failed("Staff access storage is unavailable. No further changes can be made in this session.")
        } catch (_: Exception) {
            CheckoutStaffAdministrationResult.Failed("Name change was rejected. Check the name and try again.")
        }
    }

    fun changeStaffRole(actorId: String, role: CheckoutAuthorityRole): CheckoutStaffAdministrationResult = synchronized(monitor) {
        try {
            val active = managerSession()
            checkNotNull(auditCommands) { "Audited staff administration is unavailable" }
                .changeRolePrepared(newOperationId(), active, scope, actorId, role)
            invalidateAfterPolicyChange("Staff role updated. Sign in again to continue.")
            CheckoutStaffAdministrationResult.Ready(CheckoutStaffAdministrationSnapshot(emptyList(), "Staff role updated."))
        } catch (_: CheckoutAuthorityDeniedException) {
            CheckoutStaffAdministrationResult.Failed("Role change was rejected. Keep an active recovery manager enabled.")
        } catch (_: CheckoutAuthorityAuditReconciliationRequiredException) {
            CheckoutStaffAdministrationResult.Failed("A previous staff change is unresolved. Reopen staff administration to reconcile it; no new change was applied.")
        } catch (_: CheckoutAuthorityAuditUnavailableException) {
            failClosed()
            CheckoutStaffAdministrationResult.Failed("Staff audit storage is unavailable. No further staff changes can be made in this session.")
        } catch (_: CheckoutAuthorityUnavailableException) {
            failClosed()
            CheckoutStaffAdministrationResult.Failed("Staff access storage is unavailable. No further changes can be made in this session.")
        } catch (_: Exception) {
            CheckoutStaffAdministrationResult.Failed("Role change was rejected. Check staff status and try again.")
        }
    }

    fun changeStaffPin(actorId: String, newPin: CharArray): CheckoutStaffAdministrationResult = synchronized(monitor) {
        try {
            val active = managerSession()
            checkNotNull(auditCommands) { "Audited staff administration is unavailable" }
                .changePinPrepared(newOperationId(), active, scope, actorId, newPin)
            invalidateAfterPolicyChange("PIN updated. Sign in again to continue.")
            CheckoutStaffAdministrationResult.Ready(CheckoutStaffAdministrationSnapshot(emptyList(), "PIN updated."))
        } catch (_: CheckoutAuthorityDeniedException) {
            CheckoutStaffAdministrationResult.Failed("PIN change was rejected. Check the PIN and try again.")
        } catch (_: CheckoutAuthorityAuditReconciliationRequiredException) {
            CheckoutStaffAdministrationResult.Failed("A previous staff change is unresolved. Reopen staff administration to reconcile it; no new change was applied.")
        } catch (_: CheckoutAuthorityAuditUnavailableException) {
            failClosed()
            CheckoutStaffAdministrationResult.Failed("Staff audit storage is unavailable. No further staff changes can be made in this session.")
        } catch (_: CheckoutAuthorityUnavailableException) {
            failClosed()
            CheckoutStaffAdministrationResult.Failed("Staff access storage is unavailable. No further changes can be made in this session.")
        } catch (_: Exception) {
            CheckoutStaffAdministrationResult.Failed("PIN change was rejected. Check the PIN and try again.")
        } finally {
            newPin.fill('\u0000')
        }
    }

    fun disableStaffMember(actorId: String): CheckoutStaffAdministrationResult = synchronized(monitor) {
        try {
            val active = managerSession()
            checkNotNull(auditCommands) { "Audited staff administration is unavailable" }
                .disableStaffPrepared(newOperationId(), active, scope, actorId)
            invalidateAfterPolicyChange("Staff access disabled. Sign in again to continue.")
            CheckoutStaffAdministrationResult.Ready(CheckoutStaffAdministrationSnapshot(emptyList(), "Staff access disabled."))
        } catch (_: CheckoutAuthorityDeniedException) {
            CheckoutStaffAdministrationResult.Failed("Staff access could not be disabled. Keep an active recovery manager enabled.")
        } catch (_: CheckoutAuthorityAuditReconciliationRequiredException) {
            CheckoutStaffAdministrationResult.Failed("A previous staff change is unresolved. Reopen staff administration to reconcile it; no new change was applied.")
        } catch (_: CheckoutAuthorityAuditUnavailableException) {
            failClosed()
            CheckoutStaffAdministrationResult.Failed("Staff audit storage is unavailable. No further staff changes can be made in this session.")
        } catch (_: CheckoutAuthorityUnavailableException) {
            failClosed()
            CheckoutStaffAdministrationResult.Failed("Staff access storage is unavailable. No further changes can be made in this session.")
        } catch (_: Exception) {
            CheckoutStaffAdministrationResult.Failed("Staff access could not be disabled. Check staff status and try again.")
        }
    }

    /** Check a capability immediately before acting; a stale/error result locks the whole session. */
    fun authorize(capability: CheckoutAuthorityCapability): ActorAttribution? = synchronized(monitor) {
        val active = session ?: return null
        try {
            authority.authorize(active, capability, scope).also { state = CheckoutAccessState.SignedIn(it) }
        } catch (_: CheckoutAuthorityDeniedException) {
            requireSignIn()
            null
        } catch (_: Exception) {
            failClosed()
            null
        }
    }

    /**
     * Trusted command bridge. The opaque bearer never enters UI state or Android saved state;
     * the business store must still authorize the exact capability inside its mutation boundary.
     */
    internal fun runCommand(command: (CheckoutAuthoritySession) -> Unit) = synchronized(monitor) {
        val active = session ?: throw CheckoutAuthorityDeniedException()
        if (state !is CheckoutAccessState.SignedIn) throw CheckoutAuthorityDeniedException()
        try {
            command(active)
        } catch (e: CheckoutAuthorityDeniedException) {
            requireSignIn()
            throw e
        } catch (e: CheckoutAuthorityUnavailableException) {
            failClosed()
            throw e
        }
    }

    /** Authorize and execute one not-yet-attributed business command at one serialized boundary. */
    internal fun runAuthorizedCommand(
        capability: CheckoutAuthorityCapability,
        command: (ActorAttribution) -> Unit,
    ) = synchronized(monitor) {
        val active = session ?: throw CheckoutAuthorityDeniedException()
        if (state !is CheckoutAccessState.SignedIn) throw CheckoutAuthorityDeniedException()
        try {
            val actor = authority.authorize(active, capability, scope)
            state = CheckoutAccessState.SignedIn(actor)
            command(actor)
        } catch (e: CheckoutAuthorityDeniedException) {
            requireSignIn()
            throw e
        } catch (e: CheckoutAuthorityUnavailableException) {
            failClosed()
            throw e
        }
    }

    /** Build a cash command boundary around this exact authority instance and installation scope. */
    internal fun cashSessionStore(
        storage: DurableLedgerStorage,
        drawerId: String,
    ): CheckoutCashSessionStore = synchronized(monitor) {
        CheckoutCashSessionStore(storage, authority, scope, drawerId)
    }

    /** Clear all process-local access and ask the authority to invalidate its current sessions. */
    fun lock(): CheckoutAccessState = synchronized(monitor) {
        session = null
        bootstrapDraft = null
        clearRecovery()
        try { authority.lock() } catch (_: Exception) { /* remain fail closed */ }
        refresh()
    }

    private fun freshBusinessState(): Boolean = try { businessFreshness.isFresh() } catch (_: Exception) { false }

    private fun managerSession(): CheckoutAuthoritySession {
        val active = session ?: throw CheckoutAuthorityDeniedException()
        val signedIn = state as? CheckoutAccessState.SignedIn ?: throw CheckoutAuthorityDeniedException()
        if (signedIn.actor.role != CheckoutAuthorityRole.MANAGER) throw CheckoutAuthorityDeniedException()
        try {
            state = CheckoutAccessState.SignedIn(authority.authorize(active, CheckoutAuthorityCapability.STAFF_MANAGE, scope))
        } catch (denied: CheckoutAuthorityDeniedException) {
            requireSignIn()
            throw denied
        } catch (unavailable: CheckoutAuthorityUnavailableException) {
            failClosed()
            throw unavailable
        }
        return active
    }

    private fun invalidateAfterPolicyChange(message: String) {
        session = null
        try { authority.lock() } catch (_: Exception) { }
        val profile = try { authority.loginProfile() } catch (_: Exception) { null }
        if (profile == null || profile.scope != scope) {
            failClosed()
        } else {
            state = CheckoutAccessState.SignInRequired(profile.enabledStaff, message)
        }
    }

    private fun newOperationId(): String = UUID.randomUUID().toString()

    private fun unavailableState(): CheckoutAccessState.Unavailable {
        val readable = try { authority.loginProfile()?.scope == scope } catch (_: Exception) { false }
        return CheckoutAccessState.Unavailable(managerRecoveryAvailable = readable)
    }

    private fun clearRecovery() {
        recoveryCode?.fill('\u0000')
        recoveryCode = null
        recoveryPin?.fill('\u0000')
        recoveryPin = null
        recoveryDraft = null
    }

    private fun requireSignIn(): CheckoutAccessState {
        session = null
        state = try {
            val profile = authority.loginProfile()
            if (profile == null || profile.scope != scope) unavailableState()
            else CheckoutAccessState.SignInRequired(profile.enabledStaff, "Your session ended. Sign in again.")
        } catch (_: Exception) {
            unavailableState()
        }
        return state
    }

    private fun failClosed(): CheckoutAccessState {
        session = null
        bootstrapDraft = null
        clearRecovery()
        state = unavailableState()
        return state
    }

    private fun charBytes(chars: CharArray): ByteArray = ByteArray(chars.size) { index -> chars[index].code.toByte() }
}
