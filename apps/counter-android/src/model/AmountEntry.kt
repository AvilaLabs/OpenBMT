package org.openbmt.counter.app

/**
 * A currency the keypad can write: its code, and how many digits follow the
 * point. The shop's signed settings carry no currency; the counter's price
 * sources are all in one (the table of `openbmt-rates`), so the shop's
 * currency is theirs.
 */
class Money(val code: String, val exponent: Int) {
    /** [minor] written to be read: "12.34 USD". */
    fun show(minor: Long): String = "${plain(minor)} $code"

    /** [minor] with separators and the point, no code: "1,234.50". */
    fun plain(minor: Long): String {
        val digits = minor.toULong().toString().padStart(exponent + 1, '0')
        val whole = digits.substring(0, digits.length - exponent)
        val out = StringBuilder()
        for ((i, c) in whole.withIndex()) {
            if (i > 0 && (whole.length - i) % 3 == 0) out.append(',')
            out.append(c)
        }
        if (exponent > 0) out.append('.').append(digits.substring(digits.length - exponent))
        return out.toString()
    }

    companion object {
        /** The currencies of the price table. Only USD has sources so far (RT-03). */
        fun of(code: String): Money? = when (code) {
            "USD", "EUR", "GBP", "CAD", "AUD", "CHF" -> Money(code, 2)
            "JPY" -> Money(code, 0)
            else -> null
        }
    }
}

/**
 * The cashier's keypad: digits push into the minor units, so 1, 2, 3, 4 is
 * 12.34. At most [MAX_DIGITS] digits.
 */
class AmountEntry(private val money: Money) {
    private var digits = ""

    fun press(d: Char): Boolean {
        if (d !in '0'..'9' || digits.length >= MAX_DIGITS) return false
        if (digits.isEmpty() && d == '0') return false
        digits += d
        return true
    }

    fun backspace() {
        digits = digits.dropLast(1)
    }

    fun clear() {
        digits = ""
    }

    /** The amount in minor units. */
    val minor: Long get() = if (digits.isEmpty()) 0 else digits.toLong()

    fun show(): String = money.show(minor)

    companion object {
        /** An amount of up to 9 digits: 9,999,999.99 in a currency with two decimals. */
        const val MAX_DIGITS = 9
    }
}
