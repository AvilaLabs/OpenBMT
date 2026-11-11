package org.openbmt.owner.app

import android.content.Context
import android.text.InputType
import android.view.View
import android.view.inputmethod.EditorInfo
import android.widget.EditText
import android.widget.LinearLayout
import org.openbmt.app.SayWords
import org.openbmt.app.Ui
import org.openbmt.app.WordsModel

/** One words on screen: the colour of its tone, the three lines, who, and the fold of details. */
fun Ui.words(w: SayWords): View = words(WordsModel.of(w))

/** A field for a whole number: digits only, no suggestions and no learning. */
fun numberField(context: Context, hint: String): EditText = EditText(context).apply {
    this.hint = hint
    inputType = InputType.TYPE_CLASS_NUMBER
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
