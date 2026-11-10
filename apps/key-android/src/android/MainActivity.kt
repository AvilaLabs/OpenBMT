package org.openbmt.key.app

import android.app.Activity
import android.content.Intent
import android.os.Bundle
import android.view.View
import android.view.WindowManager

/**
 * The one activity of the Key app, and the only exported component. Every
 * window of the app carries FLAG_SECURE: no screenshot, no screen recording
 * and a blank preview in the recent-apps list. There is no dialog, because a
 * dialog is another window that would need the flag too.
 */
class MainActivity : Activity() {
    private lateinit var controller: Controller

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        window.setFlags(WindowManager.LayoutParams.FLAG_SECURE, WindowManager.LayoutParams.FLAG_SECURE)
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

    /** Shows [page] as the whole content. */
    fun show(page: View) {
        setContentView(page)
    }
}
