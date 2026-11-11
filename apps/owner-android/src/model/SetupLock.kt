package org.openbmt.owner.app

import org.openbmt.app.Tries

/** What the start does with the stored setup before any other screen and before the node starts (OA-62). */
enum class Gate {
    /** The setup is locked: ask for the PIN. */
    Unlock,

    /** The setup was saved before the lock existed: it opens only into the step that sets a PIN (OA-61). */
    SetPin,

    /** The count of wrong tries is at the wipe count: finish the wipe first (OA-64). */
    Wipe,
}

/**
 * The pure parts of the lock on the owner phone's setup (docs/OWNER-APP-RULES.md OA-60 to OA-65). The frame,
 * the PIN rules and the KDF are the library's (`OwnerLock`, `docs/LOCK-RULES.md`); the count of wrong tries,
 * the waits, the wipe order and the 60-second rule are the shared ones (`LockModel.kt` of android-common).
 */
object SetupLock {
    /** A stored setup that is not locked is the bare text that older builds saved. */
    fun gate(wrong: Int, locked: Boolean): Gate = when {
        wrong >= Tries.WIPE_AT -> Gate.Wipe
        locked -> Gate.Unlock
        else -> Gate.SetPin
    }

    /**
     * OA-64: what the wipe deletes from the app's folder, in this order, before the Keystore key and then the
     * file of tries: the setup, and the record of a ceremony that is not finished (which holds key 1 too).
     */
    val WIPE_FILES: List<String> = listOf(AppFiles.SETUP_FILE, AppFiles.CEREMONY_FILE)
}
