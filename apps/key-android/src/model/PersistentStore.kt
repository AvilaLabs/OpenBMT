package org.openbmt.key.app

import java.security.SecureRandom
import org.openbmt.app.SealedFile
import org.openbmt.key.KeyStore

/** The library's [KeyStore] over a [SealedFile], the platform's random source and the clock. */
class PersistentStore(
    private val file: SealedFile,
    private val random: SecureRandom = SecureRandom(),
    private val clock: () -> Long = { System.currentTimeMillis() / 1000 },
) : KeyStore {
    override fun load(): ByteArray? = file.load()

    override fun save(bytes: ByteArray) = file.save(bytes)

    override fun random(n: Int): ByteArray = ByteArray(n).also { random.nextBytes(it) }

    override fun now(): Long = clock()
}
