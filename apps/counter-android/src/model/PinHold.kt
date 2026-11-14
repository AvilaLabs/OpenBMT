package org.openbmt.counter.app

/**
 * The phone's two counters that a hold of wrong PIN tries is measured by: the milliseconds since the phone started
 * (`SystemClock.elapsedRealtime`, which moving the date does not change) and which start of the phone it is
 * (`Settings.Global.BOOT_COUNT`).
 */
class PinNow(val elapsedMs: Long, val boot: Long)

/** The seconds a hold has left ([remainingSecs], 0 when it is over) and whether the hold begins again now ([restart]). */
class HoldCheck(val remainingSecs: Long, val restart: Boolean)

/**
 * How long a hold of wrong PIN tries lasts (CR-85), told by the time since the phone started and never by the date.
 * The start of a hold is the `elapsedMs` of the wrong try and its `boot`. The hold is over only when that much time
 * has passed on the same start of the phone. After the phone has started again (the boot count differs, or the time
 * since start is less than at the start of the hold) the time of the old hold cannot be told, so the hold begins again
 * at full length at the first check after the start; the caller stores [PinHold.stamp] for it.
 */
object PinHold {
    /** The start that means no hold. */
    const val NONE = 0L

    /** The hold of [lengthSecs] seconds that began at [startMs] (the phone's [startBoot]th start), looked at [now]. */
    fun check(startMs: Long, startBoot: Long, now: PinNow, lengthSecs: Long): HoldCheck =
        check(startMs, startBoot, now.elapsedMs, now.boot, lengthSecs)

    fun check(startMs: Long, startBoot: Long, nowMs: Long, nowBoot: Long, lengthSecs: Long): HoldCheck {
        if (startMs == NONE || lengthSecs <= 0) return HoldCheck(0, false)
        if (startBoot != nowBoot || nowMs < startMs) return HoldCheck(lengthSecs, true)
        val leftMs = startMs + lengthSecs * 1000 - nowMs
        return HoldCheck(if (leftMs <= 0) 0 else (leftMs + 999) / 1000, false)
    }
}
