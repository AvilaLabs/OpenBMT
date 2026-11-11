package org.openbmt.app

import android.Manifest
import android.app.Activity
import android.content.pm.PackageManager
import android.os.Handler
import android.os.Looper
import android.view.SurfaceHolder
import android.view.SurfaceView
import android.view.View
import android.view.ViewGroup
import android.widget.LinearLayout

/** Why a scan did not happen: the camera is off for the app ([off]), or it did not start ([problem] is the system's text). */
class ScanFailure(val off: Boolean, val problem: String?)

/**
 * The scan, from the person's tap to the bytes of a code: asks for the camera permission only now (at
 * run time, when Scan is tapped), shows a page with a preview and a Cancel button, runs a [QrScanner]
 * and gives the bytes of the first code to the caller. Nothing is kept: no frame, no code.
 *
 * Every call is on the main thread. A caller that clears its screen when the activity pauses calls [stop]
 * there, and holds back that clearing while [asking] is true, because the system's permission dialog
 * pauses the activity too. [permissionResult] is the activity's `onRequestPermissionsResult`.
 */
class ScanFlow(private val act: Activity, private val ui: Ui) {
    private val main = Handler(Looper.getMainLooper())
    private var scanner: QrScanner? = null
    private var pending: Request? = null
    private var generation = 0

    /** True while the system's permission dialog is open. */
    var asking = false
        private set

    private class Request(
        val show: (View) -> Unit,
        val onBytes: (ByteArray) -> Unit,
        val onFailed: (ScanFailure) -> Unit,
        val onCancel: () -> Unit,
    )

    /**
     * Starts a scan. [show] puts a page on the screen. Exactly one of [onBytes] (the first code's bytes),
     * [onFailed] and [onCancel] is called later, unless [stop] comes first.
     */
    fun begin(show: (View) -> Unit, onBytes: (ByteArray) -> Unit, onFailed: (ScanFailure) -> Unit, onCancel: () -> Unit) {
        stop()
        val request = Request(show, onBytes, onFailed, onCancel)
        if (act.checkSelfPermission(Manifest.permission.CAMERA) == PackageManager.PERMISSION_GRANTED) {
            scan(request)
            return
        }
        pending = request
        asking = true
        act.requestPermissions(arrayOf(Manifest.permission.CAMERA), REQUEST_CAMERA)
    }

    /** The answer of the permission dialog. Denied and "don't ask again" both arrive here as denied. */
    fun permissionResult(code: Int, results: IntArray) {
        if (code != REQUEST_CAMERA) return
        asking = false
        val request = pending ?: return
        pending = null
        when {
            results.isEmpty() -> request.onCancel()
            results[0] == PackageManager.PERMISSION_GRANTED -> scan(request)
            else -> request.onFailed(ScanFailure(true, null))
        }
    }

    /** Releases the camera and forgets the request; no callback is made. Safe at any time. */
    fun stop() {
        generation++
        scanner?.stop()
        scanner = null
        pending = null
        asking = false
    }

    private fun scan(request: Request) {
        val mine = ++generation
        val s = QrScanner(
            act, main,
            { bytes ->
                if (mine == generation) {
                    stop()
                    request.onBytes(bytes)
                }
            },
            { text ->
                if (mine == generation) {
                    stop()
                    request.onFailed(ScanFailure(false, text))
                }
            },
        )
        val preview = try {
            s.prepare()
        } catch (e: IllegalStateException) {
            request.onFailed(ScanFailure(false, e.message))
            return
        }
        scanner = s
        val column = LinearLayout(act).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(ui.px(16), ui.px(24), ui.px(16), ui.px(24))
            filterTouchesWhenObscured = true
        }
        column.addView(ui.text("Scan the code", 20f, bold = true), ui.margins(bottom = 8))
        column.addView(ui.text("Hold the phone still with the whole code in view. It is read when it is steady.", 14f), ui.margins(bottom = 12))
        if (preview != null) {
            val v = SurfaceView(act)
            v.holder.setFixedSize(preview.first, preview.second)
            v.holder.addCallback(
                object : SurfaceHolder.Callback {
                    override fun surfaceCreated(holder: SurfaceHolder) {
                        if (mine == generation) s.start(holder.surface)
                    }

                    override fun surfaceChanged(holder: SurfaceHolder, format: Int, width: Int, height: Int) {}

                    override fun surfaceDestroyed(holder: SurfaceHolder) {
                        s.stop()
                    }
                },
            )
            // The sensor's long side runs along the phone's long side: a portrait screen shows the
            // preview turned, so its shape is the long side over the short one.
            val aspect = maxOf(preview.first, preview.second).toDouble() / minOf(preview.first, preview.second)
            val metrics = act.resources.displayMetrics
            val portrait = metrics.heightPixels >= metrics.widthPixels
            val maxWidth = (metrics.widthPixels - 2 * ui.px(16)).toDouble()
            val maxHeight = metrics.heightPixels * 0.6
            val width: Int
            val height: Int
            if (portrait) {
                height = minOf(maxWidth * aspect, maxHeight).toInt()
                width = (height / aspect).toInt()
            } else {
                height = minOf(maxHeight, maxWidth / aspect).toInt()
                width = (height * aspect).toInt()
            }
            column.addView(v, LinearLayout.LayoutParams(width, height))
        }
        column.addView(
            ui.button("Cancel") {
                stop()
                request.onCancel()
            },
            ui.margins(top = 12),
        )
        column.layoutParams = ViewGroup.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT)
        request.show(column)
        if (preview == null) s.start(null)
    }

    companion object {
        /** The request code of the permission dialog. */
        const val REQUEST_CAMERA = 7301
    }
}
