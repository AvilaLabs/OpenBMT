package org.openbmt.counter.app

import android.app.Activity
import android.content.res.Configuration
import android.os.Bundle
import android.view.View
import android.view.WindowManager
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

    /** Shows [page] as the whole content; the flag of [keepOutOfCapture] does not carry over to it. */
    fun show(page: View) {
        window.clearFlags(WindowManager.LayoutParams.FLAG_SECURE)
        setContentView(page)
    }

    /** Keeps the screen on show out of screenshots, screen recordings and the recent-apps preview, until the next screen is shown. */
    fun keepOutOfCapture() {
        window.addFlags(WindowManager.LayoutParams.FLAG_SECURE)
    }
}
