package org.openbmt.app

import android.app.Activity
import android.os.Build
import android.view.View
import android.view.ViewGroup
import android.view.WindowManager

/**
 * The one helper every screen that must not be seen or driven by another app goes through (SS-12, SS-33): the
 * screens that show or take words, the Key app's screens, and the screens that pay.
 */
object WindowGuard {
    /**
     * `FLAG_SECURE` (no screenshot, no recording, blank in recent apps), other apps' overlay windows hidden (Android 12 and
     * later), and touches made through an overlay dropped on every view of [root].
     */
    fun protect(act: Activity, root: View) {
        act.window.addFlags(WindowManager.LayoutParams.FLAG_SECURE)
        if (Build.VERSION.SDK_INT >= 31) act.window.setHideOverlayWindows(true)
        filterTouches(root)
    }

    /** Every other screen lets overlays be drawn again. */
    fun release(act: Activity) {
        if (Build.VERSION.SDK_INT >= 31) act.window.setHideOverlayWindows(false)
    }

    private fun filterTouches(view: View) {
        view.filterTouchesWhenObscured = true
        if (view is ViewGroup) for (i in 0 until view.childCount) filterTouches(view.getChildAt(i))
    }

    /** Marks the words image and the keypad as sensitive for accessibility services (Android 14 and later). */
    fun sensitive(view: View) {
        if (Build.VERSION.SDK_INT >= 34) view.setAccessibilityDataSensitive(View.ACCESSIBILITY_DATA_SENSITIVE_YES)
        if (view is ViewGroup) for (i in 0 until view.childCount) sensitive(view.getChildAt(i))
    }
}
