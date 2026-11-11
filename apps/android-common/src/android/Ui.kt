package org.openbmt.app

import android.app.Activity
import android.content.Context
import android.content.ContextWrapper
import android.content.res.ColorStateList
import android.content.res.Configuration
import android.graphics.Color
import android.graphics.Typeface
import android.graphics.drawable.GradientDrawable
import android.graphics.drawable.RippleDrawable
import android.os.Build
import android.text.InputType
import android.view.Gravity
import android.view.View
import android.view.ViewGroup
import android.view.WindowInsets
import android.view.WindowInsetsController
import android.view.inputmethod.EditorInfo
import android.widget.Button
import android.widget.EditText
import android.widget.LinearLayout
import android.widget.RadioButton
import android.widget.RadioGroup
import android.widget.ScrollView
import android.widget.TextView

/** True when [c] is the phone's night mode. */
fun isNight(c: Configuration): Boolean =
    (c.uiMode and Configuration.UI_MODE_NIGHT_MASK) == Configuration.UI_MODE_NIGHT_YES

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
enum class ButtonKind { Primary, Secondary, Danger }

/** Plain `android.widget` views made in code, in the colours, sizes and distances of [Palette], [Type] and [Space]. */
class Ui(private val context: Context) {
    private val density = context.resources.displayMetrics.density

    /** True when the phone is in night mode: every colour is chosen from the dark scheme. */
    val night: Boolean = isNight(context.resources.configuration)

    /** The colours of the phone's current mode. */
    val pal: Scheme = Palette.of(night)

    fun px(dp: Int): Int = (dp * density).toInt()

    fun margins(top: Int = 0, bottom: Int = 0): LinearLayout.LayoutParams =
        LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT)
            .also { it.setMargins(0, px(top), 0, px(bottom)) }

    fun text(
        text: String,
        size: Float = Type.BODY,
        bold: Boolean = false,
        mono: Boolean = false,
        colour: Int? = null,
    ): TextView = TextView(context).apply {
        this.text = text
        textSize = size
        setTextColor(colour ?: pal.text)
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
    }

    /** A secondary button: outlined. */
    fun button(label: String, onClick: () -> Unit): Button = secondary(label, onClick)

    /** The screen's main action: filled in the tone of [colour] (info by default) with white text. */
    fun primary(label: String, colour: Colour = Colour.Blue, onClick: () -> Unit): Button =
        styled(label, ButtonKind.Primary, pal.of(colour), onClick)

    /** A quieter action: outlined. */
    fun secondary(label: String, onClick: () -> Unit): Button = styled(label, ButtonKind.Secondary, pal.info, onClick)

    /** An action that wipes, removes, forgets, declines or cancels: outlined in the stop colour. */
    fun danger(label: String, onClick: () -> Unit): Button = styled(label, ButtonKind.Danger, pal.stop, onClick)

    private fun styled(label: String, kind: ButtonKind, tone: Tone, onClick: () -> Unit): Button = UiButton(context).apply {
        text = label
        isAllCaps = false
        textSize = Type.BODY
        setTypeface(Typeface.DEFAULT, if (kind == ButtonKind.Primary) Typeface.BOLD else Typeface.NORMAL)
        minHeight = px(Space.BUTTON)
        minimumHeight = px(Space.BUTTON)
        minWidth = 0
        minimumWidth = 0
        gravity = Gravity.CENTER
        setPadding(px(Space.GUTTER), 0, px(Space.GUTTER), 0)
        stateListAnimator = null
        elevation = 0f
        val ink = when (kind) {
            ButtonKind.Primary -> Palette.WHITE
            ButtonKind.Secondary -> pal.text
            ButtonKind.Danger -> tone.ink
        }
        setTextColor(ink)
        val fill = GradientDrawable().apply {
            cornerRadius = px(Space.RADIUS).toFloat()
            if (kind == ButtonKind.Primary) {
                setColor(tone.band)
            } else {
                setColor(Color.TRANSPARENT)
                setStroke(px(2), if (kind == ButtonKind.Danger) tone.ink else pal.secondary)
            }
        }
        val mask = GradientDrawable().apply {
            cornerRadius = px(Space.RADIUS).toFloat()
            setColor(Color.WHITE)
        }
        // The pressed state: a ripple, black over a filled button, the text's own colour over an outlined one.
        val ripple = if (kind == ButtonKind.Primary) 0x40000000 else (ink and 0x00FFFFFF) or 0x33000000
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
     * than the text, a surface with an outline, at least 56 dp high. The input type and the flags are the caller's.
     */
    fun style(field: EditText, mono: Boolean = false): EditText = field.apply {
        typeface = if (mono) Typeface.MONOSPACE else Typeface.DEFAULT
        textSize = if (mono) Type.CODE else Type.BODY
        setTextColor(pal.text)
        setHintTextColor(pal.hint)
        background = GradientDrawable().apply {
            setColor(pal.surface)
            setStroke(px(1), pal.hint)
            cornerRadius = px(8).toFloat()
        }
        setPadding(px(Space.BLOCK), px(8), px(Space.BLOCK), px(8))
        minHeight = px(Space.FIELD)
        minimumHeight = px(Space.FIELD)
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
        return ScrollView(context).apply {
            setBackgroundColor(pal.background)
            addView(column)
            isFillViewport = true
            clipToPadding = false
            filterTouchesWhenObscured = true
            setOnApplyWindowInsetsListener { _, insets ->
                val bars = barInsets(insets)
                // A first view that reaches under the status bar takes the bar's height itself.
                column.setPadding(bars[0], if (lead != null) 0 else topBase + bars[1], bars[2], bottomBase + bars[3])
                lead?.setTopInset(bars[1])
                // The window's own theme sets the icons again as the window comes up; the page has the last word.
                appearance()
                insets
            }
            addOnAttachStateChangeListener(
                object : View.OnAttachStateChangeListener {
                    override fun onViewAttachedToWindow(v: View) = appearance()

                    override fun onViewDetachedFromWindow(v: View) {}
                },
            )
        }
    }

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

    private fun activity(): Activity? {
        var c: Context = context
        while (c is ContextWrapper) {
            if (c is Activity) return c
            c = c.baseContext
        }
        return null
    }
}
