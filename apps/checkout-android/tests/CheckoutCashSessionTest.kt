package org.openbmt.checkout

import java.io.ByteArrayOutputStream
import java.io.DataOutputStream

/** Coordinator-owned JVM contract cases for durable manual cash sessions. */
object CheckoutCashSessionTest {
    fun run() {
        val authorityContext = CashAuthorityFixture()
        val cashStorage = MemoryStorage()
        val ledgerStorage = MemoryStorage()
        var ledger = CheckoutLedger(ledgerStorage)
        ledger.createSale("sale-cash", 1300, "USD", 1)
        val payment = ledger.recordCashPayment("sale-cash", "payment-cash", 2000, 2)
        check(payment.amountMinor == 1300L && payment.tenderedMinor == 2000L && payment.changeMinor == 700L)

        var sessions = CashCommands(cashStorage, "drawer-a", authorityContext)
        val opened = sessions.openSession("open-op", "session-a", "drawer-a", "staff-a", 5000, 10)
        check(opened.expectedCashMinor == 5000L && opened.status == CashSessionStatus.OPEN)
        check(sessions.openSession("open-op", "session-a", "drawer-a", "staff-a", 5000, 10) == opened)
        check(runCatching { sessions.openSession("open-op", "session-a", "drawer-a", "staff-a", 5001, 10) }.isFailure)
        check(runCatching { sessions.openSession("open-other", "session-b", "drawer-a", "staff-b", 0, 11) }.isFailure)

        val beforeForgedCall = cashStorage.read()!!.copyOf()
        check(runCatching {
            sessions.store.openSession(CheckoutAuthoritySession(), "forged-open", "forged-session", 0, 11)
        }.exceptionOrNull() is CheckoutAuthorityDeniedException)
        check(beforeForgedCall.contentEquals(cashStorage.read()))
        val foreignAuthority = CashAuthorityFixture(CheckoutAuthorityScope(
            "merchant-other", "store-other", "location-other", "register-other", "device-other"))
        check(runCatching {
            sessions.store.openSession(foreignAuthority.commandSession, "foreign-open", "foreign-session", 0, 11)
        }.exceptionOrNull() is CheckoutAuthorityDeniedException)
        check(beforeForgedCall.contentEquals(cashStorage.read()))
        val unwiredStorage = MemoryStorage()
        @Suppress("DEPRECATION")
        val unwiredStore = CheckoutCashSessionStore(unwiredStorage)
        @Suppress("DEPRECATION")
        val unwiredAttempt = runCatching { unwiredStore.openSession("no-auth", "no-auth-session", "main-drawer", "fake", 0, 12) }
        check(unwiredAttempt.exceptionOrNull() is CheckoutAuthorityUnavailableException)
        check(unwiredStorage.read() == null)

        sessions.recordMovement("session-a", "in-op", CashMovementKind.CASH_IN, 250,
            "Change fund", "staff-a", 12)
        sessions.recordMovement("session-a", "out-op", CashMovementKind.CASH_OUT, 100,
            "Supplies", "staff-b", 13)
        sessions.linkCashSale("session-a", "sale-op", "sale-cash", "payment-cash", "staff-a", 14, ledger)
        val expected = sessions.getSession("session-a")
        check(expected.expectedCashMinor == 6450L) // opening + 250 - 100 + net 1300; change is not counted twice
        check(expected.sales.single().changeMinor == 700L)
        check(expected.sales.single().staffId == "Test Manager")
        check((expected.sales.single().actor as CashActorAttribution.Authenticated).value.actorId == authorityContext.managerActorId)
        check(runCatching { sessions.linkCashSale("session-a", "sale-op", "sale-cash", "payment-cash", "staff-a", 15, ledger) }.isFailure)

        val closed = sessions.closeSession("session-a", "close-op", 6400, "manager", 20)
        check(closed.status == CashSessionStatus.CLOSED)
        val closeout = checkNotNull(closed.closeout)
        check(closeout.countedMinor == 6400L && closeout.expectedMinor == 6450L &&
            closeout.discrepancyMinor == -50L && closeout.staffId == "Test Manager")
        check(closeout.actor is CashActorAttribution.Authenticated)
        check(sessions.closeSession("session-a", "close-op", 6400, "manager", 20) == closed)
        check(sessions.recordMovement("session-a", "in-op", CashMovementKind.CASH_IN, 250,
            "Change fund", "staff-a", 12) == closed) // successful retry remains replayable after close
        check(sessions.linkCashSale("session-a", "sale-op", "sale-cash", "payment-cash", "staff-a", 14, ledger) == closed)
        check(runCatching { sessions.closeSession("session-a", "another-close", 7000, "manager", 21) }.isFailure)
        check(runCatching { sessions.recordMovement("session-a", "in-op", CashMovementKind.CASH_IN, 251,
            "Change fund", "staff-a", 12) }.isFailure) // changed payload still rejected
        check(runCatching { sessions.recordMovement("session-a", "late-move", CashMovementKind.CASH_IN, 1, "Late", "staff-a", 22) }.isFailure)
        check(runCatching { sessions.linkCashSale("session-a", "late-sale", "sale-cash", "payment-cash", "staff-a", 22, ledger) }.isFailure)
        val exposed = sessions.getSession("session-a").movements as MutableList<CashMovement>
        check(runCatching { exposed.clear() }.isFailure)

        sessions = CashCommands(cashStorage, "drawer-a", authorityContext)
        check(sessions.getSession("session-a") == closed)
        check(sessions.recordMovement("session-a", "in-op", CashMovementKind.CASH_IN, 250,
            "Change fund", "staff-a", 12) == closed)
        check(sessions.linkCashSale("session-a", "sale-op", "sale-cash", "payment-cash", "staff-a", 14, ledger) == closed)
        check(sessions.closeSession("session-a", "close-op", 6400, "manager", 20).closeout == closed.closeout)
        val nextSession = sessions.openSession("open-next", "session-next", "drawer-a", "staff-next", 0, 30)
        check(nextSession.status == CashSessionStatus.OPEN)
        check(runCatching { sessions.linkCashSale("session-next", "sale-second-link", "sale-cash", "payment-cash", "staff-next", 31, ledger) }.isFailure)
        check(sessions.getSession("session-next").sales.isEmpty()) // payment IDs are global across sessions

        // A pre-commit failure leaves the old durable image; a poisoned instance requires reopen.
        val before = MemoryStorage().also { it.failBeforeCommit = true }
        val stale = CashCommands(before, "drawer-before", authorityContext)
        check(runCatching { stale.openSession("open-before", "before", "drawer-before", "staff", 0, 1) }.isFailure)
        check(runCatching { stale.sessions() }.isFailure)
        var reopened = CashCommands(before, "drawer-before", authorityContext)
        check(reopened.sessions().isEmpty())
        reopened.openSession("open-before", "before", "drawer-before", "staff", 0, 1)

        // A post-commit failure is uncertain to the caller but replay on a reopened store is safe.
        val after = MemoryStorage().also { it.failAfterCommit = true }
        val uncertain = CashCommands(after, "drawer-after", authorityContext)
        check(runCatching { uncertain.openSession("open-after", "after", "drawer-after", "staff", 0, 1) }.isFailure)
        check(runCatching { uncertain.sessions() }.isFailure)
        reopened = CashCommands(after, "drawer-after", authorityContext)
        check(reopened.openSession("open-after", "after", "drawer-after", "staff", 0, 1).sessionId == "after")

        val overflowStorage = MemoryStorage()
        val overflow = CashCommands(overflowStorage, "overflow-drawer", authorityContext)
        overflow.openSession("overflow-open", "overflow", "overflow-drawer", "staff", Long.MAX_VALUE, 1)
        check(runCatching { overflow.recordMovement("overflow", "overflow-in", CashMovementKind.CASH_IN, 1, "Too much", "staff", 2) }.isFailure)
        check(overflow.getSession("overflow").expectedCashMinor == Long.MAX_VALUE)
        check(runCatching { CashCommands(MemoryStorage().also { it.bytes = ByteArray(3) }, "bad-drawer", authorityContext) }.isFailure)

        // Field boundaries must remain distinct even when user text contains the old delimiter.
        val collisionStore = CashCommands(MemoryStorage(), "collision-drawer", authorityContext)
        collisionStore.openSession("collision-open", "collision", "collision-drawer", "staff", 0, 1)
        collisionStore.recordMovement("collision", "collision-op", CashMovementKind.CASH_IN, 1, "a|b", "c", 2)
        check(runCatching {
            collisionStore.recordMovement("collision", "collision-op", CashMovementKind.CASH_IN, 1, "a", "b|c", 2)
        }.isFailure)
        check(collisionStore.getSession("collision").movements.single().reason == "a|b")
        check(runCatching { collisionStore.recordMovement("collision", "control", CashMovementKind.CASH_IN, 1, "bad\nreason", "staff", 3) }.isFailure)

        // Maximum individually valid identifiers/text still fit operation replay storage and codec.
        val maxFieldStorage = MemoryStorage()
        val maxField = "x".repeat(256)
        val maxFieldStore = CashCommands(maxFieldStorage, maxField, authorityContext)
        maxFieldStore.openSession(maxField, maxField, maxField, maxField, 0, 1)
        maxFieldStore.recordMovement(maxField, "y".repeat(256), CashMovementKind.CASH_IN, 1,
            maxField, maxField, 2)
        val maxFieldReloaded = CashCommands(maxFieldStorage, maxField, authorityContext).getSession(maxField)
        check(maxFieldReloaded.movements.single().reason == maxField && maxFieldReloaded.movements.single().staffId == "Test Manager")

        // v2 did not store the operator on a sale link; preserve it as explicitly unknown.
        val v2Storage = MemoryStorage().also { it.bytes = legacyV2LinkedCash() }
        val v2Store = CashCommands(v2Storage, "drawer-v2", authorityContext)
        check(v2Store.getSession("legacy-v2").sales.single().staffId == null)
        val wrongDrawerStore = CheckoutCashSessionStore(v2Storage, authorityContext.authority,
            authorityContext.scope, "different-drawer")
        val beforeWrongDrawerCommand = v2Storage.read()!!.copyOf()
        check(runCatching {
            wrongDrawerStore.closeSession(authorityContext.commandSession, "legacy-v2", "wrong-drawer-close", 100, 149)
        }.isFailure)
        check(beforeWrongDrawerCommand.contentEquals(v2Storage.read()))
        val v2Ledger = CheckoutLedger(MemoryStorage())
        v2Ledger.createSale("new-cash", 50, "USD", 150)
        v2Ledger.recordCashPayment("new-cash", "new-payment", 50, 151)
        v2Store.linkCashSale("legacy-v2", "new-link-op", "new-cash", "new-payment", "new-staff", 152, v2Ledger)
        val v3Reloaded = CashCommands(v2Storage, "drawer-v2", authorityContext)
        val recoveredLinks = v3Reloaded.getSession("legacy-v2").sales.associateBy { it.paymentId }
        check(recoveredLinks.getValue("legacy-payment").staffId == null)
        check(recoveredLinks.getValue("new-payment").staffId == "Test Manager")
        val replayed = v3Reloaded.linkCashSale("legacy-v2", "new-link-op", "new-cash", "new-payment", "new-staff", 152, v2Ledger)
        check(replayed.sales.associateBy { it.paymentId }.getValue("new-payment").staffId == "Test Manager")
        v3Reloaded.closeSession("legacy-v2", "migrated-close", 150, "caller", 200)
        val beforeLegacyReplay = v2Storage.read()!!.copyOf()
        check(runCatching {
            v3Reloaded.openSession("legacy-open-op", "new-after-migration", "drawer-v2", "caller", 0, 201)
        }.isFailure)
        check(beforeLegacyReplay.contentEquals(v2Storage.read())) // the old operation record survived the v4 writes

        // v1-v3 values remain legacy attributions when the first authenticated write upgrades the file.
        val v1Storage = MemoryStorage().also { it.bytes = legacyLinkedCash(1, null) }
        val v1Store = CashCommands(v1Storage, "drawer-v2", authorityContext)
        check(v1Store.getSession("legacy-v2").openedBy == CashActorAttribution.LegacyLabel("old-opener"))
        check(v1Store.getSession("legacy-v2").sales.single().actor == CashActorAttribution.LegacyLabel(null))
        val v3Storage = MemoryStorage().also { it.bytes = legacyLinkedCash(3, "old-sale-operator") }
        val v3Store = CashCommands(v3Storage, "drawer-v2", authorityContext)
        val legacyV3 = v3Store.getSession("legacy-v2")
        check(legacyV3.openedBy == CashActorAttribution.LegacyLabel("old-opener"))
        check(legacyV3.sales.single().actor == CashActorAttribution.LegacyLabel("old-sale-operator"))
        v3Store.recordMovement("legacy-v2", "new-v3-move", CashMovementKind.CASH_IN, 10, "Migration test", "caller-label", 160)
        val v4Reloaded = CashCommands(v3Storage, "drawer-v2", authorityContext).getSession("legacy-v2")
        check(v4Reloaded.openedBy == CashActorAttribution.LegacyLabel("old-opener"))
        check(v4Reloaded.sales.single().actor == CashActorAttribution.LegacyLabel("old-sale-operator"))
        check(v4Reloaded.movements.single().actor is CashActorAttribution.Authenticated)
        check(java.nio.ByteBuffer.wrap(v3Storage.read()!!, 4, 4).int == 4)

        actorBoundReplayRejectsDifferentStaff()
    }

    private fun actorBoundReplayRejectsDifferentStaff() {
        val fixture = CashAuthorityFixture()
        val storage = MemoryStorage()
        val commands = CashCommands(storage, "actor-drawer", fixture)
        commands.openSession("actor-open", "actor-session", "actor-drawer", "ignored", 100, 1)
        val original = commands.store.recordMovement(fixture.commandSession, "actor-session", "actor-op",
            CashMovementKind.CASH_IN, 10, "float", 2)
        val originalActor = (original.movements.single().actor as CashActorAttribution.Authenticated).value
        fixture.useCashier()
        check(runCatching {
            commands.store.recordMovement(fixture.commandSession, "actor-session", "actor-op",
                CashMovementKind.CASH_IN, 10, "float", 2)
        }.isFailure)
        val cashierMovement = commands.store.recordMovement(fixture.commandSession, "actor-session", "cashier-op",
            CashMovementKind.CASH_IN, 1, "cashier move", 3)
        check(cashierMovement.movements.last().actor.let { it as CashActorAttribution.Authenticated }.value.actorId != originalActor.actorId)
        fixture.useManager()
        val closed = commands.store.closeSession(fixture.commandSession, "actor-session", "actor-close", 111, 4)
        val closeActor = (checkNotNull(closed.closeout).actor as CashActorAttribution.Authenticated).value
        check(closeActor.actorId == originalActor.actorId)
        val reopened = CashCommands(storage, "actor-drawer", fixture).getSession("actor-session")
        check(reopened.movements.first().actor == CashActorAttribution.Authenticated(originalActor))
        check(reopened.movements.last().actor == cashierMovement.movements.last().actor)
        check((checkNotNull(reopened.closeout).actor as CashActorAttribution.Authenticated).value.actorId == originalActor.actorId)
    }

    private fun legacyV2LinkedCash(): ByteArray = legacyLinkedCash(2, null)

    private fun legacyLinkedCash(version: Int, saleActor: String?): ByteArray = ByteArrayOutputStream().also { bytes ->
        DataOutputStream(bytes).use { out ->
            out.writeInt(0x4f424353); out.writeInt(version); out.writeInt(1)
            writeString(out, "legacy-v2"); writeString(out, "drawer-v2"); writeString(out, "old-opener")
            out.writeLong(100); out.writeLong(0); out.writeInt(CashSessionStatus.OPEN.ordinal); out.writeLong(100)
            out.writeInt(0) // movements
            out.writeInt(1) // linked sale payment
            writeString(out, "legacy-sale"); writeString(out, "legacy-payment")
            out.writeLong(100); out.writeLong(120); out.writeLong(20); out.writeLong(101)
            if (version >= 3) {
                out.writeBoolean(saleActor != null)
                saleActor?.let { writeString(out, it) }
            }
            out.writeBoolean(false) // no closeout
            out.writeInt(1) // preserve this pre-migration idempotency payload verbatim
            writeString(out, "legacy-open-op"); writeString(out, "legacy-payload"); writeString(out, "legacy-v2")
        }
    }.toByteArray()

    private fun writeString(out: DataOutputStream, value: String) {
        val bytes = value.toByteArray(Charsets.UTF_8)
        out.writeInt(bytes.size); out.write(bytes)
    }

    /** Test adapter preserves the old call convenience while every production store call is authorized. */
    private class CashCommands(
        storage: DurableLedgerStorage,
        private val drawerId: String,
        private val authorityContext: CashAuthorityFixture,
    ) {
        val store = CheckoutCashSessionStore(storage, authorityContext.authority, authorityContext.scope, drawerId)

        fun sessions() = store.sessions()
        fun getSession(sessionId: String) = store.getSession(sessionId)

        fun openSession(operationId: String, sessionId: String, requestedDrawer: String, ignoredCallerLabel: String,
                        openingFloatMinor: Long, openedAt: Long) = run {
            require(requestedDrawer == drawerId)
            store.openSession(authorityContext.commandSession, operationId, sessionId, openingFloatMinor, openedAt)
        }

        fun recordMovement(sessionId: String, operationId: String, kind: CashMovementKind, amountMinor: Long,
                           reason: String, ignoredCallerLabel: String, recordedAt: Long) =
            store.recordMovement(authorityContext.commandSession, sessionId, operationId, kind, amountMinor, reason, recordedAt)

        fun linkCashSale(sessionId: String, operationId: String, saleId: String, paymentId: String,
                         ignoredCallerLabel: String, recordedAt: Long, ledger: CheckoutLedger) =
            store.linkCashSale(authorityContext.commandSession, sessionId, operationId, saleId, paymentId, recordedAt, ledger)

        fun closeSession(sessionId: String, operationId: String, countedMinor: Long,
                         ignoredCallerLabel: String, closedAt: Long) =
            store.closeSession(authorityContext.commandSession, sessionId, operationId, countedMinor, closedAt)
    }

    private class CashAuthorityFixture(
        val scope: CheckoutAuthorityScope = CheckoutAuthorityScope(
            "merchant-test", "store-test", "location-test", "register-test", "device-test"),
    ) {
        private val clock = CashAuthorityClock()
        val authority = CheckoutAuthority(MemoryAuthorityStorage(), clock, defaultIterations = 1_000, sessionLifetimeMillis = 60_000)
        private val managerPin = "123456".toCharArray()
        val managerActorId = authority.bootstrapManager(
            authority.prepareBootstrap(), scope, "Test Manager", managerPin,
        ).managerActorId
        var commandSession: CheckoutAuthoritySession = authenticate(managerActorId, "123456")
            private set
        private var cashierActorId: String? = null

        fun useCashier() {
            val manager = authenticate(managerActorId, "123456")
            if (cashierActorId == null) {
                cashierActorId = authority.createStaff(manager, scope, "Test Cashier", CheckoutAuthorityRole.CASHIER,
                    "234567".toCharArray()).actorId
            }
            commandSession = authenticate(cashierActorId!!, "234567")
        }

        fun useManager() {
            commandSession = authenticate(managerActorId, "123456")
        }

        private fun authenticate(actorId: String, pin: String) =
            authority.authenticate(actorId, pin.toCharArray(), scope)
    }

    private class CashAuthorityClock : CheckoutAuthorityClock {
        override fun elapsedRealtimeMillis(): Long = 10_000L
        override fun wallTimeMillis(): Long = 1_800_000_000_000L
        override fun bootIdentity(): String = "cash-test-boot"
    }

    private class MemoryAuthorityStorage : CheckoutAuthorityStorage {
        private var bytes: ByteArray? = null
        override fun read(): ByteArray? = bytes?.copyOf()
        override fun compareAndSwap(expectedBytes: ByteArray?, nextBytes: ByteArray): Boolean {
            val current = bytes
            val matches = if (current == null || expectedBytes == null) current == null && expectedBytes == null
            else current.contentEquals(expectedBytes)
            if (!matches) return false
            bytes = nextBytes.copyOf()
            return true
        }
    }

    private class MemoryStorage : DurableLedgerStorage {
        var bytes: ByteArray? = null
        var failBeforeCommit = false
        var failAfterCommit = false
        override fun read(): ByteArray? = bytes?.copyOf()
        override fun writeAtomically(bytes: ByteArray) {
            if (failBeforeCommit) { failBeforeCommit = false; error("simulated pre-commit failure") }
            this.bytes = bytes.copyOf()
            if (failAfterCommit) { failAfterCommit = false; error("simulated post-commit failure") }
        }
    }
}
