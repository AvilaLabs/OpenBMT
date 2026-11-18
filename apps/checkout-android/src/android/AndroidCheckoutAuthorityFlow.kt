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

enum class CheckoutLedgerFailureKind { DEFINITIVE_CONFLICT, UNCERTAIN_COMMIT }

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
    private val ledgerFailureMonitor = Any()
    private val executor = Executors.newSingleThreadExecutor(ThreadFactory { task ->
        Thread(task, "checkout-authority").apply { isDaemon = true }
    })
    private val expireSession = Runnable {
        if (!closed && publishedState is CheckoutAccessState.SignedIn) lock()
    }
    @Volatile private var publishedState: CheckoutAccessState = CheckoutAccessState.Loading
    @Volatile private var controller: CheckoutAuthorityController? = null
    @Volatile private var ledgerOutcomeNeedsReopen = false
    @Volatile private var ledgerFailureKind: CheckoutLedgerFailureKind? = null
    private var authorityRecord: CheckoutAuthority? = null
    private var installationScopeStore: AndroidCheckoutInstallationScopeStore? = null
    private var installationDescriptor: CheckoutInstallationDescriptor? = null
    @Volatile private var closed = false

    fun state(): CheckoutAccessState = if (ledgerOutcomeNeedsReopen) CheckoutAccessState.Unavailable() else publishedState
    fun isSignedIn(): Boolean = !ledgerOutcomeNeedsReopen && publishedState is CheckoutAccessState.SignedIn
    fun isManager(): Boolean = !ledgerOutcomeNeedsReopen &&
        (publishedState as? CheckoutAccessState.SignedIn)?.actor?.role == CheckoutAuthorityRole.MANAGER
    fun requiresLedgerReopen(): Boolean = ledgerOutcomeNeedsReopen
    fun ledgerReloadFailureKind(): CheckoutLedgerFailureKind? = ledgerFailureKind

    /** Latch a definitive CAS loss or an uncertain storage write and publish recovery state. */
    fun latchLedgerWriteFailure(error: Exception): CheckoutLedgerFailureKind? {
        val detected = when (error) {
            is CheckoutLedgerConcurrentWriteException -> CheckoutLedgerFailureKind.DEFINITIVE_CONFLICT
            is CheckoutLedgerWriteUncertainException -> CheckoutLedgerFailureKind.UNCERTAIN_COMMIT
            else -> return null
        }
        val (kind, ticket) = synchronized(ledgerFailureMonitor) {
            ledgerFailureKind?.let { return@synchronized it to null }
            ledgerFailureKind = detected
            ledgerOutcomeNeedsReopen = true
            val next = generation.incrementAndGet()
            publishedState = CheckoutAccessState.Unavailable()
            detected to next
        }
        ticket?.let { stateTicket ->
            main.post {
                if (!closed && generation.get() == stateTicket) onState(CheckoutAccessState.Unavailable())
            }
        }
        return kind
    }

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
            authority().staffAdministration()
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
        if (ledgerOutcomeNeedsReopen) return completion(CheckoutAuthorityUnavailableException())
        if (closed) return completion(CheckoutAuthorityUnavailableException())
        val ticket = generation.get()
        executor.execute {
            if (closed || generation.get() != ticket) return@execute
            if (ledgerOutcomeNeedsReopen) {
                main.post { if (!closed) completion(CheckoutAuthorityUnavailableException()) }
                return@execute
            }
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

    /** Create one sale without allowing its opaque authority session to cross this worker API. */
    fun executeSaleCreate(
        ledger: CheckoutLedger,
        operationId: String,
        saleId: String,
        amountMinor: Long,
        currency: String,
        createdAt: Long,
        items: List<SaleItem>,
        pricing: CheckoutPricingSnapshot?,
        completion: (Exception?) -> Unit,
    ) {
        if (ledgerOutcomeNeedsReopen) return completion(CheckoutAuthorityUnavailableException())
        if (closed) return completion(CheckoutAuthorityUnavailableException())
        val ticket = generation.get()
        try {
            executor.execute {
                if (closed) return@execute
                if (ledgerOutcomeNeedsReopen) {
                    main.post { if (!closed) completion(CheckoutAuthorityUnavailableException()) }
                    return@execute
                }
                if (generation.get() != ticket) {
                    // The worker has not touched the ledger, so this cancellation is definitive.
                    main.post { if (!closed) completion(CheckoutAuthorityUnavailableException()) }
                    return@execute
                }
                val error = try {
                    checkNotNull(controller) { "Checkout authority is unavailable" }.runCommand { session ->
                        val authority = checkNotNull(authorityRecord) { "Checkout authority is unavailable" }
                        val scope = checkNotNull(installationDescriptor) { "Checkout installation is unavailable" }.scope
                        ledger.bindCommandAuthority(authority, scope)
                        if (pricing != null) {
                            ledger.createPricedSale(session, operationId, saleId, currency, createdAt, pricing)
                        } else {
                            ledger.createSale(session, operationId, saleId, amountMinor, currency, createdAt, items)
                        }
                    }
                    null
                } catch (failure: Exception) {
                    failure
                }
                error?.let(::latchLedgerWriteFailure)
                val next = controller?.state() ?: CheckoutAccessState.Unavailable()
                main.post {
                    if (!closed && generation.get() == ticket && (ledgerOutcomeNeedsReopen || next !is CheckoutAccessState.SignedIn)) {
                        main.removeCallbacks(expireSession)
                        val effective = if (ledgerOutcomeNeedsReopen) CheckoutAccessState.Unavailable() else next
                        publishedState = effective
                        onState(effective)
                    }
                    // The operation may already have committed. Never discard a post-access outcome
                    // because the UI generation changed while the serialized worker was running.
                    completion(error)
                }
            }
        } catch (_: java.util.concurrent.RejectedExecutionException) {
            completion(CheckoutAuthorityUnavailableException())
        }
    }

    /** Persist the cash handover intent before the Activity offers an acceptance action. */
    fun executeCashPrepare(
        ledger: CheckoutLedger,
        operationId: String,
        saleId: String,
        paymentId: String,
        tenderedMinor: Long,
        preparedAt: Long,
        expectedSaleVersion: Long,
        expectedRemainingMinor: Long,
        completion: (CheckoutLedgerCommandResult<CashPaymentIntent>?, Exception?) -> Unit,
    ) {
        if (ledgerOutcomeNeedsReopen || closed) {
            completion(null, CheckoutAuthorityUnavailableException())
            return
        }
        val ticket = generation.get()
        try {
            executor.execute {
                if (closed || ledgerOutcomeNeedsReopen) {
                    main.post { if (!closed) completion(null, CheckoutAuthorityUnavailableException()) }
                    return@execute
                }
                if (generation.get() != ticket) {
                    // Authority changed before ledger access, so this cash command did not run.
                    main.post { if (!closed) completion(null, CheckoutAuthorityUnavailableException()) }
                    return@execute
                }
                var result: CheckoutLedgerCommandResult<CashPaymentIntent>? = null
                val error = try {
                    checkNotNull(controller) { "Checkout authority is unavailable" }.runCommand { session ->
                        val authority = checkNotNull(authorityRecord) { "Checkout authority is unavailable" }
                        val scope = checkNotNull(installationDescriptor) { "Checkout installation is unavailable" }.scope
                        ledger.bindCommandAuthority(authority, scope)
                        result = ledger.prepareCashPayment(
                            session,
                            operationId,
                            saleId,
                            paymentId,
                            tenderedMinor,
                            preparedAt,
                            expectedSaleVersion,
                            expectedRemainingMinor,
                        )
                    }
                    null
                } catch (failure: Exception) {
                    failure
                }
                error?.let(::latchLedgerWriteFailure)
                val next = controller?.state() ?: CheckoutAccessState.Unavailable()
                main.post {
                    if (!closed && generation.get() == ticket && (ledgerOutcomeNeedsReopen || next !is CheckoutAccessState.SignedIn)) {
                        main.removeCallbacks(expireSession)
                        val effective = if (ledgerOutcomeNeedsReopen) CheckoutAccessState.Unavailable() else next
                        publishedState = effective
                        onState(effective)
                    }
                    // A post-access result remains authoritative even after lock or Activity recreation.
                    completion(result, error)
                }
            }
        } catch (_: java.util.concurrent.RejectedExecutionException) {
            completion(null, CheckoutAuthorityUnavailableException())
        }
    }

    /** Complete an already prepared cash intent after explicit confirmation of physical receipt. */
    fun executeCashComplete(
        ledger: CheckoutLedger,
        saleId: String,
        intent: CashPaymentIntent,
        completedAt: Long,
        expectedSaleVersion: Long,
        expectedRemainingMinor: Long,
        completion: (CheckoutLedgerCommandResult<CashPayment>?, Exception?) -> Unit,
    ) = executeCashIntentMutation(ledger, saleId, intent, completedAt,
        expectedSaleVersion, expectedRemainingMinor, complete = true, completion = completion)

    /** Cancel a prepared intent only after the cashier confirms that no cash was received. */
    fun executeCashCancel(
        ledger: CheckoutLedger,
        saleId: String,
        intent: CashPaymentIntent,
        canceledAt: Long,
        expectedSaleVersion: Long,
        completion: (CheckoutLedgerCommandResult<CashPaymentIntent>?, Exception?) -> Unit,
    ) = executeCashIntentMutation(ledger, saleId, intent, canceledAt,
        expectedSaleVersion, null, complete = false, completion = completion)

    private fun <T> executeCashIntentMutation(
        ledger: CheckoutLedger,
        saleId: String,
        intent: CashPaymentIntent,
        at: Long,
        expectedSaleVersion: Long,
        expectedRemainingMinor: Long?,
        complete: Boolean,
        completion: (CheckoutLedgerCommandResult<T>?, Exception?) -> Unit,
    ) {
        if (ledgerOutcomeNeedsReopen || closed) return completion(null, CheckoutAuthorityUnavailableException())
        val ticket = generation.get()
        try {
            executor.execute {
                if (closed || ledgerOutcomeNeedsReopen || generation.get() != ticket) {
                    main.post { if (!closed) completion(null, CheckoutAuthorityUnavailableException()) }
                    return@execute
                }
                var result: CheckoutLedgerCommandResult<T>? = null
                val error = try {
                    checkNotNull(controller) { "Checkout authority is unavailable" }.runCommand { session ->
                        val authority = checkNotNull(authorityRecord) { "Checkout authority is unavailable" }
                        val scope = checkNotNull(installationDescriptor) { "Checkout installation is unavailable" }.scope
                        ledger.bindCommandAuthority(authority, scope)
                        @Suppress("UNCHECKED_CAST")
                        result = (if (complete) {
                            ledger.completePreparedCashPayment(session, intent.completeOperationId, saleId,
                                intent.paymentId, at, expectedSaleVersion,
                                checkNotNull(expectedRemainingMinor) { "current cash balance snapshot is required" })
                        } else {
                            ledger.cancelPreparedCashPayment(session, intent.cancelOperationId, saleId,
                                intent.paymentId, at, expectedSaleVersion)
                        }) as CheckoutLedgerCommandResult<T>
                    }
                    null
                } catch (failure: Exception) { failure }
                error?.let(::latchLedgerWriteFailure)
                val next = controller?.state() ?: CheckoutAccessState.Unavailable()
                main.post {
                    if (!closed && generation.get() == ticket && (ledgerOutcomeNeedsReopen || next !is CheckoutAccessState.SignedIn)) {
                        main.removeCallbacks(expireSession)
                        val effective = if (ledgerOutcomeNeedsReopen) CheckoutAccessState.Unavailable() else next
                        publishedState = effective
                        onState(effective)
                    }
                    completion(result, error)
                }
            }
        } catch (_: java.util.concurrent.RejectedExecutionException) {
            completion(null, CheckoutAuthorityUnavailableException())
        }
    }

    /** Authorize and serialize a non-cash command; attribution remains inside the worker. */
    fun executeCapability(
        capability: CheckoutAuthorityCapability,
        operation: () -> Unit,
        completion: (Exception?) -> Unit,
    ) {
        if (ledgerOutcomeNeedsReopen) return completion(CheckoutAuthorityUnavailableException())
        if (closed) return completion(CheckoutAuthorityUnavailableException())
        val ticket = generation.get()
        executor.execute {
            if (closed || generation.get() != ticket) return@execute
            if (ledgerOutcomeNeedsReopen) {
                main.post { if (!closed) completion(CheckoutAuthorityUnavailableException()) }
                return@execute
            }
            val error = try {
                checkNotNull(controller) { "Checkout authority is unavailable" }
                    .runAuthorizedCommand(capability) { operation() }
                null
            } catch (e: Exception) {
                e
            }
            error?.let(::latchLedgerWriteFailure)
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
        publish(if (ledgerOutcomeNeedsReopen) CheckoutAccessState.Unavailable() else CheckoutAccessState.Loading, ticket)
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
        publish(if (ledgerOutcomeNeedsReopen) CheckoutAccessState.Unavailable() else CheckoutAccessState.Loading, ticket)
        executor.execute {
            val next = if (ledgerOutcomeNeedsReopen) CheckoutAccessState.Unavailable()
                else try { operation() } catch (_: Exception) { CheckoutAccessState.Unavailable() }
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
                if (ledgerOutcomeNeedsReopen) {
                    secrets.forEach { it.fill('\u0000') }
                    main.post {
                        if (!closed) completion(CheckoutStaffAdministrationResult.Failed("Checkout authority is unavailable."))
                    }
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
        val effective = if (ledgerOutcomeNeedsReopen) CheckoutAccessState.Unavailable() else next
        publishedState = effective
        main.post {
            if (!closed && generation.get() == ticket) {
                main.removeCallbacks(expireSession)
                if (effective is CheckoutAccessState.SignedIn) {
                    main.postDelayed(expireSession, SESSION_LIFETIME_MILLIS)
                }
                onState(effective)
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
        if (ledgerOutcomeNeedsReopen) return CheckoutAccessState.Unavailable()
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
