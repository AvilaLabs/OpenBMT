package org.openbmt.counter.app

import org.openbmt.counter.Place
import org.openbmt.counter.SaleState

/**
 * A counter at a place of giving (docs/GIVING-RULES.md, GV-11 to GV-14), as small values and rules. No Android class
 * is used. The settings' `place` decides: a shop keeps every screen as it was.
 */
object Giving {
    /** GV-14: seconds without a touch after which a giving point returns to the amount screen. */
    const val IDLE_SECS = 120L

    /** Whether the place takes gifts rather than sales. */
    fun isGiving(place: Place): Boolean = place == Place.Giving

    /**
     * GV-14: whether a screen that is not the amount screen or the gift in progress needs the manager's PIN: every
     * setting, the gift list and the day's totals, until the manager has typed it.
     */
    fun needsPin(place: Place, managerIn: Boolean): Boolean = isGiving(place) && !managerIn

    /** GV-11: home is the amount the giver types, while nobody has typed the manager's PIN. */
    fun homeIsAmount(place: Place, managerIn: Boolean): Boolean = needsPin(place, managerIn)

    /** GV-12: the end of a gift: a gift that has arrived, which ends on the giving outcome screen and not on the hand-over screens. */
    fun endsAsGift(place: Place, state: SaleState): Boolean = isGiving(place) && SaleModel.isPaid(state)

    /**
     * GV-13: whether the home screens offer the steps of a till that sells: the requests from the Box till and the
     * cashier's lock. Each stays in the settings.
     */
    fun homeOffersTillSteps(place: Place): Boolean = !isGiving(place)

    /** GV-12: the gift's own page has no button for details and refunds, the sale list or the hand-by-hand accept, unless the manager is in. */
    fun pageOffersManagerSteps(place: Place, managerIn: Boolean): Boolean = !needsPin(place, managerIn)

    /**
     * GV-14: whether a screen is left for the amount screen after [IDLE_SECS] without a touch. The gift in progress,
     * which shows the code the giver pays, is the one screen that stays: the giver pays with their own phone.
     */
    fun leavesWhenIdle(giftInProgress: Boolean): Boolean = !giftInProgress
}

/**
 * GV-14: the manager's PIN having been typed at a giving point, and the time of the last touch. [clock] counts seconds
 * that do not go back. A touch, or the PIN, starts the count again; once the time is up, the manager is out again and
 * the screen returns to the amount.
 */
class GivingDesk(private val clock: () -> Long) {
    /** The manager has typed the PIN, and has not been out since. */
    var managerIn: Boolean = false
        private set

    private var touchedAt: Long = clock()

    /** Someone touched the screen. */
    fun touch() {
        touchedAt = clock()
    }

    /** The manager typed the right PIN. */
    fun managerUnlocked() {
        managerIn = true
        touch()
    }

    /** The manager leaves, or the app starts again. */
    fun leave() {
        managerIn = false
        touch()
    }

    /** Whether [Giving.IDLE_SECS] have gone by since the last touch. */
    fun idle(): Boolean = clock() - touchedAt >= Giving.IDLE_SECS

    /**
     * True when the screen should return to the amount now: the time is up and the screen is not the gift in progress.
     * The manager is then out, and the count starts again.
     */
    fun returnsToAmount(giftInProgress: Boolean): Boolean {
        if (!idle() || !Giving.leavesWhenIdle(giftInProgress)) return false
        leave()
        return true
    }
}
