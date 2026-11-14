package org.openbmt.app

import android.animation.Animator
import android.animation.AnimatorListenerAdapter
import android.animation.ValueAnimator
import android.content.Context
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.Canvas
import android.graphics.Paint
import android.graphics.Path
import android.graphics.RadialGradient
import android.graphics.RectF
import android.graphics.Shader
import android.media.AudioAttributes
import android.media.AudioManager
import android.media.SoundPool
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.provider.Settings
import android.view.Gravity
import android.view.HapticFeedbackConstants
import android.view.MotionEvent
import android.view.View
import android.view.accessibility.AccessibilityNodeInfo
import android.view.animation.LinearInterpolator
import android.widget.LinearLayout

/**
 * Sounds, haptics and motion, made once per app. The sounds are the five files of res/raw, played through one
 * `SoundPool` as sonification (the levels are in [Sound]); they do nothing when the ringer is silent or on
 * vibrate, or when [soundOn] is false. Haptics go through `View.performHapticFeedback` only, so no permission
 * is needed. When the system's animator duration scale is 0 every animation here jumps to its end state at once.
 */
object Feedback {
    /** The app's own sound switch; the counter app wires its setting to it. */
    @Volatile
    var soundOn = true

    private var app: Context? = null
    private var pool: SoundPool? = null
    private val ids = HashMap<Sound, Int>()
    private val loaded = HashSet<Int>()

    /** Makes the pool and loads the files; safe to call again. Called by the first sound, or at the app's start. */
    @Synchronized
    fun init(context: Context) {
        if (pool != null) return
        val ctx = context.applicationContext ?: context
        app = ctx
        val attrs = AudioAttributes.Builder()
            .setUsage(AudioAttributes.USAGE_ASSISTANCE_SONIFICATION)
            .setContentType(AudioAttributes.CONTENT_TYPE_SONIFICATION)
            .build()
        val p = SoundPool.Builder().setMaxStreams(4).setAudioAttributes(attrs).build()
        p.setOnLoadCompleteListener { _, id, status ->
            if (status == 0) synchronized(this) { loaded.add(id) }
        }
        for (s in Sound.values()) {
            val res = ctx.resources.getIdentifier(s.file, "raw", ctx.packageName)
            if (res == 0) continue
            try {
                ctx.resources.openRawResourceFd(res).use { ids[s] = p.load(it, 1) }
            } catch (_: java.io.IOException) {
            }
        }
        pool = p
    }

    /** True unless the system's animator duration scale is 0 (the "reduce motion" state). */
    fun motionOn(context: Context): Boolean =
        Settings.Global.getFloat(context.contentResolver, Settings.Global.ANIMATOR_DURATION_SCALE, 1f) != 0f

    /** Plays [sound] at its level times the master volume, at [rate] (1 is normal). Nothing when silent, on vibrate or off. */
    fun play(context: Context, sound: Sound, rate: Float = 1f) {
        if (!soundOn) return
        init(context)
        val ctx = app ?: return
        val audio = ctx.getSystemService(Context.AUDIO_SERVICE) as? AudioManager ?: return
        if (audio.ringerMode != AudioManager.RINGER_MODE_NORMAL) return
        val id = ids[sound] ?: return
        val ready = synchronized(this) { loaded.contains(id) }
        if (!ready) return
        val v = sound.volume
        pool?.play(id, v, v, 1, 0, rate)
    }

    /** The haptic [h] on [view]: confirm and reject from API 30, a long press before. */
    fun haptic(view: View, h: Haptic) {
        val modern = Build.VERSION.SDK_INT >= 30
        val constant = when (h) {
            Haptic.Confirm -> if (modern) HapticFeedbackConstants.CONFIRM else HapticFeedbackConstants.LONG_PRESS
            Haptic.Reject -> if (modern) HapticFeedbackConstants.REJECT else HapticFeedbackConstants.LONG_PRESS
            Haptic.Tick -> HapticFeedbackConstants.KEYBOARD_TAP
            Haptic.LongPress -> HapticFeedbackConstants.LONG_PRESS
        }
        view.performHapticFeedback(constant)
    }

    /**
     * A sale's outcome: good plays the confirmation with a confirm haptic, stop plays the error with a reject
     * haptic, every other tone ([Tones]) nothing. Called only when a sale's screen first shows green or red.
     */
    fun outcome(view: View, tone: Int) {
        val o = Outcomes.of(tone) ?: return
        play(view.context, o.sound)
        haptic(view, o.haptic)
    }

    /** The same, for a colour of the words. */
    fun outcome(view: View, colour: Colour) = outcome(
        view,
        when (colour) {
            Colour.Green -> Tones.GOOD
            Colour.Red -> Tones.STOP
            Colour.Blue -> Tones.INFO
            Colour.Neutral -> Tones.ENDED
        },
    )

    /** A new part of an animated code was read. */
    fun scanPart(view: View) {
        play(view.context, Sound.Tick)
        haptic(view, Haptic.Tick)
    }

    /** A code was read to the end: the drop and a confirm haptic. */
    fun scanDone(view: View) {
        play(view.context, Sound.Drop)
        haptic(view, Haptic.Confirm)
    }

    /** A refused PIN: the view moves 6 dp left and right three times over 300 ms, with a reject haptic and no sound. */
    fun shake(view: View) {
        haptic(view, Haptic.Reject)
        if (!motionOn(view.context)) return
        val dp = view.resources.displayMetrics.density
        ValueAnimator.ofFloat(0f, 1f).apply {
            duration = Shake.DURATION_MS.toLong()
            interpolator = LinearInterpolator()
            addUpdateListener { view.translationX = Shake.offsetDp(it.animatedFraction.toDouble()) * dp }
            addListener(
                object : AnimatorListenerAdapter() {
                    override fun onAnimationEnd(animation: Animator) {
                        view.translationX = 0f
                    }
                },
            )
            start()
        }
    }

    /** The background of [view] goes from [fromColour] to [toColour] over 200 ms. What the screen says never waits for it. */
    fun crossfade(view: View, fromColour: Int, toColour: Int) {
        view.setBackgroundColor(fromColour)
        if (!motionOn(view.context)) {
            view.setBackgroundColor(toColour)
            return
        }
        ValueAnimator.ofArgb(fromColour, toColour).apply {
            duration = 200
            addUpdateListener { view.setBackgroundColor(it.animatedValue as Int) }
            start()
        }
    }
}

/**
 * Approving in the Key app: a 96 dp circle with the label under it and "Hold until the circle fills." under that.
 * Holding for 1.2 s fills a ring round the circle (the timing is [HoldClock]); letting go early rewinds it over
 * 250 ms and holding again carries on from where it is. At the end the circle springs back, fills with the good
 * band, a tick draws itself in, a ripple ring fades out, a confirm and then a long-press haptic fire, and
 * [onApproved] runs. A double tap, or the accessibility action "Approve", completes it at once. No sound.
 */
class HoldButton(context: Context, private val label: String, private val onApproved: () -> Unit) : LinearLayout(context) {
    private val ui = Ui(context)
    private val disc = Disc(context)
    private val main = Handler(Looper.getMainLooper())
    private val clock = HoldClock()
    private var finished = false
    private var finishStart = 0L
    private var heldFactor = 0f
    private var lastFrame = 0L
    private var framing = false

    init {
        orientation = VERTICAL
        gravity = Gravity.CENTER_HORIZONTAL
        clipChildren = false
        clipToPadding = false
        contentDescription = label
        isFocusable = true
        isClickable = true
        filterTouchesWhenObscured = true
        val size = ui.px(HoldTiming.CIRCLE_DP + 2 * PAD)
        addView(disc, LayoutParams(size, size))
        addView(
            ui.text(label, 17f, bold = true).also {
                it.gravity = Gravity.CENTER
                it.importantForAccessibility = IMPORTANT_FOR_ACCESSIBILITY_NO
            },
            LayoutParams(LayoutParams.WRAP_CONTENT, LayoutParams.WRAP_CONTENT).also { it.topMargin = -ui.px(PAD - 8) },
        )
        addView(
            ui.text("Hold until the circle fills.", 14f, colour = ui.pal.secondary).also {
                it.gravity = Gravity.CENTER
                it.importantForAccessibility = IMPORTANT_FOR_ACCESSIBILITY_NO
            },
            LayoutParams(LayoutParams.WRAP_CONTENT, LayoutParams.WRAP_CONTENT).also { it.topMargin = ui.px(6) },
        )
        super.setOnClickListener { complete() }
    }

    private fun now(): Long = SystemClock.uptimeMillis()

    /**
     * A touch that reached the hold through a window drawn over it, over all of it or over only a part, is not the
     * person's. `filterTouchesWhenObscured` does not catch an overlay that cannot be touched and covers only part of the
     * screen, so both flags are looked at here. A hold that has begun lets go at once.
     */
    override fun onFilterTouchEventForSecurity(event: MotionEvent): Boolean {
        val covered = MotionEvent.FLAG_WINDOW_IS_OBSCURED or MotionEvent.FLAG_WINDOW_IS_PARTIALLY_OBSCURED
        if (event.flags and covered != 0) {
            if (clock.holding) {
                clock.release(now())
                startFrames()
            }
            return false
        }
        return super.onFilterTouchEventForSecurity(event)
    }

    private fun inCircle(e: MotionEvent): Boolean {
        val cx = disc.left + disc.width / 2f
        val cy = disc.top + disc.height / 2f
        val r = ui.px(HoldTiming.CIRCLE_DP / 2 + 8).toFloat()
        val dx = e.x - cx
        val dy = e.y - cy
        return dx * dx + dy * dy <= r * r
    }

    override fun onTouchEvent(e: MotionEvent): Boolean {
        if (finished) return true
        when (e.actionMasked) {
            MotionEvent.ACTION_DOWN -> {
                if (!inCircle(e)) return false
                parent?.requestDisallowInterceptTouchEvent(true)
                clock.press(now())
                startFrames()
                return true
            }
            MotionEvent.ACTION_UP, MotionEvent.ACTION_CANCEL -> {
                clock.release(now())
                // With animations off the ring does not rewind in view: it is empty at once.
                if (!Feedback.motionOn(context)) clock.advance(now() + HoldTiming.REWIND_MS)
                startFrames()
                return true
            }
        }
        return true
    }

    private fun startFrames() {
        if (framing) return
        framing = true
        lastFrame = now()
        postOnAnimation(frame)
    }

    private val frame = object : Runnable {
        override fun run() {
            val t = now()
            val dt = (t - lastFrame).coerceAtLeast(0)
            lastFrame = t
            val target = if (clock.holding) 1f else 0f
            heldFactor = if (!Feedback.motionOn(context)) target else {
                val step = dt / 120f
                if (target > heldFactor) minOf(target, heldFactor + step) else maxOf(target, heldFactor - step)
            }
            for (q in clock.advance(t)) {
                if (q < HoldTiming.QUARTERS) Feedback.haptic(this@HoldButton, Haptic.Tick)
            }
            if (clock.done && !finished) complete()
            disc.invalidate()
            val resting = !clock.holding && clock.progress == 0.0 && heldFactor == 0f
            if (finished) {
                if (t - finishStart < FINISH_MS) postOnAnimation(this) else framing = false
            } else if (resting) {
                framing = false
            } else {
                postOnAnimation(this)
            }
        }
    }

    /** The end of the hold, or the double tap: the haptics, the flourish, then [onApproved]. */
    private fun complete() {
        if (finished) return
        finished = true
        clock.complete()
        val moving = Feedback.motionOn(context)
        finishStart = if (moving) now() else now() - FINISH_MS
        Feedback.haptic(this, Haptic.Confirm)
        main.postDelayed({ Feedback.haptic(this, Haptic.LongPress) }, HoldTiming.HAPTIC_GAP_MS)
        main.postDelayed({ onApproved() }, if (moving) HoldTiming.APPROVED_AT_MS else HoldTiming.HAPTIC_GAP_MS)
        if (!framing) {
            framing = true
            lastFrame = now()
            postOnAnimation(frame)
        }
        disc.invalidate()
    }

    override fun onInitializeAccessibilityNodeInfo(info: AccessibilityNodeInfo) {
        super.onInitializeAccessibilityNodeInfo(info)
        info.addAction(AccessibilityNodeInfo.AccessibilityAction(ACTION_APPROVE, "Approve"))
    }

    override fun performAccessibilityAction(action: Int, arguments: Bundle?): Boolean {
        if (action == ACTION_APPROVE) {
            complete()
            return true
        }
        return super.performAccessibilityAction(action, arguments)
    }

    override fun onDetachedFromWindow() {
        main.removeCallbacksAndMessages(null)
        framing = false
        super.onDetachedFromWindow()
    }

    /** The circle, the ring, the glow and the flourish. */
    private inner class Disc(context: Context) : View(context) {
        private val paint = Paint(Paint.ANTI_ALIAS_FLAG)
        private val tick = IconDrawable(Icon.Tick, Palette.WHITE, 2.6f)
        private val spring = Spring.make(HoldTiming.OVERSHOOT, 2.0 * Math.PI * 3.0)

        init {
            importantForAccessibility = IMPORTANT_FOR_ACCESSIBILITY_NO
            clipToOutline = false
        }

        override fun onDraw(canvas: Canvas) {
            val dp = resources.displayMetrics.density
            val pal = ui.pal
            val cx = width / 2f
            val cy = height / 2f
            val sinceFinish = if (finished) (now() - finishStart) / 1000.0 else -1.0
            val baseR = HoldTiming.CIRCLE_DP / 2f * dp
            val scale = if (finished) {
                HoldTiming.PRESSED_SCALE + (1f - HoldTiming.PRESSED_SCALE) * spring.at(sinceFinish).toFloat()
            } else {
                1f - (1f - HoldTiming.PRESSED_SCALE) * heldFactor
            }
            val r = baseR * scale
            val progress = if (finished) 1f else clock.progress.toFloat()
            // The glow behind: soft, in the good colour, builds with the ring.
            val glowAlpha = (0.45f * progress).coerceIn(0f, 1f)
            if (glowAlpha > 0f) {
                val gr = r + 22f * dp
                val c = pal.good.band
                paint.style = Paint.Style.FILL
                paint.shader = RadialGradient(
                    cx, cy, gr,
                    intArrayOf(withAlpha(c, glowAlpha), withAlpha(c, glowAlpha * 0.5f), withAlpha(c, 0f)),
                    floatArrayOf(r / gr * 0.9f, r / gr, 1f), Shader.TileMode.CLAMP,
                )
                canvas.drawCircle(cx, cy, gr, paint)
                paint.shader = null
            }
            paint.style = Paint.Style.FILL
            paint.color = pal.good.tint
            canvas.drawCircle(cx, cy, r, paint)
            // The ring: its track, then the part that is filled.
            val ringW = (HoldTiming.RING_DP + (HoldTiming.RING_HELD_DP - HoldTiming.RING_DP) * heldFactor) * dp
            val ringR = r + 6f * dp
            paint.style = Paint.Style.STROKE
            paint.strokeWidth = ringW
            paint.color = pal.fieldLine
            canvas.drawCircle(cx, cy, ringR, paint)
            if (progress > 0f) {
                paint.color = pal.good.band
                paint.strokeCap = Paint.Cap.ROUND
                canvas.drawArc(RectF(cx - ringR, cy - ringR, cx + ringR, cy + ringR), -90f, 360f * progress, false, paint)
                paint.strokeCap = Paint.Cap.BUTT
            }
            if (finished) {
                val fill = (sinceFinish * 1000.0 / HoldTiming.FILL_MS).coerceIn(0.0, 1.0).toFloat()
                paint.style = Paint.Style.FILL
                paint.color = pal.good.band
                canvas.drawCircle(cx, cy, r * fill, paint)
                val tickP = ((sinceFinish * 1000.0 - HoldTiming.TICK_AT_MS) / HoldTiming.TICK_MS).coerceIn(0.0, 1.0).toFloat()
                if (tickP > 0f) {
                    tick.drawn = tickP
                    val half = (r * 0.5f).toInt()
                    tick.setBounds((cx - half).toInt(), (cy - half).toInt(), (cx + half).toInt(), (cy + half).toInt())
                    tick.draw(canvas)
                }
                val rip = (sinceFinish * 1000.0 / HoldTiming.RIPPLE_MS).coerceIn(0.0, 1.0).toFloat()
                if (rip < 1f) {
                    paint.style = Paint.Style.STROKE
                    paint.strokeWidth = 3f * dp
                    paint.color = withAlpha(pal.good.band, 1f - rip)
                    canvas.drawCircle(cx, cy, ringR * (1f + (HoldTiming.RIPPLE_SCALE - 1f) * rip), paint)
                }
            }
        }
    }

    companion object {
        private const val PAD = 30
        private const val ACTION_APPROVE = 0x4F42_4D31
        private const val FINISH_MS = 600L

        private fun withAlpha(colour: Int, alpha: Float): Int = (colour and 0x00FFFFFF) or ((alpha.coerceIn(0f, 1f) * 255f).toInt() shl 24)
    }
}

/**
 * The logo that clicks together: the ₿ appears, the bars clamp shut one per beat, and the name rises in (the
 * motion and its timing are [SnapTiming], ported from the bench page). [play] runs it once with its sounds and
 * the confirm haptic on the click. Without a play, and with the animator duration scale at 0, it is the still logo.
 */
class LogoSnapView(context: Context, private val sizeDp: Int = 160) : View(context) {
    private val bitmaps = SnapTiming.pieces.associate { it.name to load(context, "logo_piece_${it.name}") }
    private val paint = Paint(Paint.ANTI_ALIAS_FLAG or Paint.FILTER_BITMAP_FLAG)
    private val clip = Path()
    private var frame: SnapFrame = SnapTiming.end()
    private var animator: ValueAnimator? = null
    private var cued = 0

    init {
        contentDescription = "OpenBMT"
        filterTouchesWhenObscured = true
    }

    private fun load(context: Context, name: String): Bitmap? {
        val id = context.resources.getIdentifier(name, "drawable", context.packageName)
        return if (id == 0) null else BitmapFactory.decodeResource(context.resources, id)
    }

    override fun onMeasure(widthSpec: Int, heightSpec: Int) {
        val px = (sizeDp * resources.displayMetrics.density).toInt()
        setMeasuredDimension(px, px)
    }

    /** Runs the snap once; with the animator duration scale at 0 it is the still logo at once, silently. */
    fun play() {
        animator?.cancel()
        if (!Feedback.motionOn(context)) {
            frame = SnapTiming.end()
            invalidate()
            return
        }
        cued = 0
        frame = SnapTiming.frame(0.0)
        val total = SnapTiming.total
        animator = ValueAnimator.ofFloat(0f, total.toFloat()).apply {
            duration = (total * 1000).toLong()
            interpolator = LinearInterpolator()
            addUpdateListener {
                val t = (it.animatedValue as Float).toDouble()
                frame = SnapTiming.frame(t)
                while (cued < SnapTiming.cues.size && SnapTiming.cues[cued].atSeconds <= t) {
                    val cue = SnapTiming.cues[cued++]
                    Feedback.play(context, cue.sound)
                    cue.haptic?.let { h -> Feedback.haptic(this@LogoSnapView, h) }
                }
                invalidate()
            }
            addListener(
                object : AnimatorListenerAdapter() {
                    override fun onAnimationEnd(animation: Animator) {
                        frame = SnapTiming.end()
                        invalidate()
                    }
                },
            )
            start()
        }
    }

    override fun onDetachedFromWindow() {
        animator?.cancel()
        super.onDetachedFromWindow()
    }

    override fun onDraw(canvas: Canvas) {
        val size = width.toFloat()
        val u = size / SnapTiming.CANVAS.toFloat()
        val f = frame
        canvas.save()
        canvas.translate(0f, f.tileDy.toFloat() * size)
        canvas.scale(f.tileScale.toFloat(), f.tileScale.toFloat(), size / 2f, size / 2f)
        clip.reset()
        clip.addRoundRect(RectF(0f, 0f, size, size), size * 0.18f, size * 0.18f, Path.Direction.CW)
        paint.style = Paint.Style.FILL
        paint.color = Palette.BRAND
        canvas.drawPath(clip, paint)
        canvas.clipPath(clip)
        for (p in SnapTiming.pieces) {
            val bmp = bitmaps[p.name] ?: continue
            val s = when (p.name) {
                "top" -> f.top
                "b" -> f.b
                "bottom" -> f.bottom
                else -> f.word
            }
            val inner = when (p.name) {
                "top" -> f.topInner * p.h
                "bottom" -> f.bottomInner * p.h
                else -> 0.0
            }
            val isB = p.name == "b"
            canvas.save()
            canvas.translate(0f, ((s.dy + inner) * u).toFloat())
            val cx = (p.x + p.w / 2f) * u
            val cy = (p.y + p.h / 2f) * u
            canvas.scale((s.sx * (if (isB) f.bInnerSx else 1.0)).toFloat(), (s.sy * (if (isB) f.bInnerSy else 1.0)).toFloat(), cx, cy)
            paint.alpha = (s.alpha * 255).toInt().coerceIn(0, 255)
            canvas.drawBitmap(bmp, null, RectF(p.x * u, p.y * u, (p.x + p.w) * u, (p.y + p.h) * u), paint)
            canvas.restore()
        }
        paint.alpha = 255
        canvas.restore()
    }
}
