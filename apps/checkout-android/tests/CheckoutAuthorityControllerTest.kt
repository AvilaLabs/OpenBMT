package org.openbmt.checkout

import java.security.SecureRandom

object CheckoutAuthorityControllerTest {
    fun run() {
        recoveryAcknowledgementPrecedesEnrollmentAndSessionIsVolatile()
        staleStoresAndWrongCredentialsFailClosed()
        managerRecoveryRequiresReplacementCodeAcknowledgement()
        uncertainRecoveryKeepsReplacementCodeVisible()
        managerStaffAdministrationRequiresCapabilityAndInvalidatesPolicySessions()
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

    private fun managerStaffAdministrationRequiresCapabilityAndInvalidatesPolicySessions() {
        val fixture = Fixture()
        val controller = fixture.controller()
        val bootstrap = controller.prepareManagerEnrollment() as CheckoutAccessState.RecoveryCodeConfirmation
        controller.confirmRecoveryCode(bootstrap.recoveryCode.toCharArray())
        val enrolled = controller.enrollManager("Manager", "112233".toCharArray()) as CheckoutAccessState.SignInRequired
        val managerId = enrolled.staff.single().actorId
        controller.signIn(managerId, "112233".toCharArray())

        val created = controller.createStaffMember("Cashier", CheckoutAuthorityRole.CASHIER, "445566".toCharArray())
        check(created is CheckoutStaffAdministrationResult.Ready)
        check(controller.state() is CheckoutAccessState.SignInRequired)
        val directory = fixture.controller()
        directory.refresh()
        val cashierId = (directory.signIn(managerId, "112233".toCharArray()) as CheckoutAccessState.SignedIn).actor.actorId
        val staff = (directory.staffAdministration() as CheckoutStaffAdministrationResult.Ready).snapshot.staff
        val cashier = staff.single { it.actorId != managerId }
        check(cashierId == managerId)

        val cashierSessionController = fixture.controller()
        cashierSessionController.refresh()
        cashierSessionController.signIn(cashier.actorId, "445566".toCharArray())
        check(cashierSessionController.staffAdministration() is CheckoutStaffAdministrationResult.Failed)
        check((cashierSessionController.state() as CheckoutAccessState.SignedIn).actor.role == CheckoutAuthorityRole.CASHIER)

        val manager = fixture.controller()
        manager.refresh()
        manager.signIn(managerId, "112233".toCharArray())
        val renamed = manager.renameStaffMember(cashier.actorId, "Front Cashier") as CheckoutStaffAdministrationResult.Ready
        check(manager.state() is CheckoutAccessState.SignInRequired)
        manager.signIn(managerId, "112233".toCharArray())
        check((manager.staffAdministration() as CheckoutStaffAdministrationResult.Ready).snapshot.staff.first { it.actorId == cashier.actorId }.displayName == "Front Cashier")
        check(manager.changeStaffRole(cashier.actorId, CheckoutAuthorityRole.MANAGER) is CheckoutStaffAdministrationResult.Ready)
        check(manager.state() is CheckoutAccessState.SignInRequired)

        manager.signIn(managerId, "112233".toCharArray())
        check(manager.changeStaffPin(cashier.actorId, "778899".toCharArray()) is CheckoutStaffAdministrationResult.Ready)
        check(manager.state() is CheckoutAccessState.SignInRequired)
        manager.signIn(managerId, "112233".toCharArray())
        check(manager.disableStaffMember(cashier.actorId) is CheckoutStaffAdministrationResult.Ready)
        check(manager.state() is CheckoutAccessState.SignInRequired)

        val audit = CheckoutAuthorityAuditJournal(fixture.auditStorage).snapshot()
        check(audit.map { it.action }.count { it in setOf(
            CheckoutAuthorityAuditAction.STAFF_CREATE_PREPARED_WITH_RECEIPT,
            CheckoutAuthorityAuditAction.STAFF_RENAME_PREPARED_WITH_RECEIPT,
            CheckoutAuthorityAuditAction.STAFF_ROLE_PREPARED_WITH_RECEIPT,
            CheckoutAuthorityAuditAction.STAFF_PIN_PREPARED_WITH_RECEIPT,
            CheckoutAuthorityAuditAction.STAFF_DISABLE_PREPARED_WITH_RECEIPT,
        ) } == 5)
        check(audit.filter { it.action in setOf(
            CheckoutAuthorityAuditAction.STAFF_CREATED,
            CheckoutAuthorityAuditAction.STAFF_RENAMED,
            CheckoutAuthorityAuditAction.STAFF_ROLE_CHANGED,
            CheckoutAuthorityAuditAction.STAFF_PIN_CHANGED,
            CheckoutAuthorityAuditAction.STAFF_DISABLED,
        ) }.all { it.actorId == managerId && it.capabilityId == CheckoutAuthorityCapability.STAFF_MANAGE.id })
    }

    private class Fixture(var fresh: Boolean = true) {
        val storage = MemoryAuthorityStorage()
        val auditStorage = MemoryAuthorityStorage()
        private val clock = FakeClock()
        private val scope = CheckoutAuthorityScope("merchant", "store", "location", "register", "device")

        fun controller(): CheckoutAuthorityController {
            val authority = CheckoutAuthority(storage, clock, FakeRandom(), defaultIterations = 1_000)
            val commands = CheckoutAuthorityCommandService(authority, CheckoutAuthorityAuditJournal(auditStorage), clock)
            return CheckoutAuthorityController(authority, scope, CheckoutBusinessFreshnessCheck { fresh }, commands)
        }
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
