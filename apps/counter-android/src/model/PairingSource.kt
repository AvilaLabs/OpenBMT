package org.openbmt.counter.app

import org.openbmt.app.SayWords

/**
 * Where the pairing text comes from, other than the text field of the first
 * screen: a file picked from the phone, or the pairing code read with the
 * camera. Each is a source behind the same two calls. [begin] gives the text
 * to [onText], or the words of why there is none to [onFailed]; it may do
 * either later, on the screen thread. A source the person cancels does
 * neither and shows the first screen again.
 */
interface PairingSource {
    /** What the button for this source says. */
    val label: String

    fun begin(onText: (String) -> Unit, onFailed: (SayWords) -> Unit)
}
