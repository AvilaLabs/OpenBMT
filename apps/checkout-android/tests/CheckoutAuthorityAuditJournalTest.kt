package org.openbmt.checkout

import java.nio.file.Files

/** Contract coverage for authenticated authority administration events and strict journal storage. */
object CheckoutAuthorityAuditJournalTest {
    fun run() {
        recordsAdminAndRecoveryEventsWithInternalAttribution()
        preparedStaffWritesAreAuditedAndUnresolvedIntentsBlockFurtherAdmin()
        operationIdsAreIdempotentAndConflictsFailClosed()
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
            CheckoutAuthorityAuditAction.STAFF_CREATE_PREPARED,
            CheckoutAuthorityAuditAction.STAFF_CREATED,
        ))
        check(events[1].actorId == draft.managerActorId && events[1].capabilityId == CheckoutAuthorityCapability.STAFF_MANAGE.id)
        check(events[2].subjectActorId == created.subjectActorId)

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
        check(unresolvedJournal.snapshot().any { it.action == CheckoutAuthorityAuditAction.STAFF_CREATE_PREPARED })
        val managerAfterWrite = authority2.authenticate(draft2.managerActorId, chars("654321"), SCOPE)
        check(authority2.listStaff(managerAfterWrite, SCOPE).size == 2) // mutation committed, journal remains explicitly unresolved.
        val reopenedService = CheckoutAuthorityCommandService(authority2, unresolvedJournal, clock)
        check(runCatching {
            reopenedService.renameStaffPrepared("rename-prepared-02", managerAfterWrite, SCOPE, draft2.managerActorId, "Other")
        }.exceptionOrNull() is CheckoutAuthorityAuditUnavailableException)
    }

    private fun operationIdsAreIdempotentAndConflictsFailClosed() {
        val authStore=MemoryStorage(); val auditStore=MemoryStorage(); val clock=TestClock()
        val authority=newAuthority(authStore,clock); val journal=CheckoutAuthorityAuditJournal(auditStore)
        val service=CheckoutAuthorityCommandService(authority,journal,clock)
        val draft=authority.prepareBootstrap()
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
}
