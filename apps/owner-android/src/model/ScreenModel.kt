package org.openbmt.owner.app

import org.openbmt.app.SayWords

/** What is saved on this phone. The phone is set up only when both are there. */
class SetupStatus(val setup: Boolean, val config: Boolean) {
    val complete: Boolean get() = setup && config
}

/** One root screen of the app, chosen from what is saved and what is running. */
sealed class Screen {
    /** The app could not read its own state or the library did not load: nothing else is offered but trying again. */
    class Fault(val words: SayWords) : Screen()

    /** Nothing, or not everything, is saved: the setup text and the Tor address are asked for. */
    object FirstStart : Screen()

    /** Orbot did not answer: nothing is sent, and the app says so (OA-05). */
    class TorBlocked(val words: SayWords) : Screen()

    /** The node is being started. */
    object Starting : Screen()

    /** The node runs: the home screen. */
    object Home : Screen()
}

object ScreenModel {
    /**
     * A fault comes first; then an unfinished setup; then Tor without Orbot;
     * then the node that is not running yet; then home.
     */
    fun pick(fault: SayWords?, setup: SetupStatus, torBlocked: SayWords?, running: Boolean): Screen = when {
        fault != null -> Screen.Fault(fault)
        !setup.complete -> Screen.FirstStart
        torBlocked != null -> Screen.TorBlocked(torBlocked)
        !running -> Screen.Starting
        else -> Screen.Home
    }
}
