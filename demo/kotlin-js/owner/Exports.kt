package org.openbmt.owner.demo

import org.openbmt.owner.BooksDay
import org.openbmt.owner.Books
import org.openbmt.owner.Native
import org.openbmt.owner.app.BooksModel
import org.openbmt.owner.app.RequestForm
import org.openbmt.owner.app.RequestModel
import org.openbmt.owner.app.StarterChecks
import org.openbmt.owner.app.StarterHome
import org.openbmt.owner.app.StarterSetup

/**
 * What the page's script can call, as the proof that the owner app's own
 * model runs in the browser: the books page over a few days, the dates and
 * limits of the setup and the request form (which run on the calendar and
 * big-number shims), and the one native call.
 */
@JsExport
object OwnerModelDemo {
    /** The rows of the books page for the days given as parallel arrays, one "date|totals" text each. */
    fun booksRows(currency: String, dates: Array<String>, sales: Array<Double>, amounts: Array<String>): Array<String> {
        val days = dates.indices.map { BooksDay(dates[it], sales[it].toLong(), 0, amounts[it]) }
        return BooksModel.page(Books(currency, days, emptyList(), null)).rows.map { "${it.date}|${it.totals}" }.toTypedArray()
    }

    /** The limit typed, in satoshis at [pricePerBtc]; -1 when it does not read. */
    fun limitSats(text: String, pricePerBtc: Double): Double = StarterSetup.limitSats(text, pricePerBtc.toLong())?.toDouble() ?: -1.0

    /** The date the setup suggests, 30 days on from [now] (seconds). */
    fun suggestedDate(now: Double): String = StarterSetup.suggestedDate(now.toLong())

    /** The second the typed day begins (UTC), or -1. */
    fun moveBy(text: String, now: Double): Double = StarterSetup.moveBy(text, now.toLong())?.toDouble() ?: -1.0

    /** Whether the security-patch date is more than six months before [today] (yyyy-mm-dd). */
    fun patchIsOld(patch: String, today: String): Boolean = StarterChecks.patchIsOld(patch, StarterChecks.parseDate(today)!!)

    /** The day of the shop's time zone at [now], as the form writes it. */
    fun shopDay(now: Double, offsetMinutes: Int): String = RequestModel.today(now.toLong(), offsetMinutes).toString()

    /** "yes", "no" or "bad": whether the typed due day is from [today] to 366 days on. */
    fun dueDay(text: String, today: String): String {
        val due = RequestForm.day(text) ?: return "bad"
        return if (RequestForm.inRange(due, StarterChecks.parseDate(today)!!)) "yes" else "no"
    }

    /** The date of [secs] as the starter home writes it. */
    fun homeDate(secs: Double): String = StarterHome.date(secs.toLong())

    /** The request bytes through [Native.call] and the answer's length, or -1 when the page has no answer. */
    fun nativeAnswerLength(request: ByteArray): Int = Native.call(request)?.size ?: -1
}
