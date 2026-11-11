package org.openbmt.counter.app

import android.app.Activity
import android.content.Intent
import android.os.Bundle
import android.view.View
import android.view.WindowManager

/**
 * The one activity of the counter app, and the only exported component. The
 * node runs while the activity is visible (onStart to onStop), with the
 * screen kept on; there is no foreground service and no notification, so the
 * app is kept open at the counter. FLAG_SECURE is not set: the counter holds
 * no spending key, and the sale screen is meant to be seen by the customer.
 */
class MainActivity : Activity() {
    private lateinit var controller: Controller

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        controller = Controller(this)
    }

    override fun onStart() {
        super.onStart()
        controller.visible()
    }

    override fun onStop() {
        controller.hidden()
        super.onStop()
    }

    override fun onDestroy() {
        controller.hidden()
        super.onDestroy()
    }

    @Suppress("DEPRECATION")
    @Deprecated("The platform's own result call; no AndroidX is used")
    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        super.onActivityResult(requestCode, resultCode, data)
        controller.picked(requestCode, resultCode, data)
    }

    override fun onRequestPermissionsResult(requestCode: Int, permissions: Array<out String>, grantResults: IntArray) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults)
        controller.permission(requestCode, grantResults)
    }

    /** Shows [page] as the whole content. */
    fun show(page: View) {
        setContentView(page)
    }
}
