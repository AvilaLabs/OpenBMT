package org.openbmt.counter.app

import java.io.IOException
import javax.crypto.SecretKeyFactory
import javax.crypto.spec.PBEKeySpec

/** PBKDF2 with HMAC-SHA-256 from the platform's `SecretKeyFactory`: 256 bits out. */
object Pbkdf2Hasher : PinHasher {
    override fun hash(pin: CharArray, salt: ByteArray, iterations: Int): ByteArray {
        val spec = PBEKeySpec(pin, salt, iterations, PinStore.HASH_LEN * 8)
        try {
            return SecretKeyFactory.getInstance("PBKDF2WithHmacSHA256").generateSecret(spec).encoded
        } catch (e: java.security.GeneralSecurityException) {
            throw IOException("the platform has no PBKDF2 with HMAC-SHA-256", e)
        } finally {
            spec.clearPassword()
        }
    }
}
