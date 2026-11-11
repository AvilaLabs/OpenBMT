package org.openbmt.counter.app

import org.openbmt.app.Format

/**
 * A currency the keypad can write: its code, and how many digits follow the
 * point. It is the shop's currency from the signed settings (CR-193): two
 * digits for most, none for yen and won (CR-196), and for Bitcoin Standard
 * (`SAT`, CR-197) the amount is whole satoshis with no point.
 */
class Money(val code: String, val exponent: Int) {
    /** Bitcoin Standard: the amount is satoshis, and no price is asked (CR-197). */
    val isSats: Boolean get() = code == SATS

    /** [minor] written to be read: "12.34 USD", "1,500 JPY", and for Bitcoin Standard "7,500 sats". */
    fun show(minor: Long): String = if (isSats) Format.sats(minor) else "${plain(minor)} $code"

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

    /**
     * What a person typed for an amount, in minor units: digits, and at most
     * [exponent] digits after one point ("12", "12.3", "12.34", ",": separators
     * are dropped). Null for anything else, for nothing, and for more than 12
     * digits.
     */
    fun parse(text: String): Long? {
        val t = text.trim().replace(",", "")
        val (whole, fraction) = if (t.contains('.')) t.substringBefore('.') to t.substringAfter('.') else t to ""
        if (whole.isEmpty() && fraction.isEmpty()) return null
        if (!(whole + fraction).all { it in '0'..'9' } || fraction.length > exponent || t.count { it == '.' } > 1) return null
        if (whole.length + exponent > 12) return null
        val digits = whole + fraction.padEnd(exponent, '0')
        return digits.toLongOrNull()
    }

    companion object {
        /** The code a sale records for Bitcoin Standard (CR-193). */
        const val SATS = "SAT"

        /** The shop's choices (CR-193) with the places after the point of ISO 4217; yen, won and satoshis have none. */
        fun of(code: String): Money? = when (code) {
            "USD", "EUR", "GBP", "AUD", "INR", "IDR", "SGD" -> Money(code, 2)
            "JPY", "KRW", SATS -> Money(code, 0)
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
