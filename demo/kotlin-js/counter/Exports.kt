package org.openbmt.counter.demo

import org.openbmt.counter.Native
import org.openbmt.counter.app.Money

/**
 * What the page's script can call, as the first proof that the apps' own
 * model runs in the browser: the amounts of the counter's keypad, and the
 * one native call.
 */
@JsExport
object CounterModelDemo {
    /** [minor] in [code] as the counter writes it ("12.34 USD"), or null for a currency the keypad has not. */
    fun showAmount(code: String, minor: Double): String? = Money.of(code)?.show(minor.toLong())

    /** What a person typed, in minor units; -1 when it is not an amount. */
    fun parseAmount(code: String, text: String): Double = Money.of(code)?.parse(text)?.toDouble() ?: -1.0

    /** The request bytes through [Native.call] and the answer's length, or -1 when the page has no answer. */
    fun nativeAnswerLength(request: ByteArray): Int = Native.call(request)?.size ?: -1
}
