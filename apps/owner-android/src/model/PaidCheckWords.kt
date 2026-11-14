package org.openbmt.owner.app

import org.openbmt.app.Format
import org.openbmt.app.SayWords
import org.openbmt.app.Tones
import org.openbmt.app.Who
import org.openbmt.owner.PaidAlarm
import org.openbmt.owner.PaidArrived
import org.openbmt.owner.PaidDay
import org.openbmt.owner.PaidSaleLine

/**
 * Every text of the counter checks that the app writes itself (`docs/OWNER-COINS-RULES.md`, OC-60 to OC-64), listed
 * under the rule of its screen: the alarm of a sale a counter showed paid that this phone does not see, the line of a
 * payment that arrived since, the sales waiting for a block or too old to check, a counter's day, and the checks that
 * could not be kept. A `{name}` in a text is filled by the app. The list is written to docs/screens/owner-app.md by
 * [AppWords.screensMarkdown]. No Android class is used.
 */
object PaidCheckWords {
    private val registry = ArrayList<Pair<String, String>>()

    /** Written words, listed under the key of their rule. */
    private fun say(key: String, text: String): String {
        registry.add(key to text)
        return text
    }

    // ---- the alarm (OC-62) ----
    val mismatchHeading = say("OC62_MISMATCH_HEADING", "Paid on counter, not seen")
    val mismatchWhat = say("OC62_MISMATCH_WHAT", "{counter} showed sale {number} as paid, {amount}, at {time}, taken by {cashier}. This phone sees {seen} paid to that sale's address.")
    val mismatchNext = say("OC62_MISMATCH_NEXT", "Check the counter: it may have been tampered with. Clear this once you know what happened.")
    val clearButton = say("OC62_CLEAR_BUTTON", "Clear")
    val arrivedLine = say("OC62_ARRIVED_LINE", "The payment for sale {number} has now arrived in full.")

    // ---- the sales the check cannot judge yet (OC-61) ----
    val waitingHeading = say("OC61_WAITING_HEADING", "Waiting for a block")
    val waitingWhat = say("OC61_WAITING_WHAT", "The counter showed this sale as paid, and its payment is not in a block yet. One with a low fee can take up to a day.")
    val waitingNext = say("OC61_WAITING_NEXT", "Nothing to do yet.")
    val notCheckableHeading = say("OC61_NOT_CHECKABLE_HEADING", "Too old to check")
    val notCheckableWhat = say("OC61_NOT_CHECKABLE_WHAT", "This sale is too old for the wallet to check what was paid.")
    val notCheckableNext = say("OC61_NOT_CHECKABLE_NEXT", "Check it by hand if it matters.")

    // ---- the day of a counter (OC-63) ----
    val dayLine = say("OC63_DAY_LINE", "{counter}: {n} sales paid on chain, {total}. Seen {seen}, waiting {waiting}, too old to check {old}, missing {missing}.")

    // ---- the checks that could not be kept (OC-64) ----
    val notKeptHeading = say("OC64_NOT_KEPT_HEADING", "Checks not kept")
    val notKeptWhat = say("OC64_NOT_KEPT_WHAT", "This phone could not read back its record of the counter checks.")
    val notKeptNext = say("OC64_NOT_KEPT_NEXT", "Clear the checks to start them again.")
    val fullWhat = say("OC64_FULL_WHAT", "This phone's record of the counter checks is full, so new checks are shown but not kept.")
    val fullNext = say("OC64_FULL_NEXT", "Clear the checks you have dealt with.")

    // ---- a counter whose records cannot be checked (OC-61) ----
    val unreadableHeading = say("OC61_UNREADABLE_HEADING", "Counter cannot be checked")
    val unreadableWhat = say("OC61_UNREADABLE_WHAT", "The records of {counter} do not read as sales, so this phone cannot check the payments of its sales.")
    val unreadableNext = say("OC61_UNREADABLE_NEXT", "Check the counter: it may have been tampered with.")

    private fun said(heading: String, what: String, next: String, tone: Int): SayWords =
        SayWords(tone, Who.OWNER, heading, what, next, null, null, null, null)

    /** The counter as the app names it elsewhere ("Counter 1a2b3c4d"). */
    fun counterName(counter: ByteArray): String = OwnerModel.counterLabel(counter)

    /** OC-62: the alarm of a sale, the facts filled in; the cashier's name is text of the counter's and is cleaned. */
    fun mismatch(a: PaidAlarm): SayWords = said(
        mismatchHeading,
        mismatchWhat
            .replace("{counter}", counterName(a.counter))
            .replace("{number}", a.sale.toString())
            .replace("{amount}", Format.sats(a.recordedSats))
            .replace("{time}", Format.localTime(a.paidAt))
            .replace("{cashier}", Format.cleanForeign(a.cashier))
            .replace("{seen}", Format.sats(a.seenSats)),
        mismatchNext,
        Tones.STOP,
    )

    /** OC-62: the line of a payment that arrived in full after its alarm. */
    fun arrived(a: PaidArrived): String = arrivedLine.replace("{number}", a.sale.toString())

    /** OC-61: the words of a sale whose payment is not in a block yet. */
    val waiting: SayWords get() = said(waitingHeading, waitingWhat, waitingNext, Tones.INFO)

    /** OC-61: the words of a sale too old for the wallet to check. */
    val notCheckable: SayWords get() = said(notCheckableHeading, notCheckableWhat, notCheckableNext, Tones.INFO)

    /** OC-64: the words of checks that cannot be kept. */
    val notKept: SayWords get() = said(notKeptHeading, notKeptWhat, notKeptNext, Tones.STOP)

    /** OC-63: a counter's day. */
    fun day(d: PaidDay): String = dayLine
        .replace("{counter}", counterName(d.counter))
        .replace("{n}", d.sales.toString())
        .replace("{total}", Format.sats(d.totalSats))
        .replace("{seen}", d.seen.toString())
        .replace("{waiting}", d.waiting.toString())
        .replace("{old}", d.tooOld.toString())
        .replace("{missing}", d.missing.toString())

    /** A sale of a waiting or too-old list: the counter and the sale's number, as a row's title. */
    fun saleTitle(s: PaidSaleLine): String = counterName(s.counter) + " #" + s.sale

    /** What the counter recorded for it and when, as a row's second line. */
    fun saleSub(s: PaidSaleLine): String = Format.sats(s.recordedSats) + ", " + Format.localTime(s.paidAt)

    /** The words the app says with, for the test that checks them (the alarm for a sample). */
    fun messages(sample: PaidAlarm): List<Pair<String, SayWords>> = listOf(
        "mismatch" to mismatch(sample),
        "waiting" to waiting,
        "notCheckable" to notCheckable,
        "notKept" to notKept,
    )

    /** Every key and its text, in order, for docs/screens/owner-app.md. */
    fun all(): List<Pair<String, String>> = registry.toList()
}
