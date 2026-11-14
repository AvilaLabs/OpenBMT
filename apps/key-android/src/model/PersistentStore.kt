package org.openbmt.key.app

import org.openbmt.app.SealedFile
import org.openbmt.key.KeyStore

/** The library's [KeyStore] over a [SealedFile] and the clock. The library draws its own random bytes. */
class PersistentStore(
    private val file: SealedFile,
    private val clock: () -> Long = { System.currentTimeMillis() / 1000 },
) : KeyStore {
    override fun load(): ByteArray? = file.load()

    override fun save(bytes: ByteArray) = file.save(bytes)

    override fun now(): Long = clock()
}
