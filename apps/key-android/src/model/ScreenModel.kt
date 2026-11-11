package org.openbmt.key.app

import org.openbmt.key.Key2TakenView
import org.openbmt.key.Key3View
import org.openbmt.key.LockStatus
import org.openbmt.key.SetupState
import org.openbmt.key.Words

/** One screen of the app, chosen from what the library and the system say. */
sealed class Screen {
    /** The app will not run: full-screen words, nothing else. */
    class Refused(val words: List<Words>) : Screen()

    /** The library or the saved state could not be used; nothing else is offered but trying again. */
    class Fault(val words: Words) : Screen()

    /** The key is behind a PIN: ask for it before any other screen (SR-77). */
    object Unlock : Screen()

    /**
     * No PIN yet: choose one. [upgrade] is true for a key saved by an older build, which is locked behind the
     * PIN and then used as it is; false for a new or restored key, whose first store is already locked (SR-72).
     */
    class SetPin(val upgrade: Boolean) : Screen()

    /** The 10th wrong try wiped this phone's copy of the key (SR-76). */
    object Wiped : Screen()

    /** No key yet: make one from dice, coins, typed words or the device, or be an approver only. */
    object Setup : Screen()

    /** The words of the new key, shown once. */
    object ShowWords : Screen()

    /** An identity and maybe key 2, and no key 3 yet: scan it (DESIGN section 2.1, step 2). */
    class Key3(val holdsKey2: Boolean) : Screen()

    /** Key 3 is held and key 2 is on a hardware signer: scan key 2 from it (SR-84). */
    class Key2(val key3: Key3View) : Screen()

    /** Key 3 is held: show the public answer, then read the owner phone's wallet (step 5). */
    class GiveAnswer(val holdsKey2: Boolean, val key3: Key3View) : Screen()

    /** A wallet passed every check and waits for "they match" or "they differ" (SR-67). */
    class ComparePending(val holdsKey2: Boolean) : Screen()

    /** The wallet is accepted: approve requests, check the kit, see the limits. */
    class Home(val holdsKey2: Boolean) : Screen()
}

/** Picks the screen. No Android class is used here, so the JVM test runs it. */
object ScreenModel {
    /**
     * A refusal of the system comes first; then a fault; then the screen
     * of the library's state. A null [state] with no [fault] means the library
     * has not been opened yet, which is a fault of the app and is shown as one.
     * [key3] is key 3 as the library keeps it, or null; it only matters for an
     * identity that is waiting for the wallet. [key2Taken] is key 2 as taken from its signer, or null; it only
     * matters for an identity that holds no key 2. [lock] is where the lock is; it only matters while the state is
     * empty (a PIN comes before the first key) or locked.
     */
    fun pick(
        refusals: List<Words>,
        fault: Words?,
        state: SetupState?,
        key3: Key3View? = null,
        lock: LockStatus? = null,
        key2Taken: Key2TakenView? = null,
    ): Screen = when {
        refusals.isNotEmpty() -> Screen.Refused(refusals)
        fault != null -> Screen.Fault(fault)
        state == null -> Screen.Fault(AppWords.libraryMissing(null))
        else -> when (state) {
            is SetupState.Empty -> if (lock == LockStatus.NoPin) Screen.SetPin(false) else Screen.Setup
            is SetupState.Locked -> if (lock == LockStatus.Legacy) Screen.SetPin(true) else Screen.Unlock
            is SetupState.HasIdentity ->
                when {
                    state.wordsWaiting -> Screen.ShowWords
                    key3 == null -> Screen.Key3(state.holdsKey2)
                    !state.holdsKey2 && key2Taken == null -> Screen.Key2(key3)
                    else -> Screen.GiveAnswer(state.holdsKey2, key3)
                }
            is SetupState.WalletPending -> Screen.ComparePending(state.holdsKey2)
            is SetupState.Ready -> Screen.Home(state.holdsKey2)
        }
    }
}
