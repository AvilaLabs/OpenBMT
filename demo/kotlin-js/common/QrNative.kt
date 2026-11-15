package org.openbmt.common

/** The browser's twin of the camera reader: a page has no camera frame to read, so there is no answer. */
object QrNative {
    fun decode(width: Int, height: Int, stride: Int, luma: ByteArray): ByteArray? = null

    /** The reader is not loaded in the page. */
    fun load(): String? = "the page has no reader library"
}
