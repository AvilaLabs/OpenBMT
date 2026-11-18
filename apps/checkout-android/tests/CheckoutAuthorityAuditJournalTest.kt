package org.openbmt.checkout

import java.io.ByteArrayOutputStream
import java.io.DataOutputStream
import java.nio.file.Files
import java.security.MessageDigest
import java.util.Collections
import java.util.concurrent.CountDownLatch
import java.util.concurrent.atomic.AtomicInteger

/** Contract coverage for authenticated authority administration events and strict journal storage. */
object CheckoutAuthorityAuditJournalTest {
    fun run() {
        recordsAdminAndRecoveryEventsWithInternalAttribution()
        preparedStaffWritesAreAuditedAndUnresolvedIntentsBlockFurtherAdmin()
        operationIdsAreIdempotentAndConflictsFailClosed()
        legacyV1ReservedPrefixesRemainOpaque()
        unresolvedStaffWritesReconcileOnlyFromDurableEvidence()
        viewsAndStringsDoNotExposeSecretsOrMutableState()
        corruptAndUncertainJournalWritesFailClosed()
        nioJournalReopensAtomically()
    }

    private fun recordsAdminAndRecoveryEventsWithInternalAttribution() {
        val authStore=MemoryStorage(); val auditStore=MemoryStorage(); val clock=TestClock()
        val authority=newAuthority(authStore,clock); val journal=CheckoutAuthorityAuditJournal(auditStore)
        val service=CheckoutAuthorityCommandService(authority,journal,clock)
        val draft=authority.prepareBootstrap()
        val boot=service.bootstrap("bootstrap-op-0001",draft,SCOPE,"Root Manager",chars("123456"))
        check(boot.action==CheckoutAuthorityAuditAction.BOOTSTRAP && boot.actorId==null && boot.subjectActorId==draft.managerActorId)
        val failedPin=chars("999999")
        check(runCatching { service.signIn(draft.managerActorId,failedPin,SCOPE) }.exceptionOrNull() is CheckoutAuthorityDeniedException)
        check(failedPin.all { it == '\u0000' } && journal.snapshot().size==1)
        val manager=service.signIn(draft.managerActorId,chars("123456"),SCOPE)
        val created=service.createStaff("create-op-00001",manager,SCOPE,"Cashier A",CheckoutAuthorityRole.CASHIER,chars("234567"))
        check(created.action==CheckoutAuthorityAuditAction.STAFF_CREATED && created.subjectRoleAfter==CheckoutAuthorityRole.CASHIER)
        val renamedBy=service.signIn(draft.managerActorId,chars("123456"),SCOPE)
        val renamed=service.renameStaff("rename-op-0001",renamedBy,SCOPE,created.subjectActorId!!,"Cashier B")
        check(renamed.subjectDisplayNameBefore=="Cashier A" && renamed.subjectDisplayNameAfter=="Cashier B")
        val roleBy=service.signIn(draft.managerActorId,chars("123456"),SCOPE)
        val role=service.changeRole("role-op-000001",roleBy,SCOPE,created.subjectActorId,CheckoutAuthorityRole.MANAGER)
        check(role.subjectRoleBefore==CheckoutAuthorityRole.CASHIER && role.subjectRoleAfter==CheckoutAuthorityRole.MANAGER)
        val pinBy=service.signIn(draft.managerActorId,chars("123456"),SCOPE)
        val pin=service.changePin("pin-op-000001",pinBy,SCOPE,created.subjectActorId,chars("345678"))
        check(pin.action==CheckoutAuthorityAuditAction.STAFF_PIN_CHANGED && pin.subjectActorId==created.subjectActorId)
        check(pin.toString().contains("redacted"))
        val disableBy=service.signIn(draft.managerActorId,chars("123456"),SCOPE)
        val disabled=service.disableStaff("disable-op-0001",disableBy,SCOPE,created.subjectActorId)
        check(disabled.action==CheckoutAuthorityAuditAction.STAFF_DISABLED)
        val recoveryDraft=authority.prepareRecovery()
        val recovered=service.recoverManager("recover-op-0001",SCOPE,draft.recoveryCode.toCharArray(),chars("456789"),recoveryDraft)
        check(recovered.action==CheckoutAuthorityAuditAction.MANAGER_RECOVERED && recovered.policyGeneration>1)
        val locked=service.lockForLifecycle("lifecycle-op-01",SCOPE)
        check(locked.action==CheckoutAuthorityAuditAction.LIFECYCLE_LOCKED && locked.actorId==null)
        val events=journal.snapshot()
        check(events.map{it.sequence}==(1L..8L).toList())
        check(events.map{it.action}==listOf(
            CheckoutAuthorityAuditAction.BOOTSTRAP,CheckoutAuthorityAuditAction.STAFF_CREATED,
            CheckoutAuthorityAuditAction.STAFF_RENAMED,CheckoutAuthorityAuditAction.STAFF_ROLE_CHANGED,
            CheckoutAuthorityAuditAction.STAFF_PIN_CHANGED,CheckoutAuthorityAuditAction.STAFF_DISABLED,
            CheckoutAuthorityAuditAction.MANAGER_RECOVERED,CheckoutAuthorityAuditAction.LIFECYCLE_LOCKED))
        check(events[1].actorId==draft.managerActorId && events[1].actorDisplayName=="Root Manager")
        check(events.none{it.capabilityId.isBlank() || it.scope!=SCOPE})
    }

    private fun preparedStaffWritesAreAuditedAndUnresolvedIntentsBlockFurtherAdmin() {
        val auth = MemoryStorage(); val audit = MemoryStorage(); val clock = TestClock()
        val authority = newAuthority(auth, clock)
        val journal = CheckoutAuthorityAuditJournal(audit)
        val service = CheckoutAuthorityCommandService(authority, journal, clock)
        val draft = authority.prepareBootstrap()
        service.bootstrap("bootstrap-prepared-01", draft, SCOPE, "Manager", chars("123456"))
        val manager = service.signIn(draft.managerActorId, chars("123456"), SCOPE)
        val created = service.createStaffPrepared(
            "create-prepared-01", manager, SCOPE, "Cashier", CheckoutAuthorityRole.CASHIER, chars("234567"),
        )
        val events = journal.snapshot()
        check(events.map { it.action } == listOf(
            CheckoutAuthorityAuditAction.BOOTSTRAP,
            CheckoutAuthorityAuditAction.STAFF_CREATE_PREPARED_WITH_RECEIPT,
            CheckoutAuthorityAuditAction.STAFF_CREATED,
        ))
        check(events[1].actorId == draft.managerActorId && events[1].capabilityId == CheckoutAuthorityCapability.STAFF_MANAGE.id)
        check(events[2].subjectActorId == created.subjectActorId)
        val replayManager = authority.authenticate(draft.managerActorId, chars("123456"), SCOPE)
        val replayPin = chars("999999")
        val replayed = service.createStaffPrepared(
            "create-prepared-01", replayManager, SCOPE, "Cashier", CheckoutAuthorityRole.CASHIER, replayPin,
        )
        check(replayed.sequence == created.sequence) {
            "staff replay returned sequence ${replayed.sequence}, expected ${created.sequence}"
        }
        check(authority.listStaff(replayManager, SCOPE).size == 2) {
            "staff replay changed the authority roster"
        }
        check(replayPin.all { it == '\u0000' })
        check(runCatching {
            service.createStaffPrepared("create-prepared-01", replayManager, SCOPE, "Different", CheckoutAuthorityRole.CASHIER, chars("888888"))
        }.exceptionOrNull() is CheckoutAuthorityAuditOperationConflictException)
        val audit2 = MemoryStorage(); val authority2Storage = MemoryStorage()
        val authority2 = newAuthority(authority2Storage, clock)
        val journal2 = CheckoutAuthorityAuditJournal(audit2)
        val service2 = CheckoutAuthorityCommandService(authority2, journal2, clock)
        val draft2 = authority2.prepareBootstrap()
        service2.bootstrap("bootstrap-prepared-02", draft2, SCOPE, "Manager", chars("654321"))
        val manager2 = service2.signIn(draft2.managerActorId, chars("654321"), SCOPE)
        audit2.permitWritesBeforeReject = 1 // persist prepare; fail every attempt to append the commit.
        check(runCatching {
            service2.createStaffPrepared("create-prepared-02", manager2, SCOPE, "Cashier", CheckoutAuthorityRole.CASHIER, chars("345678"))
        }.exceptionOrNull() is CheckoutAuthorityAuditUnavailableException)
        audit2.permitWritesBeforeReject = null
        val unresolvedJournal = CheckoutAuthorityAuditJournal(audit2)
        val unresolvedPrepare = unresolvedJournal.snapshot().single { it.action == CheckoutAuthorityAuditAction.STAFF_CREATE_PREPARED_WITH_RECEIPT }
        val managerAfterWrite = authority2.authenticate(draft2.managerActorId, chars("654321"), SCOPE)
        check(authority2.listStaff(managerAfterWrite, SCOPE).size == 2) // mutation committed, journal remains explicitly unresolved.
        val reopenedService = CheckoutAuthorityCommandService(authority2, unresolvedJournal, clock)
        check(runCatching {
            reopenedService.renameStaffPrepared("rename-prepared-02", managerAfterWrite, SCOPE, draft2.managerActorId, "Other")
        }.exceptionOrNull() is CheckoutAuthorityAuditUnavailableException)
        val recoveryController = CheckoutAuthorityController(
            authority2, SCOPE, CheckoutBusinessFreshnessCheck { false }, reopenedService,
        )
        recoveryController.refresh()
        recoveryController.signIn(draft2.managerActorId, chars("654321"))
        val recoveredDirectory = recoveryController.staffAdministration() as CheckoutStaffAdministrationResult.Ready
        check(recoveredDirectory.snapshot.message?.contains("reconciled") == true)
        val reconciled = unresolvedJournal.snapshot()
        check(reconciled.map { it.operationId }.takeLast(2) == listOf("p-create-prepared-02", "c-create-prepared-02"))
        check(reconciled.last().action == CheckoutAuthorityAuditAction.STAFF_CREATED)
        check(reconciled.last().actorId == draft2.managerActorId && reconciled.last().subjectActorId == unresolvedPrepare.subjectActorId)
    }

    private fun unresolvedStaffWritesReconcileOnlyFromDurableEvidence() {
        val clock = TestClock()
        val auth = MemoryStorage(); val audit = MemoryStorage()
        val authority = newAuthority(auth, clock); val journal = CheckoutAuthorityAuditJournal(audit)
        val service = CheckoutAuthorityCommandService(authority, journal, clock)
        val draft = authority.prepareBootstrap()
        service.bootstrap("bootstrap-prestate-01", draft, SCOPE, "Manager", chars("123456"))
        val manager = service.signIn(draft.managerActorId, chars("123456"), SCOPE)
        val auditBefore = audit.read()!!.copyOf()
        auth.permitWritesBeforeReject = 0
        check(runCatching {
            service.createStaffPrepared("create-prestate-01", manager, SCOPE, "Not Saved", CheckoutAuthorityRole.CASHIER, chars("234567"))
        }.exceptionOrNull() is CheckoutAuthorityAuditUnavailableException)
        auth.permitWritesBeforeReject = null
        val reopenedAuthority = newAuthority(auth, clock)
        val reopenedJournal = CheckoutAuthorityAuditJournal(audit)
        val reopenedService = CheckoutAuthorityCommandService(reopenedAuthority, reopenedJournal, clock)
        val reopenedManager = reopenedService.signIn(draft.managerActorId, chars("123456"), SCOPE)
        check(reopenedService.reconcileStaffIntents(reopenedManager, SCOPE) == 1)
        check(reopenedJournal.snapshot().last().action == CheckoutAuthorityAuditAction.STAFF_OPERATION_RECONCILED_NO_CHANGE)
        check(reopenedAuthority.listStaff(reopenedManager, SCOPE).size == 1)
        check(!audit.read()!!.contentEquals(auditBefore))

        // A lost audit prefix cannot erase evidence of an authority-side durable receipt.
        val committedAuth = MemoryStorage(); val committedAudit = MemoryStorage()
        val committedAuthority = newAuthority(committedAuth, clock)
        val committedJournal = CheckoutAuthorityAuditJournal(committedAudit)
        val committedService = CheckoutAuthorityCommandService(committedAuthority, committedJournal, clock)
        val committedDraft = committedAuthority.prepareBootstrap()
        committedService.bootstrap("bootstrap-rollback-01", committedDraft, SCOPE, "Manager", chars("345678"))
        val committedManager = committedService.signIn(committedDraft.managerActorId, chars("345678"), SCOPE)
        val prefix = committedAudit.read()!!.copyOf()
        committedAudit.permitWritesBeforeReject = 1
        check(runCatching {
            committedService.createStaffPrepared("create-rollback-01", committedManager, SCOPE, "Saved", CheckoutAuthorityRole.CASHIER, chars("456789"))
        }.exceptionOrNull() is CheckoutAuthorityAuditUnavailableException)
        committedAudit.permitWritesBeforeReject = null
        committedAudit.bytes = prefix
        val rollbackService = CheckoutAuthorityCommandService(newAuthority(committedAuth, clock), CheckoutAuthorityAuditJournal(committedAudit), clock)
        val rollbackManager = rollbackService.signIn(committedDraft.managerActorId, chars("345678"), SCOPE)
        check(runCatching { rollbackService.reconcileStaffIntents(rollbackManager, SCOPE) }
            .exceptionOrNull() is CheckoutAuthorityAuditUnavailableException)

        val rollbackAuth = MemoryStorage(); val rollbackAudit = MemoryStorage()
        val rollbackAuthorityInitial = newAuthority(rollbackAuth, clock)
        val rollbackJournalInitial = CheckoutAuthorityAuditJournal(rollbackAudit)
        val rollbackServiceInitial = CheckoutAuthorityCommandService(rollbackAuthorityInitial, rollbackJournalInitial, clock)
        val rollbackDraft = rollbackAuthorityInitial.prepareBootstrap()
        rollbackServiceInitial.bootstrap("bootstrap-auth-rollback", rollbackDraft, SCOPE, "Manager", chars("345678"))
        val authorityBeforeStaffWrite = rollbackAuth.read()!!.copyOf()
        val rollbackManagerInitial = rollbackServiceInitial.signIn(rollbackDraft.managerActorId, chars("345678"), SCOPE)
        rollbackServiceInitial.createStaffPrepared("create-authority-rollback", rollbackManagerInitial, SCOPE, "Keep", CheckoutAuthorityRole.CASHIER, chars("567890"))
        rollbackAuth.bytes = authorityBeforeStaffWrite
        val authorityRollbackService = CheckoutAuthorityCommandService(newAuthority(rollbackAuth, clock), CheckoutAuthorityAuditJournal(rollbackAudit), clock)
        val authorityRollbackManager = authorityRollbackService.signIn(rollbackDraft.managerActorId, chars("345678"), SCOPE)
        check(runCatching { authorityRollbackService.reconcileStaffIntents(authorityRollbackManager, SCOPE) }
            .exceptionOrNull() is CheckoutAuthorityAuditUnavailableException)

        preparedJournalRejectsConcurrentUnresolvedIntents(clock)
        liveStaffOperationCannotBeClosedByAnotherService(clock)
        pinCommitReconcilesWithoutAuditingPinMaterial(clock)
    }

    private fun liveStaffOperationCannotBeClosedByAnotherService(clock: TestClock) {
        val authorityStorage = MemoryStorage(); val audit = MemoryStorage()
        val authority = newAuthority(authorityStorage, clock)
        val writer = CheckoutAuthorityCommandService(authority, CheckoutAuthorityAuditJournal(audit), clock)
        val draft = authority.prepareBootstrap()
        writer.bootstrap("bootstrap-live-owner", draft, SCOPE, "Manager", chars("135790"))
        val manager = writer.signIn(draft.managerActorId, chars("135790"), SCOPE)
        val attribution = authority.authorize(manager, CheckoutAuthorityCapability.STAFF_MANAGE, SCOPE)
        val operationId = "live-owner-0001"
        val intent = auditPrepare(attribution, operationId, "actor-live-owner-00000000000000000001", "Pending")
            .let { CheckoutAuthorityAuditEvent(it.sequence, it.operationId, it.action, it.actorId, it.actorDisplayName,
                it.actorRole, it.capabilityId, it.policyGeneration, it.scope, it.occurredAtMillis,
                it.subjectActorId, it.subjectDisplayNameBefore, it.subjectDisplayNameAfter,
                it.subjectRoleBefore, it.subjectRoleAfter, 2, CheckoutAuthorityStaffOperationOwnership.processEpoch) }
        check(CheckoutAuthorityStaffOperationOwnership.claim(operationId))
        try {
            CheckoutAuthorityAuditJournal(audit).appendStaffPrepare(intent)
            val recoveryService = CheckoutAuthorityCommandService(authority, CheckoutAuthorityAuditJournal(audit), clock)
            val recoveryManager = recoveryService.signIn(draft.managerActorId, chars("135790"), SCOPE)
            check(runCatching { recoveryService.reconcileStaffIntents(recoveryManager, SCOPE) }
                .exceptionOrNull() is CheckoutAuthorityAuditReconciliationRequiredException)
            check(CheckoutAuthorityAuditJournal(audit).snapshot().none { it.operationId == "n-$operationId" })
        } finally {
            CheckoutAuthorityStaffOperationOwnership.release(operationId)
        }
        val restartedService = CheckoutAuthorityCommandService(authority, CheckoutAuthorityAuditJournal(audit), clock)
        val restartedManager = restartedService.signIn(draft.managerActorId, chars("135790"), SCOPE)
        check(restartedService.reconcileStaffIntents(restartedManager, SCOPE) == 1)
        check(CheckoutAuthorityAuditJournal(audit).snapshot().last().operationId == "n-$operationId")
    }

    private fun preparedJournalRejectsConcurrentUnresolvedIntents(clock: TestClock) {
        val authorityStorage = MemoryStorage(); val authority = newAuthority(authorityStorage, clock)
        val draft = authority.prepareBootstrap()
        val actor = authority.bootstrapManager(draft, SCOPE, "Manager", chars("123456")).managerActorId
        val manager = authority.authenticate(actor, chars("123456"), SCOPE)
        val attribution = authority.authorize(manager, CheckoutAuthorityCapability.STAFF_MANAGE, SCOPE)
        val racingStorage = BarrierStorage(null)
        val firstJournal = CheckoutAuthorityAuditJournal(racingStorage)
        val secondJournal = CheckoutAuthorityAuditJournal(racingStorage)
        val first = auditPrepare(attribution, "race-prep-0001", "actor-11111111111111111111111111111111", "Cashier A")
        val second = auditPrepare(attribution, "race-prep-0002", "actor-22222222222222222222222222222222", "Cashier B")
        racingStorage.arm()
        val results = Collections.synchronizedList(mutableListOf<Result<CheckoutAuthorityAuditEvent>>())
        val outcomes = listOf(
            Thread { results += runCatching { firstJournal.appendStaffPrepare(first) } },
            Thread { results += runCatching { secondJournal.appendStaffPrepare(second) } },
        )
        outcomes.forEach { it.start() }; outcomes.forEach { it.join() }
        check(results.count { it.isSuccess } == 1)
        check(results.count { it.exceptionOrNull() is CheckoutAuthorityAuditReconciliationRequiredException } == 1)
        check(CheckoutAuthorityAuditJournal(racingStorage).snapshot().count { it.action == CheckoutAuthorityAuditAction.STAFF_CREATE_PREPARED_WITH_RECEIPT } == 1)
    }

    private fun pinCommitReconcilesWithoutAuditingPinMaterial(clock: TestClock) {
        val auth = MemoryStorage(); val audit = MemoryStorage()
        val authority = newAuthority(auth, clock); val journal = CheckoutAuthorityAuditJournal(audit)
        val service = CheckoutAuthorityCommandService(authority, journal, clock)
        val draft = authority.prepareBootstrap()
        service.bootstrap("bootstrap-pin-recover", draft, SCOPE, "Manager", chars("123456"))
        var manager = service.signIn(draft.managerActorId, chars("123456"), SCOPE)
        val cashier = service.createStaffPrepared("create-pin-target-01", manager, SCOPE, "Cashier", CheckoutAuthorityRole.CASHIER, chars("234567"))
        manager = service.signIn(draft.managerActorId, chars("123456"), SCOPE)
        audit.permitWritesBeforeReject = 1
        check(runCatching {
            service.changePinPrepared("change-pin-target-01", manager, SCOPE, cashier.subjectActorId!!, chars("778899"))
        }.exceptionOrNull() is CheckoutAuthorityAuditUnavailableException)
        audit.permitWritesBeforeReject = null
        val interveningManager = authority.authenticate(draft.managerActorId, chars("123456"), SCOPE)
        authority.renameStaff(interveningManager, SCOPE, cashier.subjectActorId!!, "Cashier Updated")
        val bytes = audit.read()!!
        check(!bytes.containsAscii("778899"))
        check(CheckoutAuthorityAuditEvent::class.java.declaredFields.none {
            it.name.contains("pin", ignoreCase = true) || it.name.contains("verifier", ignoreCase = true) ||
                it.name.contains("salt", ignoreCase = true) || it.type == ByteArray::class.java
        })
        val reopenedAuthority = newAuthority(auth, clock)
        val reopenedService = CheckoutAuthorityCommandService(reopenedAuthority, CheckoutAuthorityAuditJournal(audit), clock)
        val reopenedManager = reopenedService.signIn(draft.managerActorId, chars("123456"), SCOPE)
        check(reopenedService.reconcileStaffIntents(reopenedManager, SCOPE) == 1)
        val cashierActorId = requireNotNull(cashier.subjectActorId)
        reopenedService.signIn(cashierActorId, chars("778899"), SCOPE)
        check(runCatching { reopenedService.signIn(cashierActorId, chars("234567"), SCOPE) }
            .exceptionOrNull() is CheckoutAuthorityDeniedException)
        check(CheckoutAuthorityAuditJournal(audit).snapshot().last().action == CheckoutAuthorityAuditAction.STAFF_PIN_CHANGED)
    }

    private fun auditPrepare(actor: ActorAttribution, operationId: String, subject: String, name: String) =
        CheckoutAuthorityAuditEvent(0, "p-$operationId", CheckoutAuthorityAuditAction.STAFF_CREATE_PREPARED_WITH_RECEIPT,
            actor.actorId, actor.displayNameAtEvent, actor.role, actor.capabilityId, actor.policyGeneration, actor.scope,
            actor.eventTimeMillis, subject, null, name, null, CheckoutAuthorityRole.CASHIER,
            2, CheckoutAuthorityStaffOperationOwnership.processEpoch)

    private fun ByteArray.containsAscii(value: String): Boolean {
        val needle = value.toByteArray(Charsets.US_ASCII)
        return indices.any { start -> start + needle.size <= size && needle.indices.all { this[start + it] == needle[it] } }
    }

    private fun operationIdsAreIdempotentAndConflictsFailClosed() {
        val authStore=MemoryStorage(); val auditStore=MemoryStorage(); val clock=TestClock()
        val authority=newAuthority(authStore,clock); val journal=CheckoutAuthorityAuditJournal(auditStore)
        val service=CheckoutAuthorityCommandService(authority,journal,clock)
        val draft=authority.prepareBootstrap()
        check(runCatching {
            service.bootstrap("p-reserved-bootstrap", draft, SCOPE, "Manager", chars("123456"))
        }.exceptionOrNull() is IllegalArgumentException)
        check(authStore.read() == null && auditStore.read() == null) {
            "reserved journal operation ID reached the durable mutation"
        }
        val first=service.bootstrap("bootstrap-op-0002",draft,SCOPE,"Manager",chars("123456"))
        val replayBootstrapPin=chars("777777")
        val duplicate=service.bootstrap("bootstrap-op-0002",draft,SCOPE,"Manager",replayBootstrapPin)
        check(first.sequence==duplicate.sequence && journal.snapshot().size==1)
        check(replayBootstrapPin.all { it == '\u0000' })
        check(runCatching { service.bootstrap("bootstrap-op-0002",draft,SCOPE,"Different Manager",chars("888888")) }
            .exceptionOrNull() is CheckoutAuthorityAuditOperationConflictException)
        val session=service.signIn(draft.managerActorId,chars("123456"),SCOPE)
        val created=service.createStaff("create-op-00002",session,SCOPE,"Cashier",CheckoutAuthorityRole.CASHIER,chars("234567"))
        // A retried command returns its recorded event and does not create a second staff record.
        val replayPin=chars("999999")
        val replay=service.createStaff("create-op-00002",session,SCOPE,"Cashier",CheckoutAuthorityRole.CASHIER,replayPin)
        check(created.sequence==replay.sequence && journal.snapshot().size==2)
        check(replayPin.all { it == '\u0000' })
        check(runCatching { service.disableStaff("create-op-00002",session,SCOPE,created.subjectActorId!!) }
            .exceptionOrNull() is CheckoutAuthorityAuditOperationConflictException)
        val cashier=service.signIn(created.subjectActorId!!,chars("234567"),SCOPE)
        check(runCatching { service.auditSnapshot(cashier,SCOPE) }.exceptionOrNull() is CheckoutAuthorityDeniedException)
        val manager=service.signIn(draft.managerActorId,chars("123456"),SCOPE)
        check(service.auditSnapshot(manager,SCOPE).size==2)
    }

    private fun legacyV1ReservedPrefixesRemainOpaque() {
        val storage = MemoryStorage()
        storage.bytes = legacyV1BootstrapWithReservedPrefix()
        val journal = CheckoutAuthorityAuditJournal(storage)
        check(journal.snapshot().single().operationId == "p-legacy-op0001")
        val next = CheckoutAuthorityAuditEvent(0, "bootstrap-next-v2", CheckoutAuthorityAuditAction.BOOTSTRAP,
            null, null, null, "authority.bootstrap", 0, SCOPE, 2,
            "actor-legacy-next", null, "Next Manager", null, CheckoutAuthorityRole.MANAGER)
        journal.append(next)
        check(journal.snapshot().map { it.operationId } == listOf("p-legacy-op0001", "bootstrap-next-v2"))

        val authorityStorage = MemoryStorage(); val clock = TestClock()
        val authority = newAuthority(authorityStorage, clock)
        val draft = authority.prepareBootstrap()
        authority.bootstrapManager(draft, SCOPE, "Manager", chars("123456"))
        val manager = authority.authenticate(draft.managerActorId, chars("123456"), SCOPE)
        val before = authorityStorage.read()!!.copyOf()
        val collisionStorage = MemoryStorage().apply {
            bytes = legacyV1BootstrapWithReservedPrefix("c-legacy-collision-01")
        }
        val service = CheckoutAuthorityCommandService(authority, CheckoutAuthorityAuditJournal(collisionStorage), clock)
        check(runCatching {
            service.createStaffPrepared("legacy-collision-01", manager, SCOPE, "Cashier",
                CheckoutAuthorityRole.CASHIER, chars("234567"))
        }.exceptionOrNull() is CheckoutAuthorityAuditOperationConflictException)
        check(authorityStorage.read()!!.contentEquals(before))
        check(authority.staffOperationReceipts(manager, SCOPE).isEmpty())
    }

    private fun legacyV1BootstrapWithReservedPrefix(operationId: String = "p-legacy-op0001"): ByteArray {
        val payloadBytes = ByteArrayOutputStream()
        DataOutputStream(payloadBytes).use { out ->
            out.writeInt(0x4F42414A); out.writeInt(1); out.writeInt(1)
            out.writeLong(1); writeLegacyText(out, operationId); out.writeByte(CheckoutAuthorityAuditAction.BOOTSTRAP.ordinal)
            out.writeBoolean(false); out.writeBoolean(false); out.writeByte(-1)
            writeLegacyText(out, "authority.bootstrap"); out.writeLong(0)
            listOf(SCOPE.merchantId, SCOPE.storeId, SCOPE.locationId, SCOPE.registerId, SCOPE.deviceId).forEach { writeLegacyText(out, it) }
            out.writeLong(1); out.writeBoolean(true); writeLegacyText(out, "actor-legacy-manager")
            out.writeBoolean(false); out.writeBoolean(true); writeLegacyText(out, "Legacy Manager")
            out.writeByte(-1); out.writeByte(CheckoutAuthorityRole.MANAGER.ordinal)
        }
        val payload = payloadBytes.toByteArray()
        return payload + MessageDigest.getInstance("SHA-256").digest(payload)
    }

    private fun writeLegacyText(out: DataOutputStream, value: String) {
        val bytes = value.toByteArray(Charsets.UTF_8)
        out.writeShort(bytes.size); out.write(bytes)
    }

    private fun viewsAndStringsDoNotExposeSecretsOrMutableState() {
        val auth=MemoryStorage(); val audit=MemoryStorage(); val clock=TestClock()
        val authority=newAuthority(auth,clock); val journal=CheckoutAuthorityAuditJournal(audit)
        val service=CheckoutAuthorityCommandService(authority,journal,clock)
        val draft=authority.prepareBootstrap(); service.bootstrap("bootstrap-op-0003",draft,SCOPE,"Private Name",chars("918273"))
        val view=journal.snapshot()
        check(runCatching { (view as MutableList<CheckoutAuthorityAuditEvent>).clear() }.exceptionOrNull() is UnsupportedOperationException)
        check(!view.toString().contains("918273") && !view.toString().contains(draft.recoveryCode))
        check(!view.first().toString().contains("Private Name"))
    }

    private fun corruptAndUncertainJournalWritesFailClosed() {
        val auth=MemoryStorage(); val audit=MemoryStorage(); val clock=TestClock()
        val authority=newAuthority(auth,clock); val journal=CheckoutAuthorityAuditJournal(audit)
        val service=CheckoutAuthorityCommandService(authority,journal,clock)
        val draft=authority.prepareBootstrap(); service.bootstrap("bootstrap-op-0004",draft,SCOPE,"Manager",chars("123456"))
        val valid=audit.read()!!
        val corrupt=MemoryStorage().apply{bytes=valid.copyOf().also{it[it.lastIndex-3]=(it[it.lastIndex-3].toInt() xor 0x40).toByte()}}
        check(runCatching{CheckoutAuthorityAuditJournal(corrupt).snapshot()}.exceptionOrNull() is CheckoutAuthorityAuditUnavailableException)

        val auth2=MemoryStorage(); val uncertain=MemoryStorage(); val authority2=newAuthority(auth2,clock)
        val journal2=CheckoutAuthorityAuditJournal(uncertain); val service2=CheckoutAuthorityCommandService(authority2,journal2,clock)
        val draft2=authority2.prepareBootstrap(); uncertain.writeThenThrowAfterCommit=true
        check(runCatching{service2.bootstrap("bootstrap-op-0005",draft2,SCOPE,"Manager",chars("654321"))}.exceptionOrNull() is CheckoutAuthorityAuditUnavailableException)
        check(runCatching{service2.lockForLifecycle("lifecycle-op-02",SCOPE)}.exceptionOrNull() is CheckoutAuthorityAuditUnavailableException)
        val reopened=CheckoutAuthorityAuditJournal(uncertain)
        check(reopened.findOperation("bootstrap-op-0005")?.action==CheckoutAuthorityAuditAction.BOOTSTRAP)
        val retry=CheckoutAuthorityCommandService(authority2,reopened,clock)
        check(retry.bootstrap("bootstrap-op-0005",draft2,SCOPE,"Manager",chars("000000")).action==CheckoutAuthorityAuditAction.BOOTSTRAP)
        check(reopened.snapshot().size==1)
    }

    private fun nioJournalReopensAtomically() {
        val dir=Files.createTempDirectory("openbmt-authority-audit-"); val file=dir.resolve("audit.bin")
        try {
            val auth=MemoryStorage(); val clock=TestClock(); val authority=newAuthority(auth,clock)
            val storage=NioCheckoutAuthorityStorage(file); val journal=CheckoutAuthorityAuditJournal(storage)
            val service=CheckoutAuthorityCommandService(authority,journal,clock); val draft=authority.prepareBootstrap()
            service.bootstrap("bootstrap-op-0006",draft,SCOPE,"Manager",chars("123456"))
            check(CheckoutAuthorityAuditJournal(NioCheckoutAuthorityStorage(file)).snapshot().size==1)
        } finally {
            Files.deleteIfExists(file.resolveSibling("audit.bin.lock")); Files.deleteIfExists(file); Files.deleteIfExists(dir)
        }
    }

    private fun newAuthority(storage:MemoryStorage,clock:TestClock)=CheckoutAuthority(storage,clock,Random(),defaultIterations=1_000)
    private fun chars(s:String)=s.toCharArray()
    private val SCOPE=CheckoutAuthorityScope("merchant-main","store-main","location-main","register-main","device-main")
    private class TestClock:CheckoutAuthorityClock { var elapsed=10000L; var wall=1800000000000L
        override fun elapsedRealtimeMillis()=elapsed; override fun wallTimeMillis()=wall; override fun bootIdentity()="audit-test-boot" }
    private class Random:CheckoutAuthorityRandom { private var n=0; override fun bytes(count:Int)=ByteArray(count){(++n).toByte()} }
    private class MemoryStorage:CheckoutAuthorityStorage { var bytes:ByteArray?=null; var writeThenThrowAfterCommit=false
        var permitWritesBeforeReject:Int?=null
        override fun read()=bytes?.copyOf()
        override fun compareAndSwap(expectedBytes:ByteArray?,nextBytes:ByteArray):Boolean {
            val current=bytes; if(expectedBytes==null){if(current!=null)return false}else if(current==null||!current.contentEquals(expectedBytes))return false
            val permitted = permitWritesBeforeReject
            if (permitted != null) {
                if (permitted == 0) return false
                permitWritesBeforeReject = permitted - 1
            }
            bytes=nextBytes.copyOf(); if(writeThenThrowAfterCommit){writeThenThrowAfterCommit=false;throw IllegalStateException("uncertain commit")}; return true
        }
    }
    private class BarrierStorage(initial: ByteArray?) : CheckoutAuthorityStorage {
        @Volatile private var bytes: ByteArray? = initial?.copyOf()
        private var barrier: CountDownLatch? = null
        private val compareCalls = AtomicInteger()
        fun arm() { compareCalls.set(0); barrier = CountDownLatch(2) }
        override fun read(): ByteArray? = synchronized(this) { bytes?.copyOf() }
        override fun compareAndSwap(expectedBytes: ByteArray?, nextBytes: ByteArray): Boolean {
            val currentBarrier = barrier
            if (currentBarrier != null && compareCalls.incrementAndGet() <= 2) {
                currentBarrier.countDown()
                currentBarrier.await()
                if (compareCalls.get() >= 2) barrier = null
            }
            return synchronized(this) {
                val current = bytes
                val matches = if (current == null || expectedBytes == null) current == null && expectedBytes == null
                    else current.contentEquals(expectedBytes)
                if (!matches) false else { bytes = nextBytes.copyOf(); true }
            }
        }
    }
}
