package org.openbmt.checkout

/** Focused JVM coverage for the durable standalone catalog model. */
object CheckoutCatalogTest {
    @JvmStatic fun run() {
        restartAndStableCommands()
        identityUniquenessAndHistoricalIds()
        versionsAndFrozenSelection()
        uncertainWriteRequiresReopen()
        decodedRecordsAreSemanticallyValidated()
        identifiersMatchSharedOpaqueIdRules()
    }

    private fun restartAndStableCommands() {
        val storage = MemoryStorage()
        var catalog = CheckoutCatalog(storage)
        val variant = CatalogVariant("regular", "Regular", 1250, "SKU-1", "012345")
        val created = catalog.createProduct("cmd-create", "coffee", "Coffee", listOf(variant), listOf(CatalogModifier("oat", "Oat milk", 75)))
        check(created.version == 1L && created.variants.single().priceMinor == 1250L)
        check(catalog.createProduct("cmd-create", "coffee", "Coffee", listOf(variant), listOf(CatalogModifier("oat", "Oat milk", 75))) == created)
        check(runCatching { catalog.createProduct("cmd-create", "coffee", "Different", listOf(variant)) }.isFailure)
        catalog = CheckoutCatalog(storage)
        check(catalog.getProduct("coffee") == created)
        check(catalog.findBySku("SKU-1")?.first?.productId == "coffee")
        check(catalog.findByBarcode("012345")?.second?.variantId == "regular")
        check(catalog.search("cof").single().productId == "coffee")
    }

    private fun identityUniquenessAndHistoricalIds() {
        val catalog = CheckoutCatalog(MemoryStorage())
        catalog.createProduct("c1", "p1", "One", listOf(CatalogVariant("v1", "Each", 100, "UNIQUE", "BAR-1")))
        check(runCatching { catalog.createProduct("c2", "p2", "Two", listOf(CatalogVariant("v2", "Each", 200, "OTHER", "BAR-1"))) }.isFailure)
        check(runCatching { catalog.createProduct("c3", "p3", "Three", listOf(CatalogVariant("v3", "Each", 200, "UNIQUE", "BAR-3"))) }.isFailure)
        catalog.archiveProduct("a1", "p1", 1)
        check(catalog.findByBarcode("BAR-1") == null)
        check(runCatching { catalog.createProduct("c4", "p1", "Reuse", listOf(CatalogVariant("v4", "Each", 300))) }.isFailure)
        check(catalog.getProductVersion("p1", 1)?.archived == false)
        check(catalog.getProduct("p1")?.archived == true)
    }

    private fun versionsAndFrozenSelection() {
        val catalog = CheckoutCatalog(MemoryStorage())
        catalog.createProduct("create", "tea", "Tea", listOf(CatalogVariant("cup", "Cup", 300)), listOf(CatalogModifier("lemon", "Lemon", 25)))
        val line = catalog.snapshotLine("line-1", "tea", "cup", 2, listOf("lemon"))
        check(line == CheckoutLineInput("line-1", "tea", "Tea — Cup", 300, 2, listOf(SelectedModifier("lemon", "Lemon", 25))))
        val oldRevision = catalog.getProductVersion("tea", 1)!!
        val changed = catalog.editProduct("edit-1", "tea", 1, "Tea updated", listOf(CatalogVariant("cup", "Cup", 450)))
        check(changed.version == 2L && oldRevision.name == "Tea" && oldRevision.variants.single().priceMinor == 300L)
        check(line.name == "Tea — Cup" && line.unitMinor == 300L && line.modifiers.single().amountMinor == 25L)
        @Suppress("UNCHECKED_CAST")
        val selectedModifiers = line.modifiers as MutableList<SelectedModifier>
        check(runCatching { selectedModifiers.add(SelectedModifier("extra", "Extra", 1)) }.isFailure)
        check(line.modifiers == listOf(SelectedModifier("lemon", "Lemon", 25)))
        check(runCatching { catalog.editProduct("stale", "tea", 1, "stale", listOf(CatalogVariant("cup", "Cup", 1))) }.isFailure)
        check(catalog.editProduct("edit-1", "tea", 1, "Tea updated", listOf(CatalogVariant("cup", "Cup", 450))) == changed)
    }

    private fun uncertainWriteRequiresReopen() {
        val storage = MemoryStorage()
        val catalog = CheckoutCatalog(storage)
        storage.failNextWrite = true
        check(runCatching { catalog.createProduct("create", "p", "Product", listOf(CatalogVariant("v", "Each", 100))) }.isFailure)
        check(runCatching { catalog.getProduct("p") }.isFailure)
        val reopened = CheckoutCatalog(storage)
        check(reopened.getProduct("p") == null) // failed write did not reach durable storage
        reopened.createProduct("create", "p", "Product", listOf(CatalogVariant("v", "Each", 100)))
        check(CheckoutCatalog(storage).getProduct("p")?.version == 1L)
    }

    private fun decodedRecordsAreSemanticallyValidated() {
        val negativePrice = MemoryStorage()
        CheckoutCatalog(negativePrice).createProduct("c", "p", "Product", listOf(CatalogVariant("v", "Each", 100)))
        val priceBytes = java.nio.ByteBuffer.allocate(8).putLong(100L).array()
        val priceAt = indexOf(negativePrice.bytes!!, priceBytes)
        check(priceAt >= 0)
        negativePrice.bytes = negativePrice.bytes!!.copyOf().also {
            java.nio.ByteBuffer.wrap(it, priceAt, 8).putLong(-1L)
        }
        check(runCatching { CheckoutCatalog(negativePrice) }.isFailure)

        val duplicateBarcode = MemoryStorage()
        CheckoutCatalog(duplicateBarcode).createProduct("c", "p", "Product", listOf(
            CatalogVariant("v1", "First", 100, barcode = "BAR1"),
            CatalogVariant("v2", "Second", 200, barcode = "BAR2"),
        ))
        val bytes = duplicateBarcode.bytes!!.copyOf()
        val secondBarcodeAt = indexOf(bytes, "BAR2".toByteArray(Charsets.UTF_8))
        check(secondBarcodeAt >= 0)
        "BAR1".toByteArray(Charsets.UTF_8).copyInto(bytes, secondBarcodeAt)
        duplicateBarcode.bytes = bytes
        check(runCatching { CheckoutCatalog(duplicateBarcode) }.isFailure)
    }

    private fun identifiersMatchSharedOpaqueIdRules() {
        val catalog = CheckoutCatalog(MemoryStorage())
        check(runCatching { catalog.createProduct("unicode", "café", "Cafe", listOf(CatalogVariant("v", "Each", 100))) }.isFailure)
        check(runCatching { catalog.createProduct("space", "two words", "Two words", listOf(CatalogVariant("v", "Each", 100))) }.isFailure)
        check(runCatching { catalog.createProduct("variant", "p", "Product", listOf(CatalogVariant("variant id", "Each", 100))) }.isFailure)
        check(runCatching { catalog.createProduct("modifier", "p", "Product", listOf(CatalogVariant("v", "Each", 100)), listOf(CatalogModifier("é", "Accent", 0))) }.isFailure)
        val accepted = catalog.createProduct("valid", "product-1", "Café", listOf(CatalogVariant("variant-1", "Each", 100)))
        check(accepted.name == "Café") // display names remain UTF-8
    }

    private fun indexOf(bytes: ByteArray, target: ByteArray): Int {
        if (target.isEmpty()) return 0
        for (start in 0..bytes.size - target.size) {
            var matches = true
            for (offset in target.indices) if (bytes[start + offset] != target[offset]) { matches = false; break }
            if (matches) return start
        }
        return -1
    }

    private class MemoryStorage : DurableLedgerStorage {
        var bytes: ByteArray? = null
        var failNextWrite = false
        override fun read(): ByteArray? = bytes?.copyOf()
        override fun writeAtomically(bytes: ByteArray) {
            if (failNextWrite) { failNextWrite = false; throw IllegalStateException("simulated uncertain write") }
            this.bytes = bytes.copyOf()
        }
    }
}
