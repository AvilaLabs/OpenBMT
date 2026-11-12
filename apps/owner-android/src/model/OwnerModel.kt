package org.openbmt.owner.app

import org.openbmt.app.Format
import org.openbmt.app.SayWords
import org.openbmt.owner.CashAmount
import org.openbmt.owner.CashOutArgs
import org.openbmt.owner.CashPrice
import org.openbmt.owner.CounterState
import org.openbmt.owner.EntryKind
import org.openbmt.owner.BatchWaiting
import org.openbmt.owner.LinkState
import org.openbmt.owner.OutboxKind
import org.openbmt.owner.OutboxRow
import org.openbmt.owner.RefundArgs
import org.openbmt.owner.SendKind
import org.openbmt.owner.SendRow
import org.openbmt.owner.SettingsHeld
import org.openbmt.owner.ShopCurrency
import org.openbmt.owner.Limit

/**
 * What the screens write from what the library says, as plain text. The
 * sentences about a state are the library's own words; these are the labels,
 * the counts and the numbers around them. No Android class is used.
 */
object OwnerModel {
    /** "No alerts.", "1 alert." or "3 alerts." */
    fun alertsLine(count: Int): String = when (count) {
        0 -> "No alerts."
        1 -> "1 alert."
        else -> "$count alerts."
    }

    /** Payments the node follows that are not finished: not done and not replaced. */
    fun sendsInProgress(rows: List<SendRow>): Int =
        rows.count { it.state != SendKind.Done && it.state != SendKind.Replaced }

    fun sendsLine(inProgress: Int): String = when (inProgress) {
        0 -> "No payments in progress."
        1 -> "1 payment in progress."
        else -> "$inProgress payments in progress."
    }

    /** The link in one line: [link] is null when the phone was set up without one. */
    fun linkLine(link: LinkState?, counters: Int): String = when {
        link == null -> "No link to the counters."
        counters == 0 -> if (link.reachable) "The relay is reachable. No counter is on the list." else "The relay is not reachable. No counter is on the list."
        link.reachable -> "The relay is reachable. $counters ${if (counters == 1) "counter is" else "counters are"} on the list."
        else -> "The relay is not reachable. $counters ${if (counters == 1) "counter is" else "counters are"} on the list."
    }

    /** The number a counter shows for the sale at a receive index: the first sale is 1. */
    fun saleNumber(index: Long): Long = index + 1

    /** A counter's identity for a list: the first 4 bytes in hex. */
    fun counterLabel(counter: ByteArray): String = "Counter " + counter.take(4).joinToString("") { "%02x".format(it) }

    /** "3 minutes ago", for a time in the past as seconds since 1970; "just now" for the last 10 seconds or a time that is not past. */
    fun ago(now: Long, then: Long): String {
        val s = now - then
        return when {
            s < 10 -> "just now"
            s < 90 -> "$s seconds ago"
            s < 90 * 60 -> "${(s + 30) / 60} minutes ago"
            s < 36 * 3600 -> "${(s + 1800) / 3600} hours ago"
            else -> "${(s + 43200) / 86400} days ago"
        }
    }

    /** One counter of the link: its place in the chain of records and its last check-in. */
    fun counterLines(c: CounterState, now: Long): List<String> {
        val out = ArrayList<String>()
        out.add("${counterLabel(c.counter)}: ${c.records} records.")
        out.add("Head: " + c.head.take(8).joinToString("") { "%02x".format(it) } + "...")
        val seq = c.checkInSeq
        if (seq == null) {
            out.add("No check-in has been taken yet.")
        } else {
            val at = c.lastCheckInAt?.let { ", " + ago(now, it) } ?: ""
            val held = when (c.checkInHeld) {
                true -> ", held"
                false -> ", not held"
                null -> ""
            }
            out.add("Last check-in: number $seq$at$held.")
        }
        return out
    }

    // ---- the outbox -----------------------------------------------------------------

    fun entryTitle(kind: EntryKind, id: Long): String = when (kind) {
        EntryKind.Batch -> "Address batch $id"
        EntryKind.SettingsChange -> "Settings change $id"
        EntryKind.Pairing -> "Pairing $id"
    }

    fun outboxTitle(row: OutboxRow): String = when (row.kind) {
        OutboxKind.Transaction -> "Payment ${row.id}"
        OutboxKind.Batch -> "Address batch ${row.id}"
        OutboxKind.Settings -> "Settings ${row.id}"
        OutboxKind.Pairing -> "Pairing ${row.id}"
        OutboxKind.BoxWelcome -> "Box welcome ${row.id}"
    }

    /** What is known about a waiting object. The node keeps a signing time for a batch only. */
    fun outboxLines(row: OutboxRow, now: Long): List<String> {
        val out = ArrayList<String>()
        row.txid?.let { out.add("Transaction $it") }
        row.counter?.let { out.add(counterLabel(it)) }
        row.version?.let { out.add("Settings version $it") }
        out.add(row.signedAt?.let { "Signed ${Format.localTime(it)}, ${ago(now, it)}." } ?: "The node keeps no signing time for this one.")
        out.add("Size: ${row.size} bytes.")
        return out
    }

    /** New addresses that wait for a counter to hold them: whose, and since when (LK-70). */
    fun batchWaitingLines(b: BatchWaiting, now: Long): List<String> = listOf(
        "${counterLabel(b.counter)}: its new addresses are on their way.",
        "First sent ${Format.localTime(b.since)}, ${ago(now, b.since)}.",
    )

    // ---- settings -------------------------------------------------------------------

    /** A limit in the shop's currency; its satoshis follow, except for Bitcoin Standard, where they are the same number. */
    private fun limit(l: Limit, c: ShopCurrency): String =
        if (c == ShopCurrency.Sats) SettingsEditModel.unitsText(c, l.displayUsd)
        else "${SettingsEditModel.unitsText(c, l.displayUsd)} (${Format.sats(l.sats)})"

    private fun percent(bps: Long): String = "${bps / 100}.${(bps % 100).toString().padStart(2, '0')}%"

    /** Every line of the settings the node holds, as label and value, in a fixed order. */
    fun settingsLines(s: SettingsHeld): List<Pair<String, String>> = listOf(
        "Version" to s.version.toString(),
        "Currency" to s.currency.label,
        "Largest instant sale, light check" to limit(s.lightPerSale, s.currency),
        "Instant total, light check" to limit(s.lightBudget, s.currency),
        "Largest instant sale, own node" to limit(s.nodePerSale, s.currency),
        "Instant total, own node" to limit(s.nodeBudget, s.currency),
        "Largest instant sale, Box reports" to limit(s.boxPerSale, s.currency),
        "Instant total, Box reports" to limit(s.boxBudget, s.currency),
        "Underpayment still counted" to percent(s.underpaymentToleranceBps),
        "Largest single refund" to limit(s.refundCap, s.currency),
        "Refunds per day" to limit(s.refundDailyCap, s.currency),
        "Cashier's override per day" to limit(s.cashierOverrideDailyCap, s.currency),
        "Manager's override ceiling" to limit(s.managerOverrideCeiling, s.currency),
        "Blocks to settle a sale" to s.minConfirmations.toString(),
        "Blocks until a refund may be paid" to s.finalConfirmations.toString(),
        "Longest heartbeat age for instant" to "${s.heartbeatMaxAgeSecs} seconds",
        "Tor" to if (s.torOn) "on" else "off",
        "Customers can join the Wi-Fi" to if (s.customerWifi) "yes or unsure" else "no",
        "Smallest sale a counter opens" to if (s.minSaleSats == 0L) "no floor" else Format.sats(s.minSaleSats),
        "Tips" to SettingsEditModel.tipsNow(s),
        "Devices that write records" to s.devices.size.toString(),
        "Counters and their shares" to if (s.counters.isEmpty()) "none" else s.counters.joinToString("; ") { counterLabel(it.counter) + " " + percent(it.bps) },
        "Boxes" to s.boxes.size.toString(),
        "Lightning node" to SettingsEditModel.nodeNow(s),
    )
}

/** Why a form was not turned into a call: the app's words, or the arguments of the call. */
sealed class Form<out T> {
    class Ok<T>(val value: T) : Form<T>()
    class Refused(val words: SayWords) : Form<Nothing>()
}

/**
 * The forms of the screens, turned into the arguments of a call. Only the
 * shape of a number is checked here: an address, a name and the amounts'
 * sense (zero, too small, more than was received) are the library's to
 * judge, and its words are shown (OA-07).
 */
object Forms {
    /** A whole number of digits only, at most 18 of them. */
    fun number(text: String): Long? = Format.toAmount(text)

    /** A number that fits a u32. */
    fun number32(text: String): Long? = number(text)?.takeIf { it <= 0xffffffffL }

    /** A counter's identity: exactly 64 hex digits. */
    fun counter(text: String): ByteArray? {
        val t = text.trim()
        return if (t.length == 64) Format.fromHex(t) else null
    }

    fun feeRate(text: String): Form<Long> =
        number(text)?.let { Form.Ok(it) } ?: Form.Refused(AppWords.notANumber)

    fun refund(counter: ByteArray?, saleText: String, cashier: String, address: String, satsText: String): Form<RefundArgs> {
        if (counter == null) return Form.Refused(AppWords.counterNeeded)
        if (counter.size != 32) return Form.Refused(AppWords.counterNotAccepted)
        // A person counts sales from 1, as the counter shows them; the library counts receive indices from 0.
        val saleNumber = number32(saleText)?.takeIf { it >= 1 } ?: return Form.Refused(AppWords.notANumber)
        val sale = saleNumber - 1
        val sats = number(satsText) ?: return Form.Refused(AppWords.notANumber)
        return Form.Ok(RefundArgs(counter, sale, cashier.trim(), address.trim(), sats))
    }

    /** [all] sends everything the wallet holds; otherwise [satsText] is the amount. */
    fun cashOut(address: String, all: Boolean, satsText: String, label: String): Form<CashOutArgs> {
        val amount = if (all) {
            CashAmount.All
        } else {
            CashAmount.Sats(number(satsText) ?: return Form.Refused(AppWords.notANumber))
        }
        return Form.Ok(CashOutArgs(address.trim(), amount, label.trim()))
    }

    /** The price a cash-out is recorded at, typed or fetched, in whole units of the shop's currency (OA-102). */
    fun cashOutPrice(text: String, currency: ShopCurrency): Form<CashPrice> =
        when (val f = CounterModel.price(text, currency)) {
            is Form.Ok -> Form.Ok(CashPrice(currency.code, f.value))
            is Form.Refused -> Form.Refused(f.words)
        }
}
