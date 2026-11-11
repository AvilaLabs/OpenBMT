package org.openbmt.owner.app

import org.openbmt.app.Format
import org.openbmt.owner.CounterShare
import org.openbmt.owner.WaitingCounter

/**
 * What the screens that add a counter write and decide, as plain text and
 * plain values (OA-22 to OA-25). Everything that has a meaning is the
 * library's: the request is read and judged by the node (AS-69), the
 * settings and the first batch are made by it (AS-70, AS-71), and so is the
 * code the counter takes (AS-72). No Android class is used.
 */
object CounterModel {
    /** A counter's request is about 100 bytes plus its name; a code that holds more than this is not one. */
    const val MAX_REQUEST_BYTES = 1_024

    /** The types of code the scan of a counter takes: the counter's `ur:bytes` and nothing else. */
    val REQUEST_TYPES: Set<String> = ExchangeModel.RESPONSE_TYPES

    /** What the first screen says before the camera opens: two lines, in the order the owner does the steps. */
    val explanation: List<String> = listOf(
        "Scan the counter's code, then approve on the Key app.",
        "Last, show the counter a code. It is paired when it shows Paired.",
    )

    /** The four words, written large for the owner to compare with the counter's and the Key app's. */
    fun wordsLine(words: List<String>): String = words.joinToString("   ")

    /** A share of the limit in basis points as a percent: 5000 is "50%", 3333 is "33.33%". */
    fun percent(bps: Long): String =
        if (bps % 100 == 0L) "${bps / 100}%" else "${bps / 100}.${"%02d".format(bps % 100)}%"

    /** One line for each counter's share, in the library's order. */
    fun shareLines(shares: List<CounterShare>): List<String> =
        shares.map { "${OwnerModel.counterLabel(it.counter)}: ${percent(it.bps)}" }

    /** The name a counter gave itself, made safe to show: it is foreign text. */
    fun nameOf(name: String): String = Format.cleanForeign(name).ifEmpty { "a counter without a name" }

    /** The minutes a counter waits for its code, rounded down but never zero. */
    fun windowMinutes(secs: Long): Long = maxOf(1L, secs / 60)

    /** How long the counter waits, and that the steps must fit in it. */
    fun windowLine(secs: Long): String {
        val minutes = windowMinutes(secs)
        return "The counter waits $minutes ${if (minutes == 1L) "minute" else "minutes"} from the moment it showed its code. Do the steps below within that time."
    }

    /** How many addresses the first batch holds. */
    fun batchLine(count: Long): String = "The first batch for this counter holds $count addresses."

    /** What the owner does after the counter is queued. */
    const val nextLine: String =
        "Next, build the request for the Key app, show it there, compare the four words, approve there, and bring its response back."

    /** What the scan of a counter gave, made into the same two results the other sources give. */
    fun requestResult(payload: ByteArray): SourceResult =
        if (payload.size > MAX_REQUEST_BYTES) SourceResult.Failed(AppWords.counterCodeNotRequest) else SourceResult.Bytes(payload)

    /** The rate typed for a phone that holds no settings yet: a whole number above 0. */
    fun rate(text: String): Form<Long> =
        Forms.number(text)?.takeIf { it > 0 }?.let { Form.Ok(it) } ?: Form.Refused(AppWords.counterRateNeeded)

    /** The words of the code for the counter: with the first addresses in it or without. */
    fun codeStep(made: Boolean, withBatch: Boolean, animated: Boolean): ExchangeStep.Code = when {
        !made -> ExchangeStep.Code(AppWords.counterCodeNotMade, false)
        !withBatch -> ExchangeStep.Code(AppWords.counterCodeNoBatch, true)
        animated -> ExchangeStep.Code(AppWords.counterCodeAnimated, true)
        else -> ExchangeStep.Code(AppWords.counterCodeStill, true)
    }

    /** The label of a button that shows the code of a waiting counter. */
    fun showCodeLabel(w: WaitingCounter): String = "Show the code for ${nameOf(w.name)}"

    /** The counter waiting under [counter], if one waits. */
    fun waitingFor(rows: List<WaitingCounter>, counter: ByteArray): WaitingCounter? =
        rows.firstOrNull { it.counter.contentEquals(counter) }
}
