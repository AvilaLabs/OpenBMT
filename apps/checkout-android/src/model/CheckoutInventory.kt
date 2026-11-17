package org.openbmt.checkout

import java.io.ByteArrayInputStream
import java.io.ByteArrayOutputStream
import java.io.DataInputStream
import java.io.DataOutputStream
import java.nio.ByteBuffer
import java.nio.charset.CodingErrorAction
import java.util.Collections

data class InventoryPurchaseLine(val productId: String, val quantity: Int)
data class InventoryProduct(val productId: String, val tracked: Boolean, val onHand: Int, val version: Long)
data class InventoryAdjustment(val operationId: String, val productId: String, val delta: Int, val reason: String, val resultingCount: Int, val version: Long)
data class FulfillmentRecord(val fulfillmentId: String, val saleId: String, val saleVersion: Long,
                             val payload: List<InventoryPurchaseLine>, val completedAt: Long,
                             val trackedProductIds: List<String> = emptyList())
private fun freezeInventoryPayload(payload: List<InventoryPurchaseLine>) = Collections.unmodifiableList(ArrayList(payload.map { it.copy() }))
private fun freezeInventoryStrings(values: List<String>) = Collections.unmodifiableList(ArrayList(values))
class InventorySnapshot(products: List<InventoryProduct>, adjustments: List<InventoryAdjustment>, fulfillments: List<FulfillmentRecord>) {
    val products = Collections.unmodifiableList(ArrayList(products))
    val adjustments = Collections.unmodifiableList(ArrayList(adjustments))
    val fulfillments = Collections.unmodifiableList(ArrayList(fulfillments.map { it.copy(payload = freezeInventoryPayload(it.payload), trackedProductIds = freezeInventoryStrings(it.trackedProductIds)) }))
}
class InsufficientInventoryException(val productId: String, val requested: Int, val available: Int) : IllegalStateException("insufficient stock for $productId: requested $requested, available $available")

/** Bounded single-device inventory store. On-hand values are operator-entered counts, not physical-count proof. */
class CheckoutInventory(private val storage: DurableLedgerStorage) {
    private val lock = Any()
    private var state = decode(storage.read())
    private var poisoned = false

    fun snapshot(): InventorySnapshot = synchronized(lock) { healthy(); view() }

    fun addProduct(productId: String, openingCount: Int, tracked: Boolean, reason: String, operationId: String): InventoryProduct = synchronized(lock) {
        healthy(); id(productId); operationId(operationId); validateReason(reason); require(openingCount >= 0)
        val request = Operation(productId, openingCount, reason, -1L)
        state.operationPayloads[operationId]?.let {
            require(it == request && state.products[productId] != null) { "operation id reused with different payload" }
            return@synchronized product(productId)
        }
        require(productId !in state.products)
        require(operationId !in state.operationPayloads && operationId !in state.modeOperations) { "operation id already used" }
        require(state.products.size < MAX_PRODUCTS && state.operations < MAX_OPERATIONS)
        state.products[productId] = Product(tracked, tracked, openingCount, 0)
        state.operationPayloads[operationId] = Operation(productId, openingCount, reason, -1)
        state.adjustments.add(InventoryAdjustment(operationId, productId, openingCount, reason, openingCount, 0))
        commit(product(productId))
    }

    /** Version checked and idempotent; reusing an operation ID with changed content is rejected. */
    fun adjustStock(productId: String, delta: Int, reason: String, operationId: String, expectedVersion: Long): InventoryProduct = synchronized(lock) {
        healthy(); operationId(operationId); validateReason(reason); require(delta != 0)
        val request = Operation(productId, delta, reason, expectedVersion)
        state.operationPayloads[operationId]?.let { require(it == request) { "operation id reused with different payload" }; return@synchronized product(productId) }
        require(operationId !in state.modeOperations) { "operation id reused for another operation" }
        val old = state.products[productId] ?: error("unknown product")
        require(old.version == expectedVersion) { "inventory version conflict" }
        require(state.operations < MAX_OPERATIONS)
        val count = Math.addExact(old.count, delta); require(count >= 0) { "stock count cannot be negative" }
        val version = Math.addExact(old.version, 1)
        state.products[productId] = Product(old.initialTracked, old.tracked, count, version)
        state.operationPayloads[operationId] = request
        state.adjustments.add(InventoryAdjustment(operationId, productId, delta, reason, count, version))
        commit(product(productId))
    }

    fun setTracked(productId: String, tracked: Boolean, reason: String, operationId: String, expectedVersion: Long): InventoryProduct = synchronized(lock) {
        healthy(); operationId(operationId); validateReason(reason)
        val request = ModeOperation(productId, tracked, reason, expectedVersion)
        state.modeOperations[operationId]?.let { require(it == request) { "operation id reused with different payload" }; return@synchronized product(productId) }
        require(operationId !in state.operationPayloads) { "operation id reused for another operation" }
        val old = state.products[productId] ?: error("unknown product"); require(old.version == expectedVersion) { "inventory version conflict" }
        require(state.operations < MAX_OPERATIONS)
        val version = Math.addExact(old.version, 1); state.products[productId] = Product(old.initialTracked, tracked, old.count, version)
        state.modeOperations[operationId] = request
        state.adjustments.add(InventoryAdjustment(operationId, productId, 0, reason, old.count, version))
        commit(product(productId))
    }

    /** Sale/version are trusted caller snapshots. This store cannot authenticate or atomically update the separate ledger. */
    fun fulfill(sale: Sale, expectedSaleVersion: Long, currentSaleVersion: Long, fulfillmentId: String, completedAt: Long): FulfillmentRecord = synchronized(lock) {
        healthy(); id(fulfillmentId); id(sale.saleId); require(expectedSaleVersion >= 0 && currentSaleVersion >= 0 && completedAt >= 0)
        val payload = freezeInventoryPayload(aggregate(sale.items.map { InventoryPurchaseLine(it.productId, it.quantity) }))
        val existing = state.fulfillments[fulfillmentId]
        if (existing != null) {
            require(existing.saleId == sale.saleId && existing.saleVersion == expectedSaleVersion && existing.payload == payload) { "fulfillment id reused with different sale or payload" }
            return@synchronized existing.copy(payload = freezeInventoryPayload(existing.payload), trackedProductIds = freezeInventoryStrings(existing.trackedProductIds))
        }
        require(expectedSaleVersion == currentSaleVersion) { "sale version conflict" }
        require(state.fulfillments.values.none { it.saleId == sale.saleId }) { "sale already fulfilled" }
        require(state.fulfillments.size < MAX_FULFILLMENTS)
        eligible(sale)
        payload.forEach { require(it.productId in state.products) { "inventory mode is not configured: ${it.productId}" } }
        val tracked = payload.filter { state.products.getValue(it.productId).tracked }
        require(state.adjustments.size + tracked.size <= MAX_ADJUSTMENTS && state.operations + tracked.size <= MAX_OPERATIONS) { "inventory operation limit reached" }
        for (line in tracked) {
            val p = state.products[line.productId] ?: error("inventory product is not configured: ${line.productId}")
            if (p.count < line.quantity) throw InsufficientInventoryException(line.productId, line.quantity, p.count)
        }
        payload.forEachIndexed { index, line -> if (state.products.getValue(line.productId).tracked) {
            val p = state.products.getValue(line.productId); val version = Math.addExact(p.version, 1)
            val operationId = "fulfillment:$fulfillmentId:$index"
            val delta = -line.quantity; val adjustmentReason = "fulfillment $fulfillmentId"
            id(operationId); state.products[line.productId] = Product(p.initialTracked, p.tracked, p.count + delta, version)
            state.operationPayloads[operationId] = Operation(line.productId, delta, adjustmentReason, p.version)
            state.adjustments.add(InventoryAdjustment(operationId, line.productId, delta, adjustmentReason, p.count + delta, version))
        } }
        val record = FulfillmentRecord(fulfillmentId, sale.saleId, expectedSaleVersion, payload, completedAt, freezeInventoryStrings(tracked.map { it.productId }))
        state.fulfillments[fulfillmentId] = record
        commit(record.copy(payload = freezeInventoryPayload(payload), trackedProductIds = freezeInventoryStrings(record.trackedProductIds)))
    }

    private fun eligible(s: Sale) {
        require(s.paid && s.remainingMinor == 0L && !s.requiresReview && !s.lateBitcoinReceipt && s.overpaidMinor == 0L && s.overpaidSats == 0L) { "sale is not eligible for fulfillment" }
        require(s.refunds.isEmpty()) { "sale with refunds is not eligible for fulfillment" }
        require(s.attempts.none { attempt -> attempt.tender == Tender.BITCOIN && attempt.bitcoinEvidence.any { it.chainState == BitcoinChainState.REORGED } }) {
            "sale has reorged Bitcoin activity"
        }
        require(s.attempts.all { attempt -> when (attempt.state) {
            PaymentState.PAID, PaymentState.CONFIRMED, PaymentState.CANCELED -> true
            PaymentState.PARTIAL -> settledPartialAttempt(attempt)
            else -> false
        } }) { "sale has unresolved, unconfirmed, or reorged payment activity" }
        require(s.items.isNotEmpty()) { "sale has no immutable purchased line payload" }
        require(s.items.all { text(it.productId) && it.quantity in 1..MAX_QUANTITY }) { "invalid purchased line payload" }
    }

    private fun <T> commit(value: T): T {
        val bytes = try { validateState(state); encode(state) } catch (e: Throwable) { poisoned = true; throw e }
        try { storage.writeAtomically(bytes) } catch (e: Throwable) { poisoned = true; try { state = decode(storage.read()) } catch (_: Throwable) {}; throw IllegalStateException("inventory write outcome is uncertain; reopen before continuing", e) }
        return value
    }
    private fun healthy() { check(!poisoned) { "inventory write outcome is uncertain; reopen before continuing" } }
    private fun product(id: String) = state.products[id]?.let { InventoryProduct(id, it.tracked, it.count, it.version) } ?: error("unknown product")
    private fun view() = InventorySnapshot(state.products.map { InventoryProduct(it.key, it.value.tracked, it.value.count, it.value.version) }, state.adjustments, state.fulfillments.values.toList())
    private data class Product(val initialTracked: Boolean, val tracked: Boolean, val count: Int, val version: Long)
    private data class Operation(val productId: String, val delta: Int, val reason: String, val expectedVersion: Long)
    private data class ModeOperation(val productId: String, val tracked: Boolean, val reason: String, val expectedVersion: Long)
    private class State {
        val products = linkedMapOf<String, Product>(); val adjustments = mutableListOf<InventoryAdjustment>()
        val operationPayloads = linkedMapOf<String, Operation>(); val modeOperations = linkedMapOf<String, ModeOperation>()
        val fulfillments = linkedMapOf<String, FulfillmentRecord>(); val operations get() = operationPayloads.size + modeOperations.size
    }

    private fun encode(s: State): ByteArray {
        require(s.products.size <= MAX_PRODUCTS && s.adjustments.size <= MAX_ADJUSTMENTS && s.operations <= MAX_OPERATIONS && s.fulfillments.size <= MAX_FULFILLMENTS)
        val b = ByteArrayOutputStream(); DataOutputStream(b).use { o ->
            o.writeInt(MAGIC); o.writeInt(VERSION); o.writeInt(s.products.size)
            s.products.forEach { (id,p) -> o.str(id); o.writeBoolean(p.initialTracked); o.writeBoolean(p.tracked); o.writeInt(p.count); o.writeLong(p.version) }
            o.writeInt(s.adjustments.size); s.adjustments.forEach { a -> o.str(a.operationId); o.str(a.productId); o.writeInt(a.delta); o.str(a.reason); o.writeInt(a.resultingCount); o.writeLong(a.version) }
            o.writeInt(s.operationPayloads.size); s.operationPayloads.forEach { (id,p) -> o.str(id); o.str(p.productId); o.writeInt(p.delta); o.str(p.reason); o.writeLong(p.expectedVersion) }
            o.writeInt(s.modeOperations.size); s.modeOperations.forEach { (id,p) -> o.str(id); o.str(p.productId); o.writeBoolean(p.tracked); o.str(p.reason); o.writeLong(p.expectedVersion) }
            o.writeInt(s.fulfillments.size); s.fulfillments.forEach { (_,f) -> o.str(f.fulfillmentId); o.str(f.saleId); o.writeLong(f.saleVersion); o.writeLong(f.completedAt); o.writeInt(f.payload.size); f.payload.forEach { o.str(it.productId); o.writeInt(it.quantity) }; o.writeInt(f.trackedProductIds.size); f.trackedProductIds.forEach { o.str(it) } }
        }
        return b.toByteArray().also { require(it.size <= MAX_FILE_BYTES) { "inventory exceeds storage limit" } }
    }
    private fun decode(bytes: ByteArray?): State {
        if (bytes == null) return State(); require(bytes.size <= MAX_FILE_BYTES) { "inventory file exceeds size limit" }
        DataInputStream(ByteArrayInputStream(bytes)).use { i ->
            require(i.readInt() == MAGIC && i.readInt() == VERSION) { "unsupported inventory format" }; val s = State()
            repeat(i.count(MAX_PRODUCTS)) { val id=i.str(); val initial=i.bool(); val tracked=i.bool(); val p=Product(initial,tracked,i.readInt(),i.readLong()); id(id); require(p.count >= 0 && p.version >= 0 && s.products.put(id,p)==null) }
            repeat(i.count(MAX_ADJUSTMENTS)) { val a=InventoryAdjustment(i.str(),i.str(),i.readInt(),i.str(),i.readInt(),i.readLong()); require(a.resultingCount>=0 && a.version>=0); s.adjustments.add(a) }
            repeat(i.count(MAX_OPERATIONS)) { val id=i.str(); require(s.operationPayloads.put(id,Operation(i.str(),i.readInt(),i.str(),i.readLong()))==null) }
            repeat(i.count(MAX_OPERATIONS)) { val id=i.str(); require(s.modeOperations.put(id,ModeOperation(i.str(),i.bool(),i.str(),i.readLong()))==null) }
            repeat(i.count(MAX_FULFILLMENTS)) { val id=i.str(); val sale=i.str(); val v=i.readLong(); val at=i.readLong(); val lines=mutableListOf<InventoryPurchaseLine>(); repeat(i.count(MAX_LINES)) { lines.add(InventoryPurchaseLine(i.str(),i.readInt())) }; val tracked=mutableListOf<String>(); repeat(i.count(MAX_LINES)) { tracked.add(i.str()) }; require(v>=0 && at>=0 && id !in s.fulfillments); s.fulfillments[id]=FulfillmentRecord(id,sale,v,freezeInventoryPayload(lines),at,freezeInventoryStrings(tracked)) }
            require(i.available()==0) { "trailing inventory data" }; validateState(s); return s
        }
    }
    private fun DataOutputStream.str(s: String) { val b=s.toByteArray(Charsets.UTF_8); require(b.size<=MAX_TEXT); writeInt(b.size); write(b) }
    private fun DataInputStream.str(): String { val n=readInt(); require(n in 0..MAX_TEXT); val b=ByteArray(n); readFully(b); return Charsets.UTF_8.newDecoder().onMalformedInput(CodingErrorAction.REPORT).onUnmappableCharacter(CodingErrorAction.REPORT).decode(ByteBuffer.wrap(b)).toString() }
    private fun DataInputStream.bool()=when(val n=readUnsignedByte()){0->false;1->true;else->error("invalid boolean $n")}
    private fun DataInputStream.count(max:Int)=readInt().also { require(it in 0..max) }
    private fun id(v:String) { require(text(v) && v.length<=MAX_ID) { "invalid identifier" } }
    private fun operationId(v:String) { id(v); require(!v.startsWith("fulfillment:")) { "operation ID uses a reserved prefix" } }
    private fun validateReason(v:String) { require(text(v) && v.length<=MAX_REASON) { "invalid adjustment reason" } }

    private fun settledPartialAttempt(attempt: PaymentAttempt): Boolean = when (attempt.tender) {
        Tender.CARD -> attempt.cardEvidence.maxByOrNull { it.observedAt }?.let { latest ->
            latest.result == CardResult.COMPLETED && latest.amountMinor > 0 && latest.amountMinor < attempt.amountMinor &&
                attempt.receivedMinor == latest.amountMinor
        } == true
        Tender.BITCOIN -> attempt.bitcoinEvidence
            .filter { it.chainState == BitcoinChainState.ACTIVE && it.receivedSats > 0 }
            .let { active -> active.isNotEmpty() && active.all { it.confirmations >= it.requiredConfirmations } && attempt.receivedMinor > 0 }
    }

    private fun validateState(s: State) {
        require(s.operations <= MAX_OPERATIONS && s.adjustments.size <= MAX_ADJUSTMENTS)
        require(s.operationPayloads.keys.intersect(s.modeOperations.keys).isEmpty()) { "duplicate operation ID" }
        val seenOps = HashSet<String>()
        val counts = HashMap<String, Int>()
        val versions = HashMap<String, Long>()
        val tracked = HashMap<String, Boolean>()
        val usedOps = HashSet<String>()
        for (a in s.adjustments) {
            id(a.operationId); id(a.productId); validateReason(a.reason)
            require(seenOps.add(a.operationId)) { "duplicate adjustment operation ID" }
            val p = s.products[a.productId] ?: error("adjustment references unknown product")
            val priorCount = counts[a.productId]
            if (priorCount == null) {
                require(a.version == 0L && a.resultingCount == a.delta && a.delta >= 0)
                require(a.operationId !in s.modeOperations)
                val op = s.operationPayloads[a.operationId] ?: error("missing opening adjustment operation")
                require(op == Operation(a.productId, a.delta, a.reason, -1L))
                usedOps.add(a.operationId); counts[a.productId] = a.resultingCount; versions[a.productId] = 0L
                tracked[a.productId] = p.initialTracked
                continue
            }
            val oldVersion = versions.getValue(a.productId)
            require(a.version == Math.addExact(oldVersion, 1L)) { "nonsequential inventory version" }
            val mode = s.modeOperations[a.operationId]
            if (mode != null) {
                require(a.operationId !in s.operationPayloads)
                require(mode.productId == a.productId && mode.reason == a.reason && mode.expectedVersion == oldVersion)
                require(a.delta == 0 && a.resultingCount == priorCount)
                tracked[a.productId] = mode.tracked
            } else {
                val op = s.operationPayloads[a.operationId] ?: error("missing adjustment operation")
                require(op.productId == a.productId && op.delta == a.delta && op.reason == a.reason && op.expectedVersion == oldVersion)
                val next = Math.addExact(priorCount, a.delta)
                require(next >= 0 && next == a.resultingCount)
                counts[a.productId] = next
            }
            require(usedOps.add(a.operationId)) { "operation represented more than once" }
            versions[a.productId] = a.version
        }
        require(counts.keys == s.products.keys) { "product has no opening adjustment" }
        require(usedOps == s.operationPayloads.keys + s.modeOperations.keys) { "operation journal does not match adjustment history" }
        for ((id, p) in s.products) {
            id(id); require(counts[id] == p.count && versions[id] == p.version && tracked[id] == p.tracked) { "inventory product does not match adjustment history" }
        }
        val saleIds = HashSet<String>()
        for ((id, f) in s.fulfillments) {
            require(id == f.fulfillmentId); id(id); id(f.saleId)
            require(saleIds.add(f.saleId)) { "sale has multiple fulfillment records" }
            validateManifest(f.payload)
            f.payload.forEach { line -> require(line.productId in s.products) { "fulfillment references unknown product" } }
            require(f.trackedProductIds.distinct() == f.trackedProductIds && f.trackedProductIds.all { productId -> f.payload.any { it.productId == productId } }) { "invalid fulfillment stock manifest" }
            require(f.trackedProductIds == f.payload.map { it.productId }.filter { it in f.trackedProductIds }) { "fulfillment stock manifest order mismatch" }
            val matched = f.payload.indices.filter { index -> "fulfillment:$id:$index" in s.operationPayloads }
            require(matched == matched.filter { index ->
                val line = f.payload[index]; val operationId = "fulfillment:$id:$index"; val op = s.operationPayloads.getValue(operationId)
                op.productId == line.productId && op.delta == -line.quantity && op.reason == "fulfillment $id"
            }) { "fulfillment adjustment does not match manifest" }
            require(matched.all { index -> "fulfillment:$id:$index" in seenOps })
            require(matched.map { f.payload[it].productId } == f.trackedProductIds) { "fulfillment stock manifest does not match applied adjustments" }
        }
        for (operationId in s.operationPayloads.keys) if (operationId.startsWith("fulfillment:")) {
            val fulfillmentId = operationId.removePrefix("fulfillment:").substringBeforeLast(':')
            val index = operationId.substringAfterLast(':').toIntOrNull() ?: error("invalid fulfillment operation ID")
            val f = s.fulfillments[fulfillmentId] ?: error("adjustment references unknown fulfillment")
            require(index in f.payload.indices)
        }
    }

    companion object {
        private const val MAGIC=0x4f424349; private const val VERSION=1; private const val MAX_FILE_BYTES=1024*1024; private const val MAX_TEXT=512; private const val MAX_ID=220; private const val MAX_REASON=200
        private const val MAX_PRODUCTS=500; private const val MAX_ADJUSTMENTS=5000; private const val MAX_OPERATIONS=5000; private const val MAX_FULFILLMENTS=5000; private const val MAX_LINES=100; private const val MAX_QUANTITY=999
        private const val MAX_AGGREGATED_QUANTITY=MAX_LINES*MAX_QUANTITY
        private fun text(s:String)=s.isNotBlank() && s.none { it.isISOControl() }
        private fun aggregate(input:List<InventoryPurchaseLine>):List<InventoryPurchaseLine> {
            require(input.isNotEmpty() && input.size <= MAX_LINES) { "invalid purchased line count" }
            val sums=linkedMapOf<String,Int>()
            input.forEach { line ->
                require(text(line.productId) && line.productId.length <= 220 && line.quantity in 1..MAX_QUANTITY) { "invalid purchased line" }
                val total=Math.addExact(sums[line.productId]?:0,line.quantity)
                require(total <= MAX_AGGREGATED_QUANTITY) { "aggregate quantity limit exceeded" }
                sums[line.productId]=total
            }
            return sums.map { InventoryPurchaseLine(it.key,it.value) }
        }
        private fun validateManifest(payload:List<InventoryPurchaseLine>) {
            require(payload.isNotEmpty() && payload.size <= MAX_LINES) { "invalid fulfillment payload size" }
            require(payload.map { it.productId }.distinct().size == payload.size) { "fulfillment payload products must be unique" }
            payload.forEach { require(text(it.productId) && it.productId.length <= 220 && it.quantity in 1..MAX_AGGREGATED_QUANTITY) { "invalid fulfillment payload quantity" } }
        }
    }
}
