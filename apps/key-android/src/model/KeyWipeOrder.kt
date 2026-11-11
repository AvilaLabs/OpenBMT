package org.openbmt.key.app

import java.io.File
import org.openbmt.app.TriesFile
import org.openbmt.app.WipeOrder

/**
 * SR-76: the Key app's wipe is the shared order (`WipeOrder`) over its one sealed file: the state file goes
 * first, then the Keystore key, then the file of tries.
 */
object Wipe {
    const val STATE_FILE = "state.obks"

    fun run(dir: File, tries: TriesFile, deleteKey: () -> Unit) = WipeOrder.run(dir, listOf(STATE_FILE), tries, deleteKey)
}
