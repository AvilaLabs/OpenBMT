package org.openbmt.owner.app

import java.io.IOException
import org.openbmt.app.KeystoreSealer

/** OA-64: deletes the Keystore key that seals the setup and the ceremony's record. */
object OwnerWipe {
    /** Throws [IOException] when the key is still there afterwards. */
    fun deleteKey() {
        val keystore = java.security.KeyStore.getInstance(KeystoreSealer.PROVIDER)
        keystore.load(null)
        if (keystore.containsAlias(AppFiles.KEY_ALIAS)) keystore.deleteEntry(AppFiles.KEY_ALIAS)
        if (keystore.containsAlias(AppFiles.KEY_ALIAS)) throw IOException("the key of the saved setup is still there")
    }
}
