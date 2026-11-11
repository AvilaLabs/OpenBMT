package org.openbmt.counter.app

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
     * with its words. CR-109: a sale paid by Lightning is final and takes no button.
     */
    fun buttons(state: SaleState): SaleButtons = SaleButtons(
        requote = state == SaleState.NotPaidYet || state == SaleState.PartPaid,
        cancel = state == SaleState.NotPaidYet,
        override = state == SaleState.Waiting || state == SaleState.PartPaid || state == SaleState.DoNotHandOver,
    )

    /** The amount line of the sale (SC-09): the amount in the shop's currency, then what the customer sends in sats. */
    fun amountLine(money: Money, amountMinor: Long?, needsSats: Long): String =
        if (amountMinor != null) "${money.show(amountMinor)}, ${Format.sats(needsSats)} to send" else "${Format.sats(needsSats)} to send"

    /** What has arrived of what the sale needs. */
    fun receivedLine(received: Long, needs: Long): String = "Received ${Format.sats(received)} of ${Format.sats(needs)}"

    /** Whether the sale is over for the cashier's list (nothing more will change on its own). */
    fun isEnded(state: SaleState): Boolean =
        state == SaleState.Expired || state == SaleState.Cancelled || state == SaleState.PaidLightning
}

/** The shop's currency from the table of price sources: the currency all of them share, or null when they differ or there are none. */
object ShopCurrency {
    fun of(sources: List<org.openbmt.counter.RateSource>): Money? {
        val codes = sources.map { it.currency }.distinct()
        return if (codes.size == 1) Money.of(codes[0]) else null
    }
}
