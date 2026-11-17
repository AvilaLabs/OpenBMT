package org.openbmt.checkout

import java.nio.ByteBuffer

/** Local durable change journal contract tests. The coordinator owns compilation and execution. */
object CheckoutEventsTest {
    @JvmStatic fun main(args: Array<String>) = run()
    fun run() {
        val storage = MemoryStorage()
        var ledger = CheckoutLedger(storage)
        check(!ledger.eventSnapshot().initialized)
        ledger.createSale("sale-a", 1000, "USD", 1)
        val initial = ledger.initializeEvents()
        check(initial.initialized && initial.highWater == 0L && initial.saleVersions["sale-a"] == 0L)
        val cursor = initial.highWater
        check(runCatching { ledger.recordCashPayment("sale-a", "too-little", 10, 2) }.isFailure)
        check(ledger.eventSnapshot().highWater == cursor)

        ledger.createSale("sale-b", 1000, "USD", 3)
        ledger.beginCardAttempt("sale-a", "card-a", "processor", "private-stable", 1000)
        val created = ledger.readEvents(initial.epoch!!, 0, 20)
        check(created.events.map { it.type } == listOf(CheckoutEventType.SALE_CREATED, CheckoutEventType.PAYMENT_CHANGED))
        check(created.events.map { it.cursor } == listOf(1L, 2L))
        check(created.events.map { it.saleVersion } == listOf(1L, 1L))
        check(created.events.all { it.eventId.isNotBlank() && it.saleId.isNotBlank() })
        check(created.toString().let { text -> listOf("private-stable", "processor", "private-quote", "txid").none(text::contains) })
        check(CheckoutEvent::class.java.declaredFields
            .filterNot { java.lang.reflect.Modifier.isStatic(it.modifiers) }
            .map { it.name }.toSet() == setOf("eventId", "epoch", "cursor", "saleId", "saleVersion", "type"))

        ledger.recordCardEvidence("sale-a", "card-a", "evidence-a", "private-stable", CardResult.COMPLETED, 1000, 4)
        val afterPayment = ledger.eventSnapshot()
        check(afterPayment.highWater == 3L && afterPayment.saleVersions["sale-a"] == 2L)
        ledger.recordCardEvidence("sale-a", "card-a", "evidence-a", "private-stable", CardResult.COMPLETED, 1000, 4)
        check(ledger.eventSnapshot().highWater == 3L) // exact replay is not a change
        check(runCatching { ledger.recordCardEvidence("sale-a", "card-a", "evidence-a", "private-stable", CardResult.FAILED, 0, 3) }.isFailure)
        check(ledger.eventSnapshot().highWater == 3L)
        ledger.requestRefund("sale-a", "refund-a", "card-a", 400, 0, 5)
        check(ledger.readEvents(initial.epoch, 2, 10).events.last().type == CheckoutEventType.REFUND_CHANGED)

        val page = ledger.readEvents(initial.epoch, 0, 2)
        check(page.events.size == 2 && page.highWater == 4L && page.nextCursor == 2L && page.hasMore)
        check(ledger.readEvents("wrong-epoch", 0, 1).resyncRequired)
        check(ledger.readEvents("wrong-epoch", 500, 1).resyncRequired)
        check(runCatching { ledger.readEvents(initial.epoch, 5, 1) }.isFailure)
        ledger = CheckoutLedger(storage)
        check(ledger.eventSnapshot().epoch == initial.epoch && ledger.eventSnapshot().highWater == 4L)
        check(ledger.readEvents(initial.epoch, 3, 10).events.single().type == CheckoutEventType.REFUND_CHANGED)

        val legacy = MemoryStorage().also { it.bytes = legacyV5() }
        val beforeRead = legacy.bytes!!.copyOf()
        val migrated = CheckoutLedger(legacy)
        check(migrated.getSale("legacy").cashPayments.single().paymentId == "cash-legacy")
        check(legacy.bytes!!.contentEquals(beforeRead)) // read-only legacy open does not rewrite
        check(!migrated.eventSnapshot().initialized)
        val legacyEpoch = migrated.initializeEvents().epoch!!
        check(CheckoutLedger(legacy).eventSnapshot().epoch == legacyEpoch)

        val failing = MemoryStorage()
        val failureLedger = CheckoutLedger(failing)
        failureLedger.initializeEvents()
        failing.mode = FailureMode.BEFORE_COMMIT
        check(runCatching { failureLedger.createSale("failed", 50, "USD", 10) }.isFailure)
        failing.mode = FailureMode.NONE
        var reopened = CheckoutLedger(failing)
        check(reopened.eventSnapshot().highWater == 0L && runCatching { reopened.getSale("failed") }.isFailure)
        failing.mode = FailureMode.AFTER_COMMIT
        check(runCatching { reopened.createSale("committed", 60, "USD", 11) }.isFailure)
        reopened = CheckoutLedger(failing)
        check(reopened.getSale("committed").amountMinor == 60L)
        check(reopened.eventSnapshot().highWater == 1L)
        check(reopened.readEvents(reopened.eventSnapshot().epoch!!, 0, 2).events.single().saleId == "committed")

        initializeFailureOutcomes()

        val corruptBase = MemoryStorage()
        val corruptLedger = CheckoutLedger(corruptBase)
        corruptLedger.initializeEvents()
        corruptLedger.createSale("corrupt-a", 5, "USD", 1)
        corruptLedger.createSale("corrupt-b", 5, "USD", 2)
        val eventIds = corruptLedger.readEvents(corruptLedger.eventSnapshot().epoch!!, 0, 10).events.map { it.eventId }
        val gapBytes = corruptBase.bytes!!.copyOf()
        ByteBuffer.wrap(gapBytes).putLong(eventCursorOffset(gapBytes, eventIds[1]), 3L)
        check(runCatching { CheckoutLedger(MemoryStorage().also { it.bytes = gapBytes }) }.isFailure)
        val duplicateBytes = corruptBase.bytes!!.copyOf()
        val firstIdOffset = eventIdContentOffset(duplicateBytes, eventIds[0])
        val secondIdOffset = eventIdContentOffset(duplicateBytes, eventIds[1])
        eventIds[0].toByteArray().copyInto(duplicateBytes, secondIdOffset)
        check(firstIdOffset != secondIdOffset)
        check(runCatching { CheckoutLedger(MemoryStorage().also { it.bytes = duplicateBytes }) }.isFailure)

        val bounded = MemoryStorage()
        val boundedLedger = CheckoutLedger(bounded)
        val boundEpoch = boundedLedger.initializeEvents().epoch!!
        repeat(1001) { i -> boundedLedger.createSale("s$i", 1, "USD", i.toLong()) }
        val pruned = boundedLedger.eventSnapshot()
        check(pruned.highWater == 1001L && pruned.floor == 1L)
        check(boundedLedger.readEvents(boundEpoch, 0, 10).resyncRequired)
        check(boundedLedger.readEvents(boundEpoch, 1, 2).events.map { it.cursor } == listOf(2L, 3L))

        val cashStorage = MemoryStorage()
        val cashLedger = CheckoutLedger(cashStorage)
        cashLedger.initializeEvents()
        cashLedger.createSale("cash-sale", 250, "USD", 20)
        val cash = cashLedger.recordCashPayment("cash-sale", "cash-payment", 300, 21)
        val cashCursor = cashLedger.eventSnapshot().highWater
        check(cash.amountMinor == 250L && cash.changeMinor == 50L)
        check(cashLedger.recordCashPayment("cash-sale", "cash-payment", 300, 21) == cash)
        check(cashLedger.eventSnapshot().highWater == cashCursor)
    }

    private fun initializeFailureOutcomes() {
        val before = MemoryStorage()
        val beforeLedger = CheckoutLedger(before)
        beforeLedger.createSale("before-init", 125, "USD", 1)
        val original = before.bytes!!.copyOf()
        before.mode = FailureMode.BEFORE_COMMIT
        check(runCatching { beforeLedger.initializeEvents() }.isFailure)
        check(runCatching { beforeLedger.eventSnapshot() }.isFailure) // uncertain writes poison this instance
        before.mode = FailureMode.NONE
        var reopened = CheckoutLedger(before)
        check(!reopened.eventSnapshot().initialized)
        check(reopened.getSale("before-init").amountMinor == 125L)
        check(before.bytes!!.contentEquals(original))
        val beforeEpoch = reopened.initializeEvents().epoch!!
        check(reopened.eventSnapshot().saleVersions == mapOf("before-init" to 0L))
        reopened.createSale("after-before-init", 50, "USD", 2)
        check(reopened.readEvents(beforeEpoch, 0, 10).events.single().saleId == "after-before-init")

        val after = MemoryStorage()
        var afterLedger = CheckoutLedger(after)
        afterLedger.createSale("after-init", 225, "USD", 3)
        after.mode = FailureMode.AFTER_COMMIT
        check(runCatching { afterLedger.initializeEvents() }.isFailure)
        check(runCatching { afterLedger.getSale("after-init") }.isFailure)
        after.mode = FailureMode.NONE
        afterLedger = CheckoutLedger(after)
        val committedEpoch = afterLedger.eventSnapshot().epoch!!
        check(afterLedger.eventSnapshot().initialized)
        check(afterLedger.getSale("after-init").amountMinor == 225L)
        check(afterLedger.initializeEvents().epoch == committedEpoch) // retry does not rotate a committed epoch
        check(afterLedger.eventSnapshot().saleVersions == mapOf("after-init" to 0L))
        afterLedger.createSale("after-commit-init", 75, "USD", 4)
        check(afterLedger.readEvents(committedEpoch, 0, 10).events.single().saleId == "after-commit-init")

        val legacy = MemoryStorage().also { it.bytes = legacyV5() }
        val legacyOriginal = legacy.bytes!!.copyOf()
        val legacyLedger = CheckoutLedger(legacy)
        check(legacyLedger.getSale("legacy").cashPayments.single().changeMinor == 20L)
        legacy.mode = FailureMode.BEFORE_COMMIT
        check(runCatching { legacyLedger.initializeEvents() }.isFailure)
        check(runCatching { legacyLedger.eventSnapshot() }.isFailure)
        legacy.mode = FailureMode.NONE
        var legacyReopened = CheckoutLedger(legacy)
        check(!legacyReopened.eventSnapshot().initialized)
        check(legacy.bytes!!.contentEquals(legacyOriginal)) // failed initialization preserves raw v5 bytes
        val upgradedEpoch = legacyReopened.initializeEvents().epoch!!
        legacyReopened = CheckoutLedger(legacy)
        check(legacyReopened.eventSnapshot().epoch == upgradedEpoch)
        check(legacyReopened.eventSnapshot().saleVersions == mapOf("legacy" to 0L))
        check(legacyReopened.getSale("legacy").let { it.amountMinor == 100L && it.cashPayments.single().paymentId == "cash-legacy" })
    }

    private enum class FailureMode { NONE, BEFORE_COMMIT, AFTER_COMMIT }
    private class MemoryStorage : DurableLedgerStorage {
        var bytes: ByteArray? = null
        var mode = FailureMode.NONE
        override fun read() = bytes?.copyOf()
        override fun writeAtomically(bytes: ByteArray) {
            when (mode) {
                FailureMode.NONE -> this.bytes = bytes.copyOf()
                FailureMode.BEFORE_COMMIT -> error("simulated pre-commit failure")
                FailureMode.AFTER_COMMIT -> { this.bytes = bytes.copyOf(); error("simulated post-commit failure") }
            }
        }
    }

    private fun eventIdContentOffset(bytes: ByteArray, id: String): Int {
        val content = id.toByteArray()
        val found = find(bytes, content)
        check(found >= 4 && ByteBuffer.wrap(bytes).getInt(found - 4) == content.size)
        return found
    }

    private fun eventCursorOffset(bytes: ByteArray, id: String): Int {
        val idAt = eventIdContentOffset(bytes, id)
        val epochLengthAt = idAt + id.length
        val epochLength = ByteBuffer.wrap(bytes).getInt(epochLengthAt)
        return epochLengthAt + 4 + epochLength
    }

    private fun find(bytes: ByteArray, needle: ByteArray): Int {
        for (i in 0..bytes.size - needle.size) if (needle.indices.all { bytes[i + it] == needle[it] }) return i
        error("byte pattern not found")
    }

    /** v5 ledger containing one cash sale, to prove migration is explicit and read-only. */
    private fun legacyV5(): ByteArray {
        val out = java.io.ByteArrayOutputStream()
        java.io.DataOutputStream(out).use { d ->
            d.writeInt(0x4f42434c); d.writeInt(5); d.writeInt(1)
            fun str(s: String) { val b = s.toByteArray(); d.writeInt(b.size); d.write(b) }
            str("legacy"); d.writeLong(100); str("USD"); d.writeLong(1); d.writeInt(0); d.writeInt(0); d.writeInt(0)
            d.writeInt(1); str("cash-legacy"); d.writeLong(100); d.writeLong(120); d.writeLong(20); d.writeLong(2)
        }
        return out.toByteArray()
    }
}
