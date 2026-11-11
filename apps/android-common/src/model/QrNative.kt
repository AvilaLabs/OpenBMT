package org.openbmt.common

/**
 * The QR reader of `crates/openbmt-qr-jni`: one native function. The library is loaded by name on first
 * use ([load]); the JVM harness test loads the host-built file by path instead.
 */
object QrNative {
    /**
     * Reads the Y plane [luma] of a [width] by [height] frame whose rows are [stride] bytes apart. The
     * answer is byte 0 = 0 and the bytes of the code, or byte 0 = 1 and the rule code of the refusal as
     * ASCII (QR-12 when the frame has no code). Null when the library could not make an answer.
     */
    @JvmStatic
    external fun decode(width: Int, height: Int, stride: Int, luma: ByteArray): ByteArray?

    /** Loads `libopenbmt_qr_jni.so`. Null when it loaded, otherwise the system's text. */
    @Synchronized
    fun load(): String? = try {
        System.loadLibrary("openbmt_qr_jni")
        null
    } catch (e: UnsatisfiedLinkError) {
        e.message ?: "the reader library did not load"
    }
}
