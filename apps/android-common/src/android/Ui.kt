package org.openbmt.app

import android.content.Context
import android.graphics.Color
import android.graphics.Typeface
import android.text.InputType
import android.view.View
import android.view.ViewGroup
import android.view.inputmethod.EditorInfo
import android.widget.Button
import android.widget.EditText
import android.widget.LinearLayout
import android.widget.RadioButton
import android.widget.RadioGroup
import android.widget.ScrollView
import android.widget.TextView

/**
 * One `Words` on screen: a coloured band with the heading, the two
 * sentences, who does the next step, quoted text marked as from elsewhere, and
 * a "Details" fold that holds the rule, the time and the detail.
 */
class WordsView(context: Context, model: WordsModel) : LinearLayout(context) {
    init {
        orientation = VERTICAL
        val ui = Ui(context)
        val band = LinearLayout(context).apply {
            orientation = VERTICAL
            setBackgroundColor(model.colour.argb)
            setPadding(ui.px(16), ui.px(12), ui.px(16), ui.px(12))
        }
        band.addView(ui.text(model.heading, 22f, bold = true, colour = Color.WHITE))
        addView(band)
        addView(ui.text(model.what, 17f).also { it.setPadding(0, ui.px(12), 0, 0) })
        addView(ui.text(model.next, 17f, bold = true).also { it.setPadding(0, ui.px(8), 0, 0) })
        addView(ui.text(model.who, 14f, colour = 0xFF455A64.toInt()).also { it.setPadding(0, ui.px(4), 0, 0) })
        if (model.quotedText != null) {
            addView(ui.quoted(model.quotedLabel ?: "", model.quotedText), ui.margins(top = 12))
        }
        if (model.details.isNotEmpty()) {
            val fold = ui.text(model.details.joinToString("\n"), 13f, mono = true).apply { visibility = GONE }
            val toggle = ui.text("${WordsModel.DETAILS_TITLE} (show)", 14f, bold = true, colour = 0xFF1565C0.toInt())
            toggle.setPadding(0, ui.px(12), 0, ui.px(4))
            toggle.setOnClickListener {
                val open = fold.visibility != VISIBLE
                fold.visibility = if (open) VISIBLE else GONE
                toggle.text = "${WordsModel.DETAILS_TITLE} (${if (open) "hide" else "show"})"
            }
            addView(toggle)
            addView(fold)
        }
        filterTouchesWhenObscured = true
    }
}

/** Plain `android.widget` views made in code. */
class Ui(private val context: Context) {
    private val density = context.resources.displayMetrics.density

    fun px(dp: Int): Int = (dp * density).toInt()

    fun margins(top: Int = 0, bottom: Int = 0): LinearLayout.LayoutParams =
        LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT)
            .also { it.setMargins(0, px(top), 0, px(bottom)) }

    fun text(
        text: String,
        size: Float = 16f,
        bold: Boolean = false,
        mono: Boolean = false,
        colour: Int = Color.BLACK,
    ): TextView = TextView(context).apply {
        this.text = text
        textSize = size
        setTextColor(colour)
        if (bold) setTypeface(typeface, Typeface.BOLD)
        if (mono) typeface = Typeface.MONOSPACE
        filterTouchesWhenObscured = true
    }

    /** Text from elsewhere: a marking line, then the text in quotation marks (SC-08). */
    fun quoted(label: String, quotedText: String): View = LinearLayout(context).apply {
        orientation = LinearLayout.VERTICAL
        setBackgroundColor(0xFFECEFF1.toInt())
        setPadding(px(12), px(8), px(12), px(8))
        addView(text(label, 13f, colour = 0xFF455A64.toInt()))
        addView(text(quotedText, 16f, mono = true))
    }

    fun words(model: WordsModel): View = WordsView(context, model)

    fun button(label: String, onClick: () -> Unit): Button = Button(context).apply {
        text = label
        isAllCaps = false
        setOnClickListener { onClick() }
        filterTouchesWhenObscured = true
    }

    /**
     * A field for typed text. No keyboard may suggest, remember or learn what
     * is typed: the type is "visible password without suggestions" and the
     * keyboard is asked not to learn (design: no third-party keyboard for words).
     */
    fun field(hint: String, lines: Int = 1): EditText = EditText(context).apply {
        this.hint = hint
        var type = InputType.TYPE_CLASS_TEXT or
            InputType.TYPE_TEXT_FLAG_NO_SUGGESTIONS or
            InputType.TYPE_TEXT_VARIATION_VISIBLE_PASSWORD
        if (lines > 1) type = type or InputType.TYPE_TEXT_FLAG_MULTI_LINE
        inputType = type
        imeOptions = EditorInfo.IME_FLAG_NO_PERSONALIZED_LEARNING or EditorInfo.IME_FLAG_NO_EXTRACT_UI
        if (lines > 1) minLines = lines
        filterTouchesWhenObscured = true
    }

    fun choice(options: List<String>, selected: Int = 0): RadioGroup = RadioGroup(context).apply {
        orientation = RadioGroup.VERTICAL
        for ((i, label) in options.withIndex()) {
            addView(
                RadioButton(context).apply {
                    text = label
                    id = View.generateViewId()
                    isChecked = i == selected
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

    /** A scrolling page of [views] in a column. */
    fun page(views: List<View>): View {
        val column = LinearLayout(context).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(px(16), px(24), px(16), px(24))
        }
        for (v in views) column.addView(v, margins(bottom = 12))
        return ScrollView(context).apply {
            addView(column)
            isFillViewport = true
            filterTouchesWhenObscured = true
        }
    }
}
