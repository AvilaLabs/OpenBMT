package org.openbmt.key.app

import android.view.View
import org.openbmt.app.Ui
import org.openbmt.app.WordsModel
import org.openbmt.key.Words

/** One `Words` of the Key binding on screen. */
fun Ui.words(w: Words): View = words(WordsModel.of(w.say()))
