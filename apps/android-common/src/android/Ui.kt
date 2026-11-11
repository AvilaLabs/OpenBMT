package org.openbmt.app

import android.app.Activity
import android.content.Context
import android.content.ContextWrapper
import android.content.res.ColorStateList
import android.content.res.Configuration
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Paint
import android.graphics.RectF
import android.graphics.Typeface
import android.graphics.drawable.ColorDrawable
import android.graphics.drawable.Drawable
import android.graphics.drawable.GradientDrawable
import android.graphics.drawable.RippleDrawable
import android.graphics.drawable.StateListDrawable
import android.os.Build
import android.text.InputType
import android.view.Gravity
import android.view.View
import android.view.ViewGroup
import android.view.ViewOutlineProvider
import android.view.WindowInsets
import android.view.WindowInsetsController
import android.view.inputmethod.EditorInfo
import android.widget.Button
import android.widget.EditText
import android.widget.FrameLayout
import android.widget.ImageView
import android.widget.LinearLayout
import android.widget.RadioButton
import android.widget.RadioGroup
import android.widget.ScrollView
import android.widget.TextView

/** True when [c] is the phone's night mode. */
fun isNight(c: Configuration): Boolean =
    (c.uiMode and Configuration.UI_MODE_NIGHT_MASK) == Configuration.UI_MODE_NIGHT_YES

/**
 * After a change between light and dark with the activity kept: the window's own colour is set again from the
 * new palette (the theme's was read when the window was made), the bars stay transparent, and the decor is asked
 * for its insets once more. The caller draws its current screen again, whose page sets the bar icons.
 */
@Suppress("DEPRECATION")
fun Activity.applyNight() {
    val pal = Palette.of(isNight(resources.configuration))
    window.setBackgroundDrawable(ColorDrawable(pal.background))
    window.statusBarColor = Color.TRANSPARENT
    window.navigationBarColor = Color.TRANSPARENT
    window.decorView.requestApplyInsets()
}

/**
 * A view that is the first of a page and reaches up under the status bar in its own colour: the page tells
 * it how tall the bar is, and it adds that to its own top padding.
 */
interface Bleed {
    fun setTopInset(px: Int)
}

/**
 * One `Words` on screen: a coloured band with the heading, the two
 * sentences, who does the next step, quoted text marked as from elsewhere, and
 * a "Details" fold that holds the rule, the time and the detail. As the first
 * view of a page the band runs the full width and under the status bar; anywhere
 * else it sits inside the page's gutters. With [showBand] false the heading is
 * left out, for a screen whose own band already says it.
 */
class WordsView(context: Context, model: WordsModel, showBand: Boolean = true) : LinearLayout(context), Bleed {
    private val ui = Ui(context)
    private val band = LinearLayout(context)
    private val body = LinearLayout(context)
    private val shape = GradientDrawable()

    init {
        orientation = VERTICAL
        band.orientation = VERTICAL
        shape.setColor(model.colour.band(ui.night))
        shape.cornerRadius = ui.px(Space.RADIUS).toFloat()
        band.background = shape
        band.setPadding(ui.px(Space.GUTTER), ui.px(Space.BLOCK), ui.px(Space.GUTTER), ui.px(Space.BLOCK))
        band.addView(ui.text(model.heading, Type.HEADING, bold = true, colour = Palette.WHITE))
        if (showBand) addView(band)
        body.orientation = VERTICAL
        body.addView(ui.text(model.what, Type.BODY).also { it.setPadding(0, ui.px(Space.BLOCK), 0, 0) })
        body.addView(ui.text(model.next, Type.BODY, bold = true).also { it.setPadding(0, ui.px(8), 0, 0) })
        body.addView(ui.text(model.who, Type.SECONDARY, colour = ui.pal.secondary).also { it.setPadding(0, ui.px(4), 0, 0) })
        if (model.quotedText != null) {
            body.addView(ui.quoted(model.quotedLabel ?: "", model.quotedText), ui.margins(top = Space.BLOCK))
        }
        if (model.details.isNotEmpty()) {
            val fold = ui.text(model.details.joinToString("\n"), Type.CODE - 2f, mono = true).apply { visibility = GONE }
            val toggle = ui.text("${WordsModel.DETAILS_TITLE} (show)", Type.SECONDARY, bold = true, colour = ui.pal.info.ink)
            toggle.minimumHeight = ui.px(Space.TOUCH)
            toggle.gravity = Gravity.CENTER_VERTICAL
            toggle.setOnClickListener {
                val open = fold.visibility != VISIBLE
                fold.visibility = if (open) VISIBLE else GONE
                toggle.text = "${WordsModel.DETAILS_TITLE} (${if (open) "hide" else "show"})"
            }
            body.addView(toggle)
            body.addView(fold)
        }
        addView(body)
        filterTouchesWhenObscured = true
    }

    override fun setTopInset(px: Int) {
        shape.cornerRadius = 0f
        band.setPadding(ui.px(Space.GUTTER), ui.px(Space.BLOCK) + px, ui.px(Space.GUTTER), ui.px(Space.BLOCK))
        body.setPadding(ui.px(Space.GUTTER), 0, ui.px(Space.GUTTER), 0)
    }
}

/**
 * The top of a counter's sale screen: a band in the colour of the state that fills a third of the height,
 * with the state in large type and the amount at 44 sp under it, so that it reads across a counter. The
 * state's own colour is the words' (stop is red with white text; waiting is never red or amber).
 */
class SaleBand(context: Context) : LinearLayout(context), Bleed {
    private val ui = Ui(context)
    private val heading = ui.text("", Type.STATE, bold = true, colour = Palette.WHITE)
    private val amount = ui.text("", Type.AMOUNT, bold = true, colour = Palette.WHITE)
    private val line = ui.text("", Type.BODY, colour = Palette.WHITE)
    private var inset = 0

    init {
        orientation = VERTICAL
        gravity = Gravity.CENTER_VERTICAL
        minimumHeight = context.resources.displayMetrics.heightPixels / 3
        addView(heading)
        addView(amount)
        addView(line)
        setBackgroundColor(Colour.Neutral.band(ui.night))
        setTopInset(0)
        filterTouchesWhenObscured = true
    }

    /** The state as the words give it, the amount, and the line under the amount (empty for none). */
    fun show(model: WordsModel, amountText: String, lineText: String) {
        setBackgroundColor(model.colour.band(ui.night))
        heading.text = model.heading
        amount.text = amountText
        line.text = lineText
        line.visibility = if (lineText.isEmpty()) GONE else VISIBLE
    }

    override fun setTopInset(px: Int) {
        inset = px
        setPadding(ui.px(Space.GUTTER), inset + ui.px(Space.BLOCK), ui.px(Space.GUTTER), ui.px(Space.BLOCK))
    }
}

/** A button that shows when it is off: faded. */
private class UiButton(context: Context) : Button(context) {
    override fun setEnabled(enabled: Boolean) {
        super.setEnabled(enabled)
        alpha = if (enabled) 1f else 0.4f
    }
}

/** What a button is for: the one main action of a screen, a quieter one, or one that destroys something. */
enum class ButtonKind { Primary, Secondary, Danger, Good }

/** Plain `android.widget` views made in code, in the colours, sizes and distances of [Palette], [Type] and [Space]. */
class Ui(internal val context: Context) {
    private val density = context.resources.displayMetrics.density

    /** True when the phone is in night mode: every colour is chosen from the dark scheme. Read each time, so that a change of mode is seen by the next view made. */
    val night: Boolean get() = isNight(context.resources.configuration)

    /** The colours of the phone's current mode. */
    val pal: Scheme get() = Palette.of(night)

    fun px(dp: Int): Int = (dp * density).toInt()

    fun pxf(dp: Float): Int = (dp * density).toInt().coerceAtLeast(1)

    fun margins(top: Int = 0, bottom: Int = 0): LinearLayout.LayoutParams =
        LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT)
            .also { it.setMargins(0, px(top), 0, px(bottom)) }

    fun text(
        text: String,
        size: Float = Type.BODY,
        bold: Boolean = false,
        mono: Boolean = false,
        colour: Int? = null,
        medium: Boolean = false,
    ): TextView = TextView(context).apply {
        this.text = text
        textSize = size
        setTextColor(colour ?: pal.text)
        if (medium) typeface = Typeface.create("sans-serif-medium", Typeface.NORMAL)
        if (bold) setTypeface(typeface, Typeface.BOLD)
        if (mono) typeface = if (bold) Typeface.create(Typeface.MONOSPACE, Typeface.BOLD) else Typeface.MONOSPACE
        filterTouchesWhenObscured = true
    }

    /** Text from elsewhere: a marking line, then the text in quotation marks (SC-08). */
    fun quoted(label: String, quotedText: String): View = LinearLayout(context).apply {
        orientation = LinearLayout.VERTICAL
        background = GradientDrawable().apply {
            setColor(pal.surface)
            cornerRadius = px(8).toFloat()
        }
        setPadding(px(Space.BLOCK), px(8), px(Space.BLOCK), px(8))
        addView(text(label, Type.SECONDARY, colour = pal.secondary))
        addView(text(quotedText, Type.CODE, mono = true))
    }

    fun words(model: WordsModel): View = WordsView(context, model)

    /** The words without their heading band, for a screen whose own band says the state. */
    fun wordsBelowBand(model: WordsModel): View = WordsView(context, model, showBand = false)

    /** A button of the given [kind]: for a screen whose main action depends on what it shows. */
    fun button(label: String, kind: ButtonKind, onClick: () -> Unit): Button = when (kind) {
        ButtonKind.Primary -> primary(label, Colour.Blue, onClick)
        ButtonKind.Secondary -> secondary(label, onClick)
        ButtonKind.Danger -> danger(label, onClick)
        ButtonKind.Good -> good(label, onClick)
    }

    /** A secondary button: outlined. */
    fun button(label: String, onClick: () -> Unit): Button = secondary(label, onClick)

    /**
     * The screen's main action: the brand fill with [Palette.ON_BRAND] bold text, 56 dp high.
     * [colour] is ignored (deprecated): the primary button is always the brand's.
     */
    fun primary(label: String, colour: Colour = Colour.Blue, onClick: () -> Unit): Button =
        styled(label, ButtonKind.Primary, pal.of(colour), onClick)

    /** A quieter action: no fill, a 1 dp outline, 52 dp high. */
    fun secondary(label: String, onClick: () -> Unit): Button = styled(label, ButtonKind.Secondary, pal.info, onClick)

    /** An action that wipes, removes, forgets, declines or cancels: outlined in the stop colour. */
    fun danger(label: String, onClick: () -> Unit): Button = styled(label, ButtonKind.Danger, pal.stop, onClick)

    /** An action that hands something over or confirms it: the good band's fill with white bold text, 56 dp high. */
    fun good(label: String, onClick: () -> Unit): Button = styled(label, ButtonKind.Good, pal.good, onClick)

    private fun styled(label: String, kind: ButtonKind, tone: Tone, onClick: () -> Unit): Button = UiButton(context).apply {
        text = label
        isAllCaps = false
        textSize = Type.BODY
        val filled = kind == ButtonKind.Primary || kind == ButtonKind.Good
        setTypeface(Typeface.DEFAULT, if (filled) Typeface.BOLD else Typeface.NORMAL)
        val height = px(if (filled) Space.PRIMARY else Space.SECONDARY)
        minHeight = height
        minimumHeight = height
        minWidth = 0
        minimumWidth = 0
        gravity = Gravity.CENTER
        setPadding(px(Space.GUTTER), 0, px(Space.GUTTER), 0)
        stateListAnimator = null
        elevation = 0f
        val ink = when (kind) {
            ButtonKind.Primary -> Palette.ON_BRAND
            ButtonKind.Good -> Palette.WHITE
            ButtonKind.Secondary -> pal.text
            ButtonKind.Danger -> tone.ink
        }
        setTextColor(ink)
        val fill = GradientDrawable().apply {
            cornerRadius = px(Space.BUTTON_RADIUS).toFloat()
            when (kind) {
                ButtonKind.Primary -> setColor(Palette.BRAND)
                ButtonKind.Good -> setColor(tone.band)
                ButtonKind.Secondary -> {
                    setColor(Color.TRANSPARENT)
                    setStroke(px(1), pal.fieldLine)
                }
                ButtonKind.Danger -> {
                    setColor(Color.TRANSPARENT)
                    setStroke(pxf(1.5f), tone.ink)
                }
            }
        }
        val mask = GradientDrawable().apply {
            cornerRadius = px(Space.BUTTON_RADIUS).toFloat()
            setColor(Color.WHITE)
        }
        // The pressed state: a ripple, black over a filled button, the text's own colour over an outlined one.
        val ripple = if (filled) 0x40000000 else (ink and 0x00FFFFFF) or 0x33000000
        background = RippleDrawable(ColorStateList.valueOf(ripple), fill, mask)
        setOnClickListener { onClick() }
        filterTouchesWhenObscured = true
    }

    /**
     * A field for typed text. No keyboard may suggest, remember or learn what
     * is typed: the type is "visible password without suggestions" and the
     * keyboard is asked not to learn (design: no third-party keyboard for words).
     * The visible-password type makes the font monospace, so the font is set
     * again afterwards: the system sans, or monospace when [mono] says the
     * field takes a code, an address, a key or an invitation.
     */
    fun field(hint: String, lines: Int = 1, mono: Boolean = false): EditText = EditText(context).apply {
        this.hint = hint
        var type = InputType.TYPE_CLASS_TEXT or
            InputType.TYPE_TEXT_FLAG_NO_SUGGESTIONS or
            InputType.TYPE_TEXT_VARIATION_VISIBLE_PASSWORD
        if (lines > 1) type = type or InputType.TYPE_TEXT_FLAG_MULTI_LINE
        inputType = type
        imeOptions = EditorInfo.IME_FLAG_NO_PERSONALIZED_LEARNING or EditorInfo.IME_FLAG_NO_EXTRACT_UI
        if (lines > 1) minLines = lines
        filterTouchesWhenObscured = true
        style(this, mono)
    }

    /**
     * The look of a field: the system sans (or monospace for [mono]) set after the input type, the hint lighter
     * than the text, a surface fill with a 1 dp outline, 12 dp corners, at least 52 dp high. The input type and the
     * flags are the caller's.
     */
    fun style(field: EditText, mono: Boolean = false): EditText = field.apply {
        typeface = if (mono) Typeface.MONOSPACE else Typeface.DEFAULT
        textSize = if (mono) Type.CODE else Type.FIELD
        setTextColor(pal.text)
        setHintTextColor(pal.hint)
        background = GradientDrawable().apply {
            setColor(pal.surface)
            setStroke(px(1), pal.fieldLine)
            cornerRadius = px(Space.RADIUS).toFloat()
        }
        setPadding(px(Space.GUTTER), px(8), px(Space.GUTTER), px(8))
        minHeight = px(Space.FIELD_NEW)
        minimumHeight = px(Space.FIELD_NEW)
        gravity = if (field.minLines > 1) Gravity.TOP or Gravity.START else Gravity.CENTER_VERTICAL or Gravity.START
    }

    fun choice(options: List<String>, selected: Int = 0): RadioGroup = RadioGroup(context).apply {
        orientation = RadioGroup.VERTICAL
        for ((i, label) in options.withIndex()) {
            addView(
                RadioButton(context).apply {
                    text = label
                    id = View.generateViewId()
                    isChecked = i == selected
                    textSize = Type.BODY
                    setTextColor(pal.text)
                    minHeight = px(Space.TOUCH)
                    minimumHeight = px(Space.TOUCH)
                    buttonTintList = ColorStateList(
                        arrayOf(intArrayOf(android.R.attr.state_checked), intArrayOf()),
                        intArrayOf(pal.info.ink, pal.secondary),
                    )
                    filterTouchesWhenObscured = true
                },
            )
        }
    }

    /** The index of the chosen option of [group], or -1. */
    fun chosen(group: RadioGroup): Int {
        val id = group.checkedRadioButtonId
        for (i in 0 until group.childCount) if (group.getChildAt(i).id == id) return i
        return -1
    }

    /**
     * A scrolling page of [views] in a column: the page's colour behind it, a 16 dp gutter on each side and
     * 12 dp between blocks. The root takes the system bars, the display cutout and the keyboard as padding, in
     * this one place, so that nothing interactive sits under a bar; a first view that is a [Bleed] (the heading
     * band of a screen) runs under the status bar in its own colour instead, and the status bar's icons are
     * light or dark to stay readable on it.
     */
    fun page(views: List<View>): View {
        val lead = views.firstOrNull() as? Bleed
        lead?.setTopInset(0)
        val column = LinearLayout(context).apply {
            orientation = LinearLayout.VERTICAL
        }
        for ((i, v) in views.withIndex()) {
            val params = LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT)
            val side = if (i == 0 && lead != null) 0 else px(Space.GUTTER)
            params.setMargins(side, 0, side, px(Space.BLOCK))
            column.addView(v, params)
        }
        val topBase = if (lead != null) 0 else px(24)
        val bottomBase = px(24)
        column.setPadding(0, topBase, 0, bottomBase)
        val appearance = edgeToEdge(lead != null)
        val place = { insets: WindowInsets ->
            val bars = barInsets(insets)
            // A first view that reaches under the status bar takes the bar's height itself.
            column.setPadding(bars[0], if (lead != null) 0 else topBase + bars[1], bars[2], bottomBase + bars[3])
            lead?.setTopInset(bars[1])
        }
        return ScrollView(context).apply {
            setBackgroundColor(pal.background)
            addView(column)
            isFillViewport = true
            // A page taller than the screen fades at its lower edge, so it shows there is more below.
            isVerticalFadingEdgeEnabled = true
            setFadingEdgeLength(px(32))
            clipToPadding = false
            filterTouchesWhenObscured = true
            setOnApplyWindowInsetsListener { _, insets ->
                place(insets)
                // The window's own theme sets the icons again as the window comes up; the page has the last word.
                appearance()
                insets
            }
            addOnAttachStateChangeListener(
                object : View.OnAttachStateChangeListener {
                    override fun onViewAttachedToWindow(v: View) {
                        // An activity started again after a change of night mode needs this said once more.
                        decorUnderBars()
                        appearance()
                        // A page built after the window had its insets takes them now, not at the next dispatch: the
                        // padding is recomputed whenever the views are rebuilt, so no gap shows above the band.
                        v.rootWindowInsets?.let { place(it) }
                        v.requestApplyInsets()
                    }

                    override fun onViewDetachedFromWindow(v: View) {}
                },
            )
        }
    }

    // ---- the new look: the bar, the page and the footer ------------------------------------------

    private fun rounded(fill: Int, radiusDp: Float, strokeDp: Float = 0f, stroke: Int = 0): GradientDrawable = GradientDrawable().apply {
        setColor(fill)
        cornerRadius = radiusDp * density
        if (strokeDp > 0f) setStroke(pxf(strokeDp), stroke)
    }

    private fun rippleOver(bg: Drawable?, radiusDp: Float): Drawable {
        val mask = GradientDrawable().apply {
            cornerRadius = radiusDp * density
            setColor(Color.WHITE)
        }
        val tap = if (night) 0x22FFFFFF else 0x14000000
        return RippleDrawable(ColorStateList.valueOf(tap), bg, mask)
    }

    private fun lp(width: Int = ViewGroup.LayoutParams.MATCH_PARENT, height: Int = ViewGroup.LayoutParams.WRAP_CONTENT) =
        LinearLayout.LayoutParams(width, height)

    private fun divider(): View = View(context).apply {
        setBackgroundColor(pal.divider)
        layoutParams = lp(height = pxf(1f))
    }

    private fun icon(kind: Icon, colour: Int, dp: Int, stroke: Float = 2f): ImageView = ImageView(context).apply {
        setImageDrawable(IconDrawable(kind, colour, stroke))
        layoutParams = LinearLayout.LayoutParams(px(dp), px(dp))
        importantForAccessibility = View.IMPORTANT_FOR_ACCESSIBILITY_NO
    }

    /** A 48 dp touch target with a vector icon in it. */
    private fun iconButton(kind: Icon, colour: Int, description: String, onClick: () -> Unit): ImageView = ImageView(context).apply {
        setImageDrawable(IconDrawable(kind, colour, 2f))
        contentDescription = description
        scaleType = ImageView.ScaleType.FIT_CENTER
        setPadding(px(12), px(12), px(12), px(12))
        layoutParams = LinearLayout.LayoutParams(px(Space.TOUCH), px(Space.TOUCH))
        background = rippleOver(null, 24f)
        isClickable = true
        isFocusable = true
        setOnClickListener { onClick() }
        filterTouchesWhenObscured = true
    }

    /**
     * The bar of a page: 64 dp plus the status bar's height, in the surface colour with a 1 dp divider under it.
     * A back arrow ([back]) and a gear ([gear]) are 48 dp touch targets drawn in code; the title is 18 sp medium on
     * one line and cut with an ellipsis, the [sub] 13 sp in secondary, and [logo] puts the 32 dp brand square first.
     * [onBlack] makes the bar black with white icons, for the scanner.
     */
    fun bar(
        title: String,
        sub: String? = null,
        back: (() -> Unit)? = null,
        logo: Boolean = false,
        gear: (() -> Unit)? = null,
        onBlack: Boolean = false,
    ): View {
        val fill = if (onBlack) Color.BLACK else pal.surface
        val ink = if (onBlack) Palette.WHITE else pal.text
        val subInk = if (onBlack) 0xFFD3D7DE.toInt() else pal.secondary
        val bar = BarView(context, onBlack)
        bar.orientation = LinearLayout.VERTICAL
        bar.setBackgroundColor(fill)
        bar.filterTouchesWhenObscured = true
        val row = LinearLayout(context).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
            setPadding(if (back != null) px(8) else px(Space.PAGE_GUTTER), 0, if (gear != null) px(8) else px(Space.PAGE_GUTTER), 0)
        }
        if (back != null) row.addView(iconButton(Icon.Back, ink, "Back", back))
        if (logo) {
            val id = context.resources.getIdentifier("logo", "drawable", context.packageName)
            if (id != 0) {
                row.addView(
                    ImageView(context).apply {
                        setImageResource(id)
                        scaleType = ImageView.ScaleType.FIT_CENTER
                        importantForAccessibility = View.IMPORTANT_FOR_ACCESSIBILITY_NO
                        outlineProvider = object : ViewOutlineProvider() {
                            override fun getOutline(view: View, outline: android.graphics.Outline) {
                                outline.setRoundRect(0, 0, view.width, view.height, 7f * density)
                            }
                        }
                        clipToOutline = true
                    },
                    LinearLayout.LayoutParams(px(32), px(32)).also { it.rightMargin = px(12) },
                )
            }
        }
        val names = LinearLayout(context).apply { orientation = LinearLayout.VERTICAL }
        names.addView(
            text(title, Type.BAR_TITLE, colour = ink, medium = true).also {
                it.setSingleLine(true)
                it.ellipsize = android.text.TextUtils.TruncateAt.END
            },
        )
        if (sub != null) {
            names.addView(
                text(sub, Type.BAR_SUB, colour = subInk).also {
                    it.setSingleLine(true)
                    it.ellipsize = android.text.TextUtils.TruncateAt.END
                },
            )
        }
        row.addView(names, LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f))
        if (gear != null) row.addView(iconButton(Icon.Gear, ink, "Settings", gear))
        bar.row = row
        bar.addView(row, lp(height = px(Space.BAR)))
        if (!onBlack) bar.addView(divider())
        return bar
    }

    /**
     * A page of the new look: the [bar] fixed at the top, the [body] scrolling under it (a 20 dp gutter, 24 dp at
     * the top and bottom, 18 dp between blocks; a [FullWidth] first block runs edge to edge), and the [actions]
     * fixed at the bottom over a 1 dp divider (14 dp above them, 28 dp plus the navigation bar's height below,
     * 10 dp between them). With no actions there is no footer. A [tone] tints the bar. [scroll] false lays the
     * body out without a scroll, for the scanner. The insets, the keyboard and the status bar's icons are
     * handled here, as [page] does.
     */
    fun page(bar: View, body: List<View>, actions: List<Button> = emptyList(), tone: Tone? = null, scroll: Boolean = true): View {
        val barView = bar as? BarView
        val onBlack = barView?.onBlack == true
        if (tone != null && !onBlack) bar.setBackgroundColor(tone.tint)
        val root = LinearLayout(context).apply {
            orientation = LinearLayout.VERTICAL
            setBackgroundColor(if (onBlack) Color.BLACK else pal.background)
            filterTouchesWhenObscured = true
        }
        root.addView(bar, lp())
        val column = LinearLayout(context).apply { orientation = LinearLayout.VERTICAL }
        val firstWide = body.firstOrNull() is FullWidth
        for ((i, v) in body.withIndex()) {
            val wide = v is FullWidth
            val side = if (wide) 0 else px(Space.PAGE_GUTTER)
            val params = if (v is Fills) {
                LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f)
            } else {
                LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT)
            }
            params.setMargins(side, 0, side, if (i < body.size - 1) px(Space.PAGE_BLOCK) else 0)
            column.addView(v, params)
        }
        val top = if (firstWide || !scroll) 0 else px(Space.PAGE_EDGE)
        val bottom = if (scroll) px(Space.PAGE_EDGE) else 0
        column.setPadding(0, top, 0, bottom)
        val content: View = if (scroll) {
            ScrollView(context).apply {
                addView(column)
                isFillViewport = true
                // A page taller than the screen fades at its lower edge, so it shows there is more below.
                isVerticalFadingEdgeEnabled = true
                setFadingEdgeLength(px(32))
                clipToPadding = false
                filterTouchesWhenObscured = true
            }
        } else {
            column
        }
        root.addView(content, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f))
        var footer: LinearLayout? = null
        if (actions.isNotEmpty()) {
            val f = LinearLayout(context).apply {
                orientation = LinearLayout.VERTICAL
                setBackgroundColor(pal.surface)
                filterTouchesWhenObscured = true
            }
            f.addView(divider())
            val buttons = LinearLayout(context).apply { orientation = LinearLayout.VERTICAL }
            for ((i, b) in actions.withIndex()) {
                buttons.addView(b, lp().also { it.topMargin = if (i == 0) 0 else px(10) })
            }
            f.addView(buttons, lp())
            root.addView(f, lp())
            footer = f
        }
        val appearance = edgeToEdge(onBlack)
        val place = { insets: WindowInsets ->
            val bars = barInsets(insets)
            val nav = navInset(insets)
            val keyboard = bars[3] > nav
            root.setPadding(bars[0], 0, bars[2], if (keyboard) bars[3] else 0)
            barView?.setTopInset(bars[1])
            val below = if (keyboard) 0 else nav
            if (footer != null) {
                footer.getChildAt(1).setPadding(px(Space.PAGE_GUTTER), px(14), px(Space.PAGE_GUTTER), px(28) + below)
                column.setPadding(0, top, 0, bottom)
            } else {
                column.setPadding(0, top, 0, bottom + below)
            }
        }
        footer?.getChildAt(1)?.setPadding(px(Space.PAGE_GUTTER), px(14), px(Space.PAGE_GUTTER), px(28))
        root.setOnApplyWindowInsetsListener { _, insets ->
            place(insets)
            appearance()
            insets
        }
        root.addOnAttachStateChangeListener(
            object : View.OnAttachStateChangeListener {
                override fun onViewAttachedToWindow(v: View) {
                    decorUnderBars()
                    appearance()
                    v.rootWindowInsets?.let { place(it) }
                    v.requestApplyInsets()
                }

                override fun onViewDetachedFromWindow(v: View) {}
            },
        )
        return root
    }

    /** The bottom of the system bars alone (the navigation bar), without the keyboard. */
    @Suppress("DEPRECATION")
    private fun navInset(insets: WindowInsets): Int {
        if (Build.VERSION.SDK_INT >= 30) return insets.getInsets(WindowInsets.Type.systemBars() or WindowInsets.Type.displayCutout()).bottom
        return insets.stableInsetBottom
    }

    // ---- the four ways a screen's words are shown --------------------------------------------------

    private fun detailsViews(model: WordsModel): List<View> {
        val out = ArrayList<View>()
        if (model.quotedText != null) out.add(quoted(model.quotedLabel ?: "", model.quotedText))
        if (model.details.isNotEmpty()) {
            val fold = text(model.details.joinToString("\n"), Type.CODE - 2f, mono = true).apply { visibility = View.GONE }
            val toggle = text("${WordsModel.DETAILS_TITLE} (show)", Type.SECONDARY, bold = true, colour = pal.info.ink)
            toggle.minimumHeight = px(Space.TOUCH)
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

    private fun toneOf(model: WordsModel): Tone = pal.of(model.colour)

    private fun whoLine(model: WordsModel, size: Float, ink: Int, withIcon: Boolean): View = LinearLayout(context).apply {
        orientation = LinearLayout.HORIZONTAL
        gravity = Gravity.CENTER_VERTICAL
        if (withIcon) addView(icon(Icon.Person, ink, 16, 2.2f), LinearLayout.LayoutParams(px(16), px(16)).also { it.rightMargin = px(6) })
        addView(text(model.who, size, colour = ink, medium = true))
    }

    /**
     * For a screen that is one message (a result, a refusal, an error): a full-width panel in the tone's tint with
     * a 76 dp circle in the tone's band holding a white icon (a tick, an exclamation, an "i" or a dash), the heading
     * at 27 sp bold, the what, and who does the next step; the next line below in a surface card with an arrow; the
     * quoted text and the details fold as before.
     */
    fun resultPanel(model: WordsModel): View {
        val tone = toneOf(model)
        val panel = ResultPanel(context)
        panel.orientation = LinearLayout.VERTICAL
        val head = LinearLayout(context).apply {
            orientation = LinearLayout.VERTICAL
            gravity = Gravity.CENTER_HORIZONTAL
            setBackgroundColor(tone.tint)
            setPadding(px(Space.PAGE_GUTTER), px(28), px(Space.PAGE_GUTTER), px(28))
        }
        val kind = when (model.colour) {
            Colour.Green -> Icon.Tick
            Colour.Red -> Icon.Exclaim
            Colour.Blue -> Icon.Info
            Colour.Neutral -> Icon.Dash
        }
        val circle = FrameLayout(context).apply {
            background = GradientDrawable().apply {
                shape = GradientDrawable.OVAL
                setColor(tone.band)
            }
            addView(
                icon(kind, Palette.WHITE, 40, 2.6f),
                FrameLayout.LayoutParams(px(40), px(40), Gravity.CENTER),
            )
        }
        head.addView(circle, LinearLayout.LayoutParams(px(76), px(76)))
        val headingInk = if (model.colour == Colour.Red) tone.ink else pal.text
        head.addView(
            text(model.heading, 27f, bold = true, colour = headingInk).also { it.gravity = Gravity.CENTER },
            lp().also { it.topMargin = px(18) },
        )
        head.addView(
            text(model.what, 16f, colour = pal.secondary).also { it.gravity = Gravity.CENTER },
            lp().also { it.topMargin = px(8) },
        )
        head.addView(whoLine(model, 13f, pal.secondary, true), LinearLayout.LayoutParams(-2, -2).also {
            it.topMargin = px(12)
            it.gravity = Gravity.CENTER_HORIZONTAL
        })
        panel.addView(head, lp())
        val below = LinearLayout(context).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(px(Space.PAGE_GUTTER), px(Space.PAGE_BLOCK), px(Space.PAGE_GUTTER), 0)
        }
        val next = LinearLayout(context).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
            background = rounded(pal.surface, 14f, 1f, pal.divider)
            setPadding(px(16), px(14), px(16), px(14))
        }
        next.addView(icon(Icon.Arrow, pal.text, 20), LinearLayout.LayoutParams(px(20), px(20)).also { it.rightMargin = px(12) })
        next.addView(text(model.next, 15f, colour = pal.text, medium = true), LinearLayout.LayoutParams(0, -2, 1f))
        below.addView(next, lp())
        for (v in detailsViews(model)) below.addView(v, lp().also { it.topMargin = px(Space.BLOCK) })
        panel.addView(below, lp())
        panel.filterTouchesWhenObscured = true
        return panel
    }

    /**
     * For forms, lists and detail pages: the heading at 26 sp bold, left aligned and only when it differs from
     * [barTitle], then the what at 15 sp in secondary. The page puts [hint] after the content.
     */
    fun titleBlock(model: WordsModel, barTitle: String): View = LinearLayout(context).apply {
        orientation = LinearLayout.VERTICAL
        if (model.heading != barTitle) addView(text(model.heading, 26f, bold = true), lp())
        addView(text(model.what, 15f, colour = pal.secondary), lp().also { if (model.heading != barTitle) it.topMargin = px(6) })
        filterTouchesWhenObscured = true
    }

    /** The next step under the content: an info-circle icon, the next line at 14 sp in secondary and who under it at 13 sp medium. */
    fun hint(model: WordsModel): View = LinearLayout(context).apply {
        orientation = LinearLayout.HORIZONTAL
        addView(icon(Icon.InfoCircle, pal.secondary, 20, 1.8f), LinearLayout.LayoutParams(px(20), px(20)).also {
            it.rightMargin = px(10)
            it.topMargin = px(1)
        })
        val col = LinearLayout(context).apply { orientation = LinearLayout.VERTICAL }
        col.addView(text(model.next, 14f, colour = pal.secondary), lp())
        col.addView(text(model.who, 13f, colour = pal.secondary, medium = true), lp().also { it.topMargin = px(4) })
        addView(col, LinearLayout.LayoutParams(0, -2, 1f))
        filterTouchesWhenObscured = true
    }

    /** For a screen whose main thing is a code: the heading at 22 sp bold and the what at 15 sp in secondary, both centred. The next line goes under the code as [hint]. */
    fun compactWords(model: WordsModel): View = LinearLayout(context).apply {
        orientation = LinearLayout.VERTICAL
        addView(text(model.heading, 22f, bold = true).also { it.gravity = Gravity.CENTER }, lp())
        addView(text(model.what, 15f, colour = pal.secondary).also { it.gravity = Gravity.CENTER }, lp().also { it.topMargin = px(6) })
        filterTouchesWhenObscured = true
    }

    /** For a screen that only waits: centred in the body, a 64 dp ring with an orange arc that turns once a second, the heading at 24 sp bold, the what at 16 sp in secondary and the next line at 15 sp medium. */
    fun working(model: WordsModel): View {
        val v = WorkingView(context)
        v.orientation = LinearLayout.VERTICAL
        v.gravity = Gravity.CENTER
        v.addView(SpinnerView(context, pal.fieldLine), LinearLayout.LayoutParams(px(64), px(64)))
        v.addView(text(model.heading, 24f, bold = true).also { it.gravity = Gravity.CENTER }, lp().also { it.topMargin = px(24) })
        v.addView(text(model.what, 16f, colour = pal.secondary).also { it.gravity = Gravity.CENTER }, lp().also { it.topMargin = px(8) })
        v.addView(text(model.next, 15f, medium = true).also { it.gravity = Gravity.CENTER }, lp().also { it.topMargin = px(12) })
        v.filterTouchesWhenObscured = true
        return v
    }

    // ---- blocks ------------------------------------------------------------------------------------

    /** A small heading over a group: 13 sp bold, upper case, letter-spaced 0.6, in secondary. */
    fun label(text: String): TextView = text(text, Type.LABEL, bold = true, colour = pal.secondary).apply {
        isAllCaps = true
        letterSpacing = 0.6f / Type.LABEL
    }

    /** The [label] of a field at 14 sp medium in secondary, 6 dp above [field]. The field's hint shows a format only, never a copy of the label. */
    fun labelled(label: String, field: View): View = LinearLayout(context).apply {
        orientation = LinearLayout.VERTICAL
        if (field.id == View.NO_ID) field.id = View.generateViewId()
        addView(
            text(label, Type.LABELLED, colour = pal.secondary, medium = true).also { it.labelFor = field.id },
            lp().also { it.bottomMargin = px(6) },
        )
        addView(field, lp())
    }

    /** Two PIN fields side by side, 12 dp apart, each with its label. They take digits as a password, no keyboard may learn them. */
    fun pinPair(label1: String, label2: String): PinPair {
        fun pin(): EditText = field("").also {
            it.inputType = InputType.TYPE_CLASS_NUMBER or InputType.TYPE_NUMBER_VARIATION_PASSWORD
            style(it)
        }
        val a = pin()
        val b = pin()
        val row = LinearLayout(context).apply { orientation = LinearLayout.HORIZONTAL }
        row.addView(labelled(label1, a), LinearLayout.LayoutParams(0, -2, 1f).also { it.rightMargin = px(6) })
        row.addView(labelled(label2, b), LinearLayout.LayoutParams(0, -2, 1f).also { it.leftMargin = px(6) })
        return PinPair(row, a, b)
    }

    /**
     * Up to three options as pills (40 dp, corner 20): the selected one filled in the text colour with
     * background-coloured text, the others outlined. With more than three, the radio list of [choice]. Either way
     * the result is a [RadioGroup] that [chosen] reads.
     */
    fun chips(options: List<String>, selected: Int = 0): RadioGroup {
        if (options.size > 3) return choice(options, selected)
        return RadioGroup(context).apply {
            orientation = RadioGroup.HORIZONTAL
            isBaselineAligned = false
            for ((i, label) in options.withIndex()) {
                addView(
                    PillButton(context, this@Ui).apply {
                        text = label
                        id = View.generateViewId()
                        isChecked = i == selected
                    },
                    // The pills share the height of the tallest, which grows when a label takes two lines.
                    RadioGroup.LayoutParams(0, ViewGroup.LayoutParams.MATCH_PARENT, 1f).also {
                        it.leftMargin = if (i == 0) 0 else px(4)
                        it.rightMargin = if (i == options.size - 1) 0 else px(4)
                    },
                )
            }
        }
    }

    private fun card(radius: Float = 16f): LinearLayout = LinearLayout(context).apply {
        orientation = LinearLayout.VERTICAL
        background = rounded(pal.surface, radius)
        clipToOutline = true
        filterTouchesWhenObscured = true
    }

    private fun dot(colour: Int, dp: Int): View = View(context).apply {
        background = GradientDrawable().apply {
            shape = GradientDrawable.OVAL
            setColor(colour)
        }
        layoutParams = LinearLayout.LayoutParams(px(dp), px(dp))
        importantForAccessibility = View.IMPORTANT_FOR_ACCESSIBILITY_NO
    }

    /** One surface card (corner 16) of rows of at least 56 dp, a 1 dp divider between: an optional tone dot, a title at 16 sp medium, an optional sub, an optional count badge and a chevron. */
    fun rows(items: List<RowItem>): View {
        val card = card()
        for ((i, item) in items.withIndex()) {
            if (i > 0) card.addView(divider())
            val row = LinearLayout(context).apply {
                orientation = LinearLayout.HORIZONTAL
                gravity = Gravity.CENTER_VERTICAL
                minimumHeight = px(56)
                setPadding(px(16), px(10), px(12), px(10))
            }
            if (item.colour != null) row.addView(dot(pal.of(item.colour).band, 10), LinearLayout.LayoutParams(px(10), px(10)).also { it.rightMargin = px(12) })
            val col = LinearLayout(context).apply { orientation = LinearLayout.VERTICAL }
            col.addView(text(item.title, 16f, colour = if (item.danger) pal.stop.ink else pal.text, medium = true))
            if (item.sub != null) col.addView(text(item.sub, 13f, colour = pal.secondary))
            row.addView(col, LinearLayout.LayoutParams(0, -2, 1f))
            if (item.count != null) {
                row.addView(
                    text(item.count.toString(), 13f, bold = true, colour = Palette.WHITE).also {
                        it.gravity = Gravity.CENTER
                        it.minWidth = px(24)
                        it.setPadding(px(8), px(2), px(8), px(2))
                        it.background = rounded(if (night) pal.chip else 0xFF111317.toInt(), 12f)
                    },
                    LinearLayout.LayoutParams(-2, -2).also { it.leftMargin = px(8) },
                )
            }
            row.addView(icon(Icon.Chevron, pal.hint, 20), LinearLayout.LayoutParams(px(20), px(20)).also { it.leftMargin = px(8) })
            val click = item.onClick
            if (click != null) {
                row.isClickable = true
                row.isFocusable = true
                row.background = rippleOver(null, 0f)
                row.setOnClickListener { click() }
            }
            card.addView(row, lp())
        }
        return card
    }

    /** One surface card of lines: the key in secondary on the left and the value at 15 sp medium on the right, in tabular figures. */
    fun facts(pairs: List<Pair<String, String>>): View {
        val card = card()
        for ((i, p) in pairs.withIndex()) {
            if (i > 0) card.addView(divider())
            val row = LinearLayout(context).apply {
                orientation = LinearLayout.HORIZONTAL
                setPadding(px(16), px(14), px(16), px(14))
            }
            row.addView(text(p.first, 15f, colour = pal.secondary), LinearLayout.LayoutParams(-2, -2).also { it.rightMargin = px(12) })
            row.addView(
                text(p.second, 15f, medium = true).also {
                    it.gravity = Gravity.END
                    it.fontFeatureSettings = "tnum"
                },
                LinearLayout.LayoutParams(0, -2, 1f),
            )
            card.addView(row, lp())
        }
        return card
    }

    /** One surface card of lines: a tone dot, a title in bold and its words under it. */
    fun status(rows: List<StatusItem>): View {
        val card = card()
        for ((i, item) in rows.withIndex()) {
            if (i > 0) card.addView(divider())
            val row = LinearLayout(context).apply {
                orientation = LinearLayout.HORIZONTAL
                setPadding(px(16), px(14), px(16), px(14))
            }
            row.addView(dot(pal.of(item.colour).band, 10), LinearLayout.LayoutParams(px(10), px(10)).also {
                it.rightMargin = px(12)
                it.topMargin = px(6)
            })
            val col = LinearLayout(context).apply { orientation = LinearLayout.VERTICAL }
            col.addView(text(item.title, 16f, bold = true))
            col.addView(text(item.words, 14f, colour = pal.secondary), lp().also { it.topMargin = px(2) })
            row.addView(col, LinearLayout.LayoutParams(0, -2, 1f))
            card.addView(row, lp())
        }
        return card
    }

    /** A tinted panel inside the gutter, corner 14: the heading in the tone's ink and bold, then the what in the text colour. */
    fun notice(model: WordsModel): View {
        val tone = toneOf(model)
        return LinearLayout(context).apply {
            orientation = LinearLayout.VERTICAL
            background = rounded(tone.tint, 14f)
            setPadding(px(16), px(14), px(16), px(14))
            addView(text(model.heading, 16f, bold = true, colour = tone.ink), lp())
            addView(text(model.what, 15f), lp().also { it.topMargin = px(4) })
            filterTouchesWhenObscured = true
        }
    }

    /** The check words as a 2 by 2 grid of surface cards with a 2 dp outline in the text colour: the number from 1 (13 sp bold, secondary) and the word at 24 sp bold. */
    fun wordCards(words: List<String>): View {
        val grid = LinearLayout(context).apply { orientation = LinearLayout.VERTICAL }
        for (start in words.indices step 2) {
            val row = LinearLayout(context).apply { orientation = LinearLayout.HORIZONTAL }
            for (k in 0 until 2) {
                val at = start + k
                if (at >= words.size) {
                    row.addView(View(context), LinearLayout.LayoutParams(0, 1, 1f))
                    continue
                }
                val c = LinearLayout(context).apply {
                    orientation = LinearLayout.VERTICAL
                    background = rounded(pal.surface, 14f, 2f, pal.text)
                    setPadding(px(14), px(12), px(14), px(14))
                }
                c.addView(text("${at + 1}", 13f, bold = true, colour = pal.secondary))
                c.addView(text(words[at], 24f, bold = true), lp().also { it.topMargin = px(2) })
                row.addView(
                    c,
                    LinearLayout.LayoutParams(0, -2, 1f).also {
                        it.leftMargin = if (k == 0) 0 else px(5)
                        it.rightMargin = if (k == 0) px(5) else 0
                        it.bottomMargin = if (start + 2 < words.size) px(10) else 0
                    },
                )
            }
            grid.addView(row, lp())
        }
        grid.filterTouchesWhenObscured = true
        return grid
    }

    /** A payment to approve: one surface card with a 5 dp brand strip at the top and a line per row, with dividers. */
    fun receipt(lines: List<ReceiptLine>): View {
        val card = card()
        card.addView(View(context).apply { setBackgroundColor(Palette.BRAND) }, lp(height = px(5)))
        for ((i, l) in lines.withIndex()) {
            if (i > 0) card.addView(divider())
            val t = when (l.style) {
                LineStyle.Plain -> text(l.text, 16f)
                LineStyle.Strong -> text(l.text, 18f, bold = true)
                LineStyle.Small -> text(l.text, 13f, colour = pal.secondary)
            }
            t.setPadding(px(16), px(12), px(16), px(12))
            card.addView(t, lp())
        }
        return card
    }

    /** A two-column grid of 92 dp surface cards, each with a tone dot at the top and the title at the bottom. */
    fun tiles(items: List<TileItem>): View {
        val grid = LinearLayout(context).apply { orientation = LinearLayout.VERTICAL }
        for (start in items.indices step 2) {
            val row = LinearLayout(context).apply { orientation = LinearLayout.HORIZONTAL }
            for (k in 0 until 2) {
                val at = start + k
                val params = LinearLayout.LayoutParams(0, px(92), 1f).also {
                    it.leftMargin = if (k == 0) 0 else px(6)
                    it.rightMargin = if (k == 0) px(6) else 0
                    it.bottomMargin = if (start + 2 < items.size) px(12) else 0
                }
                if (at >= items.size) {
                    row.addView(View(context), params)
                    continue
                }
                val item = items[at]
                val c = FrameLayout(context).apply {
                    background = rippleOver(rounded(pal.surface, 16f), 16f)
                    setPadding(px(16), px(14), px(16), px(14))
                    isClickable = item.onClick != null
                    isFocusable = item.onClick != null
                    item.onClick?.let { click -> setOnClickListener { click() } }
                }
                c.addView(dot(pal.of(item.colour).band, 10), FrameLayout.LayoutParams(px(10), px(10), Gravity.TOP or Gravity.START))
                c.addView(text(item.title, 16f, medium = true), FrameLayout.LayoutParams(-2, -2, Gravity.BOTTOM or Gravity.START))
                row.addView(c, params)
            }
            grid.addView(row, lp())
        }
        grid.filterTouchesWhenObscured = true
        return grid
    }

    /** A 72 dp surface square with a padlock, centred, above the words of a PIN screen. */
    fun lock(): View = LinearLayout(context).apply {
        gravity = Gravity.CENTER_HORIZONTAL
        val square = FrameLayout(context).apply {
            background = rounded(pal.surface, 20f, 1f, pal.divider)
            addView(icon(Icon.Padlock, pal.text, 34, 1.8f), FrameLayout.LayoutParams(px(34), px(34), Gravity.CENTER))
        }
        addView(square, LinearLayout.LayoutParams(px(72), px(72)))
        filterTouchesWhenObscured = true
    }

    /**
     * Six dots for the entered digits, then a 3 by 4 grid of 64 dp round keys: 1 to 9, an empty key, 0 and a
     * delete key (described as "Delete one digit"). A key gives a light haptic tick. Only for a PIN of digits:
     * never where a PIN may be a passphrase.
     */
    fun keypad(onDigit: (Int) -> Unit, onDelete: () -> Unit): Keypad = Keypad(context, this, onDigit, onDelete)

    /** A white card, corner 24, 16 dp padding, with a soft shadow, round a QR view. The code is drawn on white in both modes. */
    fun codeCard(view: View): View = LinearLayout(context).apply {
        gravity = Gravity.CENTER_HORIZONTAL
        clipChildren = false
        clipToPadding = false
        val c = FrameLayout(context).apply {
            background = rounded(Palette.WHITE, 24f)
            setPadding(px(16), px(16), px(16), px(16))
            elevation = 10f * density
            addView(view, FrameLayout.LayoutParams(-2, -2, Gravity.CENTER))
        }
        addView(c, LinearLayout.LayoutParams(-2, -2))
        filterTouchesWhenObscured = true
    }

    /**
     * The scanner screen's body: [preview] fills it on black, a 280 dp square frame is outlined in white with the
     * area outside dimmed to 45 % black, an orange 3 dp line glows across the middle, the words sit over the bottom
     * in white and the progress line shows as a pill with an orange dot. Its bar is `bar(..., onBlack = true)`.
     */
    fun cameraFrame(preview: View?): CameraFrame = CameraFrame(context, this, preview)

    /** The left, top, right and bottom insets of the system bars, the display cutout and the keyboard. */
    @Suppress("DEPRECATION")
    private fun barInsets(insets: WindowInsets): IntArray {
        if (Build.VERSION.SDK_INT >= 30) {
            val i = insets.getInsets(WindowInsets.Type.systemBars() or WindowInsets.Type.displayCutout() or WindowInsets.Type.ime())
            return intArrayOf(i.left, i.top, i.right, i.bottom)
        }
        val cut = insets.displayCutout
        return intArrayOf(
            maxOf(insets.systemWindowInsetLeft, cut?.safeInsetLeft ?: 0),
            maxOf(insets.systemWindowInsetTop, cut?.safeInsetTop ?: 0),
            maxOf(insets.systemWindowInsetRight, cut?.safeInsetRight ?: 0),
            insets.systemWindowInsetBottom,
        )
    }

    /**
     * The window draws behind both bars on every Android version the apps run on (from 15 on the system does
     * it for an app that targets 35 or later), and the bars' icons are dark on the page's light background and
     * light on a dark one or on a band, whatever the band's tone. Returns what sets the icons: the window's
     * controller exists only once the page is on the screen, so it is called then.
     */
    @Suppress("DEPRECATION")
    private fun edgeToEdge(bandOnTop: Boolean): () -> Unit {
        val act = activity() ?: return {}
        val lightStatus = !bandOnTop && !night
        val lightNav = !night
        if (Build.VERSION.SDK_INT >= 30) {
            act.window.setDecorFitsSystemWindows(false)
            return {
                val controller = act.window.insetsController
                if (controller != null) {
                    val mask = WindowInsetsController.APPEARANCE_LIGHT_STATUS_BARS or WindowInsetsController.APPEARANCE_LIGHT_NAVIGATION_BARS
                    var bits = 0
                    if (lightStatus) bits = bits or WindowInsetsController.APPEARANCE_LIGHT_STATUS_BARS
                    if (lightNav) bits = bits or WindowInsetsController.APPEARANCE_LIGHT_NAVIGATION_BARS
                    controller.setSystemBarsAppearance(bits, mask)
                }
            }
        }
        return {
            var flags = View.SYSTEM_UI_FLAG_LAYOUT_STABLE or View.SYSTEM_UI_FLAG_LAYOUT_FULLSCREEN or View.SYSTEM_UI_FLAG_LAYOUT_HIDE_NAVIGATION
            if (lightStatus) flags = flags or View.SYSTEM_UI_FLAG_LIGHT_STATUS_BAR
            if (lightNav) flags = flags or View.SYSTEM_UI_FLAG_LIGHT_NAVIGATION_BAR
            act.window.decorView.systemUiVisibility = flags
        }
    }

    /** The window lays its content out behind the bars (API 30 and later; earlier ones use the layout flags). */
    @Suppress("DEPRECATION")
    private fun decorUnderBars() {
        if (Build.VERSION.SDK_INT >= 30) activity()?.window?.setDecorFitsSystemWindows(false)
    }

    private fun activity(): Activity? {
        var c: Context = context
        while (c is ContextWrapper) {
            if (c is Activity) return c
            c = c.baseContext
        }
        return null
    }
}

/** A view of a page's body that runs edge to edge, without the page's gutter. */
interface FullWidth

/** A view of a page's body that takes the height that is left, so that what it holds is centred in the body. */
interface Fills

/** The bar of a page (see [Ui.bar]): it takes the status bar's height as top padding, like a [Bleed]. */
class BarView(context: Context, val onBlack: Boolean) : LinearLayout(context), Bleed {
    var row: LinearLayout? = null

    override fun setTopInset(px: Int) {
        setPadding(0, px, 0, 0)
    }
}

/** The panel of [Ui.resultPanel]: edge to edge. */
class ResultPanel(context: Context) : LinearLayout(context), FullWidth

/** The waiting screen of [Ui.working]: it fills the body and centres what it holds. */
class WorkingView(context: Context) : LinearLayout(context), Fills

/** A 64 dp ring with an arc that turns once a second; still when the system's animations are off. */
class SpinnerView(context: Context, private val track: Int) : View(context) {
    private val paint = Paint(Paint.ANTI_ALIAS_FLAG).apply { style = Paint.Style.STROKE }
    private var angle = 0f
    private var animator: android.animation.ValueAnimator? = null

    init {
        importantForAccessibility = IMPORTANT_FOR_ACCESSIBILITY_NO
    }

    override fun onAttachedToWindow() {
        super.onAttachedToWindow()
        if (!Feedback.motionOn(context)) return
        animator = android.animation.ValueAnimator.ofFloat(0f, 360f).apply {
            duration = 1000
            repeatCount = android.animation.ValueAnimator.INFINITE
            interpolator = android.view.animation.LinearInterpolator()
            addUpdateListener {
                angle = it.animatedValue as Float
                invalidate()
            }
            start()
        }
    }

    override fun onDetachedFromWindow() {
        animator?.cancel()
        animator = null
        super.onDetachedFromWindow()
    }

    override fun onDraw(canvas: Canvas) {
        val d = resources.displayMetrics.density
        val w = 5f * d
        paint.strokeWidth = w
        val box = RectF(w / 2, w / 2, width - w / 2, height - w / 2)
        paint.color = track
        canvas.drawOval(box, paint)
        paint.color = Palette.BRAND
        paint.strokeCap = Paint.Cap.ROUND
        canvas.drawArc(box, angle - 90f, 100f, false, paint)
    }
}

/** The two fields of [Ui.pinPair] and the row that holds them. */
class PinPair(val view: View, val first: EditText, val second: EditText)

/** One option of [Ui.chips]: a pill, filled when it is the chosen one. */
class PillButton(context: Context, private val ui: Ui) : RadioButton(context) {
    /**
     * False while [RadioButton]'s own constructor runs: it calls [setChecked] before this class has set [ui],
     * so the first restyle waits for [init].
     */
    private var built = false

    init {
        setButtonDrawable(null)
        gravity = Gravity.CENTER
        textSize = 15f
        typeface = Typeface.create("sans-serif-medium", Typeface.NORMAL)
        minHeight = ui.px(40)
        minimumHeight = ui.px(40)
        setPadding(ui.px(12), ui.px(8), ui.px(12), ui.px(8))
        filterTouchesWhenObscured = true
        built = true
        restyle()
    }

    override fun setChecked(checked: Boolean) {
        super.setChecked(checked)
        if (built) restyle()
    }

    private fun restyle() {
        val pal = ui.pal
        val on = isChecked
        setTextColor(if (on) pal.background else pal.text)
        background = GradientDrawable().apply {
            cornerRadius = ui.px(20).toFloat()
            if (on) {
                setColor(pal.text)
            } else {
                setColor(Color.TRANSPARENT)
                setStroke(ui.px(1), pal.fieldLine)
            }
        }
    }
}

/** One row of [Ui.rows]. [colour] gives the 10 dp tone dot; a [danger] row has its title in the stop ink. */
class RowItem(
    val title: String,
    val sub: String? = null,
    val colour: Colour? = null,
    val count: Int? = null,
    val danger: Boolean = false,
    val onClick: (() -> Unit)? = null,
)

/** One line of [Ui.status]: a tone dot, a title and its words. */
class StatusItem(val colour: Colour, val title: String, val words: String)

enum class LineStyle { Plain, Strong, Small }

/** One line of [Ui.receipt]: plain, strong, or small and in secondary. */
class ReceiptLine(val text: String, val style: LineStyle = LineStyle.Plain)

/** One tile of [Ui.tiles]. */
class TileItem(val colour: Colour, val title: String, val onClick: (() -> Unit)? = null)

/** The PIN keypad of [Ui.keypad]: [entered] says how many of the six dots are filled. */
class Keypad(context: Context, private val ui: Ui, private val onDigit: (Int) -> Unit, private val onDelete: () -> Unit) : LinearLayout(context) {
    private val dots = ArrayList<View>()

    /** The digits entered so far, 0 to 6. */
    var entered = 0
        set(value) {
            field = value.coerceIn(0, DOTS)
            paintDots()
        }

    init {
        orientation = VERTICAL
        gravity = Gravity.CENTER_HORIZONTAL
        filterTouchesWhenObscured = true
        val dotRow = LinearLayout(context).apply { gravity = Gravity.CENTER }
        for (i in 0 until DOTS) {
            val d = View(context)
            dots.add(d)
            dotRow.addView(d, LayoutParams(ui.px(14), ui.px(14)).also { it.setMargins(ui.px(7), 0, ui.px(7), 0) })
        }
        addView(dotRow, LayoutParams(-2, -2).also { it.bottomMargin = ui.px(24) })
        paintDots()
        val keys = listOf(1, 2, 3, 4, 5, 6, 7, 8, 9, -1, 0, -2)
        for (r in 0 until 4) {
            val row = LinearLayout(context).apply { gravity = Gravity.CENTER }
            for (c in 0 until 3) {
                val k = keys[r * 3 + c]
                row.addView(key(k), LayoutParams(ui.px(64), ui.px(64)).also { it.setMargins(ui.px(10), ui.px(6), ui.px(10), ui.px(6)) })
            }
            addView(row, LayoutParams(-2, -2))
        }
    }

    private fun paintDots() {
        val pal = ui.pal
        for ((i, d) in dots.withIndex()) {
            d.background = GradientDrawable().apply {
                shape = GradientDrawable.OVAL
                if (i < entered) {
                    setColor(pal.text)
                } else {
                    setColor(Color.TRANSPARENT)
                    setStroke(ui.px(2), pal.fieldLine)
                }
            }
        }
    }

    private fun key(k: Int): View {
        if (k == -1) return View(context).also { it.importantForAccessibility = IMPORTANT_FOR_ACCESSIBILITY_NO }
        val pal = ui.pal
        val v = TextView(context).apply {
            gravity = Gravity.CENTER
            textSize = 26f
            setTextColor(pal.text)
            typeface = Typeface.create("sans-serif-medium", Typeface.NORMAL)
            text = if (k == -2) "⌫" else k.toString()
            if (k == -2) contentDescription = "Delete one digit"
            val bg = GradientDrawable().apply {
                shape = GradientDrawable.OVAL
                setColor(pal.surface)
                setStroke(ui.px(1), pal.divider)
            }
            val tap = if (ui.night) 0x22FFFFFF else 0x14000000
            background = RippleDrawable(ColorStateList.valueOf(tap), bg, GradientDrawable().apply { shape = GradientDrawable.OVAL; setColor(Color.WHITE) })
            isClickable = true
            isFocusable = true
            filterTouchesWhenObscured = true
        }
        v.setOnClickListener {
            Feedback.haptic(v, Haptic.Tick)
            if (k == -2) {
                entered = entered - 1
                onDelete()
            } else {
                entered = entered + 1
                onDigit(k)
            }
        }
        return v
    }

    companion object {
        const val DOTS = 6
    }
}

/**
 * The body of the scanner screen ([Ui.cameraFrame]): the preview on black, a 280 dp frame with the rest dimmed,
 * an orange scan line, the words at the bottom in white and a pill for the progress.
 */
class CameraFrame(context: Context, private val ui: Ui, preview: View?) : FrameLayout(context) {
    private val overlay = Overlay(context)
    private val words = LinearLayout(context)
    private val progress = TextView(context)
    private val pill = LinearLayout(context)

    init {
        setBackgroundColor(Color.BLACK)
        filterTouchesWhenObscured = true
        if (preview != null) addView(preview, LayoutParams(-1, -1, Gravity.CENTER))
        addView(overlay, LayoutParams(-1, -1))
        words.orientation = LinearLayout.VERTICAL
        words.gravity = Gravity.CENTER_HORIZONTAL
        words.setPadding(ui.px(Space.PAGE_GUTTER), 0, ui.px(Space.PAGE_GUTTER), ui.px(Space.PAGE_EDGE))
        pill.orientation = LinearLayout.HORIZONTAL
        pill.gravity = Gravity.CENTER_VERTICAL
        pill.setPadding(ui.px(14), ui.px(8), ui.px(16), ui.px(8))
        pill.background = GradientDrawable().apply {
            setColor(0x99000000.toInt())
            cornerRadius = ui.px(20).toFloat()
        }
        val d = View(context).apply {
            background = GradientDrawable().apply {
                shape = GradientDrawable.OVAL
                setColor(Palette.BRAND)
            }
        }
        pill.addView(d, LinearLayout.LayoutParams(ui.px(8), ui.px(8)).also { it.rightMargin = ui.px(8) })
        progress.setTextColor(Palette.WHITE)
        progress.textSize = 14f
        pill.addView(progress)
        pill.visibility = GONE
        words.addView(pill, LinearLayout.LayoutParams(-2, -2).also { it.bottomMargin = ui.px(12) })
        addView(words, LayoutParams(-1, -2, Gravity.BOTTOM))
    }

    private val wordViews = ArrayList<View>()

    /** The words over the bottom: the heading, the what and the next line in white; null clears them. */
    fun words(model: WordsModel?) {
        for (v in wordViews) words.removeView(v)
        wordViews.clear()
        if (model == null) return
        val h = ui.text(model.heading, 20f, bold = true, colour = Palette.WHITE).also { it.gravity = Gravity.CENTER }
        val w = ui.text(model.what, 15f, colour = Palette.WHITE).also { it.gravity = Gravity.CENTER }
        val n = ui.text(model.next, 15f, colour = Palette.WHITE, medium = true).also { it.gravity = Gravity.CENTER }
        for (v in listOf(h, w, n)) {
            words.addView(v, LinearLayout.LayoutParams(-1, -2).also { it.topMargin = ui.px(4) })
            wordViews.add(v)
        }
    }

    /** The progress line as a pill with an orange dot; null hides it. */
    fun progress(line: String?) {
        pill.visibility = if (line == null) GONE else VISIBLE
        progress.text = line ?: ""
    }

    /** The dimmed area, the white frame and the glowing line. */
    private inner class Overlay(context: Context) : View(context) {
        private val paint = Paint(Paint.ANTI_ALIAS_FLAG)

        init {
            importantForAccessibility = IMPORTANT_FOR_ACCESSIBILITY_NO
        }

        override fun onDraw(canvas: Canvas) {
            val d = resources.displayMetrics.density
            val side = 280f * d
            val box = RectF((width - side) / 2f, (height - side) / 2f - 40f * d, (width + side) / 2f, (height + side) / 2f - 40f * d)
            val radius = 24f * d
            paint.style = Paint.Style.FILL
            paint.color = 0x73000000
            val save = canvas.saveLayer(0f, 0f, width.toFloat(), height.toFloat(), null)
            canvas.drawRect(0f, 0f, width.toFloat(), height.toFloat(), paint)
            paint.xfermode = android.graphics.PorterDuffXfermode(android.graphics.PorterDuff.Mode.CLEAR)
            canvas.drawRoundRect(box, radius, radius, paint)
            paint.xfermode = null
            canvas.restoreToCount(save)
            paint.style = Paint.Style.STROKE
            paint.strokeWidth = 3f * d
            paint.color = Palette.WHITE
            canvas.drawRoundRect(box, radius, radius, paint)
            val y = box.centerY()
            paint.style = Paint.Style.FILL
            paint.color = 0x33F7931A
            canvas.drawRoundRect(RectF(box.left + 12f * d, y - 6f * d, box.right - 12f * d, y + 6f * d), 6f * d, 6f * d, paint)
            paint.color = Palette.BRAND
            canvas.drawRoundRect(RectF(box.left + 12f * d, y - 1.5f * d, box.right - 12f * d, y + 1.5f * d), 1.5f * d, 1.5f * d, paint)
        }
    }
}
