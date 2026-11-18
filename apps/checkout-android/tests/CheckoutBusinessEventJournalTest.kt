package org.openbmt.checkout

import java.util.concurrent.CountDownLatch
import java.util.concurrent.atomic.AtomicReference
import java.security.MessageDigest

/** JVM contracts for the prepare/commit/reconcile protocol; domain stores remain untouched. */
object CheckoutBusinessEventJournalTest {
    fun run() {
        bindsAuthorizedSessionAndUsesExplicitCommitStates()
        retriesRequireReconciliationAndConflictingBindingsAreRejected()
        rejectsExpiredDisabledAndStaleSessions()
        rejectsNearPrefixUnknownAndWrongScopeTypes()
        uncertainPrepareIsDiscoverableAndCasSerializesWriters()
        tamperingAndBoundsFailClosed()
        snapshotsAreImmutableAndRedacted()
    }

    private fun bindsAuthorizedSessionAndUsesExplicitCommitStates() {
        val fixture = fixture()
        val journal = journal(MemoryStorage(), fixture)
        val digest = sha("safe canonical sale command")
        val prepared = journal.prepare("sale-op-00001", "sale.created", fixture.session,
            "sale-42", "merchant-main/store-main/sale-42", digest)
        val authorized = fixture.authority.authorize(fixture.session, CheckoutAuthorityCapability.SALE_CREATE, SCOPE)
        check(prepared.disposition == CheckoutBusinessPrepareDisposition.READY_TO_EXECUTE)
        check(prepared.event.outcome == CheckoutBusinessEventOutcome.PREPARED)
        check(prepared.event.attribution == authorized && prepared.event.attribution.capabilityId == "sale.create")
        check(prepared.event.attribution.scope == SCOPE && prepared.event.targetIdentity == "sale-42")
        check(prepared.event.businessIdentity == "merchant-main/store-main/sale-42")
        check(prepared.event.commandDigest.contentEquals(digest))

        fixture.clock.wall++
        val committed = journal.commit("sale-op-00001", "sale_saved", "ledger-record-42")
        check(committed.outcome == CheckoutBusinessEventOutcome.COMMITTED)
        check(committed.businessCommitIdentity == "ledger-record-42")
        check(committed.attribution == prepared.event.attribution && committed.operationId == prepared.event.operationId)
        check(journal.commit("sale-op-00001", "sale_saved", "ledger-record-42").sequence == committed.sequence)
        check(runCatching { journal.abort("sale-op-00001", "not_saved") }.exceptionOrNull() is CheckoutBusinessJournalTransitionException)
        val history = journal.history("sale-op-00001")
        check(history.map { it.outcome } == listOf(CheckoutBusinessEventOutcome.PREPARED, CheckoutBusinessEventOutcome.COMMITTED))
        check(history.last().previousHash.contentEquals(history.first().eventHash))
    }

    private fun retriesRequireReconciliationAndConflictingBindingsAreRejected() {
        val fixture = fixture(); val store = MemoryStorage(); val journal = journal(store, fixture)
        val digest = sha("refund:case-5:2000:sats")
        val first = journal.prepare("refund-op-0001", "refund.requested", fixture.session, "refund-5", "sale-5", digest)
        check(first.disposition == CheckoutBusinessPrepareDisposition.READY_TO_EXECUTE)
        fixture.clock.wall++
        val retry = journal.prepare("refund-op-0001", "refund.requested", fixture.session, "refund-5", "sale-5", digest)
        check(retry.disposition == CheckoutBusinessPrepareDisposition.EXISTING_RECONCILE_FIRST)
        check(retry.event.sequence == first.event.sequence)
        check(runCatching {
            journal.prepare("refund-op-0001", "refund.requested", fixture.session, "refund-5", "sale-5", sha("refund:case-5:9000:sats"))
        }.exceptionOrNull() is CheckoutBusinessOperationConflictException)

        val unknown = journal.requireReconciliation("refund-op-0001", "store_write_unknown")
        check(unknown.outcome == CheckoutBusinessEventOutcome.RECONCILE_REQUIRED)
        check(journal.requireReconciliation("refund-op-0001", "store_write_unknown").sequence == unknown.sequence)
        val resolved = journal.commit("refund-op-0001", "refund_saved", "refund-record-5")
        check(resolved.outcome == CheckoutBusinessEventOutcome.COMMITTED)
        check(journal.history("refund-op-0001").map { it.outcome } == listOf(
            CheckoutBusinessEventOutcome.PREPARED, CheckoutBusinessEventOutcome.RECONCILE_REQUIRED,
            CheckoutBusinessEventOutcome.COMMITTED,
        ))

        val next = journal.prepare("refund-op-0002", "refund.requested", fixture.session,
            "refund-6", "sale-5", sha("refund:case-6:1000:sats"))
        check(next.disposition == CheckoutBusinessPrepareDisposition.READY_TO_EXECUTE)
        val aborted = journal.abort("refund-op-0002", "owner_cancelled_before_write")
        check(aborted.outcome == CheckoutBusinessEventOutcome.ABORTED && aborted.businessCommitIdentity == null)
        check(runCatching { journal.commit("refund-op-0002", "done", "refund-record-6") }
            .exceptionOrNull() is CheckoutBusinessJournalTransitionException)
    }

    private fun rejectsExpiredDisabledAndStaleSessions() {
        val expired = fixture(sessionLifetime = 1_000); val expiredJournal = journal(MemoryStorage(), expired)
        expired.clock.elapsed += 1_001
        check(runCatching {
            expiredJournal.prepare("sale-op-expired", "sale.created", expired.session, "sale-e", "store-main/sale-e", sha("expired"))
        }.exceptionOrNull() is CheckoutAuthorityDeniedException)
        check(expiredJournal.snapshot().isEmpty())

        val disabled = fixture(); val disabledJournal = journal(MemoryStorage(), disabled)
        val cashier = disabled.authority.createStaff(disabled.session, SCOPE, "Cashier", CheckoutAuthorityRole.CASHIER, chars("234567"))
        val managerSession = disabled.authority.authenticate(disabled.managerId, chars("123456"), SCOPE)
        val cashierSession = disabled.authority.authenticate(cashier.actorId, chars("234567"), SCOPE)
        disabled.authority.disableStaff(managerSession, SCOPE, cashier.actorId)
        check(runCatching {
            disabledJournal.prepare("sale-op-disabled", "sale.created", cashierSession, "sale-d", "store-main/sale-d", sha("disabled"))
        }.exceptionOrNull() is CheckoutAuthorityDeniedException)
        check(disabledJournal.snapshot().isEmpty())

        val stale = fixture(); val staleJournal = journal(MemoryStorage(), stale)
        val secondAuthority = newAuthority(stale.authStore, stale.clock, random = TestRandom(100))
        val secondManager = secondAuthority.authenticate(stale.managerId, chars("123456"), SCOPE)
        secondAuthority.createStaff(secondManager, SCOPE, "Other Cashier", CheckoutAuthorityRole.CASHIER, chars("345678"))
        val staleFailure = runCatching {
            staleJournal.prepare("sale-op-stale", "sale.created", stale.session, "sale-s", "store-main/sale-s", sha("stale"))
        }.exceptionOrNull()
        check(staleFailure is CheckoutAuthorityUnavailableException || staleFailure is CheckoutAuthorityDeniedException)
        check(staleJournal.snapshot().isEmpty())
    }

    private fun rejectsNearPrefixUnknownAndWrongScopeTypes() {
        val fixture = fixture(); val store = MemoryStorage(); val journal = journal(store, fixture)
        for (unknown in listOf("sale.created.extra", "sale.create", "refund.requested.evil", "unknown.event")) {
            check(runCatching {
                journal.prepare("unknown-op-${unknown.hashCode().toUInt()}", unknown, fixture.session,
                    "target-1", "business-1", sha(unknown))
            }.exceptionOrNull() is CheckoutBusinessJournalAuthorizationException)
        }
        check(journal.snapshot().isEmpty())

        journal.prepare("sale-op-scope", "sale.created", fixture.session, "sale-s", "store-main/sale-s", sha("scope"))
        val validBytes = store.read()!!
        val wrongScopeJournal = CheckoutBusinessEventJournal(
            MemoryStorage().apply { bytes = validBytes }, fixture.authority,
            SCOPE.copy(deviceId = "other-device"), fixture.clock,
        )
        check(runCatching { wrongScopeJournal.history("sale-op-scope") }
            .exceptionOrNull() is CheckoutBusinessJournalUnavailableException)

        val unknownTypeStore = MemoryStorage().apply { bytes = bytesWithUnknownEventType(validBytes) }
        check(runCatching { journal(unknownTypeStore, fixture).history("sale-op-scope") }
            .exceptionOrNull() is CheckoutBusinessJournalUnavailableException)
    }

    private fun uncertainPrepareIsDiscoverableAndCasSerializesWriters() {
        val fixture = fixture(); val uncertain = MemoryStorage(); val journal = journal(uncertain, fixture)
        uncertain.writeThenThrowAfterCommit = true
        check(runCatching {
            journal.prepare("catalog-op-0001", "catalog.item_created", fixture.session, "item-7", "catalog-main", sha("item-7:v1"))
        }.exceptionOrNull() is CheckoutBusinessJournalUnavailableException)
        val reopened = journal(uncertain, fixture)
        val existing = reopened.prepare("catalog-op-0001", "catalog.item_created", fixture.session, "item-7", "catalog-main", sha("item-7:v1"))
        check(existing.disposition == CheckoutBusinessPrepareDisposition.EXISTING_RECONCILE_FIRST)
        check(existing.event.outcome == CheckoutBusinessEventOutcome.PREPARED)

        val concurrent = MemoryStorage()
        val journals = listOf(journal(concurrent, fixture), journal(concurrent, fixture))
        val start = CountDownLatch(1); val error = AtomicReference<Throwable?>(null)
        val threads = journals.mapIndexed { index, j -> Thread {
            try {
                start.await()
                j.prepare("inventory-op-000$index", "inventory.stock_adjusted", fixture.session,
                    "stock-${index + 1}", "location-main/item-1", sha("adjust:$index"))
            } catch (t: Throwable) { error.compareAndSet(null, t) }
        } }
        threads.forEach { it.start() }; start.countDown(); threads.forEach { it.join() }
        check(error.get() == null && journal(concurrent, fixture).snapshot().size == 2)
    }

    private fun tamperingAndBoundsFailClosed() {
        val fixture = fixture(); val store = MemoryStorage(); val journal = journal(store, fixture)
        journal.prepare("inventory-op-0001", "inventory.stock_adjusted", fixture.session, "stock-1", "item-1", sha("adjust:one"))
        val valid = store.read()!!
        val corrupt = MemoryStorage().apply { bytes = valid.copyOf().also { it[20] = (it[20].toInt() xor 0x01).toByte() } }
        check(runCatching { journal(corrupt, fixture).history("inventory-op-0001") }
            .exceptionOrNull() is CheckoutBusinessJournalUnavailableException)
        val future = MemoryStorage().apply { bytes = valid.copyOf().also { it[7] = 99 } }
        check(runCatching { journal(future, fixture).snapshot() }.exceptionOrNull() is CheckoutBusinessJournalUnavailableException)
        val oversized = MemoryStorage().apply { bytes = ByteArray(1024 * 1024 + 1) }
        check(runCatching { journal(oversized, fixture).snapshot() }.exceptionOrNull() is CheckoutBusinessJournalUnavailableException)
    }

    private fun snapshotsAreImmutableAndRedacted() {
        val fixture = fixture(); val journal = journal(MemoryStorage(), fixture)
        val sensitive = sha("secret value omitted; digest only")
        journal.prepare("sale-op-00002", "sale.created", fixture.session, "sale-99", "store-main/sale-99", sensitive)
        val events = journal.history("sale-op-00002")
        check(runCatching { (events as MutableList<CheckoutBusinessJournalEvent>).clear() }.exceptionOrNull() is UnsupportedOperationException)
        check(!events.toString().contains("secret") && !events.toString().contains("sale-op-00002"))
        val digest = events.single().commandDigest; digest[0] = (digest[0].toInt() xor 0xff).toByte()
        check(events.single().commandDigest.contentEquals(sensitive))
        check(events.single().toString().contains("redacted"))
    }

    private data class Fixture(
        val authority: CheckoutAuthority,
        val session: CheckoutAuthoritySession,
        val clock: TestClock,
        val authStore: MemoryStorage,
        val managerId: String,
    )
    private fun fixture(sessionLifetime: Long = 60_000): Fixture {
        val clock = TestClock(); val store = MemoryStorage(); val authority = newAuthority(store, clock, sessionLifetime)
        val draft = authority.prepareBootstrap()
        authority.bootstrapManager(draft, SCOPE, "Journal Manager", chars("123456"))
        val session = authority.authenticate(draft.managerActorId, chars("123456"), SCOPE)
        return Fixture(authority, session, clock, store, draft.managerActorId)
    }
    private fun newAuthority(
        store: MemoryStorage,
        clock: TestClock,
        lifetime: Long = 60_000,
        random: TestRandom = TestRandom(),
    ) = CheckoutAuthority(store, clock, random, defaultIterations = 1_000, sessionLifetimeMillis = lifetime)
    private fun journal(store: MemoryStorage, fixture: Fixture) =
        CheckoutBusinessEventJournal(store, fixture.authority, SCOPE, fixture.clock)
    private fun chars(value: String) = value.toCharArray()
    private fun sha(value: String) = java.security.MessageDigest.getInstance("SHA-256").digest(value.toByteArray(Charsets.UTF_8))
    private fun bytesWithUnknownEventType(source: ByteArray): ByteArray {
        val result = source.copyOf()
        val payloadSize = java.nio.ByteBuffer.wrap(result, 12, 4).int
        val payloadStart = 16
        val payload = result.copyOfRange(payloadStart, payloadStart + payloadSize)
        val known = "sale.created".toByteArray(Charsets.UTF_8)
        val unknown = "sale.creatEd".toByteArray(Charsets.UTF_8)
        val start = (0..payload.size - known.size).first { offset ->
            known.indices.all { payload[offset + it] == known[it] }
        }
        unknown.copyInto(payload, start)
        payload.copyInto(result, payloadStart)
        val previousOffset = payloadStart + payloadSize
        val previous = result.copyOfRange(previousOffset, previousOffset + 32)
        val hash = MessageDigest.getInstance("SHA-256").digest(previous + payload)
        hash.copyInto(result, previousOffset + 32)
        return result
    }
    private val SCOPE = CheckoutAuthorityScope("merchant-main", "store-main", "location-main", "register-main", "device-main")
    private class TestClock : CheckoutAuthorityClock {
        var elapsed = 10_000L; var wall = 1_800_000_000_000L
        override fun elapsedRealtimeMillis() = elapsed
        override fun wallTimeMillis() = wall
        override fun bootIdentity() = "business-journal-test-boot"
    }
    private class TestRandom(start: Int = 0) : CheckoutAuthorityRandom {
        private var value = start
        override fun bytes(count: Int) = ByteArray(count) { (++value).toByte() }
    }
    private class MemoryStorage : CheckoutAuthorityStorage {
        var bytes: ByteArray? = null; var writeThenThrowAfterCommit = false
        @Synchronized override fun read() = bytes?.copyOf()
        @Synchronized override fun compareAndSwap(expectedBytes: ByteArray?, nextBytes: ByteArray): Boolean {
            val current = bytes
            if (expectedBytes == null) { if (current != null) return false }
            else if (current == null || !current.contentEquals(expectedBytes)) return false
            bytes = nextBytes.copyOf()
            if (writeThenThrowAfterCommit) { writeThenThrowAfterCommit = false; throw IllegalStateException("uncertain commit") }
            return true
        }
    }
}
