package org.openbmt.key.app

import java.io.IOException
import org.openbmt.app.KeystoreSealer

/** SR-76: deletes the Keystore key that wraps the saved state. */
object KeyWipe {
    /** Throws [IOException] when the key is still there afterwards. */
    fun deleteKey() {
        val keystore = java.security.KeyStore.getInstance(KeystoreSealer.PROVIDER)
        keystore.load(null)
        if (keystore.containsAlias(KeystoreSealer.ALIAS)) keystore.deleteEntry(KeystoreSealer.ALIAS)
        if (keystore.containsAlias(KeystoreSealer.ALIAS)) throw IOException("the key of the saved state is still there")
    }
}
