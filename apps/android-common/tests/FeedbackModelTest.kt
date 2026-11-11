package org.openbmt.app

/**
 * The pure parts of the sounds, haptics and motion (src/model/FeedbackModel.kt): the sound levels, what an
 * outcome plays, the shake, the hold to approve and the snap of the logo. They run on the desktop JVM.
 */
object FeedbackModelTest {
    private var count = 0

    private fun ok(cond: Boolean, what: String) {
        count++
        if (!cond) throw AssertionError("FAILED: $what")
    }

    private fun near(a: Double, b: Double, eps: Double = 1e-6) = Math.abs(a - b) < eps

    private fun soundTests() {
        ok(SoundLevels.MASTER == 0.8f, "the master volume")
        ok(Sound.Confirmation.level == 0.75f && Sound.Error.level == 0.75f, "confirmation and error at 0.75")
        ok(Sound.Tick.level == 0.3f && Sound.Drop.level == 0.8f && Sound.Maximize.level == 0.7f, "tick, drop and maximize levels")
        ok(Math.abs(Sound.Tick.volume - 0.24f) < 1e-6f && Math.abs(Sound.Drop.volume - 0.64f) < 1e-6f, "the master volume multiplies")
        ok(Sound.values().map { it.file } == listOf("confirmation_002", "error_004", "tick_002", "drop_001", "maximize_008"), "the five files")
        ok(Sound.values().all { it.volume <= 1f && it.volume > 0f }, "every volume is in range")
    }

    private fun outcomeTests() {
        val good = Outcomes.of(Tones.GOOD)
        ok(good != null && good.sound == Sound.Confirmation && good.haptic == Haptic.Confirm, "good confirms")
        val stop = Outcomes.of(Tones.STOP)
        ok(stop != null && stop.sound == Sound.Error && stop.haptic == Haptic.Reject, "stop rejects")
        ok(Outcomes.of(Tones.INFO) == null, "info does nothing")
        ok(Outcomes.of(Tones.ENDED) == null, "ended does nothing")
        ok(Outcomes.of(9) == null, "an unknown tone does nothing")
    }

    private fun shakeTests() {
        ok(Shake.offsetDp(0.0) == 0f && Shake.offsetDp(1.0) == 0f, "the shake starts and ends at rest")
        val samples = (0..300).map { Shake.offsetDp(it / 300.0) }
        ok(samples.max() <= 6.0001f && samples.min() >= -6.0001f, "never more than 6 dp")
        ok(samples.max() > 5.99f && samples.min() < -5.99f, "reaches 6 dp each way")
        ok(Shake.offsetDp(1.0 / 12.0) < 0f, "to the left first")
        var crossings = 0
        for (i in 1 until samples.size) if (samples[i - 1] < 0f && samples[i] >= 0f || samples[i - 1] > 0f && samples[i] <= 0f) crossings++
        ok(crossings >= 5, "three swings each way")
        ok(Shake.DURATION_MS == 300 && Shake.CYCLES == 3, "300 ms, three times")
    }

    private fun holdTests() {
        ok((1..4).map { HoldTiming.quarterAtMs(it) } == listOf(300L, 600L, 900L, 1200L), "quarters at 0.3, 0.6, 0.9 and 1.2 s")
        val c = HoldClock()
        c.press(1000)
        ok(c.advance(1299).isEmpty() && c.progress < 0.25, "no quarter before 0.3 s")
        ok(c.advance(1300) == listOf(1), "the first quarter at 0.3 s")
        ok(c.advance(1600) == listOf(2) && c.advance(1900) == listOf(3), "the second at 0.6 s, the third at 0.9 s")
        ok(!c.done, "not done at 0.9 s")
        ok(c.advance(2200) == listOf(4) && c.done && near(c.progress, 1.0), "done at 1.2 s with the fourth quarter")
        ok(c.advance(3000).isEmpty(), "nothing more after it is done")

        // Letting go early rewinds over 250 ms, and holding again carries on from where it is.
        val d = HoldClock()
        d.press(0)
        d.advance(600)
        ok(near(d.progress, 0.5), "half way at 0.6 s")
        d.release(600)
        d.advance(725)
        ok(near(d.progress, 0.25), "half of the rewind time leaves half of it")
        d.advance(850)
        ok(near(d.progress, 0.0), "the rewind takes 250 ms")
        d.advance(2000)
        ok(near(d.progress, 0.0) && !d.holding, "stays at 0 once rewound")

        val e = HoldClock()
        e.press(0)
        e.release(600)
        ok(near(e.progress, 0.5), "release gives the progress of the time held")
        e.press(725)
        ok(near(e.progress, 0.25), "pressing again part way through the rewind starts from there")
        e.advance(725 + 300)
        ok(near(e.progress, 0.5), "and fills at the same pace")
        e.release(1025)
        e.press(1100)
        e.advance(1100 + 450)
        ok(e.progress > 0.5 && e.progress < 0.8, "resumes from part way")

        // A quarter that was rewound over is reached again.
        val f = HoldClock()
        f.press(0)
        ok(f.advance(350) == listOf(1), "quarter 1")
        f.release(350)
        f.advance(700)
        ok(f.progress == 0.0, "rewound")
        f.press(700)
        ok(f.advance(1000) == listOf(1), "quarter 1 again after the rewind")

        val g = HoldClock()
        g.complete()
        ok(g.done && g.progress == 1.0, "the accessibility approval completes at once")
        ok(HoldTiming.HOLD_MS == 1200L && HoldTiming.REWIND_MS == 250L && HoldTiming.HAPTIC_GAP_MS == 90L, "the hold's numbers")
        ok(HoldTiming.PRESSED_SCALE == 0.94f && HoldTiming.RING_DP == 4f && HoldTiming.RING_HELD_DP == 6f, "the hold's scale and ring")
        ok(HoldTiming.TICK_MS == 220L && HoldTiming.RIPPLE_SCALE == 1.6f, "the tick and the ripple")
    }

    private fun springTests() {
        val s = Spring.make(0.12, 2.0 * Math.PI * 2.6 * 1.2)
        ok(s.at(-1.0) == 0.0 && s.at(100.0) == 1.0, "a spring is 0 before and 1 after")
        val peak = (0..400).maxOf { s.at(it / 400.0 * s.seconds) }
        ok(peak > 1.05 && peak < 1.2, "the overshoot is about 0.12 (is $peak)")
        ok(s.seat > 0.0 && s.seat < s.seconds, "it seats before it settles")
        ok(Bezier(0.2, 0.7, 0.3, 1.0).at(0.0) == 0.0 && Bezier(0.2, 0.7, 0.3, 1.0).at(1.0) == 1.0, "a bezier ends where it should")
        ok(Bezier(0.2, 0.7, 0.3, 1.0).at(0.5) > 0.5, "a fast-out bezier is ahead at half")
    }

    private fun snapTests() {
        ok(SnapTiming.GAP == 0.11 && SnapTiming.OVERSHOOT == 0.12 && SnapTiming.IMPACT == 0.55, "the approved settings")
        ok(SnapTiming.topStart > 0.0 && near(SnapTiming.bottomStart - SnapTiming.topStart, 0.11), "the bars start one beat apart")
        ok(SnapTiming.topLands < SnapTiming.impact && SnapTiming.impact < SnapTiming.wordStart, "the bars land, then the name")
        val cues = SnapTiming.cues
        ok(cues.map { it.sound } == listOf(Sound.Tick, Sound.Tick, Sound.Drop, Sound.Maximize), "a tick per bar, the click, the finish")
        ok(cues.map { it.atSeconds } == cues.map { it.atSeconds }.sorted(), "the cues are in time order")
        ok(cues.count { it.haptic == Haptic.Confirm } == 1 && cues[2].haptic == Haptic.Confirm, "one confirm haptic, on the click")
        ok(SnapTiming.total > SnapTiming.wordStart, "the motion lasts until the name is in")
        val first = SnapTiming.frame(0.0)
        ok(first.b.alpha < 0.01 && first.top.dy < 0.0 && first.bottom.dy > 0.0 && first.word.alpha == 0.0, "at the start only the ₿ is about to appear, the bars are out")
        val early = SnapTiming.frame(SnapTiming.topStart * 0.9)
        ok(early.b.alpha > 0.9, "the ₿ appears first")
        val end = SnapTiming.end()
        ok(end.top.dy == 0.0 && end.bottom.dy == 0.0 && end.b.sx == 1.0 && end.word.alpha == 1.0 && end.word.dy == 0.0, "the end is the still logo")
        ok(end.tileDy == 0.0 && end.tileScale == 1.0 && end.topInner == 0.0 && end.bottomInner == 0.0, "no jolt at the end")
        val mid = SnapTiming.frame(SnapTiming.impact + 0.05)
        ok(mid.topInner > 0.0 && mid.bottomInner < 0.0, "the bars pinch on the click")
        ok(SnapTiming.pieces.map { it.name } == listOf("top", "b", "bottom", "word"), "the four pieces")
    }

    @JvmStatic
    fun main(args: Array<String>) {
        soundTests()
        outcomeTests()
        shakeTests()
        holdTests()
        springTests()
        snapTests()
        println("FeedbackModelTest: $count checks passed")
    }
}
