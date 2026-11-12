package org.openbmt.owner.app

import android.animation.ValueAnimator
import android.content.Context
import android.graphics.Canvas
import android.graphics.DashPathEffect
import android.graphics.Paint
import android.graphics.Path
import android.graphics.PathMeasure
import android.graphics.RectF
import android.view.View
import android.view.animation.LinearInterpolator
import org.openbmt.app.Feedback
import org.openbmt.app.Palette
import org.openbmt.app.Ui
import org.openbmt.owner.app.PictureTiming.Picture
import org.openbmt.owner.app.PictureTiming.bump
import org.openbmt.owner.app.PictureTiming.ease
import org.openbmt.owner.app.PictureTiming.seg

// The short looping pictures of the first run: shapes in the app's palette, drawn with Canvas, no image, no
// library and no text inside a picture. Each is 160 dp tall and loops in two to three seconds (the timing is
// PictureTiming); with the system's animator duration scale at 0 it is its last frame and never redraws.
// A scene is drawn in a 320 by 160 space that is scaled to fit and centred.

private const val W = 320f
private const val H = 160f

/** The view of one picture: a loop run by one animator while it is on screen, or the last frame when motion is off. */
private class PictureView(context: Context, private val ui: Ui, private val picture: Picture) : View(context) {
    private var animator: ValueAnimator? = null
    private var t = PictureTiming.STILL
    private val paint = Paint(Paint.ANTI_ALIAS_FLAG)

    init {
        importantForAccessibility = IMPORTANT_FOR_ACCESSIBILITY_NO
    }

    override fun onMeasure(widthSpec: Int, heightSpec: Int) {
        val h = (H * resources.displayMetrics.density).toInt()
        setMeasuredDimension(getDefaultSize(0, widthSpec), h)
    }

    override fun onAttachedToWindow() {
        super.onAttachedToWindow()
        if (!Feedback.motionOn(context)) {
            t = PictureTiming.STILL
            return
        }
        val a = ValueAnimator.ofFloat(0f, 1f)
        a.duration = picture.loopMs.toLong()
        a.repeatCount = ValueAnimator.INFINITE
        a.interpolator = LinearInterpolator()
        a.addUpdateListener {
            t = PictureTiming.position(picture, (it.animatedFraction * picture.loopMs).toLong(), true)
            invalidate()
        }
        animator = a
        a.start()
    }

    override fun onDetachedFromWindow() {
        animator?.cancel()
        animator = null
        super.onDetachedFromWindow()
    }

    override fun onDraw(canvas: Canvas) {
        val s = minOf(width / W, height / H)
        canvas.save()
        canvas.translate((width - W * s) / 2f, (height - H * s) / 2f)
        canvas.scale(s, s)
        Scene(canvas, paint, ui.pal).draw(picture, t)
        canvas.restore()
    }
}

fun Ui.welcomePicture(): View = PictureView(context, this, Picture.Welcome)
fun Ui.practicePicture(): View = PictureView(context, this, Picture.Practice)
fun Ui.tillPicture(): View = PictureView(context, this, Picture.Till)
fun Ui.moneyPicture(): View = PictureView(context, this, Picture.Money)
fun Ui.whyPicture(): View = PictureView(context, this, Picture.Why)
fun Ui.usbPicture(): View = PictureView(context, this, Picture.Usb)
fun Ui.offlinePicture(): View = PictureView(context, this, Picture.Offline)
fun Ui.scanPicture(): View = PictureView(context, this, Picture.Scan)
fun Ui.samePicture(): View = PictureView(context, this, Picture.Same)
fun Ui.shutdownPicture(): View = PictureView(context, this, Picture.Shutdown)

private fun withAlpha(c: Int, a: Float): Int = (c and 0x00FFFFFF) or ((a.coerceIn(0f, 1f) * 255f).toInt() shl 24)

private fun lerp(a: Float, b: Float, x: Float): Float = a + (b - a) * x

/** The drawing of every scene, in the 320 by 160 space. */
private class Scene(private val c: Canvas, private val p: Paint, private val pal: org.openbmt.app.Scheme) {
    private val rect = RectF()
    private val path = Path()

    fun draw(picture: Picture, t: Float) {
        when (picture) {
            Picture.Welcome -> welcome(t)
            Picture.Practice -> practice(t)
            Picture.Till -> till(t)
            Picture.Money -> money(t)
            Picture.Why -> why(t)
            Picture.Usb -> usb(t)
            Picture.Offline -> offline(t)
            Picture.Scan -> scan(t)
            Picture.Same -> same(t)
            Picture.Shutdown -> shutdown(t)
        }
    }

    // ---- shapes ---------------------------------------------------------------------------------

    private fun fill(color: Int) { p.style = Paint.Style.FILL; p.color = color; p.pathEffect = null; p.strokeCap = Paint.Cap.BUTT }

    private fun line(color: Int, w: Float, round: Boolean = true) {
        p.style = Paint.Style.STROKE
        p.color = color
        p.strokeWidth = w
        p.pathEffect = null
        p.strokeCap = if (round) Paint.Cap.ROUND else Paint.Cap.BUTT
        p.strokeJoin = Paint.Join.ROUND
    }

    private fun rr(l: Float, t: Float, r: Float, b: Float, radius: Float, color: Int) {
        fill(color)
        rect.set(l, t, r, b)
        c.drawRoundRect(rect, radius, radius, p)
    }

    private fun rrOutline(l: Float, t: Float, r: Float, b: Float, radius: Float, color: Int, w: Float = 2f) {
        line(color, w)
        rect.set(l, t, r, b)
        c.drawRoundRect(rect, radius, radius, p)
    }

    /** A card: the surface colour with an outline, so it reads on the page in both palettes. */
    private fun card(l: Float, t: Float, r: Float, b: Float, radius: Float) {
        rr(l, t, r, b, radius, pal.surface)
        rrOutline(l, t, r, b, radius, pal.fieldLine)
    }

    private fun dot(x: Float, y: Float, r: Float, color: Int) {
        fill(color)
        c.drawCircle(x, y, r, p)
    }

    /** The ₿ in strokes, [h] tall, centred at [x], [y]. */
    private fun btc(x: Float, y: Float, h: Float, color: Int) {
        line(color, h * 0.11f)
        c.save()
        c.translate(x, y)
        c.scale(h, h)
        p.strokeWidth = 0.11f
        path.reset()
        path.moveTo(-0.22f, -0.33f); path.lineTo(0.07f, -0.33f)
        path.quadTo(0.28f, -0.33f, 0.28f, -0.16f); path.quadTo(0.28f, 0f, 0.07f, 0f); path.lineTo(-0.22f, 0f)
        path.moveTo(0.07f, 0f)
        path.quadTo(0.34f, 0f, 0.34f, 0.165f); path.quadTo(0.34f, 0.33f, 0.07f, 0.33f); path.lineTo(-0.22f, 0.33f)
        path.moveTo(-0.22f, -0.33f); path.lineTo(-0.22f, 0.33f)
        path.moveTo(-0.08f, -0.46f); path.lineTo(-0.08f, -0.33f)
        path.moveTo(0.1f, -0.46f); path.lineTo(0.1f, -0.33f)
        path.moveTo(-0.08f, 0.33f); path.lineTo(-0.08f, 0.46f)
        path.moveTo(0.1f, 0.33f); path.lineTo(0.1f, 0.46f)
        c.drawPath(path, p)
        c.restore()
    }

    /** A coin seen at [turn] (1 face on, 0 edge on), with the ₿ on it while it faces us. */
    private fun coin(x: Float, y: Float, r: Float, turn: Float = 1f, alpha: Float = 1f) {
        val w = r * kotlin.math.abs(turn).coerceAtLeast(0.12f)
        fill(withAlpha(Palette.BRAND, alpha))
        rect.set(x - w, y - r, x + w, y + r)
        c.drawOval(rect, p)
        if (kotlin.math.abs(turn) > 0.35f) {
            c.save()
            c.translate(x, y)
            c.scale(kotlin.math.abs(turn), 1f)
            btc(0f, 0f, r * 1.15f, withAlpha(Palette.WHITE, alpha))
            c.restore()
        }
    }

    /** A small code: three corner marks and a few cells, [size] across; [shown] is how much of it has appeared. */
    private fun qr(x: Float, y: Float, size: Float, color: Int, shown: Float = 1f) {
        if (shown <= 0f) return
        c.save()
        c.translate(x + size / 2f, y + size / 2f)
        c.scale(shown, shown)
        c.translate(-size / 2f, -size / 2f)
        val u = size / 7f
        fun cell(i: Int, j: Int) { fill(color); c.drawRect(i * u, j * u, (i + 1) * u, (j + 1) * u, p) }
        for (corner in listOf(0 to 0, 4 to 0, 0 to 4)) {
            line(color, u * 0.8f, false)
            c.drawRect(corner.first * u + u * 0.4f, corner.second * u + u * 0.4f, (corner.first + 3) * u - u * 0.4f, (corner.second + 3) * u - u * 0.4f, p)
            cell(corner.first + 1, corner.second + 1)
        }
        for ((i, j) in listOf(3 to 1, 3 to 3, 4 to 4, 6 to 4, 5 to 5, 4 to 6, 6 to 6, 3 to 5, 3 to 6, 5 to 3, 6 to 3)) cell(i, j)
        c.restore()
    }

    /** A padlock, [x],[y] its centre, [w] wide; [closed] is 0 open to 1 shut. */
    private fun lock(x: Float, y: Float, w: Float, closed: Float, color: Int, bodyText: Int) {
        val lift = (1f - closed) * w * 0.28f
        line(color, w * 0.14f)
        val sw = w * 0.56f
        rect.set(x - sw / 2f, y - w * 0.78f - lift, x + sw / 2f, y - w * 0.1f - lift)
        path.reset()
        path.moveTo(x - sw / 2f, y - w * 0.12f - lift * 0.4f)
        path.lineTo(x - sw / 2f, y - w * 0.5f - lift)
        path.arcTo(rect, 180f, 180f)
        if (closed > 0.5f) path.lineTo(x + sw / 2f, y - w * 0.12f)
        c.drawPath(path, p)
        rr(x - w / 2f, y - w * 0.12f, x + w / 2f, y + w * 0.55f, w * 0.12f, color)
        dot(x, y + w * 0.22f, w * 0.08f, bodyText)
    }

    /** The shape of a hand with one finger out, its fingertip at [x],[y]. */
    private fun hand(x: Float, y: Float) {
        rr(x - 5f, y, x + 5f, y + 34f, 5f, pal.secondary)
        rr(x - 5f, y + 22f, x + 26f, y + 58f, 12f, pal.secondary)
        rr(x + 6f, y + 18f, x + 26f, y + 36f, 8f, pal.secondary)
    }

    /** An open laptop, [cx] its centre, [top] the top of the screen, [w] wide; returns the screen's left, top, right, bottom. */
    private fun laptop(cx: Float, top: Float, w: Float, screen: Int = pal.surface): RectF {
        val h = w * 0.62f
        rr(cx - w / 2f, top, cx + w / 2f, top + h, 6f, pal.secondary)
        rr(cx - w / 2f + 4f, top + 4f, cx + w / 2f - 4f, top + h - 4f, 3f, screen)
        rr(cx - w / 2f - 12f, top + h, cx + w / 2f + 12f, top + h + 7f, 3.5f, pal.secondary)
        return RectF(cx - w / 2f + 4f, top + 4f, cx + w / 2f - 4f, top + h - 4f)
    }

    /** A phone upright, its left and top at [x],[y]. */
    private fun phone(x: Float, y: Float, w: Float, h: Float): RectF {
        rr(x, y, x + w, y + h, 8f, pal.secondary)
        rr(x + 3f, y + 6f, x + w - 3f, y + h - 6f, 3f, pal.surface)
        return RectF(x + 3f, y + 6f, x + w - 3f, y + h - 6f)
    }

    private fun check(x: Float, y: Float, size: Float, color: Int, w: Float, drawn: Float) {
        if (drawn <= 0f) return
        path.reset()
        path.moveTo(x - size * 0.5f, y)
        path.lineTo(x - size * 0.12f, y + size * 0.38f)
        path.lineTo(x + size * 0.5f, y - size * 0.4f)
        val m = PathMeasure(path, false)
        val part = Path()
        m.getSegment(0f, m.length * drawn, part, true)
        line(color, w)
        c.drawPath(part, p)
    }

    // ---- the pictures -----------------------------------------------------------------------------

    private fun welcome(t: Float) {
        val screen = phone(36f, 22f, 64f, 116f)
        qr(screen.centerX() - 20f, screen.centerY() - 20f, 40f, pal.text)
        // The coin leaves the phone, arcs over and drops behind the wallet's front.
        val slide = ease(seg(t, 0.08f, 0.5f))
        val cx = lerp(68f, 248f, slide)
        val cy = lerp(60f, 78f, slide) - 44f * Math.sin(Math.PI * slide).toFloat() + 26f * ease(seg(t, 0.45f, 0.58f))
        coin(cx, cy, 15f)
        card(206f, 74f, 292f, 136f, 12f)
        rr(206f, 74f, 292f, 92f, 9f, pal.chip)
        rrOutline(206f, 74f, 292f, 92f, 9f, pal.fieldLine)
        val snap = ease(seg(t, 0.62f, 0.78f))
        lock(249f, 108f, 26f, snap, pal.good.band, pal.surface)
        // The click: two short ticks either side as the lock shuts.
        val click = bump(t, 0.76f, 0.92f)
        if (click > 0f) {
            line(withAlpha(pal.good.band, click), 2.5f)
            c.drawLine(223f, 90f, 217f, 84f, p)
            c.drawLine(275f, 90f, 281f, 84f, p)
            c.drawLine(249f, 74f, 249f, 66f, p)
        }
    }

    private fun practice(t: Float) {
        // A till pad: nine keys; the finger taps the middle one.
        card(30f, 14f, 150f, 146f, 10f)
        for (j in 0 until 3) for (i in 0 until 3) {
            val l = 42f + i * 36f
            val tp = 28f + j * 38f
            val pressed = if (i == 1 && j == 1) bump(t, 0.2f, 0.4f) else 0f
            rr(l, tp, l + 30f, tp + 30f, 6f, if (pressed > 0f) pal.info.band else pal.chip)
        }
        val reach = ease(seg(t, 0f, 0.22f))
        val dip = bump(t, 0.2f, 0.4f) * 5f
        hand(lerp(176f, 96f, reach), lerp(130f, 82f, reach) + dip - 12f * ease(seg(t, 0.4f, 0.6f)))
        // The code appears, then the check draws itself.
        card(186f, 22f, 296f, 138f, 10f)
        qr(206f, 34f, 70f, pal.text, ease(seg(t, 0.4f, 0.58f)))
        dot(241f, 116f, 12f * ease(seg(t, 0.62f, 0.7f)), pal.good.tint)
        check(241f, 116f, 16f, pal.good.band, 3.5f, ease(seg(t, 0.66f, 0.9f)))
    }

    private fun till(t: Float) {
        card(52f, 12f, 268f, 148f, 14f)
        rr(66f, 26f, 254f, 62f, 6f, pal.chip)
        rr(80f, 38f, 150f, 44f, 3f, pal.hint)
        rr(80f, 49f, 120f, 54f, 3f, pal.divider)
        val lit = ease(seg(t, 0.35f, 0.6f))
        val xs = listOf(66f, 118f, 170f, 222f)
        for ((k, l) in xs.withIndex()) {
            val top = 78f
            val on = if (k == 3) lit else 0f
            if (on > 0f) {
                dot(l + 16f, top + 28f, 30f * on, withAlpha(Palette.BRAND, 0.25f * on))
            }
            rr(l, top, l + 32f, top + 56f, 8f, if (on > 0.5f) Palette.BRAND else pal.chip)
            rrOutline(l, top, l + 32f, top + 56f, 8f, if (on > 0.5f) Palette.BRAND else pal.fieldLine)
            val mark = if (on > 0.5f) Palette.WHITE else pal.secondary
            val x = l + 16f
            val y = top + 28f
            when (k) {
                0 -> { rrOutline(x - 10f, y - 7f, x + 10f, y + 7f, 2f, mark); c.drawLine(x - 10f, y - 2f, x + 10f, y - 2f, p) }
                1 -> { rrOutline(x - 11f, y - 7f, x + 11f, y + 7f, 2f, mark); line(mark, 2f); c.drawCircle(x, y, 3f, p) }
                2 -> for (r in listOf(4f, 8f, 12f).also { line(mark, 2f) }) { rect.set(x - r, y - r + 4f, x + r, y + r + 4f); c.drawArc(rect, 225f, 90f, false, p) }
                else -> btc(x, y, 24f, mark)
            }
        }
    }

    private fun money(t: Float) {
        // The dashed line: the owner's side is the left.
        line(pal.hint, 2f, false)
        p.pathEffect = DashPathEffect(floatArrayOf(7f, 6f), 0f)
        c.drawLine(196f, 10f, 196f, 150f, p)
        p.pathEffect = null
        val drop = ease(seg(t, 0.1f, 0.5f))
        coin(104f, lerp(10f, 86f, drop), 14f, 1f, 1f - seg(t, 0.46f, 0.5f))
        card(56f, 82f, 152f, 142f, 12f)
        rr(56f, 82f, 152f, 98f, 8f, pal.chip)
        rrOutline(56f, 82f, 152f, 98f, 8f, pal.fieldLine)
        lock(104f, 118f, 22f, ease(seg(t, 0.52f, 0.66f)), pal.good.band, pal.surface)
        // The other side: a plain building, and a faint coin that comes up to the line and goes back.
        rr(236f, 78f, 290f, 140f, 3f, pal.divider)
        path.reset(); path.moveTo(230f, 80f); path.lineTo(263f, 52f); path.lineTo(296f, 80f); path.close()
        fill(pal.divider); c.drawPath(path, p)
        for (k in 0 until 3) rr(246f + k * 14f, 96f, 254f + k * 14f, 124f, 2f, pal.surface)
        val toward = bump(t, 0.5f, 0.92f)
        coin(lerp(262f, 214f, toward), 34f, 12f, 1f, 0.45f)
    }

    private fun why(t: Float) {
        val ground = 128f
        line(pal.divider, 3f)
        c.drawLine(70f, ground + 20f, 250f, ground + 20f, p)
        val up = seg(t, 0.05f, 0.58f)
        val y = ground - 92f * 4f * up * (1f - up)
        val spin = ease(up) * 4f * Math.PI.toFloat()
        val land = bump(t, 0.58f, 0.7f)
        coin(160f, y + land * 3f, 18f, Math.cos(spin.toDouble()).toFloat())
        val pop = seg(t, 0.68f, 0.86f)
        val s = if (pop <= 0f) 0f else ease(pop) * (1f + 0.15f * bump(t, 0.68f, 0.9f))
        c.save()
        c.translate(214f, 74f)
        c.scale(s, s)
        lock(0f, 0f, 30f, 1f, pal.good.band, pal.surface)
        c.restore()
    }

    private fun usb(t: Float) {
        laptop(190f, 26f, 150f)
        stick(lerp(8f, 47f, ease(seg(t, 0.1f, 0.6f))), if (seg(t, 0.62f, 0.7f) > 0f) pal.good.tint else withAlpha(Palette.WHITE, 0.5f))
    }

    /** The stick: the plug on the right, the body and a small light, its left at [x]. */
    private fun stick(x: Float, light: Int) {
        rr(x + 40f, 116f, x + 62f, 130f, 2f, pal.hint)
        rr(x, 108f, x + 44f, 138f, 7f, pal.info.band)
        dot(x + 12f, 123f, 4f, light)
    }

    private fun offline(t: Float) {
        laptop(110f, 40f, 130f)
        val pulse = bump(t, 0.05f, 0.5f)
        val cross = ease(seg(t, 0.4f, 0.7f))
        val cx = 242f
        val cy = 100f
        dot(cx, cy, 5f, pal.secondary)
        for ((k, r) in listOf(20f, 38f, 56f).withIndex()) {
            val a = (0.35f + 0.65f * pulse * (if (k == 1) 1f else 0.7f)) * (1f - 0.55f * cross)
            line(withAlpha(pal.secondary, a), 5f)
            rect.set(cx - r, cy - r, cx + r, cy + r)
            c.drawArc(rect, 225f, 90f, false, p)
        }
        line(pal.stop.band, 6f)
        val d = 40f * cross
        c.drawLine(cx - 42f, cy - 62f, cx - 42f + d * 2.1f, cy - 62f + d * 2.1f, p)
        val d2 = 40f * ease(seg(t, 0.55f, 0.85f))
        c.drawLine(cx + 42f, cy - 62f, cx + 42f - d2 * 2.1f, cy - 62f + d2 * 2.1f, p)
    }

    private fun scan(t: Float) {
        val s = laptop(96f, 28f, 148f)
        qr(s.centerX() - 24f, s.centerY() - 24f, 48f, pal.text)
        val come = ease(seg(t, 0.05f, 0.4f))
        val px = lerp(290f, 196f, come)
        val ph = phone(px, 36f, 56f, 104f)
        val aim = ease(seg(t, 0.4f, 0.5f))
        val color = if (seg(t, 0.86f, 0.9f) > 0f) pal.good.band else pal.info.band
        if (aim > 0f) {
            line(withAlpha(color, aim), 3f)
            val l = ph.left + 8f; val r = ph.right - 8f; val tp = ph.top + 22f; val b = ph.bottom - 22f; val k = 10f
            for ((x, y, dx, dy) in listOf(listOf(l, tp, 1f, 1f), listOf(r, tp, -1f, 1f), listOf(l, b, 1f, -1f), listOf(r, b, -1f, -1f))) {
                c.drawLine(x, y, x + dx * k, y, p); c.drawLine(x, y, x, y + dy * k, p)
            }
            val sweep = seg(t, 0.52f, 0.84f)
            if (sweep > 0f && sweep < 1f) {
                line(withAlpha(Palette.BRAND, 0.9f), 2.5f)
                val yy = lerp(tp + 4f, b - 4f, sweep)
                c.drawLine(l + 2f, yy, r - 2f, yy, p)
            }
        }
    }

    private fun same(t: Float) {
        val lefts = listOf(24f, 190f)
        for (l in lefts) card(l, 30f, l + 106f, 110f, 8f)
        for (k in 0 until 4) {
            val s = ease(seg(t, 0.05f + k * 0.1f, 0.2f + k * 0.1f))
            for (l in lefts) {
                val x = l + 20f + k * 22f
                val y = 70f
                c.save()
                c.translate(x, y)
                c.scale(s, s)
                when (k) {
                    0 -> dot(0f, 0f, 8f, pal.info.band)
                    1 -> { rr(-8f, -8f, 8f, 8f, 2f, Palette.BRAND) }
                    2 -> { path.reset(); path.moveTo(0f, -9f); path.lineTo(9f, 8f); path.lineTo(-9f, 8f); path.close(); fill(pal.good.band); c.drawPath(path, p) }
                    else -> { path.reset(); path.moveTo(0f, -9f); path.lineTo(9f, 0f); path.lineTo(0f, 9f); path.lineTo(-9f, 0f); path.close(); fill(pal.secondary); c.drawPath(path, p) }
                }
                c.restore()
            }
        }
        val pop = ease(seg(t, 0.55f, 0.65f))
        dot(160f, 70f, 17f * pop, pal.good.tint)
        check(160f, 70f, 18f, pal.good.band, 4f, ease(seg(t, 0.62f, 0.9f)))
    }

    private fun shutdown(t: Float) {
        val s = laptop(212f, 22f, 160f)
        for (k in 0 until 4) rr(s.left + 12f, s.top + 12f + k * 12f, s.left + 12f + 90f - k * 14f, s.top + 17f + k * 12f, 2.5f, pal.hint)
        val dim = ease(seg(t, 0.1f, 0.5f))
        rr(s.left, s.top, s.right, s.bottom, 3f, withAlpha(0xFF101418.toInt(), 0.92f * dim))
        // The stick comes out of the side after the screen has gone dark.
        stick(lerp(64f, 8f, ease(seg(t, 0.55f, 0.9f))), withAlpha(Palette.WHITE, 0.5f))
    }
}
