package org.openbmt.checkout

import java.io.File
import java.io.IOException
import java.nio.file.Files
import java.security.MessageDigest
import java.util.concurrent.CountDownLatch
import java.util.concurrent.atomic.AtomicInteger
import java.util.concurrent.atomic.AtomicReference
import javax.crypto.Mac
import javax.crypto.spec.SecretKeySpec

object CheckoutInstallationScopeStoreTest {
    fun run() {
        createsOnceAndReopensExactScope()
        missingEstablishedDescriptorFailsClosed()
        markerEstablishmentCanResumeOnlyBeforeCommit()
        legacyEstablishedMarkerCannotRegenerate()
        provisioningCompletesOnlyAfterManagerDurability()
        existingInstallAdoptionRequiresCeremonyAndPreservesBusinessFiles()
        corruptAndUnsupportedDescriptorsFailClosed()
        concurrentCreateRunsFactoryOnce()
    }

    private fun existingInstallAdoptionRequiresCeremonyAndPreservesBusinessFiles() {
        val directory = Files.createTempDirectory("checkout-install-adoption-").toFile()
        val ledgerFile = File(directory, "checkout-ledger.bin").apply { writeBytes(byteArrayOf(5, 4, 3, 2, 1)) }
        val pairingFile = File(directory, "pairing.obks").apply { writeBytes(byteArrayOf(9, 8, 7, 6)) }
        val ledgerBefore = ledgerFile.readBytes()
        val pairingBefore = pairingFile.readBytes()
        val store = CheckoutInstallationScopeStore(directory, MemoryMarker())
        try {
            val deniedPreflight = CheckoutExistingInstallAdoptionPreflight { null }
            check(runCatching { store.createAdoptionRequired(SCOPE, deniedPreflight) }
                .exceptionOrNull() is CheckoutAuthorityDeniedException)
            val wrongScopePreflight = CheckoutExistingInstallAdoptionPreflight { scope ->
                CheckoutInstallationAdoptionPreflightPermit.verified(scope.copy(registerId = "other-register"), "preflight-review-0000")
            }
            check(runCatching { store.createAdoptionRequired(SCOPE, wrongScopePreflight) }
                .exceptionOrNull() is CheckoutAuthorityDeniedException)
            val preflight = CheckoutExistingInstallAdoptionPreflight { scope ->
                CheckoutInstallationAdoptionPreflightPermit.verified(scope, "preflight-review-0001")
            }
            val required = store.createAdoptionRequired(SCOPE, preflight)
            check(required.mode == CheckoutInstallationMode.EXISTING_INSTALL_ADOPTION)
            check(required.adoptionState == CheckoutAdoptionState.REQUIRED)
            check(store.loadDescriptor() == required)
            check(ledgerFile.readBytes().contentEquals(ledgerBefore))
            check(pairingFile.readBytes().contentEquals(pairingBefore))

            val authority = CheckoutAuthority(
                NioCheckoutAuthorityStorage(directory.toPath().resolve("authority.bin")),
                TestClock(), TestRandom(), defaultIterations = 1_000,
            )
            authority.bootstrapManager(authority.prepareBootstrap(), SCOPE, "Manager", "123456".toCharArray())
            check(runCatching { store.validateAuthorityRecord(authority) }
                .exceptionOrNull() is CheckoutAuthorityUnavailableException)

            val wrongScopeCeremony = CheckoutExistingInstallAdoptionCeremony { scope ->
                CheckoutInstallationAdoptionPermit.approved(scope.copy(registerId = "other-register"), "approval-reference-0001")
            }
            check(runCatching { store.approveExistingInstallAdoption(SCOPE, wrongScopeCeremony) }
                .exceptionOrNull() is CheckoutAuthorityDeniedException)
            check(store.loadDescriptor().adoptionState == CheckoutAdoptionState.REQUIRED)

            var ceremonyCalls = 0
            val ceremony = CheckoutExistingInstallAdoptionCeremony { scope ->
                ceremonyCalls++
                CheckoutInstallationAdoptionPermit.approved(scope, "approval-reference-0002")
            }
            val approved = store.approveExistingInstallAdoption(SCOPE, ceremony)
            check(ceremonyCalls == 1)
            check(approved.adoptionState == CheckoutAdoptionState.PENDING_MANAGER_ENROLLMENT)
            check(store.loadDescriptor() == approved)
            check(store.validateAuthorityRecord(authority)?.scope == SCOPE)
            check(runCatching { store.approveExistingInstallAdoption(SCOPE, ceremony) }.isFailure)
            check(ledgerFile.readBytes().contentEquals(ledgerBefore))
            check(pairingFile.readBytes().contentEquals(pairingBefore))

            var regenerated = false
            check(runCatching {
                store.createOnFreshInstall(EVIDENCE) { regenerated = true; SCOPE }
            }.exceptionOrNull() is IOException)
            check(!regenerated)
        } finally {
            removeStoreFiles(directory)
        }
    }

    private fun legacyEstablishedMarkerCannotRegenerate() {
        val directory = Files.createTempDirectory("checkout-install-scope-legacy-marker-").toFile()
        val marker = MemoryMarker().also { it.establishLegacyWithoutPendingIntent() }
        val store = CheckoutInstallationScopeStore(directory, marker)
        try {
            var called = false
            check(runCatching { store.createOnFreshInstall(EVIDENCE) { called = true; SCOPE } }.exceptionOrNull() is IOException)
            check(!called)
        } finally {
            removeStoreFiles(directory)
        }
    }

    private fun createsOnceAndReopensExactScope() {
        val directory = Files.createTempDirectory("checkout-install-scope-").toFile()
        val marker = MemoryMarker()
        val store = CheckoutInstallationScopeStore(directory, marker)
        try {
            check(runCatching { store.load() }.exceptionOrNull() is IOException)
            val created = store.createOnFreshInstall(EVIDENCE) { SCOPE }
            check(created == SCOPE)
            check(store.load() == SCOPE)
            check(store.loadDescriptor().provisioningState == CheckoutProvisioningState.PENDING_ENROLLMENT)
            check(store.load(SCOPE) == SCOPE)
            check(runCatching { store.load(SCOPE.copy(deviceId = "other-device")) }.exceptionOrNull() is IOException)
            var called = false
            check(runCatching { store.createOnFreshInstall(EVIDENCE) { called = true; SCOPE } }.exceptionOrNull() is IOException)
            check(!called)
        } finally {
            removeStoreFiles(directory)
        }
    }

    private fun markerEstablishmentCanResumeOnlyBeforeCommit() {
        val directory = Files.createTempDirectory("checkout-install-scope-resume-").toFile()
        val marker = MemoryMarker()
        val store = CheckoutInstallationScopeStore(directory, marker)
        try {
            check(marker.establish()) // Simulates a crash after Keystore key creation.
            check(store.createOnFreshInstall(EVIDENCE) { SCOPE } == SCOPE)
            check(marker.isDescriptorCommitted())
            File(directory, "checkout-installation-scope.obis").delete()
            var called = false
            check(runCatching { store.createOnFreshInstall(EVIDENCE) { called = true; SCOPE } }.exceptionOrNull() is IOException)
            check(!called)
        } finally {
            removeStoreFiles(directory)
        }
    }

    private fun provisioningCompletesOnlyAfterManagerDurability() {
        val directory = Files.createTempDirectory("checkout-install-scope-provision-").toFile()
        val marker = MemoryMarker()
        val store = CheckoutInstallationScopeStore(directory, marker)
        val authorityPath = directory.toPath().resolve("authority.bin")
        val authorityStorage = NioCheckoutAuthorityStorage(authorityPath)
        val authority = CheckoutAuthority(authorityStorage, TestClock(), TestRandom(), defaultIterations = 1_000)
        try {
            store.createOnFreshInstall(EVIDENCE) { SCOPE }
            check(runCatching { store.markCompleted(SCOPE, authority) }.exceptionOrNull() is IOException)
            check(store.loadDescriptor().provisioningState == CheckoutProvisioningState.PENDING_ENROLLMENT)

            val draft = authority.prepareBootstrap()
            authority.bootstrapManager(draft, SCOPE, "Manager", "123456".toCharArray())
            val completed = store.markCompleted(SCOPE, authority)
            check(completed.provisioningState == CheckoutProvisioningState.COMPLETED)
            check(store.loadDescriptor().provisioningState == CheckoutProvisioningState.COMPLETED)

            val absentAuthority = CheckoutAuthority(
                NioCheckoutAuthorityStorage(directory.toPath().resolve("absent-authority.bin")),
                TestClock(), TestRandom(), defaultIterations = 1_000,
            )
            check(absentAuthority.loginProfile() == null)
            check(store.loadDescriptor().provisioningState == CheckoutProvisioningState.COMPLETED)
            check(runCatching { store.markCompleted(SCOPE, absentAuthority) }.exceptionOrNull() is IOException)
            check(runCatching { store.validateAuthorityRecord(absentAuthority) }.exceptionOrNull() is CheckoutAuthorityUnavailableException)
        } finally {
            removeStoreFiles(directory)
        }
    }

    private fun missingEstablishedDescriptorFailsClosed() {
        val directory = Files.createTempDirectory("checkout-install-scope-delete-").toFile()
        val marker = MemoryMarker()
        val store = CheckoutInstallationScopeStore(directory, marker)
        try {
            store.createOnFreshInstall(EVIDENCE) { SCOPE }
            File(directory, "checkout-installation-scope.obis").delete()
            check(runCatching { store.load() }.exceptionOrNull() is IOException)
            var called = false
            check(runCatching { store.createOnFreshInstall(EVIDENCE) { called = true; SCOPE } }.exceptionOrNull() is IOException)
            check(!called)
        } finally {
            removeStoreFiles(directory)
        }
    }

    private fun corruptAndUnsupportedDescriptorsFailClosed() {
        val directory = Files.createTempDirectory("checkout-install-scope-corrupt-").toFile()
        val marker = MemoryMarker()
        val store = CheckoutInstallationScopeStore(directory, marker)
        val file = File(directory, "checkout-installation-scope.obis")
        try {
            store.createOnFreshInstall(EVIDENCE) { SCOPE }
            val valid = file.readBytes()
            file.writeBytes(valid.copyOf().also { it[4] = 99 })
            check(runCatching { store.load() }.exceptionOrNull() is IOException)
            file.writeBytes(valid.copyOf().also { it[it.lastIndex] = (it.last().toInt() xor 1).toByte() })
            check(runCatching { store.load() }.exceptionOrNull() is IOException)
            file.writeBytes(valid + byteArrayOf(0))
            check(runCatching { store.load() }.exceptionOrNull() is IOException)
        } finally {
            removeStoreFiles(directory)
        }
    }

    private fun concurrentCreateRunsFactoryOnce() {
        val directory = Files.createTempDirectory("checkout-install-scope-race-").toFile()
        val marker = MemoryMarker()
        val stores = listOf(
            CheckoutInstallationScopeStore(directory, marker),
            CheckoutInstallationScopeStore(directory, marker),
        )
        val start = CountDownLatch(1)
        val factoryCalls = AtomicInteger()
        val successes = AtomicInteger()
        val unexpected = AtomicReference<Throwable?>(null)
        try {
            val threads = stores.map { store ->
                Thread {
                    try {
                        start.await()
                        store.createOnFreshInstall(EVIDENCE) {
                            factoryCalls.incrementAndGet()
                            SCOPE
                        }
                        successes.incrementAndGet()
                    } catch (_: IOException) {
                        // The losing create is the expected duplicate.
                    } catch (error: Throwable) {
                        unexpected.compareAndSet(null, error)
                    }
                }
            }
            threads.forEach(Thread::start)
            start.countDown()
            threads.forEach(Thread::join)
            check(unexpected.get() == null)
            check(successes.get() == 1 && factoryCalls.get() == 1)
            check(stores.first().load() == SCOPE)
        } finally {
            removeStoreFiles(directory)
        }
    }

    private class MemoryMarker : CheckoutInstallationMarker {
        private var established = false
        private var pendingCreation = false
        private var descriptorCommitted = false
        @Synchronized override fun isEstablished(): Boolean = established
        @Synchronized override fun isDescriptorCommitted(): Boolean = descriptorCommitted
        @Synchronized override fun canResumeInitialCreation(): Boolean = pendingCreation && !descriptorCommitted
        @Synchronized override fun establish(): Boolean {
            if (established) return false
            established = true
            pendingCreation = true
            return true
        }

        @Synchronized fun establishLegacyWithoutPendingIntent() {
            established = true
            pendingCreation = false
        }

        @Synchronized override fun commitDescriptor() {
            check(established)
            descriptorCommitted = true
            pendingCreation = false
        }

        override fun authenticate(bytes: ByteArray): ByteArray = mac(bytes)

        override fun verify(bytes: ByteArray, authenticator: ByteArray): Boolean =
            authenticator.size == 32 && MessageDigest.isEqual(mac(bytes), authenticator)

        private fun mac(bytes: ByteArray): ByteArray = Mac.getInstance("HmacSHA256")
            .apply { init(SecretKeySpec(TEST_KEY, "HmacSHA256")) }
            .doFinal(bytes)
    }

    private fun removeStoreFiles(directory: File) {
        directory.listFiles()?.forEach(File::delete)
        directory.delete()
    }

    private val EVIDENCE = CheckoutFreshInstallEvidence("fresh-install-token-01")
    private val SCOPE = CheckoutAuthorityScope("merchant-main", "store-main", "location-main", "register-main", "device-main")
    private val TEST_KEY = "only-a-test-marker-key".toByteArray(Charsets.UTF_8)

    private class TestClock : CheckoutAuthorityClock {
        override fun elapsedRealtimeMillis(): Long = 10_000
        override fun wallTimeMillis(): Long = 1_800_000_000_000
        override fun bootIdentity(): String = "scope-test-boot"
    }

    private class TestRandom : CheckoutAuthorityRandom {
        private var cursor = 1
        override fun bytes(count: Int): ByteArray = ByteArray(count) { ((cursor++ * 23 + it * 7) and 0xff).toByte() }
    }
}
