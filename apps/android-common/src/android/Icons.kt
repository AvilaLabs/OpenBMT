package org.openbmt.app

import android.graphics.Canvas
import android.graphics.ColorFilter
import android.graphics.Paint
import android.graphics.Path
import android.graphics.PixelFormat
import android.graphics.RectF
import android.graphics.drawable.Drawable

/** The icons of the apps, drawn as paths on a 24 by 24 grid: no resource files. */
enum class Icon { Back, Gear, Tick, Exclaim, Info, Dash, Person, Arrow, InfoCircle, Chevron, Padlock }

/** One [Icon] in one colour, drawn as a stroke of [strokeWidth] grid units (rounded ends) in the bounds it is given. */
class IconDrawable(private val icon: Icon, colour: Int, private val strokeWidth: Float = 2f) : Drawable() {
    private val stroke = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        style = Paint.Style.STROKE
        strokeCap = Paint.Cap.ROUND
        strokeJoin = Paint.Join.ROUND
        color = colour
    }
    private val fill = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        style = Paint.Style.FILL
        color = colour
    }

    /** The part of the tick drawn so far, 0 to 1, for a tick that draws itself in. */
    var drawn = 1f
        set(value) {
            field = value.coerceIn(0f, 1f)
            invalidateSelf()
        }

    override fun draw(canvas: Canvas) {
        val b = bounds
        if (b.isEmpty) return
        canvas.save()
        canvas.translate(b.left.toFloat(), b.top.toFloat())
        val s = minOf(b.width(), b.height()) / 24f
        canvas.scale(s, s)
        stroke.strokeWidth = strokeWidth
        when (icon) {
            Icon.Back -> canvas.drawPath(path { moveTo(19f, 12f); lineTo(5f, 12f); moveTo(12f, 5f); lineTo(5f, 12f); lineTo(12f, 19f) }, stroke)
            Icon.Arrow -> canvas.drawPath(path { moveTo(5f, 12f); lineTo(19f, 12f); moveTo(13f, 6f); lineTo(19f, 12f); lineTo(13f, 18f) }, stroke)
            Icon.Chevron -> canvas.drawPath(path { moveTo(9f, 6f); lineTo(15f, 12f); lineTo(9f, 18f) }, stroke)
            Icon.Tick -> canvas.drawPath(tickPath(), stroke)
            Icon.Dash -> canvas.drawLine(6f, 12f, 18f, 12f, stroke)
            Icon.Exclaim -> {
                canvas.drawLine(12f, 5f, 12f, 13.5f, stroke)
                canvas.drawCircle(12f, 18.5f, 1.4f, fill)
            }
            Icon.Info -> {
                canvas.drawCircle(12f, 6.5f, 1.4f, fill)
                canvas.drawLine(12f, 11f, 12f, 19f, stroke)
            }
            Icon.InfoCircle -> {
                canvas.drawCircle(12f, 12f, 9f, stroke)
                canvas.drawCircle(12f, 7.8f, 1.1f, fill)
                canvas.drawLine(12f, 11f, 12f, 17f, stroke)
            }
            Icon.Person -> {
                canvas.drawCircle(12f, 8f, 3.6f, stroke)
                canvas.drawPath(path { moveTo(5.5f, 20f); cubicTo(5.5f, 15.8f, 8.4f, 14f, 12f, 14f); cubicTo(15.6f, 14f, 18.5f, 15.8f, 18.5f, 20f) }, stroke)
            }
            Icon.Gear -> {
                canvas.drawCircle(12f, 12f, 3f, stroke)
                canvas.drawCircle(12f, 12f, 7f, stroke)
                for (i in 0 until 8) {
                    canvas.save()
                    canvas.rotate(i * 45f, 12f, 12f)
                    canvas.drawLine(12f, 2.8f, 12f, 5f, stroke)
                    canvas.restore()
                }
            }
            Icon.Padlock -> {
                canvas.drawRoundRect(RectF(5f, 11f, 19f, 21f), 2.5f, 2.5f, stroke)
                canvas.drawPath(path { moveTo(8f, 11f); lineTo(8f, 8f); arcTo(RectF(8f, 3f, 16f, 13f), 180f, 180f); lineTo(16f, 11f) }, stroke)
                canvas.drawCircle(12f, 16f, 1.2f, fill)
            }
        }
        canvas.restore()
    }

    private fun tickPath(): Path {
        val full = path { moveTo(5f, 12.5f); lineTo(10f, 17.5f); lineTo(19f, 7f) }
        if (drawn >= 1f) return full
        // The two strokes are 7.07 and 12.73 long: the first is drawn, then the second.
        val first = 7.07f
        val second = 12.73f
        val at = drawn * (first + second)
        return path {
            moveTo(5f, 12.5f)
            if (at <= first) {
                val f = at / first
                lineTo(5f + 5f * f, 12.5f + 5f * f)
            } else {
                lineTo(10f, 17.5f)
                val f = (at - first) / second
                lineTo(10f + 9f * f, 17.5f - 10.5f * f)
            }
        }
    }

    private inline fun path(build: Path.() -> Unit): Path = Path().apply(build)

    override fun setAlpha(alpha: Int) {
        stroke.alpha = alpha
        fill.alpha = alpha
    }

    override fun setColorFilter(colorFilter: ColorFilter?) {
        stroke.colorFilter = colorFilter
        fill.colorFilter = colorFilter
    }

    @Suppress("OVERRIDE_DEPRECATION")
    override fun getOpacity(): Int = PixelFormat.TRANSLUCENT
}
