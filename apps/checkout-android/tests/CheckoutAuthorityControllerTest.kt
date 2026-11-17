package org.openbmt.checkout

import java.security.SecureRandom

object CheckoutAuthorityControllerTest {
    fun run() {
        recoveryAcknowledgementPrecedesEnrollmentAndSessionIsVolatile()
        staleStoresAndWrongCredentialsFailClosed()
        managerRecoveryRequiresReplacementCodeAcknowledgement()
        uncertainRecoveryKeepsReplacementCodeVisible()
    }

    private fun recoveryAcknowledgementPrecedesEnrollmentAndSessionIsVolatile() {
        val fixture = Fixture()
        val controller = fixture.controller()
        check(controller.refresh() == CheckoutAccessState.EnrollmentRequired)
        val recovery = controller.prepareManagerEnrollment() as CheckoutAccessState.RecoveryCodeConfirmation

        val wrong = recovery.recoveryCode.toCharArray().also { it[it.lastIndex] = if (it.last() == 'A') 'B' else 'A' }
        check(controller.confirmRecoveryCode(wrong) is CheckoutAccessState.RecoveryCodeConfirmation)
        check(wrong.all { it == '\u0000' })
        check(controller.state() !is CheckoutAccessState.ManagerEnrollmentRequired)

        val correct = recovery.recoveryCode.toCharArray()
        check(controller.confirmRecoveryCode(correct) is CheckoutAccessState.ManagerEnrollmentRequired)
        check(correct.all { it == '\u0000' })
        val pin = "135790".toCharArray()
        val enrolled = controller.enrollManager("First Manager", pin) as CheckoutAccessState.SignInRequired
        check(pin.all { it == '\u0000' })
        check(enrolled.staff.single().role == CheckoutAuthorityRole.MANAGER)
        check(controller.authorize(CheckoutAuthorityCapability.SALES_READ) == null)

        val signedInPin = "135790".toCharArray()
        check(controller.signIn(enrolled.staff.single().actorId, signedInPin) is CheckoutAccessState.SignedIn)
        check(signedInPin.all { it == '\u0000' })
        check(controller.authorize(CheckoutAuthorityCapability.SALES_READ) != null)
        check(controller.lock() is CheckoutAccessState.SignInRequired)
        check(controller.authorize(CheckoutAuthorityCapability.SALES_READ) == null)
    }

    private fun staleStoresAndWrongCredentialsFailClosed() {
        val fixture = Fixture(fresh = false)
        val controller = fixture.controller()
        check(controller.refresh() is CheckoutAccessState.Unavailable)
        check(controller.prepareManagerEnrollment() is CheckoutAccessState.Unavailable)

        fixture.fresh = true
        val first = fixture.controller()
        val recovery = first.prepareManagerEnrollment() as CheckoutAccessState.RecoveryCodeConfirmation
        first.confirmRecoveryCode(recovery.recoveryCode.toCharArray())
        val enrolled = first.enrollManager("Manager", "246802".toCharArray()) as CheckoutAccessState.SignInRequired
        val login = fixture.controller()
        val badPin = "000000".toCharArray()
        val result = login.signIn(enrolled.staff.single().actorId, badPin)
        check(result is CheckoutAccessState.SignInRequired && result.message == "Sign-in failed. Check staff member and PIN.")
        check(badPin.all { it == '\u0000' })
        check(login.authorize(CheckoutAuthorityCapability.SALES_READ) == null)
    }

    private fun managerRecoveryRequiresReplacementCodeAcknowledgement() {
        val fixture = Fixture()
        val controller = fixture.controller()
        val bootstrap = controller.prepareManagerEnrollment() as CheckoutAccessState.RecoveryCodeConfirmation
        controller.confirmRecoveryCode(bootstrap.recoveryCode.toCharArray())
        val enrolled = controller.enrollManager("Manager", "123456".toCharArray()) as CheckoutAccessState.SignInRequired
        val actorId = enrolled.staff.single().actorId

        controller.signIn(actorId, "123456".toCharArray())
        try { controller.runCommand { throw CheckoutAuthorityUnavailableException() } } catch (_: Exception) { }
        val unavailable = controller.state() as CheckoutAccessState.Unavailable
        check(unavailable.managerRecoveryAvailable)
        check(controller.beginManagerRecovery() is CheckoutAccessState.ManagerRecoveryRequired)
        val oldCode = bootstrap.recoveryCode.toCharArray()
        val newPin = "975310".toCharArray()
        val codeView = controller.prepareManagerRecovery(oldCode, newPin) as CheckoutAccessState.ManagerRecoveryCodeAcknowledgement
        check(oldCode.all { it == '\u0000' } && newPin.all { it == '\u0000' })
        check(codeView.toString().contains("redacted"))

        val wrongAck = codeView.nextRecoveryCode.toCharArray().also {
            it[it.lastIndex] = if (it.last() == 'a') 'b' else 'a'
        }
        check(controller.confirmNextRecoveryCode(wrongAck) is CheckoutAccessState.ManagerRecoveryCodeAcknowledgement)
        check(wrongAck.all { it == '\u0000' })
        check(controller.state() is CheckoutAccessState.ManagerRecoveryCodeAcknowledgement)

        val result = controller.confirmNextRecoveryCode(codeView.nextRecoveryCode.toCharArray()) as CheckoutAccessState.SignInRequired
        check(result.message == "Manager access was recovered. Sign in with the new PIN.")
        check(controller.signIn(actorId, "123456".toCharArray()) is CheckoutAccessState.SignInRequired)
        check(controller.signIn(actorId, "975310".toCharArray()) is CheckoutAccessState.SignedIn)
    }

    private fun uncertainRecoveryKeepsReplacementCodeVisible() {
        val fixture = Fixture()
        val controller = fixture.controller()
        val bootstrap = controller.prepareManagerEnrollment() as CheckoutAccessState.RecoveryCodeConfirmation
        controller.confirmRecoveryCode(bootstrap.recoveryCode.toCharArray())
        val enrolled = controller.enrollManager("Manager", "123456".toCharArray()) as CheckoutAccessState.SignInRequired
        val actorId = enrolled.staff.single().actorId
        controller.beginManagerRecovery()
        val recovery = controller.prepareManagerRecovery(bootstrap.recoveryCode.toCharArray(), "987654".toCharArray())
            as CheckoutAccessState.ManagerRecoveryCodeAcknowledgement

        fixture.storage.throwAfterSuccessfulWrites = 2
        val uncertain = controller.confirmNextRecoveryCode(recovery.nextRecoveryCode.toCharArray())
            as CheckoutAccessState.ManagerRecoveryCodeAcknowledgement
        check(uncertain.message?.contains("uncertain") == true)
        check(uncertain.commitUncertain)
        check(uncertain.nextRecoveryCode == recovery.nextRecoveryCode)
        check(!uncertain.toString().contains(recovery.nextRecoveryCode))
        val retry = controller.confirmNextRecoveryCode(recovery.nextRecoveryCode.toCharArray())
        check(retry is CheckoutAccessState.ManagerRecoveryCodeAcknowledgement && retry.commitUncertain)

        val reopened = fixture.controller()
        val readRequired = reopened.refresh() as CheckoutAccessState.SignInRequired
        check(reopened.signIn(actorId, "987654".toCharArray()) is CheckoutAccessState.SignedIn)
        check(readRequired.staff.any { it.actorId == actorId })
    }

    private class Fixture(var fresh: Boolean = true) {
        val storage = MemoryAuthorityStorage()
        private val clock = FakeClock()
        private val scope = CheckoutAuthorityScope("merchant", "store", "location", "register", "device")

        fun controller(): CheckoutAuthorityController = CheckoutAuthorityController(
            CheckoutAuthority(storage, clock, FakeRandom(), defaultIterations = 1_000),
            scope,
            CheckoutBusinessFreshnessCheck { fresh },
        )
    }

    private class MemoryAuthorityStorage : CheckoutAuthorityStorage {
        private var value: ByteArray? = null
        var throwAfterSuccessfulWrites: Int = 0
        override fun read(): ByteArray? = value?.clone()
        override fun compareAndSwap(expectedBytes: ByteArray?, nextBytes: ByteArray): Boolean {
            val current = value
            val matches = if (current == null || expectedBytes == null) current == null && expectedBytes == null
            else current.contentEquals(expectedBytes)
            if (!matches) return false
            value = nextBytes.clone()
            if (throwAfterSuccessfulWrites > 0) {
                throwAfterSuccessfulWrites--
                if (throwAfterSuccessfulWrites == 0) throw IllegalStateException("uncertain test write")
            }
            return true
        }
    }

    private class FakeClock : CheckoutAuthorityClock {
        override fun elapsedRealtimeMillis(): Long = 1_000L
        override fun wallTimeMillis(): Long = 1_700_000_000_000L
        override fun bootIdentity(): String = "boot-test"
    }

    private class FakeRandom : CheckoutAuthorityRandom {
        private val secure = SecureRandom()
        override fun bytes(count: Int): ByteArray = ByteArray(count).also(secure::nextBytes)
    }
}
