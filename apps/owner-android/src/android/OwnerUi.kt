package org.openbmt.owner.app

import android.content.Context
import android.content.res.ColorStateList
import android.graphics.Canvas
import android.graphics.ColorFilter
import android.graphics.Paint
import android.graphics.Path
import android.graphics.PixelFormat
import android.graphics.RectF
import android.graphics.drawable.Drawable
import android.graphics.drawable.GradientDrawable
import android.graphics.drawable.RippleDrawable
import android.text.InputType
import android.view.Gravity
import android.view.View
import android.view.ViewGroup
import android.view.SurfaceView
import android.view.ViewOutlineProvider
import android.view.inputmethod.EditorInfo
import android.widget.Button
import android.widget.EditText
import android.widget.FrameLayout
import android.widget.ImageView
import android.widget.LinearLayout
import android.widget.TextView
import org.openbmt.app.Colour
import org.openbmt.app.Fills
import org.openbmt.app.Feedback
import org.openbmt.app.Icon
import org.openbmt.app.IconDrawable
import org.openbmt.app.Palette
import org.openbmt.app.SayWords
import org.openbmt.app.Tone
import org.openbmt.app.Ui
import org.openbmt.app.WordsModel

// The new look of the owner app, in one place: the pieces the screens are made of that the shared code does not
// have (the home card, the group rows, the folded settings, the scanner's page) and the few helpers every screen
// uses to put its bar, its body and its footer on the page. Every word on a screen is still the app's own.

/** One words on a page, as a message in the body: a tinted notice, the next step under it, and the folds. */
fun Ui.words(w: SayWords): View = inline(WordsModel.of(w))

/** The lines of a [WordsModel] that [Ui.notice] and [Ui.hint] do not show: the quoted text and the "Details" fold. */
fun Ui.foldViews(model: WordsModel): List<View> {
    val out = ArrayList<View>()
    if (model.quotedText != null) out.add(quoted(model.quotedLabel ?: "", model.quotedText))
    if (model.details.isNotEmpty()) {
        val fold = text(model.details.joinToString("\n"), 13f, mono = true).apply { visibility = View.GONE }
        val toggle = text("${WordsModel.DETAILS_TITLE} (show)", 14f, bold = true, colour = pal.info.ink)
        toggle.minimumHeight = px(48)
        toggle.gravity = Gravity.CENTER_VERTICAL
        toggle.setOnClickListener {
            val open = fold.visibility != View.VISIBLE
            fold.visibility = if (open) View.VISIBLE else View.GONE
            toggle.text = "${WordsModel.DETAILS_TITLE} (${if (open) "hide" else "show"})"
        }
        out.add(toggle)
        out.add(fold)
    }
    return out
}

/** [model] as a message inside a page: the notice, the next step and who under it, then the folds. */
fun Ui.inline(model: WordsModel): View {
    val notice = notice(model)
    val col = Slot(notice.context, 10)
    col.show(*(listOf(notice, hint(model)) + foldViews(model)).toTypedArray())
    return col
}

/** A field for a whole number: digits only, no suggestions and no learning. */
fun numberField(context: Context, hint: String): EditText = EditText(context).apply {
    this.hint = hint
    inputType = InputType.TYPE_CLASS_NUMBER
    imeOptions = EditorInfo.IME_FLAG_NO_PERSONALIZED_LEARNING or EditorInfo.IME_FLAG_NO_EXTRACT_UI
    filterTouchesWhenObscured = true
    Ui(context).style(this)
}

/** A field for a secret pasted once: its text is hidden, with no suggestions and no learning. */
fun secretField(context: Context, hint: String): EditText = EditText(context).apply {
    this.hint = hint
    inputType = InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_VARIATION_PASSWORD or InputType.TYPE_TEXT_FLAG_NO_SUGGESTIONS
    imeOptions = EditorInfo.IME_FLAG_NO_PERSONALIZED_LEARNING or EditorInfo.IME_FLAG_NO_EXTRACT_UI
    filterTouchesWhenObscured = true
    Ui(context).style(this)
}

/** A container whose content is replaced as a screen updates, so that a field elsewhere keeps its focus. The views are [gapDp] apart. */
class Slot(context: Context, private val gapDp: Int = 12) : LinearLayout(context) {
    init {
        orientation = VERTICAL
    }

    fun show(vararg views: View) {
        removeAllViews()
        val gap = (gapDp * resources.displayMetrics.density).toInt()
        for ((i, v) in views.withIndex()) {
            addView(v, LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT).also { if (i > 0) it.topMargin = gap })
        }
    }
}

/** [place] on the left and [end] on the right of one line; when both do not fit, [end] goes under [place], at the left. */
class PlaceEnd(context: Context, private val place: View, private val end: View, private val gapPx: Int) : ViewGroup(context) {
    private var wrapped = false

    init {
        addView(place)
        addView(end)
    }

    override fun onMeasure(widthSpec: Int, heightSpec: Int) {
        val width = MeasureSpec.getSize(widthSpec)
        val any = MeasureSpec.makeMeasureSpec(0, MeasureSpec.UNSPECIFIED)
        place.measure(any, any)
        end.measure(any, any)
        wrapped = place.measuredWidth + gapPx + end.measuredWidth > width
        val height = if (wrapped) {
            place.measure(MeasureSpec.makeMeasureSpec(width, MeasureSpec.AT_MOST), any)
            end.measure(MeasureSpec.makeMeasureSpec(width, MeasureSpec.AT_MOST), any)
            place.measuredHeight + gapPx / 2 + end.measuredHeight
        } else {
            maxOf(place.measuredHeight, end.measuredHeight)
        }
        setMeasuredDimension(width, height)
    }

    override fun onLayout(changed: Boolean, l: Int, t: Int, r: Int, b: Int) {
        val width = r - l
        if (wrapped) {
            place.layout(0, 0, place.measuredWidth, place.measuredHeight)
            val top = place.measuredHeight + gapPx / 2
            end.layout(0, top, end.measuredWidth, top + end.measuredHeight)
        } else {
            val height = b - t
            place.layout(0, (height - place.measuredHeight) / 2, place.measuredWidth, (height + place.measuredHeight) / 2)
            end.layout(width - end.measuredWidth, (height - end.measuredHeight) / 2, width, (height + end.measuredHeight) / 2)
        }
    }
}

/** A small pill with [text] in the ink of the warn tone on its tint, edged with its band: the palette checks that ink only on the tint. */
internal fun Controller.endChip(text: String): TextView {
    val warn = ui.pal.warn
    return ui.text(text, 13f, bold = true, colour = warn.ink).also {
        it.setPadding(ui.px(10), ui.px(3), ui.px(10), ui.px(3))
        it.background = GradientDrawable().apply {
            cornerRadius = ui.px(13).toFloat()
            setColor(warn.tint)
            setStroke(ui.px(1), warn.band)
        }
    }
}

// ---- pages ---------------------------------------------------------------------------------------------

/** [views] one under the other, [gap] dp apart: a block of a page that belongs together (a label and its lines). */
internal fun Controller.stack(vararg views: View, gap: Int = 8): View = Slot(act, gap).also { it.show(*views) }

/** A line of text in the body: [muted] for a small line in secondary. */
internal fun Controller.line(text: String, muted: Boolean = false, mono: Boolean = false, bold: Boolean = false): TextView =
    ui.text(text, if (muted) 13f else 15f, bold = bold, mono = mono, colour = if (muted) ui.pal.secondary else null)

/** A paragraph of the body: 15 sp in secondary, as the words under a heading. */
internal fun Controller.para(text: String): TextView = ui.text(text, 15f, colour = ui.pal.secondary)

/**
 * A page of the new look with a bar of [title] (a back arrow when [back] is given, and [sub] under the title),
 * the [body] scrolling and the [actions] fixed at the bottom. [words] are the screen's own words: its heading
 * (when it is not the bar's title) and what come first (after [lead], when there is one), the next step and who last, and the folds after them.
 */
internal fun Controller.screen(
    title: String,
    body: List<View>,
    actions: List<Button> = emptyList(),
    back: (() -> Unit)? = null,
    sub: String? = null,
    words: SayWords? = null,
    tone: Tone? = null,
    logo: Boolean = false,
    lead: View? = null,
) {
    val model = words?.let { WordsModel.of(it) }
    val all = ArrayList<View>()
    if (lead != null) all.add(lead)
    if (model != null) all.add(ui.titleBlock(model, title))
    all.addAll(body)
    if (model != null) {
        all.add(ui.hint(model))
        all.addAll(ui.foldViews(model))
    }
    tick = null
    act.show(ui.page(ui.bar(title, sub, back, logo = logo), all, actions, tone))
}

/** A screen that is one message (a result, a refusal, a state): the panel of the words, [extra] under it, the [actions] in the footer. */
internal fun Controller.centre(
    words: SayWords,
    actions: List<Button> = emptyList(),
    extra: List<View> = emptyList(),
    title: String = "Owner",
    back: (() -> Unit)? = null,
    logo: Boolean = back == null && title == "Owner",
) {
    tick = null
    act.show(ui.page(ui.bar(title, back = back, logo = logo), listOf(ui.resultPanel(WordsModel.of(words))) + extra, actions))
}

/** A screen that only waits: the ring, and the words. */
internal fun Controller.waiting(words: SayWords) {
    tick = null
    act.show(ui.page(ui.bar("Owner", logo = true), listOf(ui.working(WordsModel.of(words)))))
}

/** A screen of a code: the words above it, the code in its card, the next step under it, the [actions] in the footer. */
internal fun Controller.codeScreen(
    title: String,
    words: SayWords,
    code: View?,
    below: List<View> = emptyList(),
    actions: List<Button> = emptyList(),
    back: (() -> Unit)? = null,
) {
    val model = WordsModel.of(words)
    val all = ArrayList<View>()
    all.add(ui.compactWords(model))
    if (code != null) all.add(ui.codeCard(code))
    all.addAll(below)
    all.add(ui.hint(model))
    all.addAll(ui.foldViews(model))
    tick = null
    act.show(ui.page(ui.bar(title, back = back), all, actions))
}

/** A labelled field: the view to put on the page, and the field to read. */
internal class Input(val view: View, val field: EditText)

/** A field for a whole number with its [label] above it; the [hint] shows a format only. */
internal fun Controller.numberInput(label: String, hint: String = ""): Input {
    val f = numberField(act, hint)
    return Input(ui.labelled(label, f), f)
}

/** A field for typed text with its [label] above it; the [hint] shows a format only. */
internal fun Controller.textInput(label: String, hint: String = "", lines: Int = 1, mono: Boolean = false): Input {
    val f = ui.field(hint, lines, mono)
    return Input(ui.labelled(label, f), f)
}

/** [view] in the middle of the page. */
internal fun Controller.centred(view: View): View = LinearLayout(act).apply {
    gravity = Gravity.CENTER_HORIZONTAL
    addView(view)
}

/** The four check words of a line "one  two  three  four" as the cards of the new look. */
internal fun Controller.wordCards(line: String): View = ui.wordCards(line.trim().split(Regex("\\s+")).filter { it.isNotEmpty() })

/** A big amount of text that is read across a table: an address, a key. */
internal fun Controller.big(text: String): TextView = ui.text(text, 20f, bold = true, mono = true)

/** The footer's buttons: [Button] is what [Ui.page] takes. */
internal fun Controller.backAction(label: String = "Back", to: () -> Unit): Button = ui.button(label) { to() }

// ---- the scanner ------------------------------------------------------------------------------------------

/**
 * The page the shared scan puts on the screen, made over in the new look: a black bar, the preview on black with
 * the frame, the words over the bottom, the progress as a pill and Cancel in the footer. The shared scan builds a
 * column (the title, the hint, the progress, the preview and Cancel); this takes its parts out of it. A column of
 * another shape is shown as it is.
 */
internal fun Controller.showScan(column: View) {
    tick = null
    val col = column as? LinearLayout
    val title = col?.getChildAt(0) as? TextView
    val hint = col?.getChildAt(1) as? TextView
    val status = col?.getChildAt(2) as? LinearLayout
    val cancel = col?.getChildAt((col?.childCount ?: 1) - 1) as? Button
    val preview = col?.children()?.firstOrNull { it is SurfaceView }
    if (col == null || title == null || hint == null || status == null || cancel == null) {
        act.show(column)
        return
    }
    val size = preview?.layoutParams
    col.removeView(cancel)
    if (preview != null) col.removeView(preview)
    col.removeView(status)
    val frame = ui.cameraFrame(preview)
    if (preview != null && size != null) {
        preview.layoutParams = FrameLayout.LayoutParams(size.width, size.height, Gravity.CENTER)
    }
    frame.words(WordsModel(Colour.Blue, title.text.toString(), hint.text.toString(), "", "", null, null, emptyList()))
    // The shared scan writes its progress into [status] (words and a line); the pill shows what it holds.
    status.visibility = View.GONE
    frame.addView(status, FrameLayout.LayoutParams(1, 1))
    status.setOnHierarchyChangeListener(
        object : ViewGroup.OnHierarchyChangeListener {
            override fun onChildViewAdded(parent: View?, child: View?) = refresh()

            override fun onChildViewRemoved(parent: View?, child: View?) = refresh()

            private fun refresh() {
                frame.post { frame.progress(texts(status).takeIf { it.isNotEmpty() }) }
            }
        },
    )
    val holder = object : FrameLayout(act), Fills {}
    holder.addView(frame, FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT))
    act.show(ui.page(ui.bar("Scan the code", onBlack = true), listOf(holder), listOf(cancel), scroll = false))
}

private fun LinearLayout.children(): List<View> = (0 until childCount).map { getChildAt(it) }

/** The texts under [view], the empty ones left out, one to a line. */
private fun texts(view: View): String {
    val out = ArrayList<String>()
    fun walk(v: View) {
        if (v is TextView) {
            val t = v.text.toString()
            if (t.isNotBlank()) out.add(t)
        } else if (v is ViewGroup) {
            for (i in 0 until v.childCount) walk(v.getChildAt(i))
        }
    }
    walk(view)
    return out.joinToString("\n")
}

// ---- icons that the shared code does not have --------------------------------------------------------------

/** The icons of the home groups and of a fold, drawn as paths on a 24 by 24 grid. */
class OwnerIcon(private val kind: Kind, colour: Int, private val strokeWidth: Float = 2f) : Drawable() {
    enum class Kind { Money, Devices, Bell, Up, Down }

    private val stroke = Paint(Paint.ANTI_ALIAS_FLAG).apply {
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
        stroke.strokeWidth = strokeWidth
        when (kind) {
            Kind.Money -> {
                canvas.drawRect(RectF(3f, 7f, 21f, 19f), stroke)
                canvas.drawLine(3f, 11f, 21f, 11f, stroke)
                canvas.drawLine(16f, 15f, 18f, 15f, stroke)
            }
            Kind.Devices -> {
                canvas.drawRoundRect(RectF(4f, 3f, 20f, 21f), 2f, 2f, stroke)
                canvas.drawPath(Path().apply { moveTo(8f, 7f); lineTo(16f, 7f); moveTo(8f, 11f); lineTo(16f, 11f); moveTo(8f, 15f); lineTo(13f, 15f) }, stroke)
            }
            Kind.Bell -> {
                canvas.drawPath(
                    Path().apply {
                        moveTo(6f, 8f)
                        arcTo(RectF(6f, 2f, 18f, 14f), 180f, 180f)
                        cubicTo(18f, 15f, 21f, 17f, 21f, 17f)
                        lineTo(3f, 17f)
                        cubicTo(3f, 17f, 6f, 15f, 6f, 8f)
                    },
                    stroke,
                )
                canvas.drawLine(10f, 21f, 14f, 21f, stroke)
            }
            Kind.Up -> canvas.drawPath(Path().apply { moveTo(6f, 15f); lineTo(12f, 9f); lineTo(18f, 15f) }, stroke)
            Kind.Down -> canvas.drawPath(Path().apply { moveTo(6f, 9f); lineTo(12f, 15f); lineTo(18f, 9f) }, stroke)
        }
        canvas.restore()
    }

    override fun setAlpha(alpha: Int) {
        stroke.alpha = alpha
    }

    override fun setColorFilter(colorFilter: ColorFilter?) {
        stroke.colorFilter = colorFilter
    }

    @Suppress("OVERRIDE_DEPRECATION")
    override fun getOpacity(): Int = PixelFormat.TRANSLUCENT
}

// ---- cards -------------------------------------------------------------------------------------------------

private fun Controller.card(radiusDp: Int = 16, fill: Int = ui.pal.surface, line: Int = ui.pal.divider): LinearLayout = LinearLayout(act).apply {
    orientation = LinearLayout.VERTICAL
    val d = act.resources.displayMetrics.density
    background = GradientDrawable().apply {
        setColor(fill)
        cornerRadius = radiusDp * d
        setStroke(maxOf(1, d.toInt()), line)
    }
    clipToOutline = true
    filterTouchesWhenObscured = true
}

private fun Controller.divider(colour: Int = ui.pal.divider): View = View(act).apply {
    setBackgroundColor(colour)
    layoutParams = LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, maxOf(1, act.resources.displayMetrics.density.toInt()))
}

private fun Controller.tap(view: View, onClick: () -> Unit) {
    val tapColour = if (ui.night) 0x22FFFFFF else 0x14000000
    view.isClickable = true
    view.isFocusable = true
    view.foreground = RippleDrawable(ColorStateList.valueOf(tapColour), null, null)
    view.setOnClickListener { onClick() }
}

private fun Controller.icon(drawable: Drawable, dp: Int): ImageView = ImageView(act).apply {
    setImageDrawable(drawable)
    importantForAccessibility = View.IMPORTANT_FOR_ACCESSIBILITY_NO
    layoutParams = LinearLayout.LayoutParams(ui.px(dp), ui.px(dp))
}

/**
 * The pill of the network status: its dot and heading in the tone's ink on its tint. The fill is the pill's
 * background colour, clipped to a round shape, so that [Feedback.crossfade] can change it.
 */
internal fun Controller.statusPill(model: WordsModel): View {
    val tone = ui.pal.of(model.colour)
    val pill = LinearLayout(act).apply {
        orientation = LinearLayout.HORIZONTAL
        gravity = Gravity.CENTER_VERTICAL
        setPadding(ui.px(8), ui.px(5), ui.px(10), ui.px(5))
        setBackgroundColor(tone.tint)
        outlineProvider = object : ViewOutlineProvider() {
            override fun getOutline(view: View, outline: android.graphics.Outline) {
                outline.setRoundRect(0, 0, view.width, view.height, view.height / 2f)
            }
        }
        clipToOutline = true
        tag = tone.tint
    }
    pill.addView(
        View(act).apply { background = GradientDrawable().apply { shape = GradientDrawable.OVAL; setColor(tone.ink) } },
        LinearLayout.LayoutParams(ui.px(8), ui.px(8)).also { it.rightMargin = ui.px(7) },
    )
    pill.addView(ui.text(model.heading, 13f, colour = tone.ink, medium = true))
    return pill
}

/**
 * The hero card of home: the network status as a pill (with its what and next under it when it is not good), the
 * balance, and [extra] under the amount (the recovery and rescan words and buttons, the first-address line).
 * [from] is the tint of the pill the last time home was drawn, or null the first time: a change of tone fades.
 */
internal fun Controller.heroCard(status: WordsModel, balance: OwnerLook.Balance, extra: List<View>, from: Int?): View {
    val card = card(20)
    card.setPadding(ui.px(20), ui.px(18), ui.px(20), ui.px(18))
    val pill = statusPill(status)
    card.addView(pill, LinearLayout.LayoutParams(-2, -2))
    val to = ui.pal.of(status.colour).tint
    if (from != null && from != to) Feedback.crossfade(pill, from, to)
    if (status.colour != Colour.Green) {
        card.addView(ui.text(status.what, 15f, colour = ui.pal.secondary), LinearLayout.LayoutParams(-1, -2).also { it.topMargin = ui.px(10) })
        card.addView(ui.text(status.next, 15f, medium = true), LinearLayout.LayoutParams(-1, -2).also { it.topMargin = ui.px(4) })
    }
    card.addView(ui.text(balance.lead, 14f, colour = ui.pal.secondary), LinearLayout.LayoutParams(-1, -2).also { it.topMargin = ui.px(10) })
    card.addView(
        ui.text(balance.amount, 38f, bold = true).also {
            it.fontFeatureSettings = "tnum"
            it.letterSpacing = -0.015f
        },
    )
    card.addView(ui.text(balance.checked, 13f, colour = ui.pal.secondary).also { it.fontFeatureSettings = "tnum" })
    for (v in extra) card.addView(v, LinearLayout.LayoutParams(-1, -2).also { it.topMargin = ui.px(12) })
    return card
}

/** The tint of the pill in [hero], for the next draw of home to fade from. */
internal fun Controller.pillTint(hero: View): Int? = ((hero as? ViewGroup)?.getChildAt(0))?.tag as? Int

/** The warm card of what waits: a heading, and a row for each of [rows] with its count in a black badge. Null when no row. */
internal fun Controller.waitingCard(rows: List<Triple<String, Int, () -> Unit>>): View? {
    if (rows.isEmpty()) return null
    val warn = ui.pal.warn
    val card = card(16, fill = warn.tint, line = warn.band)
    card.addView(
        ui.label(OwnerLook.WAITING_TITLE).also {
            it.setTextColor(warn.ink)
            it.setPadding(ui.px(16), ui.px(12), ui.px(16), ui.px(8))
        },
    )
    for ((title, count, onClick) in rows) {
        card.addView(divider(warn.band))
        val row = LinearLayout(act).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
            minimumHeight = ui.px(52)
            setPadding(ui.px(16), ui.px(6), ui.px(12), ui.px(6))
        }
        row.addView(ui.text(title, 16f, medium = true), LinearLayout.LayoutParams(0, -2, 1f))
        row.addView(
            ui.text(count.toString(), 14f, bold = true, colour = Palette.WHITE).also {
                it.gravity = Gravity.CENTER
                it.minWidth = ui.px(26)
                it.setPadding(ui.px(8), ui.px(2), ui.px(8), ui.px(2))
                it.background = GradientDrawable().apply {
                    cornerRadius = ui.px(13).toFloat()
                    setColor(if (ui.night) ui.pal.chip else 0xFF111317.toInt())
                }
            },
            LinearLayout.LayoutParams(-2, -2).also { it.leftMargin = ui.px(8) },
        )
        row.addView(icon(IconDrawable(Icon.Chevron, ui.pal.secondary, 2f), 18), LinearLayout.LayoutParams(ui.px(18), ui.px(18)).also { it.leftMargin = ui.px(8) })
        tap(row, onClick)
        card.addView(row)
    }
    return card
}

/** One row of the group card of home: an icon tile, a title, a one-line sub and a chevron. */
internal class GroupRow(val icon: OwnerIcon.Kind?, val gear: Boolean, val title: String, val sub: String, val onClick: () -> Unit)

/** The group card of home: one surface card of rows, each with an icon tile. */
internal fun Controller.groupCard(items: List<GroupRow>): View {
    val card = card(16)
    for ((i, item) in items.withIndex()) {
        if (i > 0) card.addView(divider())
        val row = LinearLayout(act).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
            minimumHeight = ui.px(64)
            setPadding(ui.px(14), ui.px(10), ui.px(12), ui.px(10))
        }
        val tile = FrameLayout(act).apply {
            background = GradientDrawable().apply {
                cornerRadius = ui.px(12).toFloat()
                setColor(ui.pal.background)
            }
        }
        val glyph: Drawable = if (item.gear) IconDrawable(Icon.Gear, ui.pal.text, 2f) else OwnerIcon(item.icon ?: OwnerIcon.Kind.Money, ui.pal.text)
        tile.addView(icon(glyph, 20), FrameLayout.LayoutParams(ui.px(20), ui.px(20), Gravity.CENTER))
        row.addView(tile, LinearLayout.LayoutParams(ui.px(40), ui.px(40)).also { it.rightMargin = ui.px(14) })
        val col = LinearLayout(act).apply { orientation = LinearLayout.VERTICAL }
        col.addView(ui.text(item.title, 16f, medium = true))
        col.addView(ui.text(item.sub, 13f, colour = ui.pal.secondary))
        row.addView(col, LinearLayout.LayoutParams(0, -2, 1f))
        row.addView(icon(IconDrawable(Icon.Chevron, ui.pal.secondary, 2f), 18), LinearLayout.LayoutParams(ui.px(18), ui.px(18)).also { it.leftMargin = ui.px(8) })
        tap(row, item.onClick)
        card.addView(row)
    }
    return card
}

/** The instant sales card of the settings page: a heading, the table, and the tolerance as a key and value line. */
internal fun Controller.instantCard(table: OwnerLook.InstantTable): View {
    val card = card(16)
    val head = LinearLayout(act).apply {
        orientation = LinearLayout.HORIZONTAL
        gravity = Gravity.CENTER_VERTICAL
        minimumHeight = ui.px(56)
        setPadding(ui.px(16), ui.px(8), ui.px(12), ui.px(8))
    }
    head.addView(ui.text(OwnerLook.INSTANT, 16f, bold = true), LinearLayout.LayoutParams(0, -2, 1f))
    card.addView(head)
    fun cells(first: String, second: String, third: String, header: Boolean) {
        card.addView(divider())
        val row = LinearLayout(act).apply {
            orientation = LinearLayout.HORIZONTAL
            setPadding(ui.px(16), ui.px(10), ui.px(16), ui.px(10))
        }
        fun cell(text: String, weight: Float, end: Boolean) {
            val t = if (header) ui.text(text, 13f, bold = true, colour = ui.pal.secondary) else ui.text(text, 15f, medium = !end)
            t.gravity = if (end) Gravity.END else Gravity.START
            t.fontFeatureSettings = "tnum"
            row.addView(t, LinearLayout.LayoutParams(0, -2, weight).also { it.rightMargin = ui.px(8) })
        }
        cell(first, 1.3f, false)
        cell(second, 1f, true)
        cell(third, 1f, true)
        card.addView(row)
    }
    cells("", table.columns[0], table.columns[1], true)
    for ((name, perSale, total) in table.rows) cells(name, perSale, total, false)
    table.tolerance?.let { (key, value) ->
        card.addView(divider())
        val row = LinearLayout(act).apply {
            orientation = LinearLayout.HORIZONTAL
            setPadding(ui.px(16), ui.px(12), ui.px(16), ui.px(12))
        }
        row.addView(ui.text(key, 15f, colour = ui.pal.secondary), LinearLayout.LayoutParams(0, -2, 1f))
        row.addView(ui.text(value, 15f, medium = true).also { it.fontFeatureSettings = "tnum" })
        card.addView(row)
    }
    return card
}

/** The groups of the settings page as folded rows in one card: tapping a row opens its lines as facts. */
internal fun Controller.foldedCard(groups: List<Pair<String, List<Pair<String, String>>>>): View {
    val card = card(16)
    for ((i, g) in groups.withIndex()) {
        if (i > 0) card.addView(divider())
        val (title, lines) = g
        val head = LinearLayout(act).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
            minimumHeight = ui.px(56)
            setPadding(ui.px(16), ui.px(8), ui.px(12), ui.px(8))
        }
        head.addView(ui.text(title, 16f, medium = true), LinearLayout.LayoutParams(0, -2, 1f))
        val chevron = icon(OwnerIcon(OwnerIcon.Kind.Down, ui.pal.secondary), 18)
        head.addView(chevron, LinearLayout.LayoutParams(ui.px(18), ui.px(18)))
        card.addView(head)
        val body = LinearLayout(act).apply {
            orientation = LinearLayout.VERTICAL
            visibility = View.GONE
        }
        body.addView(divider())
        body.addView(ui.facts(lines))
        card.addView(body)
        tap(head) {
            val open = body.visibility != View.VISIBLE
            body.visibility = if (open) View.VISIBLE else View.GONE
            chevron.setImageDrawable(OwnerIcon(if (open) OwnerIcon.Kind.Up else OwnerIcon.Kind.Down, ui.pal.secondary))
        }
    }
    return card
}

/** A secondary action that is a button in the body rather than in the footer. */
internal fun Controller.inlineButton(label: String, onClick: () -> Unit): Button = ui.button(label) { onClick() }
