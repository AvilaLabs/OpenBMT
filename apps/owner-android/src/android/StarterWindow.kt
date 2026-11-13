package org.openbmt.owner.app

import android.app.Activity
import android.app.AppOpsManager
import android.app.KeyguardManager
import android.content.Context
import android.content.pm.ApplicationInfo
import android.content.pm.PackageManager
import android.accessibilityservice.AccessibilityServiceInfo
import android.graphics.Bitmap
import android.graphics.Color
import android.os.Build
import android.provider.Settings
import android.view.View
import android.view.ViewGroup
import android.view.WindowManager
import android.view.accessibility.AccessibilityManager
import android.widget.ImageView
import java.io.File
import java.time.LocalDate
import org.openbmt.owner.StarterImage

// What the starter's screens need of the window and of the phone: the flags of every screen that shows or takes
// words (SS-12), the picture of four words (SS-11) and what the phone says of itself (SS-33, SS-34). The decisions
// about what is found are in `StarterModel.kt`.

/** SS-12: the one helper every screen that shows or takes words goes through. */
internal object StarterWindow {
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

/**
 * SS-11: a screen of four words as the core drew it, shown as a bitmap and then cleared. The pixels the core gave
 * are overwritten as soon as the bitmap is made; [clear] recycles the bitmap and takes it off the view.
 */
internal class StarterWordsImage(act: Activity, private val view: ImageView) {
    private var bitmap: Bitmap? = null

    init {
        view.scaleType = ImageView.ScaleType.FIT_CENTER
        view.adjustViewBounds = true
        view.importantForAccessibility = View.IMPORTANT_FOR_ACCESSIBILITY_NO
        StarterWindow.sensitive(view)
    }

    /** Draws [image] (one byte for each pixel, 0 black to 255 white) and overwrites it. */
    fun show(image: StarterImage) {
        val w = image.width.toInt()
        val h = image.height.toInt()
        val pixels = IntArray(w * h)
        try {
            for (i in pixels.indices) {
                val g = image.pixels[i].toInt() and 0xff
                pixels[i] = Color.rgb(g, g, g)
            }
            val b = Bitmap.createBitmap(w, h, Bitmap.Config.ARGB_8888)
            b.setPixels(pixels, 0, w, 0, 0, w, h)
            bitmap = b
            view.setImageBitmap(b)
        } finally {
            pixels.fill(0)
            image.wipe()
        }
    }

    /** Takes the picture off the screen and recycles it. */
    fun clear() {
        view.setImageDrawable(null)
        bitmap?.recycle()
        bitmap = null
    }
}

// ---- what the phone says of itself (SS-33, SS-34) ---------------------------------------------------------------------

/**
 * Gathers what the checks look at. Best effort, and what each part can and cannot see: USB debugging and the screen lock
 * are read exactly; root is the usual `su` paths and the build's test-keys tag, which a hidden root passes; the enabled
 * accessibility services are the system's own list, with the tool mark from Android 12; apps that may draw over others
 * are only those the system shows this app (since Android 11 an app sees few others without the permission to list all
 * of them, which it does not ask for) and whose overlay permission is on, and a phone with none found may still have one.
 */
internal fun Controller.starterEnvironment(): StarterEnvironment {
    val ctx: Context = act
    val adb = try {
        Settings.Global.getInt(ctx.contentResolver, Settings.Global.ADB_ENABLED, 0) == 1
    } catch (_: SecurityException) {
        false
    }
    val su = StarterChecks.SU_PATHS.filter { File(it).exists() }
    val testKeys = Build.TAGS?.contains("test-keys") == true
    val pm = ctx.packageManager
    val services = ArrayList<AccessibilityEntry>()
    val manager = ctx.getSystemService(AccessibilityManager::class.java)
    for (s in manager?.getEnabledAccessibilityServiceList(AccessibilityServiceInfo.FEEDBACK_ALL_MASK).orEmpty()) {
        val name = s.resolveInfo?.loadLabel(pm)?.toString()?.ifBlank { null } ?: s.id ?: "?"
        val tool = Build.VERSION.SDK_INT >= 31 && s.isAccessibilityTool
        services.add(AccessibilityEntry(name, tool))
    }
    val lock = ctx.getSystemService(KeyguardManager::class.java)?.isDeviceSecure == true
    return StarterEnvironment(adb, su, testKeys, overlayApps(ctx), services, Build.VERSION.SECURITY_PATCH, lock)
}

@Suppress("DEPRECATION")
private fun overlayApps(ctx: Context): List<String> = try {
    val pm = ctx.packageManager
    val ops = ctx.getSystemService(AppOpsManager::class.java)
    pm.getInstalledPackages(PackageManager.GET_PERMISSIONS).filter { p ->
        val info = p.applicationInfo
        p.packageName != ctx.packageName && info != null && (info.flags and ApplicationInfo.FLAG_SYSTEM) == 0 &&
            p.requestedPermissions?.contains("android.permission.SYSTEM_ALERT_WINDOW") == true &&
            ops != null && ops.unsafeCheckOpNoThrow("android:system_alert_window", info.uid, p.packageName) == AppOpsManager.MODE_ALLOWED
    }.map { it.applicationInfo?.let { i -> pm.getApplicationLabel(i).toString() } ?: it.packageName }
} catch (_: SecurityException) {
    emptyList()
} catch (_: RuntimeException) {
    emptyList()
}

internal fun today(): LocalDate = LocalDate.now(java.time.ZoneOffset.UTC)
