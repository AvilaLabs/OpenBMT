package org.openbmt.counter.app

import org.openbmt.counter.Actor

/**
 * Who is at the till (docs/COUNTER-RULES.md CR-147 to CR-150), in memory only:
 * nothing here is written to a file, so an app that was stopped starts with
 * nobody signed in (CR-149). The person signs in with a PIN the app has
 * checked; the lock happens when the person taps Lock and when the app was out
 * of view for more than [OUT_SECS] seconds, but never while a sale's screen is
 * open: the lock waits and happens when that screen closes. [clock] counts
 * seconds that do not go back and do not stop while the phone sleeps.
 */
class TillLock(private val clock: () -> Long) {
    /** The person signed in, or null when the till is locked. */
    var who: Actor? = null
        private set

    private var outSince: Long? = null
    private var sale: Long? = null
    private var due = false

    /** Whether a lock is due and waits for the sale's screen to close. */
    val waiting: Boolean get() = due

    /** The person has typed the right PIN. */
    fun signIn(person: Actor) {
        who = person
        due = false
    }

    /** The person taps Lock. */
    fun lock() {
        who = null
        due = false
    }

    /** Nobody is signed in and nothing waits: the list of cashiers became empty (CR-146). */
    fun forget() {
        who = null
        due = false
        outSince = null
        sale = null
    }

    /** The app went out of view. The first time counts when it is told twice. */
    fun leftView() {
        if (outSince == null) outSince = clock()
    }

    /** The app is in view again: more than [OUT_SECS] seconds out makes a lock due. */
    fun backInView() {
        val since = outSince ?: return
        outSince = null
        if (clock() - since > OUT_SECS) due = true
        apply()
    }

    /** The screen of sale [index] is open, or, with null, no sale's screen is (the lock that waited happens now). */
    fun saleScreen(index: Long?) {
        sale = index
        apply()
    }

    /** The sale whose screen was open when the app went out of view, for a person who is still signed in. */
    fun reopen(): Long? = if (who != null) sale else null

    private fun apply() {
        if (due && sale == null) {
            who = null
            due = false
        }
    }

    companion object {
        /** More than this many seconds out of view locks the till. */
        const val OUT_SECS = 300L
    }
}
