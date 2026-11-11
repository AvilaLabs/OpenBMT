package org.openbmt.counter.app

import java.time.Instant
import java.time.ZoneId
import org.openbmt.app.Format
import org.openbmt.counter.ActorRole
import org.openbmt.counter.DayTotals
import org.openbmt.counter.PersonLine
import org.openbmt.counter.Tally

/** The window of the day's totals (CR-152): local midnight to now, on the phone's clock and in its time zone. */
object DayWindow {
    /** The start of the day that [nowSecs] is in, in [zone], as seconds since 1970. A day with a change of clocks is as long as it is. */
    fun start(nowSecs: Long, zone: ZoneId): Long =
        Instant.ofEpochSecond(nowSecs).atZone(zone).toLocalDate().atStartOfDay(zone).toEpochSecond()

    /** The end of the window, which is not counted: one second after [nowSecs], so that a record made this second is. */
    fun end(nowSecs: Long): Long = nowSecs + 1
}

/** What the screen "Today" writes from the library's totals (CR-153, CR-154). No Android class is used. */
object TodayModel {
    /** 12345 as "12,345". */
    private fun group(n: Long): String = Format.sats(n).removeSuffix(" sats").removeSuffix(" sat")

    private fun count(n: Long, one: String, many: String = one + "s"): String = "${group(n)} ${if (n == 1L) one else many}"

    private fun money(t: DayTotals, fallback: Money?): Money? = t.currency?.let { Money.of(it) } ?: fallback

    /** Number, the amount as it was shown at the time and the bitcoin: "3 sales, 45.00 USD, 54,000 sats". */
    private fun sales(n: Tally, t: DayTotals, fallback: Money?, noun: String = "sale", verb: String = ""): String {
        val m = money(t, fallback)
        val parts = ArrayList<String>()
        parts.add(count(n.count, noun))
        // Bitcoin Standard (CR-197): the amount is the sats, which are said once.
        if (m != null && !m.isSats) parts.add(m.show(n.minor))
        parts.add(Format.sats(n.sats) + verb)
        return parts.joinToString(", ")
    }

    /** The lines of the totals, one text each, in the order the screen shows them. */
    fun lines(t: DayTotals, fallback: Money?): List<String> {
        val out = ArrayList<String>()
        out.add("Sales paid: " + sales(t.paid, t, fallback))
        out.add("On the chain: " + sales(t.onChain, t, fallback))
        out.add("Over Lightning: " + sales(t.lightning, t, fallback))
        out.add("Sales still waiting: " + sales(t.waiting, t, fallback, verb = " asked for"))
        if (t.stopped > 0) out.add("Not to be handed over: " + count(t.stopped, "sale"))
        out.add("Sales cancelled or expired: " + count(t.ended, "sale"))
        out.add("Overrides: ${group(t.overrides.count)}, ${Format.sats(t.overrides.sats)} at risk")
        val withdrawn = if (t.refundsWithdrawn > 0) " (${group(t.refundsWithdrawn)} withdrawn)" else ""
        out.add("Refunds asked for: ${group(t.refunds.count)}, ${Format.sats(t.refunds.sats)}$withdrawn")
        val cash = money(t, fallback)?.takeIf { !it.isSats }?.let { it.show(t.cashRefunds.minor) + ", " }.orEmpty()
        out.add("Cash refunds: ${group(t.cashRefunds.count)}, $cash${Format.sats(t.cashRefunds.sats)}")
        // CR-191: the tips of the paid sales, in the shop's currency; the line is there when a tip was taken.
        val tipMoney = money(t, fallback)
        if (t.tips.count > 0 && tipMoney != null) out.add("${WordsOfTips.LINE}: ${count(t.tips.count, "sale")}, ${tipMoney.show(t.tips.minor)}")
        return out
    }

    /** One person's line cut in three: the cleaned name, the role as the line writes it, and what the person did. */
    class PersonParts(val name: String, val role: String, val line: String, val tips: String? = null)

    /** [money] is the shop's currency for the tips the person took (CR-191); null leaves them out. */
    fun personParts(p: PersonLine, money: Money? = null): PersonParts {
        val name = Format.cleanForeign(p.name)
        return PersonParts(
            name,
            if (p.role == ActorRole.Manager) "manager" else "cashier",
            "${count(p.sales, "sale")}, ${count(p.overrides, "override")}, " +
                "${count(p.refunds, "refund")} asked for, ${count(p.cashRefunds, "cash refund")}",
            if (p.tipsMinor > 0 && money != null) "${WordsOfTips.personLabel(name)}: ${money.show(p.tipsMinor)}" else null,
        )
    }

    /** The lines per person, as the manager sees them (CR-154); names from the records are cleaned (SC-08). */
    fun people(t: DayTotals, money: Money? = null): List<String> {
        val out = ArrayList<String>()
        for (p in t.people) {
            val parts = personParts(p, money)
            out.add("${parts.name} (${parts.role}): ${parts.line}" + (parts.tips?.let { "\n$it" } ?: ""))
        }
        if (t.unnamedSales > 0) out.add("Sales with no name in the record: ${group(t.unnamedSales)}")
        if (t.peopleLeftOut > 0) out.add("People left out of this list: ${group(t.peopleLeftOut)}")
        return out
    }

    /**
     * A line of [lines] cut at its colon and its commas: the [label] before the colon and the [parts] after it, in
     * order. No word is changed or dropped; the pieces are only set in other places on the screen.
     */
    class Split(val label: String, val parts: List<String>) {
        /** The first part: the number, as the line writes it ("3 sales"). */
        val count: String get() = parts.firstOrNull().orEmpty()

        /** The money when the line has it between the number and the bitcoin; else the last part. */
        val amount: String get() = if (parts.size >= 3) parts[1] else parts.lastOrNull().orEmpty()

        /** What is left of the line once [amount] is taken: the number and the bitcoin, joined with a dot; "" when there is nothing else. */
        val detail: String
            get() = when {
                parts.size >= 3 -> parts[0] + " \u00B7 " + parts.drop(2).joinToString(", ")
                parts.size == 2 -> parts[0]
                else -> ""
            }

        /** Every part after the first, as the line writes them. */
        val rest: String get() = parts.drop(1).joinToString(", ")

        /** The parts as one text. */
        val value: String get() = parts.joinToString(", ")
    }

    fun split(line: String): Split {
        val at = line.indexOf(": ")
        if (at < 0) return Split(line, emptyList())
        return Split(line.substring(0, at), line.substring(at + 2).split(", "))
    }
}
