package org.openbmt.owner.app

import android.app.Activity
import android.content.Intent
import android.os.Bundle
import android.view.View
import android.view.WindowManager

/**
 * The one activity of the owner app, and the only exported component. The
 * node runs while the activity is visible (onStart to onStop); there is no
 * foreground service and no notification, so the app must be open for the
 * wallet to be followed (OA-06). FLAG_SECURE is set for the window before any
 * content is shown (OA-04): the screens show balances, addresses and the
 * requests for the Key app, and neither a screenshot, a screen recording nor
 * the picture of the app in the list of recent apps may carry them.
 */
class MainActivity : Activity() {
    private lateinit var controller: Controller

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        window.addFlags(WindowManager.LayoutParams.FLAG_SECURE)
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
