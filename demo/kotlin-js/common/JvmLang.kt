// The parts of the JVM's language library that the apps' model uses, for the
// browser build only (scripts/demo-kotlin-js.sh adds this file to the source
// list; the Android app and its JVM tests never see it). Each is the small
// part the model calls, with the JVM's meaning.
@file:Suppress("PackageDirectoryMismatch", "NOTHING_TO_INLINE", "unused")

package kotlin

import kotlin.math.pow

/** Marks of the JVM that mean nothing on one thread. */
@Target(AnnotationTarget.FUNCTION, AnnotationTarget.PROPERTY_GETTER, AnnotationTarget.PROPERTY_SETTER)
annotation class Synchronized

@Target(AnnotationTarget.FIELD, AnnotationTarget.PROPERTY)
annotation class Volatile

@Target(AnnotationTarget.FUNCTION, AnnotationTarget.PROPERTY)
annotation class JvmStatic

/** `java.lang.Math`, the calls the model makes. */
object Math {
    const val PI: Double = kotlin.math.PI
    fun abs(x: Double): Double = kotlin.math.abs(x)
    fun abs(x: Float): Float = kotlin.math.abs(x)
    fun abs(x: Long): Long = kotlin.math.abs(x)
    fun abs(x: Int): Int = kotlin.math.abs(x)
    fun sin(x: Double): Double = kotlin.math.sin(x)
    fun sqrt(x: Double): Double = kotlin.math.sqrt(x)
    fun log(x: Double): Double = kotlin.math.ln(x)
    fun floor(x: Double): Double = kotlin.math.floor(x)
    fun pow(x: Double, y: Double): Double = x.pow(y)
}

/** `java.lang.Character`, the calls the model makes. */
object Character {
    fun isISOControl(c: Char): Boolean = c.code <= 0x1f || c.code in 0x7f..0x9f
    fun isWhitespace(c: Char): Boolean = c.isWhitespace()
    fun digit(c: Char, radix: Int): Int = c.digitToIntOrNull(radix) ?: -1
}

object System {
    fun arraycopy(src: ByteArray, srcPos: Int, dest: ByteArray, destPos: Int, length: Int) {
        src.copyInto(dest, destPos, srcPos, srcPos + length)
    }
}

/** `synchronized`: nothing else runs in the page while a block does. */
inline fun <R> synchronized(lock: Any, block: () -> R): R = block()

/** A library that is not there; the page's twin of the native library is always there, so it is never thrown. */
open class LinkageError(message: String? = null) : Error(message)

class InterruptedException(message: String? = null) : Exception(message)

/** A thread that runs its body when it starts, to the end: nothing else runs in the page meanwhile. */
class Thread(private val target: Runnable?) {
    var isDaemon: Boolean = false

    fun start() {
        target?.run()
    }
    fun join() {}
    fun interrupt() {}

    companion object {
        fun currentThread(): Thread = Thread(null)
    }
}

fun interface Runnable {
    fun run()
}
