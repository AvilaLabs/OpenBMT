package org.openbmt.owner.app

import android.content.Context
import android.graphics.Canvas
import android.graphics.Paint
import android.graphics.Path
import android.view.Gravity
import android.view.View
import android.view.ViewGroup
import android.widget.ImageView
import android.widget.LinearLayout
import org.openbmt.app.Icon
import org.openbmt.app.IconDrawable
import org.openbmt.app.Ui

// The key path at the top of the key-setup screens: three circles in a row, one for each key, joined by a line
// behind them, with the key's name and where it lives under each. A made key is a filled circle, the next one a
// ring with a soft halo, one not reached yet is grey. All three keys have the one blue. Shapes are drawn with
// Canvas, in the app's palette, with no image; the words are the app's (AppWords), the states and colours are
// KeyMapModel's. The path is spoken as one description; the rule is a line of its own under the screen's words.

/**
 * One key's column of the path, as wide as its third of the row and as tall as its circle and halo: the circle in
 * the middle, and the line to the key before it and to the key after it, each half drawn in this column.
 */
private class KeyCircleView(
    context: Context,
    private val state: KeyState,
    private val night: Boolean,
    private val surface: Int,
    private val before: KeyState?,
    private val hasNext: Boolean,
) : View(context) {
    private val paint = Paint(Paint.ANTI_ALIAS_FLAG)
    private val glyph = Path()

    init {
        importantForAccessibility = IMPORTANT_FOR_ACCESSIBILITY_NO
    }

    private fun dp(x: Int) = x * resources.displayMetrics.density

    override fun onMeasure(widthSpec: Int, heightSpec: Int) {
        val h = dp(KeyMapModel.CIRCLE_DP + 2 * KeyMapModel.HALO_DP).toInt()
        setMeasuredDimension(resolveSize(0, widthSpec), h)
    }

    override fun onDraw(canvas: Canvas) {
        val night = night
        val cx = width / 2f
        val cy = height / 2f
        val r = dp(KeyMapModel.CIRCLE_DP) / 2f
        val ring = dp(KeyMapModel.RING_DP)
        val blue = KeyMapModel.line(night)
        val grey = KeyMapModel.greyRing(night)
        // The line behind the circle: blue after a made key, grey otherwise; its left half is the one the key before made.
        paint.style = Paint.Style.FILL
        val half = dp(KeyMapModel.LINE_DP) / 2f
        if (before != null) {
            paint.color = if (before == KeyState.Made) blue else grey
            canvas.drawRect(0f, cy - half, cx, cy + half, paint)
        }
        if (hasNext) {
            paint.color = if (state == KeyState.Made) blue else grey
            canvas.drawRect(cx, cy - half, width.toFloat(), cy + half, paint)
        }
        // The circle: filled when made; a ring on the surface, with a halo when next, a grey ring when not yet.
        val glyphColour: Int
        when (state) {
            KeyState.Made -> {
                paint.color = KeyMapModel.fill(night)
                canvas.drawCircle(cx, cy, r, paint)
                glyphColour = android.graphics.Color.WHITE
            }
            KeyState.Next -> {
                paint.color = KeyMapModel.halo(night)
                canvas.drawCircle(cx, cy, r + dp(KeyMapModel.HALO_DP), paint)
                paint.color = surface
                canvas.drawCircle(cx, cy, r, paint)
                ringOf(canvas, cx, cy, r, ring, blue)
                glyphColour = blue
            }
            KeyState.NotYet -> {
                paint.color = surface
                canvas.drawCircle(cx, cy, r, paint)
                ringOf(canvas, cx, cy, r, ring, grey)
                glyphColour = KeyMapModel.greyGlyph(night)
            }
        }
        drawKey(canvas, cx, cy, glyphColour)
    }

    /** A ring [w] wide inside the circle of radius [r]. */
    private fun ringOf(canvas: Canvas, cx: Float, cy: Float, r: Float, w: Float, colour: Int) {
        paint.style = Paint.Style.STROKE
        paint.strokeWidth = w
        paint.color = colour
        canvas.drawCircle(cx, cy, r - w / 2f, paint)
        paint.style = Paint.Style.FILL
    }

    /** The key glyph, 34 dp square: a ring with a shaft and two teeth, as a 2.4 stroke in a 24-unit box. */
    private fun drawKey(canvas: Canvas, cx: Float, cy: Float, colour: Int) {
        val s = dp(34) / 24f
        canvas.save()
        canvas.translate(cx - 12f * s, cy - 12f * s)
        canvas.scale(s, s)
        glyph.reset()
        glyph.addCircle(7f, 12f, 4f, Path.Direction.CW)
        glyph.moveTo(11f, 12f)
        glyph.lineTo(21f, 12f)
        glyph.moveTo(17f, 12f)
        glyph.lineTo(17f, 15f)
        glyph.moveTo(20f, 12f)
        glyph.lineTo(20f, 15f)
        paint.style = Paint.Style.STROKE
        paint.strokeWidth = 2.4f
        paint.color = colour
        canvas.drawPath(glyph, paint)
        paint.style = Paint.Style.FILL
        canvas.restore()
    }
}

/**
 * The key path for [states] (key 1, key 2, key 3): three equal columns, each with its circle, its name (with a
 * drawn tick when made) and where it lives on one line. The words are described as one; the rule is [keyMapRule].
 */
fun Ui.keyMap(states: List<KeyState>): View {
    val row = LinearLayout(context).apply {
        orientation = LinearLayout.HORIZONTAL
        filterTouchesWhenObscured = true
    }
    for (i in 0..2) {
        val state = states.getOrElse(i) { KeyState.NotYet }
        val notYet = state == KeyState.NotYet
        val label = if (notYet) KeyMapModel.greyLabel(night) else pal.text
        val where = if (notYet) KeyMapModel.greyLabel(night) else pal.secondary
        val col = LinearLayout(context).apply {
            orientation = LinearLayout.VERTICAL
            gravity = Gravity.CENTER_HORIZONTAL
        }
        col.addView(
            KeyCircleView(context, state, night, pal.surface, if (i == 0) null else states.getOrElse(i - 1) { KeyState.NotYet }, i < 2),
            LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT),
        )
        val name = LinearLayout(context).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER
        }
        name.addView(text(KeyMapModel.keyName(i), 16f, bold = true, colour = label))
        if (state == KeyState.Made) {
            name.addView(
                ImageView(context).apply { setImageDrawable(IconDrawable(Icon.Tick, label, 2.6f)) },
                LinearLayout.LayoutParams(px(14), px(14)).also { it.leftMargin = px(4) },
            )
        }
        col.addView(name, LinearLayout.LayoutParams(ViewGroup.LayoutParams.WRAP_CONTENT, ViewGroup.LayoutParams.WRAP_CONTENT))
        col.addView(
            text(KeyMapModel.keyWhere(i), 14f, colour = where).also { it.gravity = Gravity.CENTER },
            LinearLayout.LayoutParams(ViewGroup.LayoutParams.WRAP_CONTENT, ViewGroup.LayoutParams.WRAP_CONTENT).also { it.topMargin = px(2) },
        )
        row.addView(col, LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f))
    }
    row.contentDescription = KeyMapModel.spoken(states)
    row.importantForAccessibility = View.IMPORTANT_FOR_ACCESSIBILITY_YES
    for (i in 0 until row.childCount) row.getChildAt(i).importantForAccessibility = View.IMPORTANT_FOR_ACCESSIBILITY_NO_HIDE_DESCENDANTS
    return row
}

/** The rule of the keys as a small line in the secondary colour, for under the words of a screen that shows the path. */
fun Ui.keyMapRule(): View = text(AppWords.keyMapRule, 14f, colour = pal.secondary).also { it.filterTouchesWhenObscured = true }
