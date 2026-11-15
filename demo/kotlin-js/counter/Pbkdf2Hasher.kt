package org.openbmt.counter.app

import java.security.Sha256

/** PBKDF2 with HMAC-SHA-256, written out, for the browser (the page's own crypto is asynchronous): 256 bits out. */
object Pbkdf2Hasher : PinHasher {
    override fun hash(pin: CharArray, salt: ByteArray, iterations: Int): ByteArray {
        val key = pin.concatToString().encodeToByteArray()
        val first = hmac(key, salt + byteArrayOf(0, 0, 0, 1))
        var u = first
        val t = first.copyOf()
        for (i in 1 until iterations) {
            u = hmac(key, u)
            for (j in t.indices) t[j] = (t[j].toInt() xor u[j].toInt()).toByte()
        }
        return t
    }

    private fun hmac(key: ByteArray, message: ByteArray): ByteArray {
        val k = (if (key.size > 64) Sha256.hash(key) else key).copyOf(64)
        val inner = ByteArray(64) { (k[it].toInt() xor 0x36).toByte() } + message
        val outer = ByteArray(64) { (k[it].toInt() xor 0x5c).toByte() }
        return Sha256.hash(outer + Sha256.hash(inner))
    }
}
