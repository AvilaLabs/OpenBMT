package org.openbmt.owner.app

import android.content.Context
import android.graphics.Canvas
import android.graphics.Paint
import android.graphics.Path
import android.graphics.RectF
import android.graphics.drawable.GradientDrawable
import android.view.Gravity
import android.view.View
import android.view.ViewGroup
import android.widget.ImageView
import android.widget.LinearLayout
import org.openbmt.app.Icon
import org.openbmt.app.IconDrawable
import org.openbmt.app.Palette
import org.openbmt.app.Ui

// The picture of the three keys at the top of the key-setup screens: one card for each key with a key shape in its
// colour, its name, where it lives and its state, and the rule under the row. Shapes are drawn with Canvas as the
// first-run pictures are, in the app's palette, with no image; the words are the app's (AppWords), the states and
// colours are KeyMapModel's. The row is spoken as one description.

/** Which of the places a key lives in is drawn: a phone, or a steel plate with four screws. */
private enum class Place { Phone, Plate }

/** A key seen from the side (a ring with a hole, a shaft, two teeth), 64 by 32 dp: filled when made, a line otherwise. */
private class KeyShapeView(context: Context, private val colour: Int, private val filled: Boolean) : View(context) {
    private val paint = Paint(Paint.ANTI_ALIAS_FLAG)
    private val path = Path()

    init {
        importantForAccessibility = IMPORTANT_FOR_ACCESSIBILITY_NO
    }

    override fun onMeasure(widthSpec: Int, heightSpec: Int) {
        val d = resources.displayMetrics.density
        setMeasuredDimension((64 * d).toInt(), (32 * d).toInt())
    }

    override fun onDraw(canvas: Canvas) {
        val s = minOf(width / 64f, height / 32f)
        canvas.save()
        canvas.scale(s, s)
        path.reset()
        path.fillType = Path.FillType.EVEN_ODD
        path.addCircle(14f, 16f, 11f, Path.Direction.CW)
        path.addCircle(14f, 16f, 4.5f, Path.Direction.CW)
        path.addRect(24f, 14f, 62f, 18.5f, Path.Direction.CW)
        path.addRect(49f, 18f, 53f, 26f, Path.Direction.CW)
        path.addRect(57f, 18f, 61f, 24f, Path.Direction.CW)
        paint.color = colour
        if (filled) {
            paint.style = Paint.Style.FILL
        } else {
            paint.style = Paint.Style.STROKE
            paint.strokeWidth = 2f
            paint.strokeJoin = Paint.Join.ROUND
        }
        canvas.drawPath(path, paint)
        canvas.restore()
    }
}

/** A small phone or steel plate, 20 dp, as a line in [colour]. */
private class PlaceView(context: Context, private val place: Place, private val colour: Int) : View(context) {
    private val paint = Paint(Paint.ANTI_ALIAS_FLAG)
    private val rect = RectF()

    init {
        importantForAccessibility = IMPORTANT_FOR_ACCESSIBILITY_NO
    }

    override fun onMeasure(widthSpec: Int, heightSpec: Int) {
        val d = resources.displayMetrics.density
        setMeasuredDimension((20 * d).toInt(), (20 * d).toInt())
    }

    override fun onDraw(canvas: Canvas) {
        val s = minOf(width, height) / 24f
        canvas.save()
        canvas.scale(s, s)
        paint.color = colour
        paint.style = Paint.Style.STROKE
        paint.strokeWidth = 1.8f
        when (place) {
            Place.Phone -> {
                rect.set(7f, 2.5f, 17f, 21.5f)
                canvas.drawRoundRect(rect, 2.4f, 2.4f, paint)
                paint.style = Paint.Style.FILL
                canvas.drawCircle(12f, 18.5f, 1f, paint)
            }
            Place.Plate -> {
                rect.set(2.5f, 6f, 21.5f, 18f)
                canvas.drawRoundRect(rect, 2f, 2f, paint)
                paint.style = Paint.Style.FILL
                for ((x, y) in listOf(6f to 9.5f, 18f to 9.5f, 6f to 14.5f, 18f to 14.5f)) canvas.drawCircle(x, y, 1.1f, paint)
            }
        }
        canvas.restore()
    }
}

/** The row of cards: three side by side, or one under the other when the row is narrower than [KeyMapModel.STACK_BELOW_DP]. */
private class KeyMapRow(context: Context) : LinearLayout(context) {
    override fun onMeasure(widthSpec: Int, heightSpec: Int) {
        val dp = MeasureSpec.getSize(widthSpec) / resources.displayMetrics.density
        val stacked = MeasureSpec.getMode(widthSpec) != MeasureSpec.UNSPECIFIED && dp < KeyMapModel.STACK_BELOW_DP
        orientation = if (stacked) VERTICAL else HORIZONTAL
        val gap = (8 * resources.displayMetrics.density).toInt()
        for (i in 0 until childCount) {
            val c = getChildAt(i)
            val p = c.layoutParams as LayoutParams
            if (stacked) {
                p.width = ViewGroup.LayoutParams.MATCH_PARENT
                p.weight = 0f
                p.leftMargin = 0
                p.topMargin = if (i == 0) 0 else gap
            } else {
                p.width = 0
                p.weight = 1f
                p.leftMargin = if (i == 0) 0 else gap
                p.topMargin = 0
            }
        }
        super.onMeasure(widthSpec, heightSpec)
    }
}

/**
 * The key map for [states] (key 1, key 2, key 3). A made key is filled; the next one has a 2 dp outline in its
 * colour; one not reached yet is a line at reduced contrast.
 */
fun Ui.keyMap(states: List<KeyState>): View {
    fun dp(x: Int) = px(x)
    val surface = pal.surface
    val col = LinearLayout(context).apply {
        orientation = LinearLayout.VERTICAL
        filterTouchesWhenObscured = true
    }
    val row = KeyMapRow(context)
    for (i in 0..2) {
        val state = states.getOrElse(i) { KeyState.NotYet }
        val ink = KeyMapModel.ink(i, night)
        val fill = KeyMapModel.fill(i, night)
        val card = LinearLayout(context).apply {
            orientation = LinearLayout.VERTICAL
            gravity = Gravity.CENTER_HORIZONTAL
            setPadding(dp(8), dp(12), dp(8), dp(12))
            background = GradientDrawable().apply {
                setColor(surface)
                cornerRadius = dp(14).toFloat()
                when (state) {
                    KeyState.Next -> setStroke(dp(2), ink)
                    KeyState.Made -> setStroke(dp(1), pal.divider)
                    KeyState.NotYet -> setStroke(dp(1), KeyMapModel.notYetLine(i, night))
                }
            }
        }
        val line = if (state == KeyState.NotYet) KeyMapModel.notYetLine(i, night) else ink
        card.addView(KeyShapeView(context, if (state == KeyState.Made) fill else line, state == KeyState.Made))
        card.addView(text(KeyMapModel.keyName(i), 16f, bold = true, colour = pal.text).also { it.gravity = Gravity.CENTER }, margins(top = 8))
        val where = LinearLayout(context).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER
        }
        where.addView(PlaceView(context, if (i == 2) Place.Plate else Place.Phone, pal.secondary), LinearLayout.LayoutParams(dp(20), dp(20)).also { it.rightMargin = dp(6) })
        where.addView(text(KeyMapModel.keyWhere(i), 13f, colour = pal.secondary), LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f))
        card.addView(where, margins(top = 4))
        val textInk = when (state) {
            KeyState.Made -> Palette.WHITE
            KeyState.Next -> ink
            KeyState.NotYet -> pal.secondary
        }
        val chip = LinearLayout(context).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER
            setPadding(dp(10), dp(4), dp(10), dp(4))
            background = GradientDrawable().apply {
                cornerRadius = dp(12).toFloat()
                setColor(
                    when (state) {
                        KeyState.Made -> fill
                        KeyState.Next -> KeyMapModel.tint(i, night)
                        KeyState.NotYet -> pal.chip
                    },
                )
            }
        }
        if (state == KeyState.Made) {
            chip.addView(
                ImageView(context).apply { setImageDrawable(IconDrawable(Icon.Tick, Palette.WHITE, 2.6f)) },
                LinearLayout.LayoutParams(dp(14), dp(14)).also { it.rightMargin = dp(4) },
            )
        }
        chip.addView(text(KeyMapModel.stateWord(state), 13f, bold = true, colour = textInk))
        card.addView(chip, LinearLayout.LayoutParams(ViewGroup.LayoutParams.WRAP_CONTENT, ViewGroup.LayoutParams.WRAP_CONTENT).also {
            it.topMargin = dp(10)
            it.gravity = Gravity.CENTER_HORIZONTAL
        })
        row.addView(card, LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f))
    }
    col.addView(row, margins())
    col.addView(text(AppWords.keyMapRule, 13f, colour = pal.secondary).also { it.gravity = Gravity.CENTER }, margins(top = 10))
    col.contentDescription = KeyMapModel.spoken(states)
    col.importantForAccessibility = View.IMPORTANT_FOR_ACCESSIBILITY_YES
    for (i in 0 until col.childCount) col.getChildAt(i).importantForAccessibility = View.IMPORTANT_FOR_ACCESSIBILITY_NO_HIDE_DESCENDANTS
    return col
}
