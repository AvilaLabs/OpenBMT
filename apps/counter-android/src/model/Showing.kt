package org.openbmt.counter.app

import org.openbmt.counter.SaleState

/**
 * Which screen the counter shows, as a small value (CR-177). It holds no picture of a screen: a screen is
 * drawn again from the library's state, so a sale comes back as it is now, not as it was.
 */
sealed class Showing {
    /** The home screen. */
    object Home : Showing()

    /** The list of sales. */
    object Sales : Showing()

    /** The screen of sale [index], or one of its own (details, refunds, the override). */
    data class Sale(val index: Long) : Showing()

    /** Any other screen: it is not drawn again from state, and the home screen takes its place. */
    object Other : Showing()
}

object Resume {
    /**
     * Whether a sale is still open: not yet ended. Expired, cancelled, paid by Lightning and paid at depth
     * (final) are ended (CR-177); the others still need the cashier.
     */
    fun open(state: SaleState): Boolean = !SaleModel.isEnded(state) && state != SaleState.Final

    /**
     * CR-177: the screen to show when the node is up again. [remembered] is what this run was showing when the
     * app left the screen, or null when the process is new. A sale comes back as a sale, the list as the list,
     * and any other screen as home.
     */
    fun after(remembered: Showing): Showing = when (remembered) {
        is Showing.Sale, Showing.Sales, Showing.Home -> remembered
        Showing.Other -> Showing.Home
    }

    /**
     * CR-177: the screen to show at a new start, from the indexes of the open sales the library lists: one open
     * sale opens on its screen, more than one on the list, none on home.
     */
    fun atStart(open: List<Long>): Showing = when (open.size) {
        0 -> Showing.Home
        1 -> Showing.Sale(open[0])
        else -> Showing.Sales
    }

    /** The screen to show: what was remembered by this run, or the choice at a new start. */
    fun choose(remembered: Showing?, open: () -> List<Long>): Showing =
        if (remembered != null) after(remembered) else atStart(open())
}
