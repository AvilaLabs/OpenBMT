package org.openbmt.checkout

import java.io.ByteArrayOutputStream
import java.io.DataOutputStream

object CheckoutInventoryTest {
    fun run() {
        duplicateFulfillmentSurvivesRestart()
        failedWritesPoisonAndReload()
        payloadAndVersionConflictsAreRejected()
        unsafePaymentStatesCannotFulfill()
        insufficientStockIsAllOrNothing()
        structurallyValidCorruptionIsRejected()
        settledSplitTenderCanFulfillButUnconfirmedBitcoinCannot()
        repeatedProductLinesAggregateWithinSaleBounds()
    }

    private fun duplicateFulfillmentSurvivesRestart() {
        val storage = MemoryStorage(); val inventory = CheckoutInventory(storage)
        inventory.addProduct("tea", 8, true, "opening count", "open-tea")
        val sale = paidSale("sale-a")
        val first = inventory.fulfill(sale, 4, 4, "fulfill-a", 10)
        check(first.payload.single() == InventoryPurchaseLine("tea", 2))
        val restarted = CheckoutInventory(storage)
        check(restarted.addProduct("tea", 8, true, "opening count", "open-tea").onHand == 6)
        check(restarted.fulfill(sale, 4, 5, "fulfill-a", 11) == first)
        check(restarted.snapshot().products.single().onHand == 6)
        expectFailure { restarted.fulfill(sale, 4, 4, "fulfill-b", 12) }
    }

    private fun failedWritesPoisonAndReload() {
        for (afterCommit in listOf(false, true)) {
            val storage = MemoryStorage(); val inventory = CheckoutInventory(storage)
            inventory.addProduct("tea", 5, true, "opening count", "open-tea")
            storage.postCommit = afterCommit
            storage.fail = afterCommit
            if (!afterCommit) storage.failBeforeCommit = true
            expectFailure { inventory.fulfill(paidSale("sale-fail"), 1, 1, "fulfill-fail", 20) }
            expectFailure { inventory.snapshot() }
            val reopened = CheckoutInventory(storage)
            check(reopened.snapshot().products.single().onHand == if (afterCommit) 3 else 5)
            if (afterCommit) check(reopened.snapshot().fulfillments.single().fulfillmentId == "fulfill-fail")
        }
    }

    private fun payloadAndVersionConflictsAreRejected() {
        val storage = MemoryStorage(); val inventory = CheckoutInventory(storage)
        inventory.addProduct("tea", 9, true, "opening count", "open-tea")
        check(inventory.adjustStock("tea", 1, "delivery", "adj-x", 0).onHand == 10)
        check(inventory.adjustStock("tea", 1, "delivery", "adj-x", 0).onHand == 10)
        expectFailure { inventory.adjustStock("tea", 2, "delivery", "adj-x", 0) }
        expectFailure { inventory.adjustStock("tea", 1, "delivery", "adj-y", 0) }
        val sale = paidSale("sale-conflict")
        expectFailure { inventory.fulfill(sale, 1, 2, "fulfill-x", 1) }
        inventory.fulfill(sale, 1, 1, "fulfill-x", 1)
        expectFailure { inventory.fulfill(sale.copy(items = listOf(SaleItem("tea", "Tea", 100, 3))), 1, 1, "fulfill-x", 2) }
        expectFailure { inventory.fulfill(sale.copy(saleId = "sale-other"), 1, 1, "fulfill-x", 2) }
    }

    private fun unsafePaymentStatesCannotFulfill() {
        val blocked = listOf(
            paidSale("partial").copy(attempts = listOf(attempt(PaymentState.PARTIAL))),
            paidSale("unknown").copy(attempts = listOf(attempt(PaymentState.UNKNOWN))),
            paidSale("unconfirmed").copy(attempts = listOf(attempt(PaymentState.UNCONFIRMED))),
            paidSale("reorg").copy(attempts = listOf(attempt(PaymentState.REORGED))),
            paidSale("late").copy(lateBitcoinReceipt = true),
            paidSale("review").copy(requiresReview = true),
            paidSale("remaining").copy(remainingMinor = 1),
            paidSale("unpaid").copy(paid = false),
        )
        for ((index, sale) in blocked.withIndex()) {
            val inventory = CheckoutInventory(MemoryStorage())
            inventory.addProduct("tea", 10, true, "opening count", "open-$index")
            expectFailure { inventory.fulfill(sale, 0, 0, "fulfill-$index", 1) }
            check(inventory.snapshot().products.single().onHand == 10)
        }
    }

    private fun insufficientStockIsAllOrNothing() {
        val inventory = CheckoutInventory(MemoryStorage())
        inventory.addProduct("tea", 4, true, "opening count", "open-tea")
        inventory.addProduct("cake", 1, true, "opening count", "open-cake")
        val sale = paidSale("sale-stock").copy(items = listOf(SaleItem("tea", "Tea", 100, 2), SaleItem("cake", "Cake", 100, 2)))
        try { inventory.fulfill(sale, 0, 0, "fulfill-stock", 1); error("expected insufficient stock") }
        catch (e: InsufficientInventoryException) { check(e.productId == "cake" && e.available == 1) }
        check(inventory.snapshot().products.associate { it.productId to it.onHand } == mapOf("tea" to 4, "cake" to 1))
        check(inventory.snapshot().fulfillments.isEmpty())
    }

    private fun structurallyValidCorruptionIsRejected() {
        expectFailure { CheckoutInventory(MemoryStorage(fixture(currentCount = 5))) }
        expectFailure { CheckoutInventory(MemoryStorage(fixture(duplicateSale = true))) }
    }

    private fun settledSplitTenderCanFulfillButUnconfirmedBitcoinCannot() {
        val ledgerStorage = MemoryStorage()
        val ledger = CheckoutLedger(ledgerStorage)
        ledger.initializeEvents()
        ledger.createSale("sale-split", 1000, "USD", 1, listOf(SaleItem("tea", "Tea", 1000, 1)))
        ledger.beginCardAttempt("sale-split", "card-split", "test-provider", "stable-split", 1000)
        ledger.recordCardEvidence("sale-split", "card-split", "evidence-split", "stable-split", CardResult.COMPLETED, 300, 2)
        ledger.recordCashPayment("sale-split", "cash-split", 700, 3)
        val paid = ledger.getSale("sale-split")
        check(paid.paid && paid.remainingMinor == 0L && paid.attempts.single().state == PaymentState.PARTIAL)
        val inventory = CheckoutInventory(MemoryStorage())
        inventory.addProduct("tea", 2, true, "opening count", "open-tea")
        val version = ledger.eventSnapshot().saleVersions.getValue("sale-split")
        inventory.fulfill(paid, version, version, "fulfill-split", 4)
        check(inventory.snapshot().products.single().onHand == 1)

        val btcLedger = CheckoutLedger(MemoryStorage())
        btcLedger.initializeEvents()
        btcLedger.createSale("sale-unconfirmed", 1000, "USD", 1, listOf(SaleItem("tea", "Tea", 1000, 1)))
        btcLedger.beginBitcoinAttempt("sale-unconfirmed", "btc-unconfirmed", 0, 1000, 10000, "quote", 1)
        btcLedger.recordBitcoinEvidence("sale-unconfirmed", "btc-unconfirmed", "out-unconfirmed", "source-unconfirmed", 1000, 0, 2, 2)
        val observed = btcLedger.getSale("sale-unconfirmed").copy(paid = true, remainingMinor = 0)
        val btcInventory = CheckoutInventory(MemoryStorage())
        btcInventory.addProduct("tea", 2, true, "opening count", "open-tea")
        val btcVersion = btcLedger.eventSnapshot().saleVersions.getValue("sale-unconfirmed")
        expectFailure { btcInventory.fulfill(observed, btcVersion, btcVersion, "fulfill-unconfirmed", 4) }
        check(btcInventory.snapshot().products.single().onHand == 2)
    }

    private fun repeatedProductLinesAggregateWithinSaleBounds() {
        val ledger = CheckoutLedger(MemoryStorage())
        ledger.initializeEvents()
        ledger.createSale("sale-large-basket", 1200, "USD", 1, listOf(
            SaleItem("tea", "Tea", 1, 600), SaleItem("tea", "Tea", 1, 600),
        ))
        ledger.recordCashPayment("sale-large-basket", "cash-large", 1200, 2)
        val sale = ledger.getSale("sale-large-basket")
        val version = ledger.eventSnapshot().saleVersions.getValue("sale-large-basket")
        val storage = MemoryStorage(); val inventory = CheckoutInventory(storage)
        inventory.addProduct("tea", 2000, true, "opening count", "open-tea")
        val record = inventory.fulfill(sale, version, version, "fulfill-large", 3)
        check(record.payload == listOf(InventoryPurchaseLine("tea", 1200)))
        check(CheckoutInventory(storage).snapshot().products.single().onHand == 800)
        check(CheckoutInventory(storage).fulfill(sale, version, version, "fulfill-large", 4) == record)
        check(CheckoutInventory(storage).snapshot().products.single().onHand == 800)

        val cleanStorage = MemoryStorage(); val clean = CheckoutInventory(cleanStorage)
        clean.addProduct("tea", 2000, true, "opening count", "open-tea")
        expectFailure { clean.fulfill(sale.copy(items = listOf(SaleItem("tea", "Tea", 1, 1000))), version, version, "bad-quantity", 3) }
        check(clean.snapshot().products.single().onHand == 2000)
    }

    private fun fixture(currentCount: Int = 4, duplicateSale: Boolean = false): ByteArray {
        val bytes = ByteArrayOutputStream()
        DataOutputStream(bytes).use { out ->
            fun str(value: String) { val encoded=value.toByteArray(Charsets.UTF_8); out.writeInt(encoded.size); out.write(encoded) }
            out.writeInt(0x4f424349); out.writeInt(1)
            out.writeInt(1); str("tea"); out.writeBoolean(true); out.writeBoolean(true); out.writeInt(currentCount); out.writeLong(0)
            out.writeInt(1); str("open"); str("tea"); out.writeInt(4); str("opening"); out.writeInt(4); out.writeLong(0)
            out.writeInt(1); str("open"); str("tea"); out.writeInt(4); str("opening"); out.writeLong(-1)
            out.writeInt(0)
            out.writeInt(if (duplicateSale) 2 else 0)
            if (duplicateSale) repeat(2) { index ->
                str("fulfill-$index"); str("same-sale"); out.writeLong(0); out.writeLong(1)
                out.writeInt(1); str("tea"); out.writeInt(1); out.writeInt(0)
            }
        }
        return bytes.toByteArray()
    }

    private fun paidSale(id: String) = Sale(id, 200, "USD", 1, emptyList(), emptyList(), 200, 0,
        true, false, false, 0, 0, listOf(SaleItem("tea", "Tea", 100, 2)))

    private fun attempt(state: PaymentState) = PaymentAttempt("attempt", Tender.CARD, 200, null, null, null,
        null, "test", "stable", false, false, emptyList(), emptyList(), state, 200, 0)

    private fun expectFailure(block: () -> Unit) { var failed = false; try { block() } catch (_: RuntimeException) { failed = true }; check(failed) }
    private class MemoryStorage : DurableLedgerStorage {
        var bytes: ByteArray? = null
        var fail = false
        constructor()
        constructor(initial: ByteArray) { bytes = initial.copyOf() }
        var failBeforeCommit = false
        override fun read() = bytes?.copyOf()
        override fun writeAtomically(bytes: ByteArray) {
            if (failBeforeCommit) { failBeforeCommit = false; throw IllegalStateException("injected pre-commit failure") }
            if (fail) { fail = false; if (postCommit) this.bytes = bytes.copyOf(); throw IllegalStateException("injected post-commit failure") }
            this.bytes = bytes.copyOf()
        }
        var postCommit = false
    }
}
