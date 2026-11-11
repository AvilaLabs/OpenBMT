package org.openbmt.app

/**
 * The pure parts of the sounds, haptics and motion (src/android/Feedback.kt): which sound at what level,
 * what a sale's outcome plays, the shake of a refused PIN, the timing of the hold to approve and the
 * timing of the logo's snap. No Android class is used, so tests/FeedbackModelTest.kt runs them on the
 * desktop JVM.
 */

/** The five sounds, with the file under res/raw and the level each plays at before the master volume. */
enum class Sound(val file: String, val level: Float) {
    Confirmation("confirmation_002", 0.75f),
    Error("error_004", 0.75f),
    Tick("tick_002", 0.3f),
    Drop("drop_001", 0.8f),
    Maximize("maximize_008", 0.7f),
    ;

    /** The volume the sound plays at: its level times [SoundLevels.MASTER]. */
    val volume: Float get() = level * SoundLevels.MASTER
}

object SoundLevels {
    const val MASTER = 0.8f
}

/** The haptic of a moment; the view's own call turns it into the platform's constant (API 30 and later have Confirm and Reject). */
enum class Haptic { Confirm, Reject, Tick, LongPress }

/** What a sale's outcome gives: a sound and a haptic. */
class OutcomeFeedback(val sound: Sound, val haptic: Haptic)

object Outcomes {
    /** Good plays confirmation with a confirm haptic, stop plays error with a reject haptic; info and ended give nothing (null). */
    fun of(tone: Int): OutcomeFeedback? = when (tone) {
        Tones.GOOD -> OutcomeFeedback(Sound.Confirmation, Haptic.Confirm)
        Tones.STOP -> OutcomeFeedback(Sound.Error, Haptic.Reject)
        else -> null
    }
}

/** The shake of a refused PIN: 6 dp left and right, three times, over 300 ms. */
object Shake {
    const val DISTANCE_DP = 6f
    const val CYCLES = 3
    const val DURATION_MS = 300

    /** The sideways offset in dp at [fraction] (0 to 1) of the shake: to the left first, back to 0 at the end. */
    fun offsetDp(fraction: Double): Float {
        val f = fraction.coerceIn(0.0, 1.0)
        return (-DISTANCE_DP * Math.sin(2.0 * Math.PI * CYCLES * f)).toFloat().let { if (Math.abs(it) < 1e-4f) 0f else it }
    }
}

/** The numbers of the hold to approve. */
object HoldTiming {
    const val HOLD_MS = 1200L
    const val REWIND_MS = 250L
    const val QUARTERS = 4

    /** The time at which quarter [n] (1 to 4) is reached by a hold that is not let go. */
    fun quarterAtMs(n: Int): Long = HOLD_MS * n / QUARTERS

    const val CIRCLE_DP = 96
    const val PRESSED_SCALE = 0.94f
    const val RING_DP = 4f
    const val RING_HELD_DP = 6f

    /** After the hold is complete: the circle springs back to 1.0 with this overshoot. */
    const val OVERSHOOT = 0.12

    /** The good band fills from the centre, the tick draws itself in, one ripple ring grows and fades. */
    const val FILL_MS = 220L
    const val TICK_AT_MS = 120L
    const val TICK_MS = 220L
    const val RIPPLE_MS = 450L
    const val RIPPLE_SCALE = 1.6f

    /** The confirm haptic is at 0 and the long press haptic this long after it. */
    const val HAPTIC_GAP_MS = 90L

    /** The approval is passed on once the tick is drawn, so that the person sees it (the haptics come first). */
    const val APPROVED_AT_MS = TICK_AT_MS + TICK_MS
}

/**
 * The ring of the hold: it fills as the finger stays, and rewinds when it lets go. [advance] is given the clock
 * (milliseconds, any origin) and returns the quarters that were reached since the last call (1 to 4).
 */
class HoldClock {
    /** The part of the ring that is filled, 0 to 1. */
    var progress = 0.0
        private set
    var holding = false
        private set
    var done = false
        private set
    private var last = 0L
    private var rewindFrom = 0.0
    private var rewindStart = 0L
    private var reached = 0

    fun press(nowMs: Long) {
        if (done) return
        advance(nowMs)
        holding = true
        last = nowMs
    }

    fun release(nowMs: Long) {
        if (done) return
        advance(nowMs)
        holding = false
        rewindFrom = progress
        rewindStart = nowMs
    }

    fun advance(nowMs: Long): List<Int> {
        if (done) return emptyList()
        if (holding) {
            progress = minOf(1.0, progress + (nowMs - last).toDouble() / HoldTiming.HOLD_MS)
        } else if (rewindFrom > 0.0) {
            val left = 1.0 - (nowMs - rewindStart).toDouble() / HoldTiming.REWIND_MS
            progress = maxOf(0.0, rewindFrom * left)
        }
        last = nowMs
        val quarter = Math.floor(progress * HoldTiming.QUARTERS + 1e-9).toInt()
        val out = ArrayList<Int>()
        if (quarter > reached) for (q in reached + 1..quarter) out.add(q)
        reached = quarter
        if (progress >= 1.0) {
            done = true
            holding = false
        }
        return out
    }

    /** The approval by a double tap or by the accessibility action: complete at once. */
    fun complete() {
        progress = 1.0
        reached = HoldTiming.QUARTERS
        done = true
        holding = false
    }
}

/** A damped spring from 0 to 1, as the bench page computes it: [xs] are samples 1/60 s apart. */
class Spring(val xs: DoubleArray, val seconds: Double, val seat: Double) {
    /** The value at [t] seconds after the start: 0 before it, 1 after it. */
    fun at(t: Double): Double {
        if (t <= 0.0) return 0.0
        if (t >= seconds) return 1.0
        val p = t * 60.0
        val i = p.toInt().coerceAtMost(xs.size - 2)
        val f = p - i
        return xs[i] + (xs[i + 1] - xs[i]) * f
    }

    companion object {
        fun make(overshoot: Double, omega: Double): Spring {
            val os = maxOf(overshoot, 0.0001)
            val zeta = if (overshoot < 0.005) 1.0 else -Math.log(os) / Math.sqrt(Math.PI * Math.PI + Math.log(os) * Math.log(os))
            val dt = 1.0 / 240.0
            var x = 0.0
            var v = 0.0
            var t = 0.0
            var step = 0
            var seat = -1.0
            val xs = ArrayList<Double>()
            xs.add(0.0)
            while (t < 4.0) {
                val a = -omega * omega * (x - 1.0) - 2.0 * zeta * omega * v
                v += a * dt
                x += v * dt
                t += dt
                step++
                if (step % 4 == 0) xs.add(x)
                if (seat < 0.0 && x >= (if (overshoot < 0.005) 0.985 else 0.999)) seat = t
                if (t > 0.1 && Math.abs(x - 1.0) < 0.0008 && Math.abs(v) < 0.02) break
            }
            xs.add(1.0)
            return Spring(xs.toDoubleArray(), t, if (seat < 0.0) t else seat)
        }
    }
}

/** A cubic bezier easing as in CSS, `cubic-bezier(x1, y1, x2, y2)`. */
class Bezier(private val x1: Double, private val y1: Double, private val x2: Double, private val y2: Double) {
    private fun x(t: Double) = 3 * (1 - t) * (1 - t) * t * x1 + 3 * (1 - t) * t * t * x2 + t * t * t
    private fun y(t: Double) = 3 * (1 - t) * (1 - t) * t * y1 + 3 * (1 - t) * t * t * y2 + t * t * t

    fun at(p: Double): Double {
        if (p <= 0.0) return 0.0
        if (p >= 1.0) return 1.0
        var lo = 0.0
        var hi = 1.0
        for (i in 0 until 40) {
            val mid = (lo + hi) / 2
            if (x(mid) < p) lo = mid else hi = mid
        }
        return y((lo + hi) / 2)
    }
}

/** A piece of the logo on the 512 canvas of the bench page. */
class LogoPiece(val name: String, val x: Int, val y: Int, val w: Int, val h: Int)

/** How a piece is drawn at one moment: moved down by [dy] canvas units, scaled about its centre, and [alpha] opaque. */
class PieceState(val dy: Double, val sx: Double, val sy: Double, val alpha: Double)

/** The whole mark at one moment: the four pieces, the inner shift of each bar (a share of its own height), and the jolt of the tile. */
class SnapFrame(
    val top: PieceState,
    val b: PieceState,
    val bottom: PieceState,
    val word: PieceState,
    val topInner: Double,
    val bottomInner: Double,
    val bInnerSx: Double,
    val bInnerSy: Double,
    /** The tile's shift down, as a share of its size, and its scale. */
    val tileDy: Double,
    val tileScale: Double,
)

/** A sound of the snap and when it plays. */
class SnapCue(val atSeconds: Double, val sound: Sound, val haptic: Haptic?)

/**
 * The approved "clamp" of the bench page (openbmt-snap): the mark appears first, then the bars close one per
 * beat, the second one landing on the click, and the name rises in on the last beat. Beats are 0.11 s apart,
 * the overshoot is 0.12 and the impact 0.55.
 */
object SnapTiming {
    const val CANVAS = 512.0
    const val GAP = 0.11
    const val OVERSHOOT = 0.12
    const val IMPACT = 0.55
    const val SPEED = 1.0

    val top = LogoPiece("top", 76, 58, 360, 40)
    val b = LogoPiece("b", 185, 110, 135, 199)
    val bottom = LogoPiece("bottom", 76, 312, 360, 40)
    val word = LogoPiece("word", 96, 384, 323, 74)
    val pieces = listOf(top, b, bottom, word)

    private val omega = 2.0 * Math.PI * 2.6 * SPEED
    private val bSpring = Spring.make(0.06, omega * 0.9)
    private val barSpring = Spring.make(OVERSHOOT, omega * 1.2)

    /** The ₿ is done, then one beat and a half before the first bar starts. */
    val topStart = bSpring.seat + GAP * 1.5

    /** The second bar starts one beat after the first. */
    val bottomStart = topStart + GAP

    val topLands = topStart + barSpring.seat
    val impact = bottomStart + barSpring.seat

    /** The name rises in after a held breath. */
    val wordStart = impact + maxOf(0.16, GAP * 1.8)
    private const val WORD_SECONDS = 0.42

    /** The whole motion, in seconds. */
    val total = maxOf(wordStart + WORD_SECONDS, impact + 0.3, bottomStart + barSpring.seconds)

    /** The sounds: a tick as each bar lands, the click as the second lands, the finish with the name. */
    val cues: List<SnapCue> = listOf(
        SnapCue(topLands, Sound.Tick, null),
        SnapCue(impact, Sound.Tick, null),
        SnapCue(impact, Sound.Drop, Haptic.Confirm),
        SnapCue(wordStart, Sound.Maximize, null),
    )

    private val bump = Bezier(0.2, 0.7, 0.3, 1.0)
    private val joltEase = Bezier(0.0, 0.0, 0.58, 1.0)
    private val wordEase = Bezier(0.2, 0.8, 0.2, 1.0)

    /** A value that goes 0, [peak] at [at], 0 over [seconds], eased as a whole. */
    private fun kick(t: Double, start: Double, seconds: Double, at: Double, peak: Double): Double {
        val p = (t - start) / seconds
        if (p <= 0.0 || p >= 1.0) return 0.0
        val e = bump.at(p)
        return if (e < at) peak * e / at else peak * (1.0 - e) / (1.0 - at)
    }

    /** The mark [t] seconds after play() started. At [total] and later it is the still logo. */
    fun frame(t: Double): SnapFrame {
        val bx = bSpring.at(t)
        val bState = PieceState(0.0, 0.6 + 0.4 * bx, 0.6 + 0.4 * bx, minOf(1.0, bx * 1.6))
        val tx = barSpring.at(t - topStart)
        val topState = PieceState(-(1.0 - tx) * 2.8 * top.h, 1.0, 1.0, 1.0)
        val lx = barSpring.at(t - bottomStart)
        val bottomState = PieceState((1.0 - lx) * 5.2 * bottom.h, 1.0, 1.0, 1.0)
        val wp = wordEase.at((t - wordStart) / WORD_SECONDS)
        val wordState = if (t < wordStart) PieceState(0.3 * word.h, 1.0, 1.0, 0.0) else PieceState((1.0 - wp) * 0.3 * word.h, 1.0, 1.0, wp)
        // The lock: the bars pinch, the ₿ squeezes, the tile jolts.
        val pinch = 0.36 * IMPACT
        val topInner = -kick(t, topLands, 0.14, 0.3, 0.10) + kick(t, impact, 0.2, 0.3, pinch)
        val bottomInner = kick(t, impact, 0.14, 0.3, 0.10) - kick(t, impact, 0.2, 0.3, pinch)
        val squeeze = kick(t, impact, 0.24, 0.25, 1.0)
        val sh = 7.0 * IMPACT / 200.0
        val jp = (t - impact) / 0.3
        var tileDy = 0.0
        var tileScale = 1.0
        if (jp > 0.0 && jp < 1.0) {
            val e = joltEase.at(jp)
            // Keyframes: 0 at 0, (sh, 0.982) at .18, (-0.3 sh) at .5, 0 at 1.
            if (e < 0.18) {
                val f = e / 0.18
                tileDy = sh * f
                tileScale = 1.0 - 0.018 * IMPACT * f
            } else if (e < 0.5) {
                val f = (e - 0.18) / 0.32
                tileDy = sh + (-0.3 * sh - sh) * f
                tileScale = 1.0 - 0.018 * IMPACT * (1 - f)
            } else {
                tileDy = -0.3 * sh * (1 - (e - 0.5) / 0.5)
            }
        }
        return SnapFrame(
            topState, bState, bottomState, wordState, topInner, bottomInner,
            1.0 + 0.08 * IMPACT * squeeze, 1.0 - 0.12 * IMPACT * squeeze, tileDy, tileScale,
        )
    }

    /** The still logo: every piece in place. */
    fun end(): SnapFrame = frame(total + 1.0)
}
