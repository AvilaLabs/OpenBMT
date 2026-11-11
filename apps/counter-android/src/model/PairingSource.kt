package org.openbmt.counter.app

import org.openbmt.app.SayWords

/**
 * Where the pairing text comes from, other than the text field of the first
 * screen. Today it is a file picked from the phone; a camera that reads the
 * pairing code will be another source behind the same two calls, and nothing
 * else of the app changes. [begin] gives the text to [onText], or the words
 * of why there is none to [onFailed]; it may do either later, on the screen
 * thread.
 */
interface PairingSource {
    /** What the button for this source says. */
    val label: String

    fun begin(onText: (String) -> Unit, onFailed: (SayWords) -> Unit)
}
