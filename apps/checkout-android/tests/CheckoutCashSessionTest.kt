package org.openbmt.checkout

import java.io.ByteArrayOutputStream
import java.io.DataOutputStream

/** Coordinator-owned JVM contract cases for durable manual cash sessions. */
object CheckoutCashSessionTest {
    fun run() {
        val cashStorage = MemoryStorage()
        val ledgerStorage = MemoryStorage()
        var ledger = CheckoutLedger(ledgerStorage)
        ledger.createSale("sale-cash", 1300, "USD", 1)
        val payment = ledger.recordCashPayment("sale-cash", "payment-cash", 2000, 2)
        check(payment.amountMinor == 1300L && payment.tenderedMinor == 2000L && payment.changeMinor == 700L)

        var sessions = CheckoutCashSessionStore(cashStorage)
        val opened = sessions.openSession("open-op", "session-a", "drawer-a", "staff-a", 5000, 10)
        check(opened.expectedCashMinor == 5000L && opened.status == CashSessionStatus.OPEN)
        check(sessions.openSession("open-op", "session-a", "drawer-a", "staff-a", 5000, 10) == opened)
        check(runCatching { sessions.openSession("open-op", "session-a", "drawer-a", "staff-a", 5001, 10) }.isFailure)
        check(runCatching { sessions.openSession("open-other", "session-b", "drawer-a", "staff-b", 0, 11) }.isFailure)

        sessions.recordMovement("session-a", "in-op", CashMovementKind.CASH_IN, 250,
            "Change fund", "staff-a", 12)
        sessions.recordMovement("session-a", "out-op", CashMovementKind.CASH_OUT, 100,
            "Supplies", "staff-b", 13)
        sessions.linkCashSale("session-a", "sale-op", "sale-cash", "payment-cash", "staff-a", 14, ledger)
        val expected = sessions.getSession("session-a")
        check(expected.expectedCashMinor == 6450L) // opening + 250 - 100 + net 1300; change is not counted twice
        check(expected.sales.single().changeMinor == 700L)
        check(expected.sales.single().staffId == "staff-a")
        check(runCatching { sessions.linkCashSale("session-a", "sale-op", "sale-cash", "payment-cash", "staff-a", 15, ledger) }.isFailure)

        val closed = sessions.closeSession("session-a", "close-op", 6400, "manager", 20)
        check(closed.status == CashSessionStatus.CLOSED)
        check(closed.closeout == CashCloseoutSnapshot(6400, 6450, -50, "manager", 20))
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

        sessions = CheckoutCashSessionStore(cashStorage)
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
        val stale = CheckoutCashSessionStore(before)
        check(runCatching { stale.openSession("open-before", "before", "drawer-before", "staff", 0, 1) }.isFailure)
        check(runCatching { stale.sessions() }.isFailure)
        var reopened = CheckoutCashSessionStore(before)
        check(reopened.sessions().isEmpty())
        reopened.openSession("open-before", "before", "drawer-before", "staff", 0, 1)

        // A post-commit failure is uncertain to the caller but replay on a reopened store is safe.
        val after = MemoryStorage().also { it.failAfterCommit = true }
        val uncertain = CheckoutCashSessionStore(after)
        check(runCatching { uncertain.openSession("open-after", "after", "drawer-after", "staff", 0, 1) }.isFailure)
        check(runCatching { uncertain.sessions() }.isFailure)
        reopened = CheckoutCashSessionStore(after)
        check(reopened.openSession("open-after", "after", "drawer-after", "staff", 0, 1).sessionId == "after")

        val overflowStorage = MemoryStorage()
        val overflow = CheckoutCashSessionStore(overflowStorage)
        overflow.openSession("overflow-open", "overflow", "overflow-drawer", "staff", Long.MAX_VALUE, 1)
        check(runCatching { overflow.recordMovement("overflow", "overflow-in", CashMovementKind.CASH_IN, 1, "Too much", "staff", 2) }.isFailure)
        check(overflow.getSession("overflow").expectedCashMinor == Long.MAX_VALUE)
        check(runCatching { CheckoutCashSessionStore(MemoryStorage().also { it.bytes = ByteArray(3) }) }.isFailure)

        // Field boundaries must remain distinct even when user text contains the old delimiter.
        val collisionStore = CheckoutCashSessionStore(MemoryStorage())
        collisionStore.openSession("collision-open", "collision", "collision-drawer", "staff", 0, 1)
        collisionStore.recordMovement("collision", "collision-op", CashMovementKind.CASH_IN, 1, "a|b", "c", 2)
        check(runCatching {
            collisionStore.recordMovement("collision", "collision-op", CashMovementKind.CASH_IN, 1, "a", "b|c", 2)
        }.isFailure)
        check(collisionStore.getSession("collision").movements.single().reason == "a|b")
        check(runCatching { collisionStore.recordMovement("collision", "control", CashMovementKind.CASH_IN, 1, "bad\nreason", "staff", 3) }.isFailure)

        // Maximum individually valid identifiers/text still fit operation replay storage and codec.
        val maxFieldStorage = MemoryStorage()
        val maxFieldStore = CheckoutCashSessionStore(maxFieldStorage)
        val maxField = "x".repeat(256)
        maxFieldStore.openSession(maxField, maxField, maxField, maxField, 0, 1)
        maxFieldStore.recordMovement(maxField, "y".repeat(256), CashMovementKind.CASH_IN, 1,
            maxField, maxField, 2)
        val maxFieldReloaded = CheckoutCashSessionStore(maxFieldStorage).getSession(maxField)
        check(maxFieldReloaded.movements.single().reason == maxField && maxFieldReloaded.movements.single().staffId == maxField)

        // v2 did not store the operator on a sale link; preserve it as explicitly unknown.
        val v2Storage = MemoryStorage().also { it.bytes = legacyV2LinkedCash() }
        val v2Store = CheckoutCashSessionStore(v2Storage)
        check(v2Store.getSession("legacy-v2").sales.single().staffId == null)
        val v2Ledger = CheckoutLedger(MemoryStorage())
        v2Ledger.createSale("new-cash", 50, "USD", 150)
        v2Ledger.recordCashPayment("new-cash", "new-payment", 50, 151)
        v2Store.linkCashSale("legacy-v2", "new-link-op", "new-cash", "new-payment", "new-staff", 152, v2Ledger)
        val v3Reloaded = CheckoutCashSessionStore(v2Storage)
        val recoveredLinks = v3Reloaded.getSession("legacy-v2").sales.associateBy { it.paymentId }
        check(recoveredLinks.getValue("legacy-payment").staffId == null)
        check(recoveredLinks.getValue("new-payment").staffId == "new-staff")
        val replayed = v3Reloaded.linkCashSale("legacy-v2", "new-link-op", "new-cash", "new-payment", "new-staff", 152, v2Ledger)
        check(replayed.sales.associateBy { it.paymentId }.getValue("new-payment").staffId == "new-staff")
    }

    private fun legacyV2LinkedCash(): ByteArray = ByteArrayOutputStream().also { bytes ->
        DataOutputStream(bytes).use { out ->
            out.writeInt(0x4f424353); out.writeInt(2); out.writeInt(1)
            writeString(out, "legacy-v2"); writeString(out, "drawer-v2"); writeString(out, "old-opener")
            out.writeLong(100); out.writeLong(0); out.writeInt(CashSessionStatus.OPEN.ordinal); out.writeLong(100)
            out.writeInt(0) // movements
            out.writeInt(1) // linked sale payment
            writeString(out, "legacy-sale"); writeString(out, "legacy-payment")
            out.writeLong(100); out.writeLong(120); out.writeLong(20); out.writeLong(101)
            out.writeBoolean(false) // no closeout
            out.writeInt(0) // operations
        }
    }.toByteArray()

    private fun writeString(out: DataOutputStream, value: String) {
        val bytes = value.toByteArray(Charsets.UTF_8)
        out.writeInt(bytes.size); out.write(bytes)
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
