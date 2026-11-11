package org.openbmt.app

import java.nio.ByteBuffer

/** The Y plane of a camera frame as the reader takes it: [stride] bytes from one row to the next. */
class Luma(val stride: Int, val bytes: ByteArray)

/** The pure parts of the camera scan: no Android class is used, so the JVM tests run them. */
object QrFrames {
    /** The frame size asked for: near 1280 by 720, enough for a code on a counter and quick to read. */
    const val TARGET_WIDTH = 1280
    const val TARGET_HEIGHT = 720

    /** No frame under this many pixels on a side is chosen unless nothing larger is offered. */
    private const val SMALLEST_SIDE = 480

    /**
     * The size to ask the camera for, from the [offered] (width, height) pairs, or null when none is
     * offered. The nearest in area to 1280 by 720 wins among the sizes whose short side is at least
     * 480; ties go to the nearer shape (16 by 9), then to the larger area. When no size has a short
     * side of 480, the largest offered is taken.
     */
    fun chooseSize(offered: List<Pair<Int, Int>>): Pair<Int, Int>? {
        val valid = offered.filter { it.first > 0 && it.second > 0 }
        if (valid.isEmpty()) return null
        val big = valid.filter { minOf(it.first, it.second) >= SMALLEST_SIDE }
        if (big.isEmpty()) return valid.maxByOrNull { it.first.toLong() * it.second }
        val target = TARGET_WIDTH.toLong() * TARGET_HEIGHT
        val shape = TARGET_WIDTH.toDouble() / TARGET_HEIGHT
        return big.sortedWith(
            compareBy<Pair<Int, Int>>(
                { Math.abs(it.first.toLong() * it.second - target) },
                { Math.abs(maxOf(it.first, it.second).toDouble() / minOf(it.first, it.second) - shape) },
                { -(it.first.toLong() * it.second) },
                { -it.first },
            ),
        ).first()
    }

    /** The bytes the reader needs of a plane with these rows: all rows, the last one cut at its width. */
    fun neededBytes(width: Int, height: Int, rowStride: Int, pixelStride: Int): Long =
        (height - 1).toLong() * rowStride + (width - 1).toLong() * pixelStride + 1

    /**
     * The Y [plane] of a frame as a buffer for the reader, or null when the plane is shorter than its
     * size and strides say or a size or stride is not positive. A plane whose pixels are next to each
     * other (pixel stride 1) is copied as it is, with its row stride, unless [tight] asks for rows of
     * exactly [width] bytes; any other pixel stride is always gathered into tight rows. The plane's
     * position and limit are not changed.
     */
    fun copyLuma(plane: ByteBuffer, width: Int, height: Int, rowStride: Int, pixelStride: Int, tight: Boolean): Luma? {
        if (width <= 0 || height <= 0 || rowStride < width || pixelStride <= 0) return null
        val src = plane.duplicate()
        src.clear()
        val needed = neededBytes(width, height, rowStride, pixelStride)
        if (needed > src.limit() || needed > Int.MAX_VALUE) return null
        if (pixelStride == 1 && !tight) {
            val out = ByteArray(needed.toInt())
            src.get(out)
            return Luma(rowStride, out)
        }
        val out = ByteArray(width * height)
        val row = ByteArray(((width - 1) * pixelStride) + 1)
        for (y in 0 until height) {
            src.position(y * rowStride)
            src.get(row)
            if (pixelStride == 1) {
                System.arraycopy(row, 0, out, y * width, width)
            } else {
                for (x in 0 until width) out[y * width + x] = row[x * pixelStride]
            }
        }
        return Luma(width, out)
    }
}

/** At most one frame in [minGapMs] is taken, so that the reader runs about eight times a second. */
class DecodeThrottle(private val minGapMs: Long = 125) {
    private var last = Long.MIN_VALUE

    /** Whether a frame that arrives at [nowMs] (a monotonic clock) is to be read; counts it when so. */
    fun admit(nowMs: Long): Boolean {
        if (last != Long.MIN_VALUE && nowMs - last < minGapMs && nowMs >= last) return false
        last = nowMs
        return true
    }
}

/** What the reader library answered for one frame. */
sealed class QrReply {
    class Decoded(val bytes: ByteArray) : QrReply()

    /** The reader refused the frame; [code] is its rule code, such as QR-12 for a frame with no code. */
    class Refused(val code: String) : QrReply()

    /** No answer, or one that does not follow the framing. */
    object Fault : QrReply()

    companion object {
        const val DECODED = 0
        const val REFUSED = 1

        fun parse(reply: ByteArray?): QrReply {
            if (reply == null || reply.isEmpty()) return Fault
            val rest = reply.copyOfRange(1, reply.size)
            return when (reply[0].toInt()) {
                DECODED -> Decoded(rest)
                REFUSED -> if (rest.all { it in 0x20..0x7e }) Refused(String(rest, Charsets.US_ASCII)) else Fault
                else -> Fault
            }
        }
    }
}

/** What a scanned text is, as far as the apps need to know. */
object UrText {
    private val sequence = Regex("^[0-9]+-[0-9]+$")

    /**
     * Whether [text] is one part of a multi-part BC-UR sequence: it starts with "ur:" (any case) and the
     * part after the type is "index-count". A single-part UR has no such part.
     */
    fun isMultiPart(text: String): Boolean {
        if (!text.startsWith("ur:", ignoreCase = true)) return false
        val parts = text.substring(3).split('/')
        return parts.size >= 3 && sequence.matches(parts[1])
    }

    fun isMultiPart(bytes: ByteArray): Boolean =
        bytes.size >= 3 && isMultiPart(String(bytes.copyOfRange(0, minOf(bytes.size, 64)), Charsets.ISO_8859_1))
}

/** The words of the camera, for both apps; [who] is the one who does the next step in that app. */
object CameraWords {
    /** The camera is not allowed: denied, or "don't ask again" was chosen. */
    fun off(who: Int) = SayWords(
        Tones.STOP, who, "Camera is off for this app",
        "This app is not allowed to use the camera, so it cannot scan a code.",
        "Allow the camera for this app in the phone's settings, or paste the text or choose a file instead.",
        null, null, null, null,
    )

    /** The camera did not start, or stopped. [problem] is the system's own text, if any. */
    fun notStarted(who: Int, problem: String?) = SayWords(
        Tones.STOP, who, "Camera did not start",
        "The camera could not be opened, so nothing was scanned.",
        "Close any other app that uses the camera and try again, or paste the text or choose a file instead.",
        null, null, null, problem?.let { Format.cleanForeign(it).take(200) },
    )
}
