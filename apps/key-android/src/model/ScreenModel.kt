package org.openbmt.key.app

import org.openbmt.key.SetupState
import org.openbmt.key.Words

/** One screen of the app, chosen from what the library and the system say. */
sealed class Screen {
    /** The app will not run: full-screen words, nothing else. */
    class Refused(val words: List<Words>) : Screen()

    /** The library or the saved state could not be used; nothing else is offered but trying again. */
    class Fault(val words: Words) : Screen()

    /** No key yet: make one from dice, coins, typed words or the device, or be an approver only. */
    object Setup : Screen()

    /** The words of the new key, shown once. */
    object ShowWords : Screen()

    /** An identity and maybe key 2, waiting for the wallet to be accepted. */
    class AcceptWallet(val holdsKey2: Boolean) : Screen()

    /** The wallet is accepted: approve requests, check the kit, see the limits. */
    class Home(val holdsKey2: Boolean) : Screen()
}

/** Picks the screen. No Android class is used here, so the JVM test runs it. */
object ScreenModel {
    /**
     * A refusal of the system comes first; then a fault; then the screen
     * of the library's state. A null [state] with no [fault] means the library
     * has not been opened yet, which is a fault of the app and is shown as one.
     */
    fun pick(refusals: List<Words>, fault: Words?, state: SetupState?): Screen = when {
        refusals.isNotEmpty() -> Screen.Refused(refusals)
        fault != null -> Screen.Fault(fault)
        state == null -> Screen.Fault(AppWords.libraryMissing(null))
        else -> when (state) {
            is SetupState.Empty -> Screen.Setup
            is SetupState.HasIdentity ->
                if (state.wordsWaiting) Screen.ShowWords else Screen.AcceptWallet(state.holdsKey2)
            is SetupState.Ready -> Screen.Home(state.holdsKey2)
        }
    }
}
