package org.openbmt.key

/**
 * What the app implements: storage, randomness and the clock. The library
 * calls these during one call of [Native.call] and on the calling thread,
 * and keeps no reference to the object afterwards.
 *
 * `save` must not return before the bytes are durable (flushed and synced).
 * The bytes hold the identity secret and, when this device holds key 2, key
 * 2's account secret: wrap them with a key from the Android Keystore before
 * they are written, and keep them out of backups and device transfer. If any
 * method throws, the library treats it as a failed store and nothing it
 * keeps changes.
 */
interface KeyStore {
    /** The bytes last passed to [save], or null on a first start. */
    fun load(): ByteArray?

    /** Store [bytes]; durable before this returns. Throw on any failure. */
    fun save(bytes: ByteArray)

    /** [n] bytes from the platform's cryptographically secure random source. */
    fun random(n: Int): ByteArray

    /** The time now, in seconds since 1970-01-01 UTC. */
    fun now(): Long
}

/** The one native function of the library. */
object Native {
    init {
        System.loadLibrary("openbmt_key_ffi")
    }

    /**
     * One request frame in, one response frame out (see PROTOCOL.md). Null
     * when the library could not make a response at all.
     */
    @JvmStatic
    external fun call(store: KeyStore, request: ByteArray): ByteArray?
}
