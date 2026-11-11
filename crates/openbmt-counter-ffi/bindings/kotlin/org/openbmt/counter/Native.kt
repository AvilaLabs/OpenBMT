package org.openbmt.counter

/**
 * The one native function of the library.
 *
 * It is safe to call from more than one thread at once: the app calls
 * `pump` in a loop on one background thread and every other call on its
 * screen thread. The library keeps nothing of the call between two calls.
 */
object Native {
    init {
        System.loadLibrary("openbmt_counter_ffi")
    }

    /**
     * One request frame in, one response frame out (see PROTOCOL.md). Null
     * when the library could not make a response at all.
     */
    @JvmStatic
    external fun call(request: ByteArray): ByteArray?
}
