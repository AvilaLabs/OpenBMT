package org.openbmt.checkout

import java.io.ByteArrayInputStream
import java.io.ByteArrayOutputStream
import java.io.DataInputStream
import java.io.DataOutputStream
import java.nio.ByteBuffer
import java.nio.charset.CodingErrorAction
import java.util.Collections

private const val CATALOG_MAGIC = 0x43415431
private const val CATALOG_VERSION = 1
private const val CATALOG_MAX_BYTES = 2 * 1024 * 1024
private const val CATALOG_MAX_PRODUCTS = 2000
private const val CATALOG_MAX_VARIANTS = 100
private const val CATALOG_MAX_MODIFIERS = 32
private const val CATALOG_MAX_COMMANDS = 5000
private const val CATALOG_MAX_REVISIONS = 1000

data class CatalogVariant(val variantId: String, val name: String, val priceMinor: Long,
                          val sku: String? = null, val barcode: String? = null)
data class CatalogModifier(val modifierId: String, val name: String, val amountMinor: Long)

/** Immutable published product revision. Prices are exact USD cents. */
class CatalogProduct internal constructor(
    val productId: String, val version: Long, val name: String, val archived: Boolean,
    variants: List<CatalogVariant>, modifiers: List<CatalogModifier>,
) {
    private val variantData = Collections.unmodifiableList(ArrayList(variants))
    private val modifierData = Collections.unmodifiableList(ArrayList(modifiers))
    val variants: List<CatalogVariant> get() = variantData
    val modifiers: List<CatalogModifier> get() = modifierData
    override fun equals(other: Any?): Boolean = other is CatalogProduct && productId == other.productId &&
        version == other.version && name == other.name && archived == other.archived && variants == other.variants && modifiers == other.modifiers
    override fun hashCode(): Int = listOf(productId, version, name, archived, variants, modifiers).hashCode()
}

/** Durable local catalog. Each successful mutation appends an immutable revision before returning. */
class CheckoutCatalog(private val storage: DurableLedgerStorage) {
    private val lock = Any()
    private var state = decode(storage.read())
    private var poisoned = false

    fun createProduct(commandId: String, productId: String, name: String,
                      variants: List<CatalogVariant>, modifiers: List<CatalogModifier> = emptyList()): CatalogProduct =
        mutate(commandId, "create", productId, null, name, variants, modifiers, false)

    fun editProduct(commandId: String, productId: String, expectedVersion: Long, name: String,
                    variants: List<CatalogVariant>, modifiers: List<CatalogModifier> = emptyList()): CatalogProduct =
        mutate(commandId, "edit", productId, expectedVersion, name, variants, modifiers, false)

    fun archiveProduct(commandId: String, productId: String, expectedVersion: Long): CatalogProduct =
        mutate(commandId, "archive", productId, expectedVersion, "", emptyList(), emptyList(), true)

    fun getProduct(productId: String): CatalogProduct? = synchronized(lock) { healthy(); state.products[productId]?.lastOrNull() }
    fun getProductVersion(productId: String, version: Long): CatalogProduct? = synchronized(lock) {
        healthy(); state.products[productId]?.firstOrNull { it.version == version }
    }
    /** Bounded, deterministic current-catalog snapshot for native product browsing. */
    fun listProducts(includeArchived: Boolean = false): List<CatalogProduct> = synchronized(lock) {
        healthy()
        Collections.unmodifiableList(ArrayList(state.products.values.mapNotNull { it.lastOrNull() }
            .filter { includeArchived || !it.archived }.sortedBy { it.productId }))
    }
    fun findBySku(sku: String): Pair<CatalogProduct, CatalogVariant>? = findIdentity(sku, false)
    fun findByBarcode(barcode: String): Pair<CatalogProduct, CatalogVariant>? = findIdentity(barcode, true)

    fun search(query: String): List<CatalogProduct> = synchronized(lock) {
        healthy(); val q = query.trim().lowercase()
        if (q.isEmpty()) emptyList() else state.products.values.mapNotNull { it.lastOrNull() }
            .filter { !it.archived && (it.name.lowercase().contains(q) || it.variants.any { v ->
                v.name.lowercase().contains(q) || v.sku?.lowercase()?.contains(q) == true || v.barcode?.contains(q) == true
            }) }.take(100)
    }

    /** Resolves current catalog selection into the existing pricing input; returned values are frozen copies. */
    fun snapshotLine(lineId: String, productId: String, variantId: String, quantity: Int,
                     selectedModifierIds: List<String> = emptyList()): CheckoutLineInput = synchronized(lock) {
        healthy(); validCatalogId(lineId, "line id"); require(quantity in 1..999) { "quantity must be between 1 and 999" }
        val p = state.products[productId]?.lastOrNull() ?: throw IllegalArgumentException("product not found")
        require(!p.archived) { "product is archived" }
        val variant = p.variants.firstOrNull { it.variantId == variantId } ?: throw IllegalArgumentException("variant not found")
        require(selectedModifierIds.size <= CATALOG_MAX_MODIFIERS && selectedModifierIds.distinct().size == selectedModifierIds.size) { "invalid modifier selection" }
        val mods = selectedModifierIds.map { id -> p.modifiers.firstOrNull { it.modifierId == id } ?: throw IllegalArgumentException("modifier not found") }
        val displayName = "${p.name} — ${variant.name}"
        validPricingText(displayName, 120, "display name")
        CheckoutLineInput(lineId, p.productId, displayName,
            variant.priceMinor, quantity, Collections.unmodifiableList(ArrayList(mods.map { SelectedModifier(it.modifierId, it.name, it.amountMinor) })))
    }

    private fun findIdentity(value: String, barcode: Boolean): Pair<CatalogProduct, CatalogVariant>? = synchronized(lock) {
        healthy(); val found = state.products.values.mapNotNull { it.lastOrNull() }.asSequence().filter { !it.archived }
            .flatMap { p -> p.variants.asSequence().filter { if (barcode) it.barcode == value else it.sku == value }.map { p to it } }.firstOrNull()
        found
    }

    private fun mutate(commandId: String, kind: String, productId: String, expectedVersion: Long?, name: String,
                       variants: List<CatalogVariant>, modifiers: List<CatalogModifier>, archive: Boolean): CatalogProduct = synchronized(lock) {
        healthy(); validText(commandId, 160, "command id"); validCatalogId(productId, "product id")
        if (expectedVersion != null) require(expectedVersion >= 1) { "invalid expected version" }
        val canonical = encodeCommand(kind, productId, expectedVersion, name, variants, modifiers)
        state.commands[commandId]?.let { prior ->
            require(prior.payload.contentEquals(canonical)) { "command id reused with different payload" }
            return@synchronized state.products[prior.productId]?.firstOrNull { it.version == prior.version }
                ?: error("catalog command result missing")
        }
        val history = state.products[productId]
        val revision = if (kind == "create") {
            require(history == null) { "product ID has already been used" }
            require(state.products.size < CATALOG_MAX_PRODUCTS) { "catalog product limit reached" }
            validate(name, variants, modifiers, productId)
            CatalogProduct(productId, 1, name, false, variants, modifiers)
        } else {
            val current = history?.lastOrNull() ?: throw IllegalArgumentException("product not found")
            require(current.version == expectedVersion) { "stale product version" }
            require(!current.archived) { "product is archived" }
            require(history.size < CATALOG_MAX_REVISIONS) { "product revision limit reached" }
            if (archive) CatalogProduct(productId, current.version + 1, current.name, true, current.variants, current.modifiers)
            else { validate(name, variants, modifiers, productId); CatalogProduct(productId, current.version + 1, name, false, variants, modifiers) }
        }
        validateIdentities(productId, revision.variants)
        require(state.commands.size < CATALOG_MAX_COMMANDS) { "catalog command limit reached" }
        val next = state.copyDeep()
        next.products.getOrPut(productId) { ArrayList() }.add(revision)
        next.commands[commandId] = Command(canonical, productId, revision.version)
        commit(next)
        revision
    }

    private fun validate(name: String, variants: List<CatalogVariant>, modifiers: List<CatalogModifier>, productId: String) {
        validateRevision(name, variants, modifiers)
    }

    private fun validateRevision(name: String, variants: List<CatalogVariant>, modifiers: List<CatalogModifier>) {
        validPricingText(name, 120, "product name"); require(variants.isNotEmpty() && variants.size <= CATALOG_MAX_VARIANTS) { "product must have 1 to $CATALOG_MAX_VARIANTS variants" }
        require(variants.map { it.variantId }.distinct().size == variants.size) { "variant IDs must be unique" }
        variants.forEach { v -> validCatalogId(v.variantId, "variant id"); validPricingText(v.name, 120, "variant name"); validPricingText("$name — ${v.name}", 120, "display name"); require(v.priceMinor >= 0) { "price must be nonnegative USD cents" }; v.sku?.let { validText(it, 160, "SKU") }; v.barcode?.let { validText(it, 160, "barcode") } }
        require(modifiers.size <= CATALOG_MAX_MODIFIERS && modifiers.map { it.modifierId }.distinct().size == modifiers.size) { "invalid modifier definitions" }
        modifiers.forEach { m -> validCatalogId(m.modifierId, "modifier id"); validPricingText(m.name, 120, "modifier name"); require(m.amountMinor >= 0) { "modifier price must be nonnegative USD cents" } }
    }

    private fun validateIdentities(productId: String, variants: List<CatalogVariant>) {
        val candidate = variants.flatMap { v -> listOfNotNull(v.sku?.let { "S:$it" }, v.barcode?.let { "B:$it" }) }
        require(candidate.distinct().size == candidate.size) { "SKU and barcode identifiers must be unique" }
        state.products.values.flatten().filter { it.productId != productId }.flatMap { it.variants }.forEach { old ->
            for (v in variants) {
                require(v.sku == null || old.sku == null || v.sku != old.sku) { "SKU already exists" }
                require(v.barcode == null || old.barcode == null || v.barcode != old.barcode) { "barcode already exists" }
            }
        }
    }

    private fun commit(next: State) {
        val bytes = encode(next)
        try { storage.writeAtomically(bytes); state = next } catch (e: Throwable) { poisoned = true; throw IllegalStateException("catalog write outcome is uncertain; reopen before continuing", e) }
    }
    private fun healthy() { check(!poisoned) { "catalog is poisoned after uncertain write; reopen before continuing" } }

    private fun encodeCommand(kind: String, productId: String, expectedVersion: Long?, name: String,
                              variants: List<CatalogVariant>, modifiers: List<CatalogModifier>): ByteArray =
        ByteArrayOutputStream().also { b -> DataOutputStream(b).use { o -> o.str(kind); o.str(productId); o.writeBoolean(expectedVersion != null); expectedVersion?.let { o.writeLong(it) }; o.str(name); o.writeInt(variants.size); variants.forEach { o.variant(it) }; o.writeInt(modifiers.size); modifiers.forEach { o.modifier(it) } } }.toByteArray()

    private data class Command(val payload: ByteArray, val productId: String, val version: Long)
    private data class State(val products: LinkedHashMap<String, MutableList<CatalogProduct>> = LinkedHashMap(), val commands: LinkedHashMap<String, Command> = LinkedHashMap()) {
        fun copyDeep() = State(LinkedHashMap<String, MutableList<CatalogProduct>>().also { out -> products.forEach { (k,v) -> out[k] = ArrayList(v) } }, LinkedHashMap(commands))
    }

    private fun encode(s: State): ByteArray = ByteArrayOutputStream().also { b -> DataOutputStream(b).use { o ->
        o.writeInt(CATALOG_MAGIC); o.writeInt(CATALOG_VERSION); o.writeInt(s.products.size)
        s.products.forEach { (id, revisions) -> o.str(id); o.writeInt(revisions.size); revisions.forEach { p -> o.writeLong(p.version); o.str(p.name); o.writeBoolean(p.archived); o.writeInt(p.variants.size); p.variants.forEach { o.variant(it) }; o.writeInt(p.modifiers.size); p.modifiers.forEach { o.modifier(it) } } }
        o.writeInt(s.commands.size); s.commands.forEach { (id,c) -> o.str(id); o.writeInt(c.payload.size); o.write(c.payload); o.str(c.productId); o.writeLong(c.version) }
    } }.toByteArray().also { require(it.size <= CATALOG_MAX_BYTES) { "catalog exceeds storage limit" } }

    private fun decode(bytes: ByteArray?): State {
        if (bytes == null) return State()
        require(bytes.size <= CATALOG_MAX_BYTES) { "catalog exceeds storage limit" }
        DataInputStream(ByteArrayInputStream(bytes)).use { i ->
            require(i.readInt() == CATALOG_MAGIC && i.readInt() == CATALOG_VERSION) { "unsupported catalog format" }
            val s = State(); repeat(i.count(CATALOG_MAX_PRODUCTS)) {
                val id = i.str(); validCatalogId(id, "product id"); require(!s.products.containsKey(id)) { "duplicate product" }
                val rows = ArrayList<CatalogProduct>(); var expected = 1L
                repeat(i.count(CATALOG_MAX_REVISIONS)) { val version = i.readLong(); require(version == expected++) { "invalid product version sequence" }; val name = i.str(); val archived = i.readBoolean(); val vs = ArrayList<CatalogVariant>(); repeat(i.count(CATALOG_MAX_VARIANTS)) { vs.add(i.variant()) }; val ms = ArrayList<CatalogModifier>(); repeat(i.count(CATALOG_MAX_MODIFIERS)) { ms.add(i.modifier()) }; validateRevision(name, vs, ms); validateIdentitiesAgainstDecoded(id, vs, s.products); require(!rows.lastOrNull()?.archived.orFalse()) { "revision follows archived product" }; rows.add(CatalogProduct(id, version, name, archived, vs, ms)) }
                require(rows.isNotEmpty()); s.products[id] = rows
            }
            repeat(i.count(CATALOG_MAX_COMMANDS)) { val id = i.str(); val n = i.count(8192); val payload = ByteArray(n); i.readFully(payload); val product = i.str(); val version = i.readLong(); require(s.products[product]?.any { it.version == version } == true && !s.commands.containsKey(id)); s.commands[id] = Command(payload, product, version) }
            require(i.read() == -1) { "trailing catalog bytes" }; return s
        }
    }

    private fun validText(value: String, max: Int, label: String) { require(value.isNotBlank() && value.length <= max && value.none { it.isISOControl() } && value.toByteArray(Charsets.UTF_8).size <= 1024) { "invalid $label" } }
    private fun validPricingText(value: String, max: Int, label: String) {
        validText(value, max, label)
        require(value.toByteArray(Charsets.UTF_8).size <= 320) { "$label exceeds checkout pricing text limit" }
    }
    private fun validCatalogId(value: String, label: String) {
        require(value.isNotBlank() && value.length <= 120 && value.all { it.code in 0x21..0x7e }) { "invalid $label" }
    }
    private fun validateIdentitiesAgainstDecoded(productId: String, variants: List<CatalogVariant>, priorProducts: Map<String, List<CatalogProduct>>) {
        val own = variants.flatMap { listOfNotNull(it.sku?.let { sku -> "S:$sku" }, it.barcode?.let { barcode -> "B:$barcode" }) }
        require(own.distinct().size == own.size) { "duplicate SKU or barcode within product revision" }
        priorProducts.forEach { (otherId, revisions) -> if (otherId != productId) revisions.flatMap { it.variants }.forEach { old ->
            variants.forEach { v ->
                require(v.sku == null || old.sku == null || v.sku != old.sku) { "duplicate SKU across products" }
                require(v.barcode == null || old.barcode == null || v.barcode != old.barcode) { "duplicate barcode across products" }
            }
        } }
    }
    private fun Boolean?.orFalse() = this ?: false
    private fun DataOutputStream.str(v: String) { val b = v.toByteArray(Charsets.UTF_8); require(b.size <= 1024); writeInt(b.size); write(b) }
    private fun DataInputStream.str(): String { val n = count(1024); val b = ByteArray(n); readFully(b); return Charsets.UTF_8.newDecoder().onMalformedInput(CodingErrorAction.REPORT).onUnmappableCharacter(CodingErrorAction.REPORT).decode(ByteBuffer.wrap(b)).toString() }
    private fun DataInputStream.count(max: Int): Int { val n = readInt(); require(n in 0..max) { "invalid catalog count" }; return n }
    private fun DataOutputStream.variant(v: CatalogVariant) { str(v.variantId); str(v.name); writeLong(v.priceMinor); nullable(v.sku); nullable(v.barcode) }
    private fun DataInputStream.variant() = CatalogVariant(str(), str(), readLong(), nullable(), nullable())
    private fun DataOutputStream.modifier(v: CatalogModifier) { str(v.modifierId); str(v.name); writeLong(v.amountMinor) }
    private fun DataInputStream.modifier() = CatalogModifier(str(), str(), readLong())
    private fun DataOutputStream.nullable(v: String?) { writeBoolean(v != null); if (v != null) str(v) }
    private fun DataInputStream.nullable(): String? = if (readBoolean()) str() else null
}
