package org.openbmt.counter.demo

import org.openbmt.counter.CounterNode
import org.openbmt.counter.Native
import org.openbmt.counter.StartConfig
import org.openbmt.counter.app.Money
import org.openbmt.counter.app.SaleModel

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

/**
 * The counter app's own entry points, driven from the page's script: the node is started with
 * [CounterNode.start], a sale is opened with [CounterNode.open], and the sale's state is read with
 * [CounterNode.judge] and [SaleModel.isPaid], all through [Native.call], which the page answers with the
 * WebAssembly build of the counter's library.
 */
@JsExport
object CounterFlowDemo {
    private var node: CounterNode? = null

    private fun running(): CounterNode = node ?: error("the node is not started")

    /** Start the node on the folder [dir] from the pairing [record], and take its first events (the app pumps in a loop). */
    fun start(dir: String, now: Double, record: ByteArray) {
        val started = CounterNode.start(StartConfig(dir, now.toLong(), record, null))
        node = started
        started.pump(now.toLong(), 0)
    }

    /** Open a sale of [amountMinor] in [currency] at [rate] minor units per bitcoin; the address the customer pays. */
    fun openSale(now: Double, amountMinor: Double, currency: String, rate: Double): String {
        val opened = running().open(now.toLong(), amountMinor.toLong(), currency, rate.toLong(), "demo", now.toLong())
        lastIndex = opened.index
        needsSats = opened.needsSats
        return opened.payment.address
    }

    private var lastIndex = 0L
    private var needsSats = 0L

    /** The sats the last sale needs. */
    fun satsNeeded(): Double = needsSats.toDouble()

    /** Take the network's events; how many reports left. */
    fun pump(now: Double): Int = running().pump(now.toLong(), 0).reports.size

    /** The name of the last sale's state now (the library's own judgment), as the model has it. */
    fun saleState(now: Double): String = running().judge(now.toLong(), lastIndex).state.name

    /** Whether the sale screen's model counts the last sale as paid. */
    fun salePaid(now: Double): Boolean = SaleModel.isPaid(running().judge(now.toLong(), lastIndex).state)

    /** Stop the node. */
    fun stop() {
        node?.stop()
        node = null
    }
}
