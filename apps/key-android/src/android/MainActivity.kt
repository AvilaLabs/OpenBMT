package org.openbmt.key.app

import android.app.Activity
import android.content.Intent
import android.content.res.Configuration
import android.os.Bundle
import android.view.Gravity
import android.view.View
import android.view.WindowManager
import android.widget.LinearLayout
import android.widget.TextView
import android.graphics.Color
import android.graphics.drawable.ColorDrawable
import org.openbmt.app.Feedback
import org.openbmt.app.Palette

/**
 * The one activity of the Key app, and the only exported component. Every
 * window of the app carries FLAG_SECURE: no screenshot, no screen recording
 * and a blank preview in the recent-apps list. There is no dialog, because a
 * dialog is another window that would need the flag too.
 */
class MainActivity : Activity() {
    private lateinit var controller: Controller

    /**
     * The Key app is always dark, so a change between light and dark has nothing to redraw (SR-83): the window,
     * the bars' icons and every screen are the dark scheme whatever the phone's mode.
     */
    override fun onConfigurationChanged(newConfig: Configuration) {
        super.onConfigurationChanged(newConfig)
        controller.redraw()
    }

    @Suppress("DEPRECATION")
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        window.setFlags(WindowManager.LayoutParams.FLAG_SECURE, WindowManager.LayoutParams.FLAG_SECURE)
        window.setBackgroundDrawable(ColorDrawable(Palette.dark.background))
        window.statusBarColor = Color.TRANSPARENT
        window.navigationBarColor = Color.TRANSPARENT
        // This app plays no sound at all; it keeps the haptics of the hold button, the scan and the wrong PIN.
        Feedback.soundOn = false
        controller = Controller(this)
        controller.start()
    }

    override fun onResume() {
        super.onResume()
        controller.resumed()
    }

    override fun onPause() {
        controller.paused()
        super.onPause()
    }

    override fun onDestroy() {
        controller.close()
        super.onDestroy()
    }

    @Suppress("DEPRECATION")
    @Deprecated("The platform's own result call; no AndroidX is used")
    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        super.onActivityResult(requestCode, resultCode, data)
        controller.picked(requestCode, resultCode, data)
    }

    /** The page on the screen now, so that a scan can return to it with what was typed on it. */
    var current: View? = null
        private set

    override fun onRequestPermissionsResult(requestCode: Int, permissions: Array<out String>, grantResults: IntArray) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults)
        controller.permission(requestCode, grantResults)
    }

    /** Shows [page] as the whole content. */
    fun show(page: View) {
        current = page
        setContentView(if (DevCopy.ON) withBand(page) else page)
    }

    /** The development copy's band above every screen (SR-91), in the stop tone. */
    @Suppress("DEPRECATION")
    private fun withBand(page: View): View {
        val density = resources.displayMetrics.density
        val tone = Palette.dark.stop
        val band = TextView(this).apply {
            text = DevCopy.BAND
            setTextColor(Palette.WHITE)
            setBackgroundColor(tone.band)
            textSize = 14f
            gravity = Gravity.CENTER
            val pad = (8 * density).toInt()
            setPadding(pad, pad, pad, pad)
            setOnApplyWindowInsetsListener { v, insets ->
                v.setPadding(pad, pad + insets.systemWindowInsetTop, pad, pad)
                insets
            }
        }
        return LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            addView(band, LinearLayout.LayoutParams(-1, -2))
            addView(page, LinearLayout.LayoutParams(-1, 0, 1f))
        }
    }
}
