package org.openbmt.counter.app

import org.openbmt.app.Colour
import org.openbmt.app.Format
import org.openbmt.counter.SaleState

/** Which buttons a sale screen shows. */
class SaleButtons(val requote: Boolean, val cancel: Boolean, val override: Boolean)

/** What the sale screens write, from what the library says. No Android class is used. */
object SaleModel {
    /**
     * CR-08: a new quote moves only what is unpaid, and a sale takes none
     * once enough has arrived, so Requote is offered while nothing or part
     * has arrived. CR-07: only a sale with nothing received is cancelled.
     * CR-20: a person accepts a sale that is waiting, part paid or not to be
     * handed over. The library still decides each; what it refuses is shown
     * with its words. CR-109: a sale paid by Lightning is final and takes no button. CR-207: a request to send keeps
     * its one quote, so it takes no Requote.
     */
    fun buttons(state: SaleState, request: Boolean = false): SaleButtons = SaleButtons(
        requote = !request && (state == SaleState.NotPaidYet || state == SaleState.PartPaid),
        cancel = state == SaleState.NotPaidYet,
        override = state == SaleState.Waiting || state == SaleState.PartPaid || state == SaleState.DoNotHandOver,
    )

    /**
     * The amount in the shop's currency when the screen has one to show: Bitcoin Standard (CR-197) has none, since
     * its amount is the sats the customer sends and they are shown once.
     */
    private fun shown(money: Money, amountMinor: Long?): Long? = if (money.isSats) null else amountMinor

    /** The amount line of the sale (SC-09): the amount in the shop's currency, then what the customer sends in sats. */
    fun amountLine(money: Money, amountMinor: Long?, needsSats: Long): String =
        shown(money, amountMinor)?.let { "${money.show(it)}, ${Format.sats(needsSats)} to send" } ?: "${Format.sats(needsSats)} to send"

    /** The amount at 44 sp in the sale's band: the shop's currency, or the sats to send when the amount is not known. */
    fun amountBig(money: Money, amountMinor: Long?, needsSats: Long): String =
        shown(money, amountMinor)?.let { money.show(it) } ?: Format.sats(needsSats)

    /** The line under it: what the customer sends, so that [amountBig] and this say what [amountLine] says. */
    fun amountSmall(money: Money, amountMinor: Long?, needsSats: Long): String =
        if (shown(money, amountMinor) != null) "${Format.sats(needsSats)} to send" else "to send"

    /** The amount line of an outcome screen: the shop's currency and the sats to send, or the sats alone when the amount is not known. */
    fun outcomeLine(money: Money, amountMinor: Long?, needsSats: Long): String =
        shown(money, amountMinor)?.let { "${money.show(it)} \u00B7 ${Format.sats(needsSats)}" } ?: Format.sats(needsSats)

    /** What has arrived of what the sale needs. */
    fun receivedLine(received: Long, needs: Long): String = "Received ${Format.sats(received)} of ${Format.sats(needs)}"

    /** Whether the sale has been paid, so that finishing is the one thing left to do. */
    fun isPaid(state: SaleState): Boolean =
        state == SaleState.PaidInstant || state == SaleState.Confirmed || state == SaleState.Final ||
            state == SaleState.Overridden || state == SaleState.PaidLightning

    /**
     * Whether the sale's screen shows the code (not paid yet, and part paid) or is the whole screen in the colour
     * of the outcome (every other state).
     */
    fun showsCode(state: SaleState): Boolean = state == SaleState.NotPaidYet || state == SaleState.PartPaid

    /** The size in sp of the heading of an outcome screen: 56 for a short one such as "Paid", 44 and then 40 for longer ones. */
    fun outcomeHeadingSp(heading: String): Float = when {
        heading.length <= 8 -> 56f
        heading.length <= 16 -> 44f
        else -> 40f
    }

    /** Whether the sale is over for the cashier's list (nothing more will change on its own). */
    fun isEnded(state: SaleState): Boolean =
        state == SaleState.Expired || state == SaleState.Cancelled || state == SaleState.PaidLightning
}

/** The price sources of the shop's currency (CR-193, RT-40). */
object ShopPrices {
    /** The sources of [money]'s currency, in the table's order; none for Bitcoin Standard (CR-197), which asks no price. */
    fun sources(money: Money, all: List<org.openbmt.counter.RateSource>): List<org.openbmt.counter.RateSource> =
        if (money.isSats) emptyList() else all.filter { it.currency == money.code }
}

/**
 * Which sales have already played their outcome sound in this run (in memory only): a sale plays the sound of
 * good once and the sound of stop once, however often its screen is drawn again or visited. Waiting and ended
 * (blue and neutral) never play.
 */
class OutcomePlays {
    private val played = HashSet<Pair<Long, Colour>>()

    /** True the first time [sale] shows [colour] and the colour plays a sound; false after that, and always for blue and neutral. */
    fun first(sale: Long, colour: Colour): Boolean {
        if (colour != Colour.Green && colour != Colour.Red) return false
        return played.add(sale to colour)
    }
}
