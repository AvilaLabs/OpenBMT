package org.openbmt.owner.app

import java.time.DateTimeException
import java.time.LocalDate
import java.time.ZoneOffset
import org.openbmt.app.Format
import org.openbmt.owner.OwnerException
import org.openbmt.owner.RequestAsk
import org.openbmt.owner.RequestDetail
import org.openbmt.owner.RequestEnd
import org.openbmt.owner.RequestPayment
import org.openbmt.owner.RequestQuote
import org.openbmt.owner.RequestReturn
import org.openbmt.owner.RequestStatus
import org.openbmt.owner.ReturnState
import org.openbmt.owner.RequestDay
import org.openbmt.owner.RequestEvery
import org.openbmt.owner.RequestList
import org.openbmt.owner.RequestPriceIn
import org.openbmt.owner.RequestRate
import org.openbmt.owner.RequestRow
import org.openbmt.owner.RequestShared
import org.openbmt.owner.RequestState
import org.openbmt.owner.ShopCurrency

/** The numbers of `docs/REQUEST-RULES.md` as the app reads them. The library holds the same and judges the rest. */
object RequestLimits {
    /** RQ-06: the longest note, in characters. */
    const val NOTE_MAX = 64

    /** RQ-08: the most units of the shop's currency a request may ask for. */
    const val MAX_CURRENCY_UNITS = 1_000_000L

    /** RQ-08: 21,000,000 bitcoin in satoshis. */
    const val MAX_SATS = 2_100_000_000_000_000L

    /** RQ-09: the due day is at most this many days after the day the request is made. */
    const val DUE_DAYS_MAX = 366L

    /** RQ-12: a rate the app fetched this many seconds ago is used again for the next call. */
    const val RATE_MAX_AGE_SECS = 120L
}

/** The parts of the list, in the order they are shown (RQ-21, RQ-25). */
enum class RequestPart { ReadyToSend, Overdue, DueSoon, Waiting, Finished }

/** One part of the list with its rows. */
class RequestSection(val part: RequestPart, val rows: List<RequestRow>)

/** What the page of one request offers. The library judges each one again when it is asked for (RQ-16, RQ-19, RQ-20). */
class PageActions(
    val share: Boolean,
    val sendAgain: Boolean,
    val cancel: Boolean,
    val close: Boolean,
    val countPaid: Boolean,
    val returnMoney: Boolean,
    val stopRepeating: Boolean,
)

/** The list of requests and their pages, as pure decisions. No Android class is used. */
object RequestModel {
    /** The day as a number that sorts: 2026-11-13 is 20261113. */
    fun dayKey(d: RequestDay): Long = d.year * 10_000 + d.month * 100 + d.day

    /** Newest due date last, then by number (RQ-21). */
    private fun ordered(rows: List<RequestRow>): List<RequestRow> =
        rows.sortedWith(compareBy<RequestRow>({ dayKey(it.due) }, { it.number }))

    /** The parts of the list that have rows, in the order of RQ-21 with "Ready to send" first (RQ-25), each with its rows in order. */
    fun sections(list: RequestList): List<RequestSection> = listOf(
        RequestSection(RequestPart.ReadyToSend, ordered(list.readyToSend)),
        RequestSection(RequestPart.Overdue, ordered(list.overdue)),
        RequestSection(RequestPart.DueSoon, ordered(list.dueSoon)),
        RequestSection(RequestPart.Waiting, ordered(list.waiting)),
        // The ones that ended are newest first, as the library gives them.
        RequestSection(RequestPart.Finished, list.finished),
    ).filter { it.rows.isNotEmpty() }

    /** The words that name a part. */
    fun partTitle(part: RequestPart): String = when (part) {
        RequestPart.ReadyToSend -> RequestWords.partReady
        RequestPart.Overdue -> RequestWords.partOverdue
        RequestPart.DueSoon -> RequestWords.partDueSoon
        RequestPart.Waiting -> RequestWords.partWaiting
        RequestPart.Finished -> RequestWords.partFinished
    }

    /** Whether a request in [part] is open: not paid, cancelled or closed. */
    fun isOpen(part: RequestPart): Boolean = part != RequestPart.Finished

    /** The part and the row of request [number], if the list has it. */
    fun find(list: RequestList, number: Long): Pair<RequestPart, RequestRow>? =
        sections(list).firstNotNullOfOrNull { s -> s.rows.firstOrNull { it.number == number }?.let { s.part to it } }

    /** RQ-19: a request that has received nothing counted can be cancelled; one that ended cannot. */
    fun canCancel(end: RequestEnd?, payments: List<RequestPayment>): Boolean = end == null && payments.none { it.confirmed }

    /** RQ-20: a part paid or almost paid request that has not ended can be closed as it is. */
    fun canClose(end: RequestEnd?, status: RequestStatus): Boolean =
        end == null && (status == RequestStatus.PartPaid || status == RequestStatus.Almost)

    /** RQ-16: only an almost paid request that has not ended can be counted as paid. */
    fun canCountPaid(end: RequestEnd?, status: RequestStatus): Boolean = end == null && status == RequestStatus.Almost

    /** RQ-26: money can be returned while less has been returned than arrived. */
    fun canReturn(receivedSats: Long, returnedSats: Long): Boolean = receivedSats > returnedSats

    /** What the page of a request offers, from what the book holds. [overdue] is the part of the list it is in (RQ-22). The library judges each again. */
    fun actions(d: RequestDetail, overdue: Boolean): PageActions = PageActions(
        // RQ-14: a request that has ended or owes nothing is not shared again.
        share = d.end == null && d.row.owed > 0,
        sendAgain = overdue,
        cancel = canCancel(d.end, d.payments),
        close = canClose(d.end, d.status),
        countPaid = canCountPaid(d.end, d.status),
        returnMoney = canReturn(d.receivedSats, d.returnedSats),
        stopRepeating = d.repeat != null,
    )

    /** An amount in the request's unit, written to be read: satoshis for a request in bitcoin, else the shop's currency. */
    fun amountIn(d: RequestDetail, n: Long): String {
        val currency = ShopCurrency.values().firstOrNull { it.code == d.unit }
        return if (d.priceIn == RequestPriceIn.Bitcoin || currency == null) Format.sats(n) else money(currency, n)
    }

    /** One quote for the page: when it was made, and what it asked for (RQ-12). */
    fun quoteRow(d: RequestDetail, q: RequestQuote): Pair<String, String> {
        val until = q.until?.let { " \u00b7 " + RequestWords.quoteUntilLabel + " " + Format.localTime(it) } ?: ""
        return Format.localTime(q.madeAt) to (amountIn(d, q.owed) + " \u00b7 " + Format.sats(q.sats) + until)
    }

    /** One payment for the page (RQ-15): the satoshis, then the start of the transaction and where it is. */
    fun paymentRow(d: RequestDetail, p: RequestPayment): Pair<String, String> {
        val where = if (p.confirmed) RequestWords.paymentBlock + " " + p.height else RequestWords.paymentWaiting
        val credit = when {
            !p.confirmed -> ""
            p.credit > 0 -> " \u00b7 " + amountIn(d, p.credit)
            else -> " \u00b7 " + RequestWords.paymentNotCredited
        }
        return Format.sats(p.sats) to (p.txid.take(12) + " \u00b7 " + where + credit)
    }

    /** The words that name where a return stands (RQ-29). */
    fun returnStateWords(state: ReturnState): String = when (state) {
        ReturnState.Queued -> RequestWords.returnQueued
        ReturnState.Sent -> RequestWords.returnSent
        ReturnState.Done -> RequestWords.returnDone
        ReturnState.Cancelled -> RequestWords.returnCancelled
    }

    /** One return for the page: the satoshis and where it stands, then the customer's address (RQ-29). */
    fun returnRow(r: RequestReturn): Pair<String, String> = (Format.sats(r.sats) + " \u00b7 " + returnStateWords(r.state)) to r.address

    /** The label of the button that shares a request again: "Send again" for an overdue one (RQ-22), "Share again" for the rest (RQ-14). */
    fun shareLabel(part: RequestPart): String =
        if (part == RequestPart.Overdue) RequestWords.sendAgain else RequestWords.shareAgain

    /** The shop's offset from UTC in minutes east, from the milliseconds a time zone gives. */
    fun offsetMinutes(offsetMillis: Int): Int = offsetMillis / 60_000

    /** What every call is given (RQ-09, RQ-12). */
    fun ask(now: Long, offsetMillis: Int, rate: RequestRate?): RequestAsk = RequestAsk(now, offsetMinutes(offsetMillis), rate)

    /** The day it is in the shop's time zone. */
    fun today(now: Long, offsetMinutes: Int): LocalDate =
        java.time.Instant.ofEpochSecond(now).atOffset(ZoneOffset.ofTotalSeconds(offsetMinutes * 60)).toLocalDate()

    /** The digits after the point of the shop's currency: none for yen, won and Bitcoin Standard, else two. */
    fun exponent(currency: ShopCurrency): Int = when (currency) {
        ShopCurrency.Jpy, ShopCurrency.Krw, ShopCurrency.Sats -> 0
        else -> 2
    }

    /** An amount of the shop's currency in minor units, written to be read: "1,234.50 USD" (RQ-35). */
    fun money(currency: ShopCurrency, minor: Long): String {
        val exp = exponent(currency)
        val digits = minor.toULong().toString().padStart(exp + 1, '0')
        val whole = digits.substring(0, digits.length - exp)
        val out = StringBuilder()
        for ((i, c) in whole.withIndex()) {
            if (i > 0 && (whole.length - i) % 3 == 0) out.append(',')
            out.append(c)
        }
        if (exp > 0) out.append('.').append(digits.substring(digits.length - exp))
        return "$out ${currency.code}"
    }

    /** What a person typed for an amount in the shop's currency, in minor units: digits and at most [exponent] after one point. Null for anything else. */
    fun parseMinor(text: String, exponent: Int): Long? {
        val t = text.trim().replace(",", "")
        if (t.count { it == '.' } > 1) return null
        val whole = t.substringBefore('.')
        val fraction = if (t.contains('.')) t.substringAfter('.') else ""
        if (whole.isEmpty() && fraction.isEmpty()) return null
        if (!(whole + fraction).all { it in '0'..'9' } || fraction.length > exponent) return null
        if (whole.length + exponent > 18) return null
        return (whole + fraction.padEnd(exponent, '0')).toLongOrNull()
    }

    /** RQ-01: the owner app offers requests only to a shop paid to its own wallet. */
    fun offered(ownWallet: Boolean): Boolean = ownWallet

    /** What Home's row says under its title: how many are overdue, from the library's own line, else the words for none (RQ-22). */
    fun homeSub(state: RequestState): String = state.overdueRow ?: RequestWords.homeRowSub
}

/** What the form of a request holds. */
class RequestDraft(
    val note: String = "",
    val amount: String = "",
    val priceIn: RequestPriceIn = RequestPriceIn.Bitcoin,
    val due: String = "",
    val repeat: RequestEvery? = null,
)

/** The form of RQ-05 to RQ-09 and RQ-24, turned into the arguments of the call. Only the shape is checked; the library judges the rest. */
object RequestForm {
    /** RQ-06: one to 64 characters with no control character; the ends are trimmed. Null when it is not that. */
    fun note(text: String): String? {
        val t = text.trim()
        val n = t.codePointCount(0, t.length)
        return if (n in 1..RequestLimits.NOTE_MAX && t.none { it.isISOControl() }) t else null
    }

    /** RQ-07, RQ-08: the amount in satoshis, or in minor units of the shop's currency; above zero and not above the limit. Null otherwise. */
    fun amount(text: String, priceIn: RequestPriceIn, currency: ShopCurrency): Long? = when (priceIn) {
        RequestPriceIn.Bitcoin -> Format.toAmount(text)?.takeIf { it in 1..RequestLimits.MAX_SATS }
        RequestPriceIn.ShopCurrency -> {
            val exp = RequestModel.exponent(currency)
            var limit = RequestLimits.MAX_CURRENCY_UNITS
            repeat(exp) { limit *= 10 }
            if (currency == ShopCurrency.Sats) null else RequestModel.parseMinor(text, exp)?.takeIf { it in 1..limit }
        }
    }

    /** RQ-09: "2026-12-01"; null when it is not a day of the calendar written so. */
    fun day(text: String): LocalDate? {
        val m = Regex("(\\d{4})-(\\d{2})-(\\d{2})").matchEntire(text.trim()) ?: return null
        return try {
            LocalDate.of(m.groupValues[1].toInt(), m.groupValues[2].toInt(), m.groupValues[3].toInt())
        } catch (_: DateTimeException) {
            null
        }
    }

    /** RQ-09: a day from [today] to 366 days after it. */
    fun inRange(due: LocalDate, today: LocalDate): Boolean =
        !due.isBefore(today) && !due.isAfter(today.plusDays(RequestLimits.DUE_DAYS_MAX))

    /** The day as the call takes it. */
    fun asDay(d: LocalDate): RequestDay = RequestDay(d.year.toLong(), d.monthValue.toLong(), d.dayOfMonth.toLong())

    /** The day as the form writes it. */
    fun dayText(d: RequestDay): String = "%04d-%02d-%02d".format(d.year, d.month, d.day)

    /** What a request in the shop's [currency] can be priced in: Bitcoin Standard has only bitcoin (RQ-07). */
    fun pricings(currency: ShopCurrency): List<RequestPriceIn> =
        if (currency == ShopCurrency.Sats) listOf(RequestPriceIn.Bitcoin) else listOf(RequestPriceIn.Bitcoin, RequestPriceIn.ShopCurrency)

    /** The arguments of the call, or the words that say what is wrong with the form. */
    fun check(draft: RequestDraft, currency: ShopCurrency, today: LocalDate): Form<RequestMake> {
        val note = note(draft.note) ?: return Form.Refused(RequestWords.noteRefused)
        if (draft.priceIn !in pricings(currency)) return Form.Refused(RequestWords.amountRefused)
        val amount = amount(draft.amount, draft.priceIn, currency) ?: return Form.Refused(RequestWords.amountRefused)
        val due = day(draft.due) ?: return Form.Refused(RequestWords.dueRefused)
        if (!inRange(due, today)) return Form.Refused(RequestWords.dueRange)
        return Form.Ok(RequestMake(note, amount, draft.priceIn, asDay(due), draft.repeat))
    }
}

/** The arguments of `requestMake` after the ask. */
class RequestMake(val note: String, val amount: Long, val priceIn: RequestPriceIn, val due: RequestDay, val repeat: RequestEvery?)

/** What the form of a return holds. */
class ReturnDraft(val inCurrency: Boolean = false, val amount: String = "", val address: String = "")

/** The arguments of `requestAddReturn` after the ask and the number. */
class ReturnMake(val inCurrency: Boolean, val amount: Long, val address: String)

/** The form of a return (RQ-26, RQ-28): the shape only. The address, the amount's size and what the request received are the library's to judge. */
object ReturnForm {
    /** The amount alone, for the line that asks the customer for an address (RQ-27). */
    fun amount(draft: ReturnDraft, currency: ShopCurrency): Form<ReturnMake> {
        val inCurrency = draft.inCurrency && currency != ShopCurrency.Sats
        val amount = if (inCurrency) RequestModel.parseMinor(draft.amount, RequestModel.exponent(currency)) else Format.toAmount(draft.amount)
        if (amount == null || amount <= 0) return Form.Refused(RequestWords.returnAmountRefused)
        return Form.Ok(ReturnMake(inCurrency, amount, ""))
    }

    /** The amount and the address as pasted or scanned, trimmed (RQ-28). */
    fun check(draft: ReturnDraft, currency: ShopCurrency): Form<ReturnMake> {
        val made = when (val f = amount(draft, currency)) {
            is Form.Ok -> f.value
            is Form.Refused -> return f
        }
        val address = draft.address.trim()
        if (address.isEmpty()) return Form.Refused(RequestWords.addressNeeded)
        return Form.Ok(ReturnMake(made.inCurrency, made.amount, address))
    }
}

/** What sharing shows and gives (RQ-12, RQ-13). */
object RequestShare {
    /** RQ-13: the text for the share sheet: the line of the words, and under it the link and nothing else. */
    fun text(shared: RequestShared): String = shared.line + "\n" + shared.link

    /** What the quote asks for, in the request's unit: a quote that has a rate is in the shop's currency. */
    fun owed(shared: RequestShared, currency: ShopCurrency): String =
        if (shared.rate != null && currency != ShopCurrency.Sats) RequestModel.money(currency, shared.owed) else Format.sats(shared.owed)

    /** When a quote in the currency runs out, on the person's clock; null for a quote in bitcoin. */
    fun until(shared: RequestShared): String? = shared.until?.let { Format.localTime(it) }
}

/** The rate a request in the shop's currency is quoted at (RQ-12). The library judges whether it is too old. */
object RequestRates {
    /** The price the sources of [currency] agree on, as the library takes it; null when there is none or the shop prices in bitcoin. */
    fun fetch(currency: ShopCurrency, port: PricePort, fetcher: RateFetcher, clockSecs: () -> Long): RequestRate? {
        if (currency == ShopCurrency.Sats) return null
        val sources = try {
            port.sourcesFor(currency.code)
        } catch (_: OwnerException) {
            return null
        }
        val replies = PriceModel.ask(sources, fetcher, clockSecs)
        return try {
            val agreed = port.agreeFor(currency.code, replies, clockSecs())
            // The rate is as old as the oldest reply it rests on.
            val at = replies.minOfOrNull { it.fetchedAt } ?: return null
            RequestRate(agreed.minorPerBtc, agreed.sourceLabel, at)
        } catch (_: OwnerException) {
            null
        }
    }

    /** [cached] when it is young enough to use again, else null. */
    fun usable(cached: RequestRate?, now: Long): RequestRate? =
        cached?.takeIf { now >= it.fetchedAt && now - it.fetchedAt <= RequestLimits.RATE_MAX_AGE_SECS }
}
