package org.openbmt.key.app

import android.app.Activity
import android.content.res.Configuration
import android.os.Handler
import android.os.Looper
import android.view.ContextThemeWrapper
import android.view.View
import android.view.ViewGroup
import android.widget.Button
import android.widget.FrameLayout
import android.widget.LinearLayout
import android.view.SurfaceView
import android.widget.TextView
import org.openbmt.app.Colour
import org.openbmt.app.Fills
import org.openbmt.app.FullWidth
import org.openbmt.app.Ui
import org.openbmt.app.WordsModel
import org.openbmt.key.Words

/** One `Words` of the Key binding on screen. */
fun Ui.words(w: Words): View = words(WordsModel.of(w.say()))

/**
 * The Key app always draws in the dark scheme, whatever the phone's mode, so that the key phone never looks like
 * the owner's. [Ui] reads night mode from the configuration of the context it is made with, so it is made with
 * this one: the activity, with the night bit of its configuration set.
 */
class DarkContext(base: Activity) : ContextThemeWrapper(base, base.applicationInfo.theme) {
    init {
        val config = Configuration(base.resources.configuration)
        config.uiMode = (config.uiMode and Configuration.UI_MODE_NIGHT_MASK.inv()) or Configuration.UI_MODE_NIGHT_YES
        applyOverrideConfiguration(config)
    }
}

/** A body view that takes the height that is left and runs edge to edge: the scanner. */
private class FillBody(context: android.content.Context) : FrameLayout(context), Fills, FullWidth

/**
 * The scanner page on the camera frame. The shared scan builds a plain column (a title, a hint, the words and
 * progress of the reading, the preview and Cancel); this takes the preview and the Cancel button out of it, shows
 * the words over the bottom of the preview and the progress as the pill, and puts Cancel in the footer. The
 * column's text is the app's own: nothing is added.
 */
fun scanPage(ui: Ui, column: View): View {
    val col = column as? LinearLayout ?: return column
    val title = (col.getChildAt(0) as? TextView)?.text?.toString() ?: ""
    val hint = (col.getChildAt(1) as? TextView)?.text?.toString() ?: ""
    val status = col.getChildAt(2) as? LinearLayout
    var surface: SurfaceView? = null
    var cancel: Button? = null
    for (i in 0 until col.childCount) {
        val c = col.getChildAt(i)
        if (c is SurfaceView) surface = c
        if (c is Button) cancel = c
    }
    val size = surface?.layoutParams
    col.removeAllViews()
    val frame = ui.cameraFrame(surface)
    val body = FillBody(ui.context)
    body.addView(frame, FrameLayout.LayoutParams(-1, -1))
    val aspect = if (size != null && size.width > 0 && size.height > 0) size.width.toDouble() / size.height else 0.0
    if (surface != null && aspect > 0.0) {
        // The preview keeps its own shape and covers the body.
        val view = surface
        frame.addOnLayoutChangeListener { _, l, t, r, b, _, _, _, _ ->
            val w = r - l
            val h = b - t
            if (w > 0 && h > 0) {
                val wide = w.toDouble() / h > aspect
                val pw = if (wide) w else (h * aspect).toInt()
                val ph = if (wide) (w / aspect).toInt() else h
                val now = view.layoutParams
                if (now == null || now.width != pw || now.height != ph) {
                    view.layoutParams = FrameLayout.LayoutParams(pw, ph, android.view.Gravity.CENTER)
                }
            }
        }
    }
    fun show() {
        val words = status?.getChildAt(0) as? ViewGroup
        val progress = (status?.getChildAt(1) as? TextView)?.text?.toString()
        val read = words?.let { readWords(it) }
        frame.words(read ?: model(title, hint, ""))
        frame.progress(progress)
    }
    if (status != null) {
        // Kept on the screen, unseen, so that the scan's haptics reach a window.
        status.visibility = View.INVISIBLE
        status.importantForAccessibility = View.IMPORTANT_FOR_ACCESSIBILITY_NO_HIDE_DESCENDANTS
        body.addView(status, FrameLayout.LayoutParams(1, 1))
        val main = Handler(Looper.getMainLooper())
        status.setOnHierarchyChangeListener(
            object : ViewGroup.OnHierarchyChangeListener {
                override fun onChildViewAdded(parent: View?, child: View?) {
                    main.post { show() }
                }

                override fun onChildViewRemoved(parent: View?, child: View?) {}
            },
        )
    }
    show()
    val back = cancel
    return ui.page(
        ui.bar(title, back = if (back != null) ({ back.performClick() }) else null, onBlack = true),
        listOf(body),
        listOfNotNull(back),
        scroll = false,
    )
}

private fun model(heading: String, what: String, next: String) =
    WordsModel(Colour.Blue, heading, what, next, "", null, null, emptyList())

/** The heading, the what and the next line out of a words view the shared scan made; null if its shape is not the expected one. */
private fun readWords(view: ViewGroup): WordsModel? {
    val band = view.getChildAt(0) as? ViewGroup ?: return null
    val body = view.getChildAt(1) as? ViewGroup ?: return null
    val heading = (band.getChildAt(0) as? TextView)?.text?.toString() ?: return null
    val what = (body.getChildAt(0) as? TextView)?.text?.toString() ?: return null
    val next = (body.getChildAt(1) as? TextView)?.text?.toString() ?: return null
    return model(heading, what, next)
}
