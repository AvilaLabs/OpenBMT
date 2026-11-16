package org.openbmt.counter.app

import android.app.Activity
import android.content.res.Configuration
import android.os.Bundle
import android.view.View
import android.view.ViewGroup
import android.view.WindowInsets
import android.view.WindowManager
import android.widget.LinearLayout
import org.openbmt.app.applyNight
import org.openbmt.app.isNight

/**
 * The one activity of the counter app, and the only exported component. The
 * node runs while the activity is visible (onStart to onStop), with the
 * screen kept on; there is no foreground service and no notification, so the
 * app is kept open at the counter. FLAG_SECURE is not set on the screens in
 * general: the counter holds no spending key, and the sale screen is meant to
 * be seen by the customer. It is set only while the screen that takes the
 * shop's Lightning credentials is on show (CR-115).
 */
class MainActivity : Activity() {
    private lateinit var controller: Controller

    /** Night mode when the screens were drawn: a change draws the current screen again in place, with the activity kept. */
    private var night = false

    override fun onConfigurationChanged(newConfig: Configuration) {
        super.onConfigurationChanged(newConfig)
        if (isNight(newConfig) != night) {
            night = isNight(newConfig)
            applyNight()
            controller.redraw()
            window.decorView.requestApplyInsets()
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        night = isNight(resources.configuration)
        window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        controller = Controller(this)
    }

    override fun onStart() {
        super.onStart()
        controller.visible()
    }

    override fun onResume() {
        super.onResume()
        controller.tapFront(true)
    }

    override fun onPause() {
        controller.tapFront(false)
        super.onPause()
    }

    override fun onStop() {
        controller.hidden()
        super.onStop()
    }

    override fun onDestroy() {
        controller.hidden()
        super.onDestroy()
    }

    override fun onRequestPermissionsResult(requestCode: Int, permissions: Array<out String>, grantResults: IntArray) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults)
        controller.permission(requestCode, grantResults)
    }

    /** GV-14: every touch starts the count of a giving point left alone again. */
    override fun dispatchTouchEvent(ev: android.view.MotionEvent): Boolean {
        controller.touched()
        return super.dispatchTouchEvent(ev)
    }

    /** The screen on show, and the notices of CR-284 that stand above it. */
    private var current: View? = null
    private var notices: List<View> = emptyList()

    /** Shows [page] as the whole content; the flag of [keepOutOfCapture] does not carry over to it. */
    fun show(page: View) {
        window.clearFlags(WindowManager.LayoutParams.FLAG_SECURE)
        current = page
        place()
    }

    /**
     * CR-284: the notices stand one under the other at the top of whatever screen shows, the PIN screen included.
     * The screen is placed under them, so that they never cover it. With none, the screen is the whole content.
     */
    fun setNotices(views: List<View>) {
        notices = views
        place()
    }

    private fun place() {
        val page = current ?: return
        (page.parent as? ViewGroup)?.removeView(page)
        if (notices.isEmpty()) {
            setContentView(page)
            return
        }
        val gap = (resources.displayMetrics.density * 8).toInt()
        val column = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(gap, 0, gap, 0)
            setOnApplyWindowInsetsListener { v, insets ->
                v.setPadding(gap, topInset(insets) + gap, gap, 0)
                insets
            }
        }
        for (view in notices) {
            (view.parent as? ViewGroup)?.removeView(view)
            column.addView(view, LinearLayout.LayoutParams(-1, -2).also { it.bottomMargin = gap })
        }
        val shell = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL }
        shell.addView(column, LinearLayout.LayoutParams(-1, -2))
        shell.addView(page, LinearLayout.LayoutParams(-1, 0, 1f))
        setContentView(shell)
        column.requestApplyInsets()
    }

    @Suppress("DEPRECATION")
    private fun topInset(insets: WindowInsets): Int =
        if (android.os.Build.VERSION.SDK_INT >= 30) insets.getInsets(WindowInsets.Type.systemBars() or WindowInsets.Type.displayCutout()).top else insets.systemWindowInsetTop

    /** Keeps the screen on show out of screenshots, screen recordings and the recent-apps preview, until the next screen is shown. */
    fun keepOutOfCapture() {
        window.addFlags(WindowManager.LayoutParams.FLAG_SECURE)
    }
}
