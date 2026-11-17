package org.openbmt.checkout

import java.nio.file.Files
import java.util.concurrent.CountDownLatch
import java.util.concurrent.atomic.AtomicReference

/** Focused JVM contract tests for local checkout identity, authentication and authorization. */
object CheckoutAuthorityTest {
    fun run() {
        correctAndWrongPinAndDurableLockout()
        exactScopeAndRoleMatrix()
        expiryAndPolicyInvalidation()
        renamePreservesStableActorId()
        disableAndManagerStaffCreation()
        recoveryRotatesOneTimeCodeAndPreservesBusinessData()
        corruptAndUnsupportedRecordsFailClosed()
        uncertainWritesFailClosed()
        uncertainBootstrapRetainsDraftForReopen()
        uncertainRecoveryKeepsPreparedCodeUsableAfterReopen()
        casPreventsStaleInstancesOverwritingState()
        duplicatePinsCannotImpersonateEnabledActors()
        loginProfileExposesOnlyNonSecretEnabledDirectory()
        nioStorageProvidesRealCompareAndSwap()
        SealedCheckoutAuthorityStorageTest.run()
        CheckoutInstallationScopeStoreTest.run()
        secretsDoNotLeakThroughStringsOrErrors()
    }

    private fun correctAndWrongPinAndDurableLockout() {
        val storage = MemoryStorage()
        val clock = TestClock()
        val authority = newAuthority(storage, clock)
        val setup = bootstrap(authority, "Owner Manager", "123456")
        check(setup.managerActorId.startsWith("actor-"))
        expectDenied { authority.authenticate(setup.managerActorId, chars("654321"), SCOPE) }
        val reopened = newAuthority(storage, clock)
        val manager = reopened.authenticate(setup.managerActorId, chars("123456"), SCOPE)
        check(reopened.authorize(manager, CheckoutAuthorityCapability.STAFF_MANAGE, SCOPE).actorId == setup.managerActorId)

        repeat(5) {
            expectDenied { reopened.authenticate(setup.managerActorId, chars("999999"), SCOPE) }
        }
        expectDenied { reopened.authenticate(setup.managerActorId, chars("123456"), SCOPE) }
        val afterRestart = newAuthority(storage, clock)
        expectDenied { afterRestart.authenticate(setup.managerActorId, chars("123456"), SCOPE) }
        clock.elapsed += 30_001
        val unlocked = afterRestart.authenticate(setup.managerActorId, chars("123456"), SCOPE)
        check(afterRestart.authorize(unlocked, CheckoutAuthorityCapability.SALES_READ, SCOPE).actorId == setup.managerActorId)
    }

    private fun exactScopeAndRoleMatrix() {
        val storage = MemoryStorage()
        val authority = newAuthority(storage, TestClock())
        val setup = bootstrap(authority, "Manager", "123456")
        val manager = authority.authenticate(setup.managerActorId, chars("123456"), SCOPE)
        check(authority.authorize(manager, CheckoutAuthorityCapability.REPORT_READ, SCOPE).role == CheckoutAuthorityRole.MANAGER)
        val cashier = authority.createStaff(manager, SCOPE, "Cashier One", CheckoutAuthorityRole.CASHIER, chars("234567"))
        val refreshedManager = authority.authenticate(setup.managerActorId, chars("123456"), SCOPE)
        val cashierSession = authority.authenticate(cashier.actorId, chars("234567"), SCOPE)
        for (capability in listOf(
            CheckoutAuthorityCapability.SALE_CREATE, CheckoutAuthorityCapability.SALE_EDIT,
            CheckoutAuthorityCapability.TENDER_BEGIN, CheckoutAuthorityCapability.RECEIPT_READ,
            CheckoutAuthorityCapability.SALES_READ, CheckoutAuthorityCapability.CASH_SESSION_OPEN,
            CheckoutAuthorityCapability.CASH_SESSION_MOVEMENT_RECORD, CheckoutAuthorityCapability.CASH_SESSION_SALE_LINK,
            CheckoutAuthorityCapability.CASH_SESSION_CLOSE, CheckoutAuthorityCapability.REFUND_REQUEST,
        )) check(authority.authorize(cashierSession, capability, SCOPE).role == CheckoutAuthorityRole.CASHIER)
        expectDenied { authority.authorize(cashierSession, CheckoutAuthorityCapability.STAFF_MANAGE, SCOPE) }
        expectDenied { authority.authorize(cashierSession, CheckoutAuthorityCapability.REPORT_READ, SCOPE) }
        expectDenied { authority.authorize(cashierSession, CheckoutAuthorityCapability.CATALOG_MANAGE, SCOPE) }
        expectDenied { authority.authorize(cashierSession, CheckoutAuthorityCapability.INVENTORY_MANAGE, SCOPE) }
        expectDenied { authority.authorize(cashierSession, CheckoutAuthorityCapability.WALLET_CONFIGURE, SCOPE) }
        check(authority.authorize(refreshedManager, CheckoutAuthorityCapability.WALLET_CONFIGURE, SCOPE).role == CheckoutAuthorityRole.MANAGER)
        expectDenied { authority.authorize(cashierSession, "wallet.sign", SCOPE) }
        expectDenied { authority.authorize(cashierSession, "payment.destination.change", SCOPE) }
        expectDenied { authority.authorize(cashierSession, "unknown.future.action", SCOPE) }
        expectDenied { authority.authorize(cashierSession, CheckoutAuthorityCapability.SALES_READ, SCOPE.copy(registerId = "other-register")) }
        val attribution = authority.authorize(refreshedManager, CheckoutAuthorityCapability.STAFF_MANAGE, SCOPE)
        check(attribution.scope == SCOPE && attribution.policyGeneration > 0)
    }

    private fun expiryAndPolicyInvalidation() {
        val clock = TestClock()
        val authority = newAuthority(MemoryStorage(), clock, lifetime = 10_000)
        val setup = bootstrap(authority, "Manager", "123456")
        val first = authority.authenticate(setup.managerActorId, chars("123456"), SCOPE)
        clock.elapsed += 10_000
        expectDenied { authority.authorize(first, CheckoutAuthorityCapability.SALES_READ, SCOPE) }

        val live = authority.authenticate(setup.managerActorId, chars("123456"), SCOPE)
        val cashier = authority.createStaff(live, SCOPE, "Cashier", CheckoutAuthorityRole.CASHIER, chars("345678"))
        expectUnavailableOrDenied { authority.authorize(live, CheckoutAuthorityCapability.STAFF_MANAGE, SCOPE) }
        val manager = authority.authenticate(setup.managerActorId, chars("123456"), SCOPE)
        val cashierSession = authority.authenticate(cashier.actorId, chars("345678"), SCOPE)
        authority.changeRole(manager, SCOPE, cashier.actorId, CheckoutAuthorityRole.MANAGER)
        expectDenied { authority.authorize(cashierSession, CheckoutAuthorityCapability.SALES_READ, SCOPE) }

        val newManager = authority.authenticate(cashier.actorId, chars("345678"), SCOPE)
        val resetTarget = authority.authenticate(setup.managerActorId, chars("123456"), SCOPE)
        authority.changePin(newManager, SCOPE, setup.managerActorId, chars("456789"))
        expectDenied { authority.authenticate(setup.managerActorId, chars("123456"), SCOPE) }
        check(authority.authenticate(setup.managerActorId, chars("456789"), SCOPE).toString() == "CheckoutAuthoritySession(redacted)")
        expectDenied { authority.authorize(resetTarget, CheckoutAuthorityCapability.SALES_READ, SCOPE) }
    }

    private fun renamePreservesStableActorId() {
        val authority = newAuthority(MemoryStorage(), TestClock())
        val setup = bootstrap(authority, "Manager", "123456")
        val initial = authority.authenticate(setup.managerActorId, chars("123456"), SCOPE)
        val cashier = authority.createStaff(initial, SCOPE, "Before Rename", CheckoutAuthorityRole.CASHIER, chars("234567"))
        val manager = authority.authenticate(setup.managerActorId, chars("123456"), SCOPE)
        val cashierSession = authority.authenticate(cashier.actorId, chars("234567"), SCOPE)
        val before = authority.authorize(cashierSession, CheckoutAuthorityCapability.SALES_READ, SCOPE)
        authority.renameStaff(manager, SCOPE, cashier.actorId, "After Rename")
        val after = authority.authorize(cashierSession, CheckoutAuthorityCapability.SALES_READ, SCOPE)
        check(before.actorId == after.actorId && before.actorId == cashier.actorId)
        check(before.displayNameAtEvent == "Before Rename" && after.displayNameAtEvent == "After Rename")
    }

    private fun disableAndManagerStaffCreation() {
        val authority = newAuthority(MemoryStorage(), TestClock())
        val setup = bootstrap(authority, "Manager", "123456")
        val firstManager = authority.authenticate(setup.managerActorId, chars("123456"), SCOPE)
        val cashier = authority.createStaff(firstManager, SCOPE, "Cashier", CheckoutAuthorityRole.CASHIER, chars("234567"))
        val manager = authority.authenticate(setup.managerActorId, chars("123456"), SCOPE)
        val second = authority.createStaff(manager, SCOPE, "Shift Manager", CheckoutAuthorityRole.MANAGER, chars("345678"))
        check(authority.listStaff(authority.authenticate(setup.managerActorId, chars("123456"), SCOPE), SCOPE)
            .any { it.actorId == second.actorId && it.role == CheckoutAuthorityRole.MANAGER })
        val managerForInvariant = authority.authenticate(setup.managerActorId, chars("123456"), SCOPE)
        expectDenied { authority.changeRole(managerForInvariant, SCOPE, setup.managerActorId, CheckoutAuthorityRole.CASHIER) }
        expectDenied { authority.disableStaff(managerForInvariant, SCOPE, setup.managerActorId) }
        val cashierSession = authority.authenticate(cashier.actorId, chars("234567"), SCOPE)
        val activeManager = authority.authenticate(setup.managerActorId, chars("123456"), SCOPE)
        authority.disableStaff(activeManager, SCOPE, cashier.actorId)
        expectDenied { authority.authorize(cashierSession, CheckoutAuthorityCapability.SALES_READ, SCOPE) }
        expectDenied { authority.authenticate(cashier.actorId, chars("234567"), SCOPE) }
    }

    private fun recoveryRotatesOneTimeCodeAndPreservesBusinessData() {
        val authStorage = MemoryStorage()
        val businessStorage = MemoryStorage()
        val clock = TestClock()
        val authority = newAuthority(authStorage, clock)
        val setup = bootstrap(authority, "Manager", "123456")
        val oldSession = authority.authenticate(setup.managerActorId, chars("123456"), SCOPE)
        businessStorage.bytes = byteArrayOf(4, 3, 2, 1)
        val beforeBusinessData = businessStorage.read()!!.copyOf()

        val preparedRecovery = authority.prepareRecovery()
        val recovered = authority.recoverManager(SCOPE, setup.recoveryCode.toCharArray(), chars("654321"), preparedRecovery)
        check(recovered.policyGeneration > 1 && recovered.recoveryCode != setup.recoveryCode)
        expectDenied { authority.authorize(oldSession, CheckoutAuthorityCapability.SALES_READ, SCOPE) }
        expectDenied { authority.authenticate(setup.managerActorId, chars("123456"), SCOPE) }
        val newSession = authority.authenticate(setup.managerActorId, chars("654321"), SCOPE)
        check(authority.authorize(newSession, CheckoutAuthorityCapability.STAFF_MANAGE, SCOPE).actorId == setup.managerActorId)
        expectDenied { authority.recoverManager(SCOPE, setup.recoveryCode.toCharArray(), chars("765432"), authority.prepareRecovery()) }
        expectDenied {
            authority.recoverManager(
                SCOPE, recovered.recoveryCode.toCharArray(), chars("765432"), preparedRecovery,
            )
        }

        val secondRecovery = authority.recoverManager(SCOPE, recovered.recoveryCode.toCharArray(), chars("765432"), authority.prepareRecovery())
        check(secondRecovery.recoveryCode != recovered.recoveryCode)
        check(businessStorage.read()!!.contentEquals(beforeBusinessData))
    }

    private fun corruptAndUnsupportedRecordsFailClosed() {
        val storage = MemoryStorage()
        val authority = newAuthority(storage, TestClock())
        val setup = bootstrap(authority, "Manager", "123456")
        val valid = storage.read()!!.copyOf()
        val corrupted = MemoryStorage().apply { bytes = valid.copyOf().also { it[0] = 0 } }
        expectUnavailable { newAuthority(corrupted, TestClock()) }
        val unsupported = MemoryStorage().apply { bytes = valid.copyOf().also { it[7] = 99 } }
        expectUnavailable { newAuthority(unsupported, TestClock()) }
        expectDenied { newAuthority(storage, TestClock()).authenticate(setup.managerActorId, chars("123456"), SCOPE.copy(deviceId = "other-device")) }
    }

    private fun uncertainWritesFailClosed() {
        val storage = MemoryStorage()
        val clock = TestClock()
        val authority = newAuthority(storage, clock)
        val setup = bootstrap(authority, "Manager", "123456")

        storage.failNextWrite = true
        expectUnavailable { authority.authenticate(setup.managerActorId, chars("123456"), SCOPE) }
        expectUnavailable { authority.authenticate(setup.managerActorId, chars("123456"), SCOPE) }
        val reopened = newAuthority(storage, clock)
        check(reopened.authenticate(setup.managerActorId, chars("123456"), SCOPE).toString() == "CheckoutAuthoritySession(redacted)")

        storage.writeThenThrowAfter = storage.successfulWrites + 1
        expectUnavailable { reopened.authenticate(setup.managerActorId, chars("000000"), SCOPE) }
        expectUnavailable { reopened.authenticate(setup.managerActorId, chars("123456"), SCOPE) }
        val afterUncertainCommit = newAuthority(storage, clock)
        repeat(4) { expectDenied { afterUncertainCommit.authenticate(setup.managerActorId, chars("000000"), SCOPE) } }
        expectDenied { afterUncertainCommit.authenticate(setup.managerActorId, chars("123456"), SCOPE) }
    }

    private fun casPreventsStaleInstancesOverwritingState() {
        val storage = MemoryStorage()
        val clock = TestClock()
        val first = newAuthority(storage, clock)
        val draft = first.prepareBootstrap()
        first.bootstrapManager(draft, SCOPE, "Manager", chars("123456"))
        val stale = newAuthority(storage, clock)

        expectDenied { first.authenticate(draft.managerActorId, chars("000000"), SCOPE) }
        expectUnavailable { stale.authenticate(draft.managerActorId, chars("000000"), SCOPE) }
        val reopened = newAuthority(storage, clock)
        repeat(4) { expectDenied { reopened.authenticate(draft.managerActorId, chars("000000"), SCOPE) } }
        expectDenied { reopened.authenticate(draft.managerActorId, chars("123456"), SCOPE) }

        clock.elapsed += 30_001
        val stalePolicy = newAuthority(storage, clock)
        val staleSession = stalePolicy.authenticate(draft.managerActorId, chars("123456"), SCOPE)
        val manager = newAuthority(storage, clock)
        val managerSession = manager.authenticate(draft.managerActorId, chars("123456"), SCOPE)
        val firstStaff = manager.createStaff(managerSession, SCOPE, "First", CheckoutAuthorityRole.CASHIER, chars("222222"))
        expectUnavailable {
            stalePolicy.authorize(staleSession, CheckoutAuthorityCapability.SALES_READ, SCOPE)
        }
        expectUnavailable {
            stalePolicy.createStaff(staleSession, SCOPE, "Stale", CheckoutAuthorityRole.CASHIER, chars("333333"))
        }
        val persisted = newAuthority(storage, clock)
        val staff = persisted.listStaff(persisted.authenticate(draft.managerActorId, chars("123456"), SCOPE), SCOPE)
        check(staff.any { it.actorId == firstStaff.actorId } && staff.none { it.displayName == "Stale" })
    }

    private fun duplicatePinsCannotImpersonateEnabledActors() {
        val storage = MemoryStorage()
        val authority = newAuthority(storage, TestClock())
        val setup = bootstrap(authority, "Manager", "123456")
        val manager = authority.authenticate(setup.managerActorId, chars("123456"), SCOPE)
        expectDenied {
            authority.createStaff(manager, SCOPE, "Shared PIN", CheckoutAuthorityRole.CASHIER, chars("123456"))
        }
        val cashier = authority.createStaff(manager, SCOPE, "Cashier", CheckoutAuthorityRole.CASHIER, chars("222222"))
        val refreshedManager = authority.authenticate(setup.managerActorId, chars("123456"), SCOPE)
        expectDenied { authority.changePin(refreshedManager, SCOPE, cashier.actorId, chars("123456")) }
        val recoveredCode = authority.prepareRecovery()
        expectDenied {
            authority.recoverManager(SCOPE, setup.recoveryCode.toCharArray(), chars("222222"), recoveredCode)
        }
        check(authority.authenticate(cashier.actorId, chars("222222"), SCOPE).let {
            authority.authorize(it, CheckoutAuthorityCapability.SALES_READ, SCOPE).actorId == cashier.actorId
        })
    }

    private fun loginProfileExposesOnlyNonSecretEnabledDirectory() {
        val storage = MemoryStorage()
        val authority = newAuthority(storage, TestClock())
        check(authority.loginProfile() == null)
        val setup = bootstrap(authority, "Manager", "123456")
        val manager = authority.authenticate(setup.managerActorId, chars("123456"), SCOPE)
        val cashier = authority.createStaff(
            manager, SCOPE, "Cashier", CheckoutAuthorityRole.CASHIER, chars("222222"),
        )
        val activeManager = authority.authenticate(setup.managerActorId, chars("123456"), SCOPE)
        authority.disableStaff(activeManager, SCOPE, cashier.actorId)
        val profile = newAuthority(storage, TestClock()).loginProfile()!!
        check(profile.scope == SCOPE)
        check(profile.enabledStaff.map { it.actorId } == listOf(setup.managerActorId))
        check(profile.toString().contains("Manager") && !profile.toString().contains("123456"))
        check(!profile.toString().contains(setup.recoveryCode))
    }

    private fun nioStorageProvidesRealCompareAndSwap() {
        val directory = Files.createTempDirectory("openbmt-authority-cas-")
        val file = directory.resolve("authority.bin")
        val lock = directory.resolve("authority.bin.lock")
        try {
            val first = NioCheckoutAuthorityStorage(file)
            val second = NioCheckoutAuthorityStorage(file)
            check(first.read() == null)
            check(first.compareAndSwap(null, byteArrayOf(1, 2, 3)))
            check(!second.compareAndSwap(null, byteArrayOf(9)))
            check(second.compareAndSwap(byteArrayOf(1, 2, 3), byteArrayOf(4, 5)))
            check(first.read()!!.contentEquals(byteArrayOf(4, 5)))
            check(!first.compareAndSwap(byteArrayOf(1, 2, 3), byteArrayOf(6)))
            val start = CountDownLatch(1)
            val failure = AtomicReference<Throwable?>(null)
            val outcomes = BooleanArray(2)
            val threads = listOf(first, second).mapIndexed { index, store ->
                Thread {
                    try {
                        start.await()
                        outcomes[index] = store.compareAndSwap(
                            byteArrayOf(4, 5), byteArrayOf((7 + index).toByte()),
                        )
                    } catch (error: Throwable) {
                        failure.compareAndSet(null, error)
                    }
                }
            }
            threads.forEach(Thread::start)
            start.countDown()
            threads.forEach(Thread::join)
            check(failure.get() == null && outcomes.count { it } == 1)
        } finally {
            Files.deleteIfExists(file)
            Files.deleteIfExists(lock)
            Files.deleteIfExists(directory)
        }
    }

    private fun uncertainRecoveryKeepsPreparedCodeUsableAfterReopen() {
        val storage = MemoryStorage()
        val clock = TestClock()
        val authority = newAuthority(storage, clock)
        val setup = bootstrap(authority, "Manager", "123456")
        val prepared = authority.prepareRecovery()
        // Recovery first persists an attempt counter, then commits the credential/code rotation.
        storage.writeThenThrowAfter = storage.successfulWrites + 2
        expectUnavailable {
            authority.recoverManager(SCOPE, setup.recoveryCode.toCharArray(), chars("654321"), prepared)
        }
        val reopened = newAuthority(storage, clock)
        check(reopened.authenticate(setup.managerActorId, chars("654321"), SCOPE).let {
            reopened.authorize(it, CheckoutAuthorityCapability.STAFF_MANAGE, SCOPE).actorId == setup.managerActorId
        })
        val nextPrepared = reopened.prepareRecovery()
        val second = reopened.recoverManager(SCOPE, prepared.recoveryCode.toCharArray(), chars("765432"), nextPrepared)
        check(second.recoveryCode == nextPrepared.recoveryCode)
    }

    private fun uncertainBootstrapRetainsDraftForReopen() {
        val storage = MemoryStorage()
        val clock = TestClock()
        val authority = newAuthority(storage, clock)
        val draft = authority.prepareBootstrap()
        storage.writeThenThrowAfter = storage.successfulWrites + 1
        expectUnavailable { authority.bootstrapManager(draft, SCOPE, "Manager", chars("123456")) }
        val reopened = newAuthority(storage, clock)
        val session = reopened.authenticate(draft.managerActorId, chars("123456"), SCOPE)
        check(reopened.authorize(session, CheckoutAuthorityCapability.STAFF_MANAGE, SCOPE).actorId == draft.managerActorId)
        val recovered = reopened.prepareRecovery()
        val result = reopened.recoverManager(SCOPE, draft.recoveryCode.toCharArray(), chars("654321"), recovered)
        check(result.recoveryCode == recovered.recoveryCode)
    }

    private fun secretsDoNotLeakThroughStringsOrErrors() {
        val authority = newAuthority(MemoryStorage(), TestClock())
        val secretPin = "908172"
        val setup = bootstrap(authority, "Manager", secretPin)
        check(!setup.toString().contains(setup.recoveryCode))
        check(!authority.toString().contains(secretPin))
        val session = authority.authenticate(setup.managerActorId, chars(secretPin), SCOPE)
        check(!session.toString().contains(secretPin))
        val denied = runCatching { authority.authenticate(setup.managerActorId, chars("111111"), SCOPE) }.exceptionOrNull()!!
        check(!denied.message.orEmpty().contains("111111"))
        check(!denied.toString().contains(setup.recoveryCode))
    }

    private fun newAuthority(
        storage: MemoryStorage,
        clock: TestClock,
        lifetime: Long = 60_000,
    ) = CheckoutAuthority(storage, clock, TEST_RANDOM, defaultIterations = 1_000, sessionLifetimeMillis = lifetime)

    private fun bootstrap(authority: CheckoutAuthority, name: String, pin: String): CheckoutAuthorityBootstrap {
        val draft = authority.prepareBootstrap()
        return authority.bootstrapManager(draft, SCOPE, name, chars(pin))
    }

    private fun chars(value: String) = value.toCharArray()

    private fun expectDenied(block: () -> Unit) {
        check(runCatching(block).exceptionOrNull() is CheckoutAuthorityDeniedException)
    }

    private fun expectUnavailable(block: () -> Unit) {
        check(runCatching(block).exceptionOrNull() is CheckoutAuthorityUnavailableException)
    }

    private fun expectUnavailableOrDenied(block: () -> Unit) {
        val error = runCatching(block).exceptionOrNull()
        check(error is CheckoutAuthorityUnavailableException || error is CheckoutAuthorityDeniedException)
    }

    private val SCOPE = CheckoutAuthorityScope("merchant-main", "store-main", "location-main", "register-main", "device-main")
    private val TEST_RANDOM = DeterministicRandom()

    private class TestClock : CheckoutAuthorityClock {
        var elapsed = 10_000L
        var wall = 1_800_000_000_000L
        override fun elapsedRealtimeMillis(): Long = elapsed
        override fun wallTimeMillis(): Long = wall
        override fun bootIdentity(): String = "test-boot-1"
    }

    private class DeterministicRandom : CheckoutAuthorityRandom {
        private var cursor = 1
        override fun bytes(count: Int): ByteArray = ByteArray(count) { index ->
            ((cursor++ * 37 + index * 13) and 0xff).toByte()
        }
    }

    private class MemoryStorage : CheckoutAuthorityStorage {
        var bytes: ByteArray? = null
        var failNextWrite = false
        var writeThenThrowAfter: Int? = null
        var successfulWrites = 0
        override fun read(): ByteArray? = bytes?.copyOf()
        override fun compareAndSwap(expectedBytes: ByteArray?, nextBytes: ByteArray): Boolean {
            if (failNextWrite) {
                failNextWrite = false
                throw IllegalStateException("simulated write failure")
            }
            val current = bytes
            if (expectedBytes == null) {
                if (current != null) return false
            } else if (current == null || !current.contentEquals(expectedBytes)) {
                return false
            }
            bytes = nextBytes.copyOf()
            successfulWrites++
            if (writeThenThrowAfter == successfulWrites) {
                writeThenThrowAfter = null
                throw IllegalStateException("simulated uncertain commit")
            }
            return true
        }
    }
}
