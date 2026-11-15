// java.util, java.text and java.time for the browser build, on the page's own
// clock and time zone (see JavaIo.kt).
@file:Suppress("PackageDirectoryMismatch", "unused")

package java.util

class Date(val millis: Long)

class Locale private constructor() {
    companion object {
        val ROOT = Locale()
    }
}

object Arrays {
    fun fill(a: ByteArray, value: Byte) = a.fill(value)
}

object Collections {
    /** The list itself: nothing else runs in the page while it is used. */
    fun <T> synchronizedList(list: MutableList<T>): MutableList<T> = list
}
