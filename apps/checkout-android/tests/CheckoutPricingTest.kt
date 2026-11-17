package org.openbmt.checkout

/** Arithmetic and snapshot behavior checks for CheckoutPricing. */
object CheckoutPricingTest {
    fun run() {
        exactExtensionsAndLargestRemainder()
        taxBoundariesAndTip()
        rejectsInvalidAndOverflow()
        freezesCallerCollections()
        structuralEqualityAndLargeIntermediates()
    }

    private fun exactExtensionsAndLargestRemainder() {
        val p = CheckoutPricing.freeze(listOf(
            CheckoutLineInput("a", "p1", "One", 5, 1),
            CheckoutLineInput("b", "p2", "Two", 5, 1),
            CheckoutLineInput("c", "p3", "Three", 5, 1),
        ), discountMinor = 2)
        check(p.subtotalMinor == 15L && p.discountMinor == 2L && p.totalMinor == 13L)
        check(p.lines.map { it.discountMinor } == listOf(1L, 1L, 0L))
        val modifier = CheckoutPricing.freeze(listOf(CheckoutLineInput("x", "p", "Meal", 499, 2,
            listOf(SelectedModifier("large", "Large", 75)))))
        check(modifier.lines.single().subtotalMinor == 1148L)
        check(modifier.totalMinor == 1148L)
    }

    private fun taxBoundariesAndTip() {
        val lines = listOf(CheckoutLineInput("a", "p", "A", 1, 1), CheckoutLineInput("b", "q", "B", 1, 1))
        val exc = CheckoutPricing.freeze(lines, tax = CheckoutTaxRule("t", "Tax", 5000, TaxTreatment.EXCLUSIVE), tipMinor = 3)
        check(exc.taxMinor == 2L) // half-up per line: each half penny rounds up
        check(exc.totalMinor == 7L)
        val inc = CheckoutPricing.freeze(listOf(CheckoutLineInput("a", "p", "A", 105, 1)),
            tax = CheckoutTaxRule("t", "Tax", 500, TaxTreatment.INCLUSIVE))
        check(inc.taxMinor == 5L && inc.totalMinor == 105L && inc.taxableAfterDiscountMinor == 105L)
        val discounted = CheckoutPricing.freeze(listOf(CheckoutLineInput("a", "p", "A", 110, 1)),
            discountMinor = 10, tax = CheckoutTaxRule("t", "Tax", 1000, TaxTreatment.EXCLUSIVE))
        check(discounted.taxMinor == 10L && discounted.totalMinor == 110L)
    }

    private fun rejectsInvalidAndOverflow() {
        check(runCatching { CheckoutPricing.freeze(listOf(CheckoutLineInput("a", "p", "A", 1, 0))) }.isFailure)
        check(runCatching { CheckoutPricing.freeze(listOf(CheckoutLineInput("a", "p", "A", Long.MAX_VALUE, 2))) }.isFailure)
        check(runCatching { CheckoutPricing.freeze(listOf(CheckoutLineInput("a", "p", "A", 1, 1)), discountMinor = 2) }.isFailure)
        check(runCatching { CheckoutPricing.freeze(listOf(CheckoutLineInput("a", "p", "A", Long.MAX_VALUE, 1)), tipMinor = 1) }.isFailure)
        check(runCatching { CheckoutPricing.freeze(listOf(CheckoutLineInput("x", "p", "A", 1, 1), CheckoutLineInput("x", "q", "B", 1, 1))) }.isFailure)
    }

    private fun freezesCallerCollections() {
        val mods = mutableListOf(SelectedModifier("m", "Milk", 25))
        val source = mutableListOf(CheckoutLineInput("l", "p", "Coffee", 300, 1, mods))
        val snapshot = CheckoutPricing.freeze(source)
        mods.clear(); source.clear()
        check(snapshot.lines.single().modifiers.single().name == "Milk")
        check(snapshot.lines.single().subtotalMinor == 325L)
        check(runCatching { (snapshot.lines as MutableList).clear() }.isFailure)
        check(runCatching { (snapshot.lines.single().modifiers as MutableList).clear() }.isFailure)
        check(snapshot.lines.single().modifiers.single().name == "Milk")
    }

    private fun structuralEqualityAndLargeIntermediates() {
        val input = listOf(CheckoutLineInput("x", "p", "Large", Long.MAX_VALUE / 2, 1))
        val tax = CheckoutTaxRule("t", "Tax", 100, TaxTreatment.EXCLUSIVE)
        val first = CheckoutPricing.freeze(input, tax = tax)
        val second = CheckoutPricing.freeze(input, tax = tax)
        check(first == second && first.hashCode() == second.hashCode())
        check(first.totalMinor > first.subtotalMinor)
        check(first.lines == second.lines && first.lines.single().hashCode() == second.lines.single().hashCode())
    }
}
