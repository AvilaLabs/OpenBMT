package org.openbmt.counter.app

import android.content.Context
import android.content.res.ColorStateList
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Paint
import android.graphics.Path
import android.graphics.RectF
import android.graphics.Typeface
import android.graphics.drawable.Drawable
import android.graphics.drawable.GradientDrawable
import android.graphics.drawable.RippleDrawable
import android.view.Gravity
import android.view.View
import android.view.ViewGroup
import android.view.WindowInsets
import android.widget.Button
import android.widget.FrameLayout
import android.widget.LinearLayout
import android.widget.TextView
import org.openbmt.app.BarView
import org.openbmt.app.Colour
import org.openbmt.app.Fills
import org.openbmt.app.Feedback
import org.openbmt.app.Haptic
import org.openbmt.app.OutcomeBand
import org.openbmt.app.Palette
import org.openbmt.app.SayWords
import org.openbmt.app.Tones
import org.openbmt.app.Ui
import org.openbmt.app.WordsModel

// The counter's own pieces of the new look: what the shared code does not have (the outcome screen of a
// sale, the tiles and the lines of home, the keypad of the amount). They use the shared palette and sizes.

/** The outcome band of a words colour: paid is green, waiting blue, stop red, ended neutral. */
fun outcomeBand(colour: Colour): OutcomeBand = when (colour) {
    Colour.Green -> Palette.paid
    Colour.Blue -> Palette.waiting
    Colour.Red -> Palette.stopped
    Colour.Neutral -> Palette.ended
}

/** The surface fill, a corner of [radius] dp. */
fun Ui.roundedFill(fill: Int, radius: Float, strokeDp: Float = 0f, stroke: Int = 0): GradientDrawable = GradientDrawable().apply {
    setColor(fill)
    cornerRadius = radius * context.resources.displayMetrics.density
    if (strokeDp > 0f) setStroke(pxf(strokeDp), stroke)
}

/** A background that ripples when pressed, over [bg], with the corner [radius] dp. */
fun Ui.rippleOver(bg: Drawable?, radius: Float): Drawable {
    val mask = GradientDrawable().apply {
        cornerRadius = radius * context.resources.displayMetrics.density
        setColor(Color.WHITE)
    }
    val tap = if (night) 0x22FFFFFF else 0x14000000
    return RippleDrawable(ColorStateList.valueOf(tap), bg, mask)
}

/** A surface card, corner 16, that holds a column of views. */
fun Ui.surfaceCard(): LinearLayout = LinearLayout(context).apply {
    orientation = LinearLayout.VERTICAL
    background = roundedFill(pal.surface, 16f)
    clipToOutline = true
    filterTouchesWhenObscured = true
}

/** A 1 dp line in the divider colour. */
fun Ui.line(): View = View(context).apply {
    setBackgroundColor(pal.divider)
    layoutParams = LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, pxf(1f))
}

/** A round dot of [dp] in [colour]. */
fun Ui.dot(colour: Int, dp: Int): View = View(context).apply {
    background = GradientDrawable().apply {
        shape = GradientDrawable.OVAL
        setColor(colour)
    }
    layoutParams = LinearLayout.LayoutParams(px(dp), px(dp))
    importantForAccessibility = View.IMPORTANT_FOR_ACCESSIBILITY_NO
}

/** The pill that says who does the next step: 13 sp medium in secondary on the chip fill. */
fun Ui.whoChip(model: WordsModel, onDark: Boolean = false): View = text(
    model.who,
    13f,
    colour = if (onDark) Palette.WHITE else pal.secondary,
    medium = true,
).apply {
    background = roundedFill(if (onDark) 0x2EFFFFFF else pal.chip, 999f)
    setPadding(px(10), px(5), px(10), px(5))
}

/** A row's width: the whole of its parent. */
fun matchWide(): LinearLayout.LayoutParams = LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT)

/** The pictures of the tiles and rows of the counter, drawn as strokes on a 24 by 24 grid. */
enum class Glyph { Plus, List, Calendar, Lock }

/** One [Glyph] in one colour. */
class GlyphDrawable(private val glyph: Glyph, colour: Int, private val width: Float = 2f) : Drawable() {
    private val paint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        style = Paint.Style.STROKE
        strokeCap = Paint.Cap.ROUND
        strokeJoin = Paint.Join.ROUND
        color = colour
    }

    override fun draw(canvas: Canvas) {
        val b = bounds
        if (b.isEmpty) return
        canvas.save()
        canvas.translate(b.left.toFloat(), b.top.toFloat())
        val s = minOf(b.width(), b.height()) / 24f
        canvas.scale(s, s)
        paint.strokeWidth = width
        when (glyph) {
            Glyph.Plus -> {
                canvas.drawLine(12f, 5f, 12f, 19f, paint)
                canvas.drawLine(5f, 12f, 19f, 12f, paint)
            }
            Glyph.List -> {
                for (y in listOf(6f, 12f, 18f)) {
                    canvas.drawLine(8f, y, 20f, y, paint)
                    canvas.drawPoint(4f, y, paint)
                }
            }
            Glyph.Calendar -> {
                canvas.drawRoundRect(RectF(4f, 5f, 20f, 20f), 2f, 2f, paint)
                canvas.drawLine(4f, 10f, 20f, 10f, paint)
                canvas.drawLine(9f, 3f, 9f, 7f, paint)
                canvas.drawLine(15f, 3f, 15f, 7f, paint)
            }
            Glyph.Lock -> {
                canvas.drawRoundRect(RectF(5f, 11f, 19f, 21f), 2.5f, 2.5f, paint)
                val shackle = Path().apply {
                    moveTo(8f, 11f)
                    lineTo(8f, 8f)
                    arcTo(RectF(8f, 4f, 16f, 12f), 180f, 180f)
                    lineTo(16f, 11f)
                }
                canvas.drawPath(shackle, paint)
            }
        }
        canvas.restore()
    }

    override fun setAlpha(alpha: Int) {
        paint.alpha = alpha
    }

    override fun setColorFilter(colorFilter: android.graphics.ColorFilter?) {
        paint.colorFilter = colorFilter
    }

    @Deprecated("Drawable's own method")
    override fun getOpacity(): Int = android.graphics.PixelFormat.TRANSLUCENT
}

/** A glyph as a view of [dp] by [dp]. */
fun Ui.glyph(glyph: Glyph, colour: Int, dp: Int, width: Float = 2f): View = View(context).apply {
    background = GlyphDrawable(glyph, colour, width)
    layoutParams = LinearLayout.LayoutParams(px(dp), px(dp))
    importantForAccessibility = View.IMPORTANT_FOR_ACCESSIBILITY_NO
}

/**
 * The mark of an outcome screen, 112 dp: a white disc with a tick for good, a clock for waiting, and a dash for
 * ended, or a white octagon with a cross for stop, the picture in the [band] colour.
 */
class OutcomeMark(context: Context, private val colour: Colour, private val band: Int) : View(context) {
    private val fill = Paint(Paint.ANTI_ALIAS_FLAG).apply { color = Palette.WHITE }
    private val stroke = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        style = Paint.Style.STROKE
        strokeCap = Paint.Cap.ROUND
        strokeJoin = Paint.Join.ROUND
        color = band
    }

    init {
        importantForAccessibility = IMPORTANT_FOR_ACCESSIBILITY_NO
    }

    override fun onMeasure(widthSpec: Int, heightSpec: Int) {
        val px = (112 * resources.displayMetrics.density).toInt()
        setMeasuredDimension(px, px)
    }

    override fun onDraw(canvas: Canvas) {
        canvas.save()
        val s = width / 112f
        canvas.scale(s, s)
        when (colour) {
            Colour.Green -> {
                canvas.drawCircle(56f, 56f, 56f, fill)
                stroke.strokeWidth = 10f
                canvas.drawPath(Path().apply { moveTo(32f, 57f); lineTo(48f, 73f); lineTo(80f, 39f) }, stroke)
            }
            Colour.Blue -> {
                canvas.drawCircle(56f, 56f, 56f, fill)
                stroke.strokeWidth = 8f
                canvas.drawCircle(56f, 56f, 30f, stroke)
                canvas.drawPath(Path().apply { moveTo(56f, 40f); lineTo(56f, 57f); lineTo(67f, 65f) }, stroke)
            }
            Colour.Red -> {
                val o = Path().apply {
                    moveTo(33f, 4f); lineTo(79f, 4f); lineTo(108f, 33f); lineTo(108f, 79f)
                    lineTo(79f, 108f); lineTo(33f, 108f); lineTo(4f, 79f); lineTo(4f, 33f); close()
                }
                canvas.drawPath(o, fill)
                stroke.strokeWidth = 10f
                canvas.drawLine(40f, 40f, 72f, 72f, stroke)
                canvas.drawLine(72f, 40f, 40f, 72f, stroke)
            }
            Colour.Neutral -> {
                canvas.drawCircle(56f, 56f, 56f, fill)
                stroke.strokeWidth = 10f
                canvas.drawLine(34f, 56f, 78f, 56f, stroke)
            }
        }
        canvas.restore()
    }
}

/** A column that takes the height that is left in the body of a page and centres what it holds. */
class Centered(context: Context) : LinearLayout(context), Fills {
    init {
        orientation = VERTICAL
        gravity = Gravity.CENTER
    }
}

/** The state of a sale as the chip of its bar: a dot and the heading in the tone's ink, on the tone's tint. */
fun Ui.stateChip(model: WordsModel): View {
    val tone = pal.of(model.colour)
    return LinearLayout(context).apply {
        orientation = LinearLayout.HORIZONTAL
        gravity = Gravity.CENTER_VERTICAL
        background = roundedFill(tone.tint, 999f)
        setPadding(px(12), px(6), px(12), px(6))
        addView(dot(tone.ink, 8), LinearLayout.LayoutParams(px(8), px(8)).also { it.rightMargin = px(8) })
        addView(text(model.heading, 14f, colour = tone.ink, medium = true).also {
            it.setSingleLine(true)
            it.ellipsize = android.text.TextUtils.TruncateAt.END
        })
    }
}

/** Puts [chip] at the right end of a bar made by [Ui.bar]. */
fun addToBar(bar: View, chip: View, ui: Ui) {
    val row = (bar as? BarView)?.row ?: return
    row.addView(chip, LinearLayout.LayoutParams(ViewGroup.LayoutParams.WRAP_CONTENT, ViewGroup.LayoutParams.WRAP_CONTENT).also { it.leftMargin = ui.px(12) })
}

/** A small outlined button for a bar: [label] in the text colour, 36 dp high. */
fun Ui.barButton(label: String, onClick: () -> Unit): Button = secondary(label, onClick).apply {
    minHeight = px(36)
    minimumHeight = px(36)
    textSize = 14f
    setPadding(px(14), 0, px(14), 0)
    setTypeface(Typeface.DEFAULT_BOLD, Typeface.NORMAL)
}

/** A button on an outcome band: a white fill with the band's colour as text for the good state, else outlined in white at 70 %. */
fun Ui.onBand(label: String, filled: Boolean, band: Int, onClick: () -> Unit): Button {
    val b = if (filled) primary(label) { onClick() } else secondary(label) { onClick() }
    val radius = 14f
    val fill = if (filled) {
        roundedFill(Palette.WHITE, radius)
    } else {
        roundedFill(Color.TRANSPARENT, radius, 1.5f, 0xB3FFFFFF.toInt())
    }
    b.setTextColor(if (filled) band else Palette.WHITE)
    b.background = rippleOver(fill, radius)
    return b
}

/**
 * Colours a page made by [Ui.page] with a bar of `onBlack = true` as the whole of an outcome screen: the band
 * behind the status bar, the bar and the footer, no divider, the title in the band's second-line colour.
 * The navigation bar's icons are set light afterwards, because the page sets them for the page's own background.
 */
fun Ui.paintBand(page: View, bar: View, band: OutcomeBand, window: android.view.Window?) {
    page.setBackgroundColor(band.band)
    bar.setBackgroundColor(band.band)
    val root = page as? LinearLayout
    if (root != null && root.childCount > 2) {
        val footer = root.getChildAt(2) as? LinearLayout
        footer?.setBackgroundColor(Color.TRANSPARENT)
        footer?.getChildAt(0)?.visibility = View.GONE
    }
    val title = ((bar as? BarView)?.row?.getChildAt(0) as? LinearLayout)?.getChildAt(0) as? TextView
    title?.setTextColor(band.sub)
    if (window != null) {
        page.addOnLayoutChangeListener { _, _, _, _, _, _, _, _, _ -> lightNavigation(page, window) }
        page.addOnAttachStateChangeListener(
            object : View.OnAttachStateChangeListener {
                override fun onViewAttachedToWindow(v: View) {
                    v.post { lightNavigation(v, window) }
                }

                override fun onViewDetachedFromWindow(v: View) {}
            },
        )
    }
}

/** The navigation bar's icons light, for a screen that is dark to the bottom edge. */
@Suppress("DEPRECATION")
private fun lightNavigation(view: View, window: android.view.Window) {
    if (android.os.Build.VERSION.SDK_INT >= 30) {
        window.insetsController?.setSystemBarsAppearance(0, android.view.WindowInsetsController.APPEARANCE_LIGHT_NAVIGATION_BARS)
    } else {
        window.decorView.systemUiVisibility = window.decorView.systemUiVisibility and View.SYSTEM_UI_FLAG_LIGHT_NAVIGATION_BAR.inv()
    }
}

/** A line of home: its words, a key that says whether a tap has opened it, and what the "Try again" button of the line does (null for none). */
class HomeLine(val key: String, val words: SayWords, val extra: String? = null, val retry: (() -> Unit)? = null)

/**
 * The health card: one surface card with the heading and a tone dot, the what under it, and, when the tone is
 * not good, who does the next step and the next line.
 */
fun Ui.healthCard(words: SayWords): View {
    val model = WordsModel.of(words)
    val tone = pal.of(model.colour)
    val card = surfaceCard()
    val row = LinearLayout(context).apply {
        orientation = LinearLayout.HORIZONTAL
        setPadding(px(16), px(16), px(16), px(16))
    }
    row.addView(dot(tone.band, 10), LinearLayout.LayoutParams(px(10), px(10)).also {
        it.rightMargin = px(12)
        it.topMargin = px(6)
    })
    val col = LinearLayout(context).apply { orientation = LinearLayout.VERTICAL }
    col.addView(text(model.heading, 15f, bold = true))
    col.addView(text(model.what, 14f, colour = pal.secondary), matchWide().also { it.topMargin = px(4) })
    if (words.tone != Tones.GOOD) {
        col.addView(text(model.next, 14f, medium = true), matchWide().also { it.topMargin = px(8) })
        col.addView(text(model.who, 13f, colour = pal.secondary, medium = true), matchWide().also { it.topMargin = px(4) })
    }
    row.addView(col, LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f))
    card.addView(row, matchWide())
    return card
}

/**
 * The other lines of home in one surface card: each shows its heading with a tone dot. A line that is good shows
 * only its heading, and a tap shows its what and its next; any other line shows its words at once, with its
 * "Try again" button under it. [open] holds the keys of the lines a tap has opened.
 */
fun Ui.homeLines(lines: List<HomeLine>, open: MutableSet<String>): View {
    val card = surfaceCard()
    for ((i, l) in lines.withIndex()) {
        if (i > 0) card.addView(line())
        val model = WordsModel.of(l.words)
        val tone = pal.of(model.colour)
        val good = l.words.tone == Tones.GOOD
        val box = LinearLayout(context).apply { orientation = LinearLayout.VERTICAL }
        val row = LinearLayout(context).apply {
            orientation = LinearLayout.HORIZONTAL
            setPadding(px(16), px(14), px(16), px(14))
        }
        row.addView(dot(tone.band, 10), LinearLayout.LayoutParams(px(10), px(10)).also {
            it.rightMargin = px(12)
            it.topMargin = px(6)
        })
        val col = LinearLayout(context).apply { orientation = LinearLayout.VERTICAL }
        col.addView(text(model.heading, 15f, bold = true))
        val detail = LinearLayout(context).apply { orientation = LinearLayout.VERTICAL }
        detail.addView(text(model.what, 14f, colour = pal.secondary), matchWide().also { it.topMargin = px(4) })
        detail.addView(text(model.next, 14f, medium = true), matchWide().also { it.topMargin = px(8) })
        if (!good) {
            detail.addView(text(model.who, 13f, colour = pal.secondary, medium = true), matchWide().also { it.topMargin = px(4) })
        }
        l.extra?.let { detail.addView(text(it, 13f, mono = true, colour = pal.secondary), matchWide().also { p -> p.topMargin = px(8) }) }
        detail.visibility = if (!good || l.key in open) View.VISIBLE else View.GONE
        col.addView(detail, matchWide())
        row.addView(col, LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f))
        if (good) {
            row.isClickable = true
            row.isFocusable = true
            row.background = rippleOver(null, 0f)
            row.setOnClickListener {
                val show = detail.visibility != View.VISIBLE
                if (show) open.add(l.key) else open.remove(l.key)
                detail.visibility = if (show) View.VISIBLE else View.GONE
            }
        }
        box.addView(row, matchWide())
        val retry = l.retry
        if (retry != null && !good) {
            box.addView(
                secondary("Try again") { retry() },
                matchWide().also { it.setMargins(px(16), 0, px(16), px(14)) },
            )
        }
        card.addView(box, matchWide())
    }
    return card
}

/** A tile of home: [label] at the bottom and [glyph] at the top. [brand] makes it the brand's fill, 104 dp high. */
fun Ui.tile(label: String, glyph: Glyph, brand: Boolean, onClick: () -> Unit): View {
    val ink = if (brand) Palette.ON_BRAND else pal.text
    val fill = if (brand) roundedFill(Palette.BRAND, 18f) else roundedFill(pal.surface, 18f, 1f, pal.divider)
    return FrameLayout(context).apply {
        background = if (brand) fillRipple(fill, 18f) else rippleOver(fill, 18f)
        setPadding(px(16), px(16), px(16), px(16))
        isClickable = true
        isFocusable = true
        contentDescription = label
        filterTouchesWhenObscured = true
        setOnClickListener { onClick() }
        addView(glyph(glyph, ink, if (brand) 28 else 24, if (brand) 2.2f else 2f), FrameLayout.LayoutParams(px(if (brand) 28 else 24), px(if (brand) 28 else 24), Gravity.TOP or Gravity.START))
        addView(
            text(label, if (brand) 20f else 18f, bold = brand, medium = !brand, colour = ink),
            FrameLayout.LayoutParams(ViewGroup.LayoutParams.WRAP_CONTENT, ViewGroup.LayoutParams.WRAP_CONTENT, Gravity.BOTTOM or Gravity.START),
        )
    }
}

private fun Ui.fillRipple(fill: Drawable, radius: Float): Drawable {
    val mask = GradientDrawable().apply {
        cornerRadius = radius * context.resources.displayMetrics.density
        setColor(Color.WHITE)
    }
    return RippleDrawable(ColorStateList.valueOf(0x40000000), fill, mask)
}

/**
 * The keys of the amount of a new sale: 1 to 9, then C, 0 and the delete key, as 64 dp round surface keys with a
 * light haptic tick. [press] is given the key's face: a digit, "C" or "⌫".
 */
fun Ui.amountPad(press: (String) -> Unit): View {
    val faces = listOf("1", "2", "3", "4", "5", "6", "7", "8", "9", "C", "0", "⌫")
    val grid = LinearLayout(context).apply {
        orientation = LinearLayout.VERTICAL
        gravity = Gravity.CENTER_HORIZONTAL
        filterTouchesWhenObscured = true
    }
    for (r in 0 until 4) {
        val row = LinearLayout(context).apply { gravity = Gravity.CENTER }
        for (c in 0 until 3) {
            val face = faces[r * 3 + c]
            val key = TextView(context).apply {
                gravity = Gravity.CENTER
                textSize = 26f
                setTextColor(pal.text)
                typeface = Typeface.create("sans-serif-medium", Typeface.NORMAL)
                text = face
                if (face == "⌫") contentDescription = "Delete one digit"
                background = rippleOver(
                    GradientDrawable().apply {
                        shape = GradientDrawable.OVAL
                        setColor(pal.surface)
                        setStroke(pxf(1f), pal.divider)
                    },
                    32f,
                )
                isClickable = true
                isFocusable = true
                filterTouchesWhenObscured = true
            }
            key.setOnClickListener {
                Feedback.haptic(key, Haptic.Tick)
                press(face)
            }
            row.addView(key, LinearLayout.LayoutParams(px(64), px(64)).also { it.setMargins(px(10), px(5), px(10), px(5)) })
        }
        grid.addView(row, LinearLayout.LayoutParams(ViewGroup.LayoutParams.WRAP_CONTENT, ViewGroup.LayoutParams.WRAP_CONTENT))
    }
    return grid
}

/** A card with the number of a total on the right, for the rows of Today that need a look: a tone dot, the title, and what sits under it. */
fun Ui.needRow(
    colour: Int,
    title: String,
    sub: String?,
    link: View?,
    count: String,
    amount: String?,
    onClick: (() -> Unit)?,
): View {
    val row = LinearLayout(context).apply {
        orientation = LinearLayout.HORIZONTAL
        gravity = Gravity.CENTER_VERTICAL
        minimumHeight = px(56)
        setPadding(px(16), px(10), px(12), px(10))
    }
    row.addView(dot(colour, 10), LinearLayout.LayoutParams(px(10), px(10)).also { it.rightMargin = px(12) })
    val col = LinearLayout(context).apply { orientation = LinearLayout.VERTICAL }
    col.addView(text(title, 16f, medium = true))
    if (sub != null) col.addView(text(sub, 13f, colour = pal.secondary))
    if (link != null) col.addView(link)
    row.addView(col, LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f))
    val right = LinearLayout(context).apply {
        orientation = LinearLayout.VERTICAL
        gravity = Gravity.END
    }
    right.addView(text(count, 16f, bold = true).also {
        it.fontFeatureSettings = "tnum"
        it.gravity = Gravity.END
    })
    if (amount != null) {
        right.addView(text(amount, 13f, colour = pal.secondary).also {
            it.fontFeatureSettings = "tnum"
            it.gravity = Gravity.END
        })
    }
    row.addView(right, LinearLayout.LayoutParams(ViewGroup.LayoutParams.WRAP_CONTENT, ViewGroup.LayoutParams.WRAP_CONTENT).also { it.leftMargin = px(8) })
    if (onClick != null) {
        row.isClickable = true
        row.isFocusable = true
        row.background = rippleOver(null, 0f)
        row.setOnClickListener { onClick() }
    }
    return row
}

/** A column of the body of a page that takes the height that is left, for the scanner. */
class FillsFrame(context: Context) : FrameLayout(context), Fills

/**
 * The camera's surface, scaled to cover the whole of the layout and centred, whatever the shape of the body: the
 * preview keeps the shape of the sensor ([aspectW] by [aspectH]) and what runs over the edges is cut.
 */
class CoverLayout(context: Context, private val aspectW: Int, private val aspectH: Int) : FrameLayout(context) {
    init {
        clipChildren = true
    }

    override fun onMeasure(widthSpec: Int, heightSpec: Int) {
        val w = MeasureSpec.getSize(widthSpec)
        val h = MeasureSpec.getSize(heightSpec)
        setMeasuredDimension(w, h)
        if (aspectW <= 0 || aspectH <= 0) return
        val scale = maxOf(w.toFloat() / aspectW, h.toFloat() / aspectH)
        val cw = (aspectW * scale).toInt()
        val ch = (aspectH * scale).toInt()
        for (i in 0 until childCount) {
            getChildAt(i).measure(MeasureSpec.makeMeasureSpec(cw, MeasureSpec.EXACTLY), MeasureSpec.makeMeasureSpec(ch, MeasureSpec.EXACTLY))
        }
    }

    override fun onLayout(changed: Boolean, l: Int, t: Int, r: Int, b: Int) {
        val w = r - l
        val h = b - t
        for (i in 0 until childCount) {
            val c = getChildAt(i)
            val cw = c.measuredWidth
            val ch = c.measuredHeight
            c.layout((w - cw) / 2, (h - ch) / 2, (w + cw) / 2, (h + ch) / 2)
        }
    }
}
