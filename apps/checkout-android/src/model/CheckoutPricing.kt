package org.openbmt.checkout

import java.math.BigInteger
import java.util.Collections

/** Explicit, integer-minor-unit pricing for the bounded standalone checkout subset.
 * USD is the initial integration currency; callers must not pass floating point amounts.
 * Tax is one flat component per basket (no compounding or jurisdiction lookup). This is
 * arithmetic support only: it does not determine legal rates, exemptions, nexus, or filing.
 * Multiple jurisdictions/components, compound tax, category exemptions, and tax-inclusive
 * line-level jurisdiction differences are outside this bounded subset.
 */
enum class TaxTreatment { EXCLUSIVE, INCLUSIVE }

data class SelectedModifier(val modifierId: String, val name: String, val amountMinor: Long)
data class CheckoutLineInput(
    val lineId: String,
    val productId: String,
    val name: String,
    val unitMinor: Long,
    val quantity: Int,
    val modifiers: List<SelectedModifier> = emptyList(),
)
data class CheckoutTaxRule(val taxId: String, val name: String, val rateBasisPoints: Int,
                           val treatment: TaxTreatment)

data class FrozenModifier(val modifierId: String, val name: String, val amountMinor: Long)
class PricedLine internal constructor(
    val lineId: String, val productId: String, val name: String, val unitMinor: Long,
    val quantity: Int, modifiers: List<FrozenModifier>, val subtotalMinor: Long,
    val discountMinor: Long, val taxMinor: Long,
) {
    private val modifierSnapshot = Collections.unmodifiableList(ArrayList(modifiers))
    val modifiers: List<FrozenModifier> get() = modifierSnapshot

    override fun equals(other: Any?): Boolean = other is PricedLine &&
        lineId == other.lineId && productId == other.productId && name == other.name &&
        unitMinor == other.unitMinor && quantity == other.quantity && modifiers == other.modifiers &&
        subtotalMinor == other.subtotalMinor && discountMinor == other.discountMinor && taxMinor == other.taxMinor

    override fun hashCode(): Int {
        var result = lineId.hashCode()
        result = 31 * result + productId.hashCode(); result = 31 * result + name.hashCode()
        result = 31 * result + unitMinor.hashCode(); result = 31 * result + quantity
        result = 31 * result + modifiers.hashCode(); result = 31 * result + subtotalMinor.hashCode()
        result = 31 * result + discountMinor.hashCode(); result = 31 * result + taxMinor.hashCode()
        return result
    }
}
data class TaxSnapshot(val taxId: String, val name: String, val rateBasisPoints: Int,
                       val treatment: TaxTreatment, val amountMinor: Long)

/** Frozen calculation output. Lists are defensively copied on construction and access. */
class CheckoutPricingSnapshot internal constructor(
    lines: List<PricedLine>, val subtotalMinor: Long, val discountMinor: Long,
    val taxableAfterDiscountMinor: Long, taxes: List<TaxSnapshot>, val taxMinor: Long,
    val tipMinor: Long, val totalMinor: Long,
) {
    private val lineSnapshot = Collections.unmodifiableList(ArrayList(lines))
    private val taxSnapshot = Collections.unmodifiableList(ArrayList(taxes))
    val lines: List<PricedLine> get() = lineSnapshot
    val taxes: List<TaxSnapshot> get() = taxSnapshot

    override fun equals(other: Any?): Boolean = other is CheckoutPricingSnapshot &&
        lines == other.lines && subtotalMinor == other.subtotalMinor && discountMinor == other.discountMinor &&
        taxableAfterDiscountMinor == other.taxableAfterDiscountMinor && taxes == other.taxes &&
        taxMinor == other.taxMinor && tipMinor == other.tipMinor && totalMinor == other.totalMinor

    override fun hashCode(): Int {
        var result = lines.hashCode()
        result = 31 * result + subtotalMinor.hashCode(); result = 31 * result + discountMinor.hashCode()
        result = 31 * result + taxableAfterDiscountMinor.hashCode(); result = 31 * result + taxes.hashCode()
        result = 31 * result + taxMinor.hashCode(); result = 31 * result + tipMinor.hashCode()
        result = 31 * result + totalMinor.hashCode()
        return result
    }
}

object CheckoutPricing {
    /**
     * Computes a frozen basket. Discount is a fixed amount distributed across line subtotals
     * proportionally, with leftover pennies assigned by largest remainder then input order.
     * Tax is rounded half-up independently per line. Inclusive tax is extracted from the
     * post-discount listed price using rate/(10000+rate); exclusive tax is added to it.
     * Tip is an explicit amount added after tax. All outputs are checked to fit nonnegative Long.
     */
    fun freeze(
        lines: List<CheckoutLineInput>, discountMinor: Long = 0,
        tax: CheckoutTaxRule? = null, tipMinor: Long = 0,
    ): CheckoutPricingSnapshot {
        require(lines.isNotEmpty()) { "sale must contain at least one line" }
        require(lines.size <= MAX_LINES) { "too many sale lines" }
        require(discountMinor >= 0 && tipMinor >= 0) { "discount and tip must be nonnegative" }
        val ids = HashSet<String>()
        val copied = lines.map { input ->
            require(validText(input.lineId, 120, 320) && ids.add(input.lineId)) { "line IDs must be valid, nonblank, and unique" }
            require(validText(input.productId, 120, 320) && validText(input.name, 120, 320)) { "product identity and name are required" }
            require(input.quantity in 1..MAX_QUANTITY) { "quantity must be between 1 and $MAX_QUANTITY" }
            require(input.unitMinor >= 0) { "unit amount must be nonnegative" }
            val modifierIds = HashSet<String>()
            require(input.modifiers.size <= MAX_MODIFIERS_PER_LINE) { "too many modifiers on line" }
            val mods = input.modifiers.map {
                require(validText(it.modifierId, 120, 320) && modifierIds.add(it.modifierId)) { "modifier IDs must be valid, nonblank, and unique per line" }
                require(validText(it.name, 120, 320) && it.amountMinor >= 0) { "modifier must have a valid name and nonnegative amount" }
                FrozenModifier(it.modifierId, it.name, it.amountMinor)
            }
            val unit = add(input.unitMinor, mods.fold(0L) { acc, mod -> add(acc, mod.amountMinor) })
            val ext = multiply(unit, input.quantity.toLong())
            Triple(input, mods, ext)
        }
        if (tax != null) {
            require(validText(tax.taxId, 120, 320) && validText(tax.name, 120, 320)) { "tax identity and name are required" }
            require(tax.rateBasisPoints in 0..100_000) { "tax rate must be between 0 and 1000 percent" }
        }
        val subtotal = copied.fold(0L) { acc, item -> add(acc, item.third) }
        require(discountMinor <= subtotal) { "discount cannot exceed subtotal" }
        val allocated = allocate(discountMinor, copied.map { it.third })
        var taxable = 0L
        var aggregateTax = 0L
        val priced = copied.mapIndexed { index, (input, mods, lineSubtotal) ->
            val afterDiscount = lineSubtotal - allocated[index]
            taxable = add(taxable, afterDiscount)
            val lineTax = if (tax == null) 0L else when (tax.treatment) {
                TaxTreatment.EXCLUSIVE -> roundedRatio(afterDiscount, tax.rateBasisPoints.toLong(), 10_000)
                TaxTreatment.INCLUSIVE -> roundedRatio(afterDiscount, tax.rateBasisPoints.toLong(), 10_000L + tax.rateBasisPoints)
            }
            aggregateTax = add(aggregateTax, lineTax)
            PricedLine(input.lineId, input.productId, input.name, input.unitMinor, input.quantity,
                mods, lineSubtotal, allocated[index], lineTax)
        }
        val taxes = if (tax == null) emptyList() else listOf(
            TaxSnapshot(tax.taxId, tax.name, tax.rateBasisPoints, tax.treatment, aggregateTax))
        val merchandise = when (tax?.treatment) {
            TaxTreatment.EXCLUSIVE, null -> add(taxable, aggregateTax)
            TaxTreatment.INCLUSIVE -> taxable
        }
        val total = add(add(merchandise, tipMinor), 0)
        return CheckoutPricingSnapshot(priced, subtotal, discountMinor, taxable, taxes,
            aggregateTax, tipMinor, total)
    }

    private fun allocate(amount: Long, weights: List<Long>): List<Long> {
        if (amount == 0L) return List(weights.size) { 0L }
        val total = weights.fold(BigInteger.ZERO) { a, b -> a + BigInteger.valueOf(b) }
        require(total.signum() > 0) { "cannot discount a zero-value basket" }
        val target = BigInteger.valueOf(amount)
        val shares = weights.map { weight ->
            val qr = target.multiply(BigInteger.valueOf(weight)).divideAndRemainder(total)
            qr[0].longValueExact() to qr[1]
        }
        val result = shares.map { it.first }.toMutableList()
        var pennies = amount - result.fold(0L, ::add)
        val order = shares.indices.sortedWith(compareByDescending<Int> { shares[it].second }.thenBy { it })
        for (i in order) { if (pennies == 0L) break; result[i]++; pennies-- }
        check(pennies == 0L)
        return result
    }

    private fun roundedRatio(amount: Long, numerator: Long, denominator: Long): Long {
        val n = BigInteger.valueOf(amount).multiply(BigInteger.valueOf(numerator))
        return n.add(BigInteger.valueOf(denominator / 2)).divide(BigInteger.valueOf(denominator)).longValueExact()
    }
    private fun add(a: Long, b: Long): Long = Math.addExact(a, b).also { require(it >= 0) { "amount overflow" } }
    private fun multiply(a: Long, b: Long): Long = Math.multiplyExact(a, b).also { require(it >= 0) { "amount overflow" } }
    private fun validText(value: String, chars: Int, bytes: Int): Boolean =
        value.isNotBlank() && value.length <= chars && value.none { it.isISOControl() } && value.toByteArray(Charsets.UTF_8).size <= bytes

    private const val MAX_LINES = 100
    private const val MAX_QUANTITY = 999
    private const val MAX_MODIFIERS_PER_LINE = 32
}
