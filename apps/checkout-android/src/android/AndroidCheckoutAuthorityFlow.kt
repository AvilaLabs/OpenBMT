package org.openbmt.checkout

import android.content.Context
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.provider.Settings
import java.io.File
import java.io.IOException
import java.util.UUID
import java.util.concurrent.Executors
import java.util.concurrent.ThreadFactory
import java.util.concurrent.atomic.AtomicLong

/**
 * Activity-facing worker boundary for authority. Every descriptor, sealed-record, refresh and
 * PBKDF2 operation is serialized on one private executor; only the immutable state is published
 * to the UI thread. Credentials and the bootstrap draft never cross through Android saved state.
 */
internal class AndroidCheckoutAuthorityFlow(
    context: Context,
    private val businessFilesWereAbsentAtLaunch: Boolean,
    private val onState: (CheckoutAccessState) -> Unit,
) {
    private val appContext = context.applicationContext
    private val main = Handler(Looper.getMainLooper())
    private val generation = AtomicLong(0)
    private val executor = Executors.newSingleThreadExecutor(ThreadFactory { task ->
        Thread(task, "checkout-authority").apply { isDaemon = true }
    })
    private val expireSession = Runnable {
        if (!closed && publishedState is CheckoutAccessState.SignedIn) lock()
    }
    @Volatile private var publishedState: CheckoutAccessState = CheckoutAccessState.Loading
    @Volatile private var controller: CheckoutAuthorityController? = null
    private var authorityRecord: CheckoutAuthority? = null
    private var installationScopeStore: AndroidCheckoutInstallationScopeStore? = null
    private var installationDescriptor: CheckoutInstallationDescriptor? = null
    @Volatile private var closed = false

    fun state(): CheckoutAccessState = publishedState
    fun isSignedIn(): Boolean = publishedState is CheckoutAccessState.SignedIn
    fun isManager(): Boolean = (publishedState as? CheckoutAccessState.SignedIn)?.actor?.role == CheckoutAuthorityRole.MANAGER

    fun start() = refresh()

    fun refresh() {
        enqueue { refreshAccess() }
    }

    fun prepareManagerEnrollment() {
        enqueue {
            val descriptor = checkNotNull(installationDescriptor)
            val profile = checkNotNull(installationScopeStore)
                .validateAuthorityRecord(checkNotNull(authorityRecord))
            if (descriptor.provisioningState == CheckoutProvisioningState.COMPLETED && profile == null) {
                CheckoutAccessState.Unavailable()
            } else authority().prepareManagerEnrollment()
        }
    }

    fun confirmRecoveryCode(value: CharArray) {
        if (closed) { value.fill('\u0000'); return }
        enqueue {
            try { authority().confirmRecoveryCode(value) }
            finally { value.fill('\u0000') }
        }
    }

    fun enrollManager(displayName: String, pin: CharArray) {
        if (closed) { pin.fill('\u0000'); return }
        enqueue {
            try {
                val state = authority().enrollManager(displayName, pin)
                if (state is CheckoutAccessState.SignInRequired) {
                    val descriptor = checkNotNull(installationDescriptor)
                    val authority = checkNotNull(authorityRecord)
                    val profile = authority.loginProfile()
                    val manager = state.staff.firstOrNull { it.role == CheckoutAuthorityRole.MANAGER }
                    if (profile == null || profile.scope != descriptor.scope || manager == null ||
                        profile.enabledStaff.none { it.actorId == manager.actorId && it.role == CheckoutAuthorityRole.MANAGER }) {
                        CheckoutAccessState.Unavailable()
                    } else {
                        checkNotNull(installationScopeStore).markCompleted(descriptor.scope, authority)
                        installationDescriptor = descriptor.copy(provisioningState = CheckoutProvisioningState.COMPLETED)
                        state
                    }
                } else state
            }
            finally { pin.fill('\u0000') }
        }
    }

    fun signIn(actorId: String, pin: CharArray) {
        if (closed) { pin.fill('\u0000'); return }
        enqueue {
            try { authority().signIn(actorId, pin) }
            finally { pin.fill('\u0000') }
        }
    }

    fun beginManagerRecovery() {
        enqueue { authority().beginManagerRecovery() }
    }

    fun prepareManagerRecovery(currentCode: CharArray, newPin: CharArray) {
        if (closed) { currentCode.fill('\u0000'); newPin.fill('\u0000'); return }
        enqueue {
            try { authority().prepareManagerRecovery(currentCode, newPin) }
            finally { currentCode.fill('\u0000'); newPin.fill('\u0000') }
        }
    }

    fun confirmNextRecoveryCode(value: CharArray) {
        if (closed) { value.fill('\u0000'); return }
        enqueue {
            try { authority().confirmNextRecoveryCode(value) }
            finally { value.fill('\u0000') }
        }
    }

    fun loadStaffAdministration(completion: (CheckoutStaffAdministrationResult) -> Unit) =
        staffOperation(completion) {
            CheckoutStaffAdministrationResult.Ready(authority().staffAdministration())
        }

    fun createStaffMember(name: String, role: CheckoutAuthorityRole, pin: CharArray,
                          completion: (CheckoutStaffAdministrationResult) -> Unit) = staffOperation(completion, arrayOf(pin)) {
        try { authority().createStaffMember(name, role, pin) } finally { pin.fill('\u0000') }
    }

    fun renameStaffMember(actorId: String, name: String,
                          completion: (CheckoutStaffAdministrationResult) -> Unit) = staffOperation(completion) {
        authority().renameStaffMember(actorId, name)
    }

    fun changeStaffRole(actorId: String, role: CheckoutAuthorityRole,
                        completion: (CheckoutStaffAdministrationResult) -> Unit) = staffOperation(completion) {
        authority().changeStaffRole(actorId, role)
    }

    fun changeStaffPin(actorId: String, pin: CharArray,
                       completion: (CheckoutStaffAdministrationResult) -> Unit) = staffOperation(completion, arrayOf(pin)) {
        try { authority().changeStaffPin(actorId, pin) } finally { pin.fill('\u0000') }
    }

    fun disableStaffMember(actorId: String,
                           completion: (CheckoutStaffAdministrationResult) -> Unit) = staffOperation(completion) {
        authority().disableStaffMember(actorId)
    }

    /** Create the cash store only after this flow has opened the installation authority. */
    fun cashSessionStore(storage: DurableLedgerStorage, drawerId: String): CheckoutCashSessionStore {
        check(isSignedIn()) { "Checkout authority is not signed in" }
        return checkNotNull(controller) { "Checkout authority is unavailable" }
            .cashSessionStore(storage, drawerId)
    }

    /** Execute one business mutation on the authority worker without exposing its bearer to UI. */
    fun executeCommand(
        operation: (CheckoutAuthoritySession) -> Unit,
        completion: (Exception?) -> Unit,
    ) {
        if (closed) return completion(CheckoutAuthorityUnavailableException())
        val ticket = generation.get()
        executor.execute {
            if (closed || generation.get() != ticket) return@execute
            val error = try {
                checkNotNull(controller) { "Checkout authority is unavailable" }.runCommand(operation)
                null
            } catch (e: Exception) {
                e
            }
            val next = controller?.state() ?: CheckoutAccessState.Unavailable()
            main.post {
                if (closed || generation.get() != ticket) return@post
                if (next !is CheckoutAccessState.SignedIn) {
                    publishedState = next
                    onState(next)
                }
                completion(error)
            }
        }
    }

    /** Authorize and serialize a non-cash command; attribution remains inside the worker. */
    fun executeCapability(
        capability: CheckoutAuthorityCapability,
        operation: () -> Unit,
        completion: (Exception?) -> Unit,
    ) {
        if (closed) return completion(CheckoutAuthorityUnavailableException())
        val ticket = generation.get()
        executor.execute {
            if (closed || generation.get() != ticket) return@execute
            val error = try {
                checkNotNull(controller) { "Checkout authority is unavailable" }
                    .runAuthorizedCommand(capability) { operation() }
                null
            } catch (e: Exception) {
                e
            }
            val next = controller?.state() ?: CheckoutAccessState.Unavailable()
            main.post {
                if (closed || generation.get() != ticket) return@post
                if (next !is CheckoutAccessState.SignedIn) {
                    main.removeCallbacks(expireSession)
                    publishedState = next
                    onState(next)
                }
                completion(error)
            }
        }
    }

    /** Lock is made visible immediately; durable invalidation runs next on the serial worker. */
    fun lock() {
        if (closed) return
        val ticket = generation.incrementAndGet()
        publish(CheckoutAccessState.Loading, ticket)
        executor.execute {
            val next = try {
                controller?.lock() ?: CheckoutAccessState.Loading
            } catch (_: Exception) {
                CheckoutAccessState.Unavailable()
            }
            publish(next, ticket)
        }
    }

    fun destroy() {
        if (closed) return
        closed = true
        generation.incrementAndGet()
        publishedState = CheckoutAccessState.Loading
        main.removeCallbacks(expireSession)
        executor.execute {
            try { controller?.lock() } catch (_: Exception) { }
            controller = null
            // Do not publish into a destroyed Activity. Complete the lock before shutting down.
            executor.shutdown()
        }
    }

    private fun enqueue(operation: () -> CheckoutAccessState) {
        if (closed) return
        val ticket = generation.incrementAndGet()
        publish(CheckoutAccessState.Loading, ticket)
        executor.execute {
            val next = try { operation() } catch (_: Exception) { CheckoutAccessState.Unavailable() }
            publish(next, ticket)
        }
    }

    private fun staffOperation(
        completion: (CheckoutStaffAdministrationResult) -> Unit,
        secrets: Array<CharArray> = emptyArray(),
        operation: () -> CheckoutStaffAdministrationResult,
    ) {
        if (closed) {
            secrets.forEach { it.fill('\u0000') }
            return completion(CheckoutStaffAdministrationResult.Failed("Checkout authority is unavailable."))
        }
        val ticket = generation.get()
        try {
            executor.execute {
                if (closed || generation.get() != ticket) {
                    secrets.forEach { it.fill('\u0000') }
                    return@execute
                }
                val result = try { operation() } catch (_: Exception) {
                    CheckoutStaffAdministrationResult.Failed("Staff administration could not be completed. Check access and try again.")
                }
                val next = controller?.state() ?: CheckoutAccessState.Unavailable()
                main.post {
                    if (closed || generation.get() != ticket) return@post
                    if (next !is CheckoutAccessState.SignedIn) {
                        main.removeCallbacks(expireSession)
                        publishedState = next
                        onState(next)
                    } else {
                        completion(result)
                    }
                }
            }
        } catch (_: java.util.concurrent.RejectedExecutionException) {
            secrets.forEach { it.fill('\u0000') }
            completion(CheckoutStaffAdministrationResult.Failed("Checkout authority is unavailable."))
        }
    }

    private fun publish(next: CheckoutAccessState, ticket: Long) {
        if (closed || generation.get() != ticket) return
        publishedState = next
        main.post {
            if (!closed && generation.get() == ticket) {
                main.removeCallbacks(expireSession)
                if (next is CheckoutAccessState.SignedIn) {
                    main.postDelayed(expireSession, SESSION_LIFETIME_MILLIS)
                }
                onState(next)
            }
        }
    }

    private fun authority(): CheckoutAuthorityController {
        controller?.let { return it }
        val noBackup = appContext.noBackupFilesDir
        val scopeStore = AndroidCheckoutInstallationScopeStore(appContext)
        val descriptor = File(noBackup, "checkout-installation-scope.obis")
        val installation = try {
            scopeStore.loadDescriptor()
        } catch (loadError: Exception) {
            // Only a genuinely absent descriptor on a fresh platform install with no preexisting
            // business files may establish a scope. Missing/corrupt state on an existing install
            // must never silently mint replacement IDs.
            if (descriptor.exists() || !platformInstallIsFresh() || !businessFilesWereAbsentAtLaunch) {
                throw IOException("Checkout authority installation scope is unavailable", loadError)
            }
            scopeStore.createDescriptorOnFreshInstall(CheckoutFreshInstallEvidence(UUID.randomUUID().toString())) {
                fun id() = UUID.randomUUID().toString().replace('-', 'a')
                CheckoutAuthorityScope(id(), id(), id(), id(), id())
            }
        }
        val scope = installation.scope
        val storage = AndroidCheckoutAuthorityStorage(appContext, scope)
        val clock = AndroidCheckoutAuthorityClock(appContext)
        val authority = CheckoutAuthority(storage, clock)
        val auditJournal = CheckoutAuthorityAuditJournal(
            AndroidCheckoutAuthorityStorage(appContext, scope, AUTHORITY_AUDIT_FILE),
        )
        val auditCommands = CheckoutAuthorityCommandService(authority, auditJournal, clock)
        val check = CheckoutBusinessFreshnessCheck {
            businessFilesWereAbsentAtLaunch && BUSINESS_FILE_NAMES.none { File(noBackup, it).exists() }
        }
        authorityRecord = authority
        installationScopeStore = scopeStore
        installationDescriptor = installation
        return CheckoutAuthorityController(authority, scope, check, auditCommands).also { controller = it }
    }

    private fun refreshAccess(): CheckoutAccessState {
        val activeController = authority()
        val scopeStore = checkNotNull(installationScopeStore)
        val descriptor = scopeStore.loadDescriptor()
        installationDescriptor = descriptor
        val authority = checkNotNull(authorityRecord)
        val profile = scopeStore.validateAuthorityRecord(authority)
        if (descriptor.provisioningState == CheckoutProvisioningState.COMPLETED && profile == null) {
            return CheckoutAccessState.Unavailable()
        }
        if (profile != null && descriptor.provisioningState == CheckoutProvisioningState.PENDING_ENROLLMENT) {
            scopeStore.markCompleted(descriptor.scope, authority)
            installationDescriptor = descriptor.copy(provisioningState = CheckoutProvisioningState.COMPLETED)
        }
        return activeController.refresh()
    }

    private fun platformInstallIsFresh(): Boolean = try {
        @Suppress("DEPRECATION")
        val info = appContext.packageManager.getPackageInfo(appContext.packageName, 0)
        info.firstInstallTime > 0 && info.firstInstallTime == info.lastUpdateTime
    } catch (_: Exception) {
        false
    }

    private companion object {
        const val AUTHORITY_AUDIT_FILE = "checkout-authority-audit.obis"
        val BUSINESS_FILE_NAMES = listOf(
            "checkout-ledger.bin",
            "checkout-catalog.bin",
            "checkout-cash-sessions.bin",
            "checkout-inventory.bin",
        )
        const val SESSION_LIFETIME_MILLIS = 5 * 60 * 1000L
    }
}

private class AndroidCheckoutAuthorityClock(private val context: Context) : CheckoutAuthorityClock {
    override fun elapsedRealtimeMillis(): Long = SystemClock.elapsedRealtime()
    override fun wallTimeMillis(): Long = System.currentTimeMillis()
    override fun bootIdentity(): String = try {
        Settings.Global.getInt(context.contentResolver, Settings.Global.BOOT_COUNT).toString()
    } catch (_: Exception) {
        // If the platform cannot provide a stable boot identity, authority operations fail closed.
        throw IOException("Checkout authority clock is unavailable")
    }
}
