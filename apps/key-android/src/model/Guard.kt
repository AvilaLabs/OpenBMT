package org.openbmt.key.app

import org.openbmt.key.Words

/**
 * Whether the app may run (docs/DESIGN.md, "Signing hygiene"): it refuses
 * when USB or wireless debugging is on or an accessibility service is on.
 * The inputs are what the system settings say; the Android side reads them.
 * The development copy (SR-91) alone skips the USB debugging check.
 */
object Guard {
    /** The words to show, one for each thing that is on; empty when the app may run. */
    fun refusals(
        adbEnabled: Int,
        wirelessAdbEnabled: Int,
        accessibilityEnabled: Int,
        accessibilityServices: Int,
    ): List<Words> {
        val out = ArrayList<Words>()
        if (adbEnabled == 1 && !DevCopy.ON) out.add(AppWords.debuggingOn)
        if (wirelessAdbEnabled == 1) out.add(AppWords.wirelessDebuggingOn)
        if (accessibilityEnabled == 1 || accessibilityServices > 0) out.add(AppWords.accessibilityOn)
        return out
    }
}
