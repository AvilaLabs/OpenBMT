package org.openbmt.app

/** Where a code of [modules] by [modules] squares is drawn in a box: [cell] whole pixels a square, from [left] and [top]. */
class AnimatedBox(val cell: Int, val left: Int, val top: Int, val modules: Int) {
    /** The left edge of the module in column [x], in pixels. */
    fun x(x: Int): Int = left + (AnimatedQr.QUIET + x) * cell

    /** The top edge of the module in row [y], in pixels. */
    fun y(y: Int): Int = top + (AnimatedQr.QUIET + y) * cell

    /** Whether a code can be drawn at all: a cell is at least 1 pixel. */
    val fits: Boolean get() = cell >= 1
}

/** The numbers of the animated code, and the pure parts of showing it (the view is `AnimatedQrView`). */
object AnimatedQr {
    /** Parts shown a second. To be tuned on a device. */
    const val FPS = 5

    /**
     * The largest fragment, in bytes, of one part. A part of this length draws as a QR code of version
     * [MAX_VERSION] or less at level L in the alphanumeric mode, for both types and any message up to a
     * megabyte (the tests assert it with the library). To be tuned on a device.
     */
    const val MAX_FRAGMENT = 230

    /** The largest QR version of any part. */
    const val MAX_VERSION = 12

    /** The error-correction level of a part: L. */
    const val LEVEL = 0

    /** The quiet zone, in modules. */
    const val QUIET = 4

    /** The index of the part on screen [elapsedMs] after the first, for [count] parts shown [fps] a second. */
    fun indexAt(elapsedMs: Long, count: Int, fps: Int = FPS): Int {
        if (count <= 1 || elapsedMs <= 0) return 0
        return ((elapsedMs * fps / 1000) % count).toInt()
    }

    /** As large as fits in [width] by [height] pixels, whole pixels a square, with the quiet zone on every side. */
    fun box(modules: Int, width: Int, height: Int): AnimatedBox {
        require(modules > 0) { "a code has modules" }
        val total = modules + 2 * QUIET
        val cell = minOf(width, height) / total
        val side = cell * total
        return AnimatedBox(cell, (width - side) / 2, (height - side) / 2, modules)
    }

    /** The parts of [payload] as [type] UR text, in upper case, at [MAX_FRAGMENT] bytes. */
    fun parts(call: UrCall, type: String, payload: ByteArray): UrOutcome<List<String>> =
        call.urEncode(type, payload, MAX_FRAGMENT)
}

/**
 * The codes of a list of [parts], drawn by the library one at a time as they are needed and kept: each
 * one is drawn once. A single part is a still code ([animated] is false).
 */
class AnimatedFrames(private val call: UrCall, val parts: List<String>, val fps: Int = AnimatedQr.FPS) {
    private val drawn = HashMap<Int, QrMatrix>()

    /** Whether the code changes: more than one part. */
    val animated: Boolean get() = parts.size > 1

    /** The index of the part [elapsedMs] after the first. */
    fun indexAt(elapsedMs: Long): Int = AnimatedQr.indexAt(elapsedMs, parts.size, fps)

    /** The code of part [index], or null when the library could not draw it. */
    fun matrix(index: Int): QrMatrix? {
        drawn[index]?.let { return it }
        val text = parts.getOrNull(index) ?: return null
        val outcome = call.qrEncode(text.toByteArray(Charsets.US_ASCII), AnimatedQr.LEVEL)
        if (outcome !is UrOutcome.Ok) return null
        drawn[index] = outcome.value
        return outcome.value
    }

    /** Draws every part now, giving the first that could not be drawn, or null when all could. */
    fun drawAll(): Int? {
        for (i in parts.indices) if (matrix(i) == null) return i
        return null
    }
}
