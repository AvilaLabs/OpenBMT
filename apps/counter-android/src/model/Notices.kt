package org.openbmt.counter.app

import org.openbmt.app.Colour

/** One notice of CR-284: the sale that turned, and to which colour. */
class SaleNotice(val index: Long, val colour: Colour) {
    /** The sale's number as the Sales list shows it. */
    val number: Long get() = index + 1

    val heading: String
        get() = if (colour == Colour.Green) LightningFirstWords.paidHeading(number) else LightningFirstWords.redHeading(number)

    val next: String
        get() = if (colour == Colour.Green) LightningFirstWords.PAID_NEXT else LightningFirstWords.RED_NEXT
}

/**
 * CR-284: the notices of sales that turned green or red while another screen showed, in memory only. A notice stays
 * until it is tapped or its sale's screen is shown. At most [MAX_SHOWN] show, the newest first, and the rest are one
 * line that opens the Sales list.
 */
class Notices {
    private val held = ArrayList<SaleNotice>()

    /**
     * A sale [index] turned to [colour]. True when a notice was added, which is when the caller plays the colour's
     * sound once: not when the sale's own screen shows ([showing]), and not when its screen already showed that
     * colour, which played the sound ([plays]). A sale that turns green and then red has the notice of the red.
     */
    fun turned(index: Long, colour: Colour, showing: Long?, plays: OutcomePlays): Boolean {
        if (colour != Colour.Green && colour != Colour.Red) return false
        if (showing == index) return false
        if (!plays.first(index, colour)) return false
        held.removeAll { it.index == index }
        held.add(SaleNotice(index, colour))
        return true
    }

    /** The screen of sale [index] shows, or its notice was tapped: the notice goes. True when there was one. */
    fun remove(index: Long): Boolean = held.removeAll { it.index == index }

    /** The notices that show, the newest first, at most [MAX_SHOWN]. */
    fun shown(): List<SaleNotice> = held.asReversed().take(MAX_SHOWN)

    /** How many more there are than show: the number of the line "and N more in Sales". */
    fun more(): Int = (held.size - MAX_SHOWN).coerceAtLeast(0)

    /** Whether nothing is to be shown. */
    val isEmpty: Boolean get() = held.isEmpty()

    companion object {
        const val MAX_SHOWN = 3
    }
}
