package org.openbmt.counter.app

import android.content.Context
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Paint
import android.text.InputType
import android.view.View
import android.view.inputmethod.EditorInfo
import android.widget.EditText
import android.widget.LinearLayout
import org.openbmt.app.SayWords
import org.openbmt.app.Ui
import org.openbmt.app.WordsModel
import org.openbmt.counter.Payment

/** One words on screen: the colour of its tone, the three lines, who, and the fold of details. */
fun Ui.words(w: SayWords): View = words(WordsModel.of(w))

/**
 * A field for a PIN: digits only, shown as dots, no suggestions and no learning.
 * The counter holds no spending key, and the screen may be shown to a customer, so
 * a PIN is the one thing typed here that is hidden.
 */
fun pinField(context: Context, hint: String): EditText = EditText(context).apply {
    this.hint = hint
    inputType = InputType.TYPE_CLASS_NUMBER or InputType.TYPE_NUMBER_VARIATION_PASSWORD
    imeOptions = EditorInfo.IME_FLAG_NO_PERSONALIZED_LEARNING or EditorInfo.IME_FLAG_NO_EXTRACT_UI
    filterTouchesWhenObscured = true
}

/** A field for a number with an optional point. */
fun amountField(context: Context, hint: String): EditText = EditText(context).apply {
    this.hint = hint
    inputType = InputType.TYPE_CLASS_NUMBER or InputType.TYPE_NUMBER_FLAG_DECIMAL
    imeOptions = EditorInfo.IME_FLAG_NO_PERSONALIZED_LEARNING or EditorInfo.IME_FLAG_NO_EXTRACT_UI
    filterTouchesWhenObscured = true
}

/** A container whose content is replaced as a screen updates, so that a field elsewhere keeps its focus. */
class Slot(context: Context) : LinearLayout(context) {
    init {
        orientation = VERTICAL
    }

    fun show(vararg views: View) {
        removeAllViews()
        for (v in views) addView(v)
    }
}

/**
 * The payment code of a sale, drawn from the library's modules: black
 * squares on white, a quiet zone of 4 squares on every side, whole pixels
 * for each square, as large as fits (the width of the view, but never more
 * than [maxHeightPx]). The mapping of modules to pixels is [QrLayout].
 */
class QrView(context: Context, private var payment: Payment, private val maxHeightPx: Int) : View(context) {
    private val black = Paint().apply { color = Color.BLACK; style = Paint.Style.FILL; isAntiAlias = false }

    init {
        contentDescription = "Payment code for the customer to scan"
    }

    fun show(p: Payment) {
        payment = p
        invalidate()
    }

    override fun onMeasure(widthMeasureSpec: Int, heightMeasureSpec: Int) {
        val w = MeasureSpec.getSize(widthMeasureSpec)
        val side = minOf(w, maxHeightPx)
        setMeasuredDimension(w, side)
    }

    override fun onDraw(canvas: Canvas) {
        canvas.drawColor(Color.WHITE)
        val n = payment.qrSize.toInt()
        if (n <= 0) return
        val box = QrLayout.box(n, width, height)
        if (!box.fits) return
        val cell = box.cell.toFloat()
        for (y in 0 until n) {
            for (x in 0 until n) {
                if (payment.module(x, y)) {
                    val left = box.x(x).toFloat()
                    val top = box.y(y).toFloat()
                    canvas.drawRect(left, top, left + cell, top + cell, black)
                }
            }
        }
    }
}
