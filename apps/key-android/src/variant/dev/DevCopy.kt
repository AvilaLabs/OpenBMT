package org.openbmt.key.app

/**
 * The development copy of the Key app (SR-91), built by `build.sh --dev`
 * only. It passes the USB debugging check so that it can be tested on a
 * phone that is plugged in for development, and says so on every screen.
 * It is never released.
 */
object DevCopy {
    const val ON = true
    const val BAND = "Development copy. Not for real money."
}
