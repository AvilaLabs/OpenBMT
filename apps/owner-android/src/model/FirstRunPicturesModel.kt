package org.openbmt.owner.app

/**
 * The timing of the short pictures at the top of the first-run screens (FirstRunPictures.kt draws them). A picture
 * is a function of one number, its position in the loop from 0 to 1; every move in it is a stretch of that range
 * ([seg]) and is finished by [DONE_BY], so the last frame, which is also the still picture, shows the finished
 * scene. No Android class is used.
 */
object PictureTiming {
    /** The pictures and the length of one loop of each, between two and three seconds. */
    enum class Picture(val loopMs: Int) {
        Welcome(3000),
        Practice(3000),
        Till(2500),
        Money(3000),
        Why(2500),
        Usb(2500),
        Offline(2500),
        Scan(3000),
        Same(2500),
        Shutdown(3000),
    }

    /** Every move is over by this position; the rest of the loop holds the finished scene. */
    const val DONE_BY = 0.92f

    /** The position of [picture] [elapsedMs] after it started; with motion off it is always the last frame. */
    fun position(picture: Picture, elapsedMs: Long, motion: Boolean): Float {
        if (!motion) return STILL
        val loop = picture.loopMs.toLong()
        val into = ((elapsedMs % loop) + loop) % loop
        return into.toFloat() / loop
    }

    /** The last frame, the one shown when animations are off. */
    const val STILL = 1f

    /** How far through the stretch [from]..[to] the position [t] is, from 0 to 1. */
    fun seg(t: Float, from: Float, to: Float): Float = if (to <= from) (if (t >= to) 1f else 0f) else ((t - from) / (to - from)).coerceIn(0f, 1f)

    /** Slow at both ends: 0 at 0 and 1 at 1. */
    fun ease(x: Float): Float {
        val c = x.coerceIn(0f, 1f)
        return c * c * (3f - 2f * c)
    }

    /** Rises from 0 to 1 and back to 0 across the stretch: a press, a pulse. */
    fun bump(t: Float, from: Float, to: Float): Float {
        val x = seg(t, from, to)
        if (x <= 0f || x >= 1f) return 0f
        return Math.sin(Math.PI * x).toFloat()
    }
}
