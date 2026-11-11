package org.openbmt.common

/**
 * The second function of `crates/openbmt-qr-jni`: one framed request in, one framed reply out, for
 * drawing a text as a QR code and for the parts of a UR (docs/UR-RULES.md, section 6). The library is
 * the one [QrNative.load] loads. The framing is written in `UrCall`, in pure Kotlin.
 */
object UrNative {
    /**
     * Runs one request (the bytes `OBMo`, a version, an operation and its fields) and gives the framed
     * reply. Null when the library could not make an answer.
     */
    @JvmStatic
    external fun call(request: ByteArray): ByteArray?
}
