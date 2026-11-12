package org.openbmt.app

import android.Manifest
import android.app.Activity
import android.content.pm.PackageManager
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.view.SurfaceHolder
import android.view.SurfaceView
import android.view.View
import android.view.ViewGroup
import android.widget.LinearLayout

/**
 * Why a scan did not happen: the camera is off for the app ([off]), or it did not start ([problem] is the
 * system's text), or the reading of an animated code stopped it, and [words] say why.
 */
class ScanFailure(val off: Boolean, val problem: String?, val words: SayWords? = null)

/**
 * A scan that also takes a code that changes: a code whose text starts with "ur:" is read as a UR, part
 * after part, and [onMessage] is called with the type and the payload when it is complete and its type is
 * one of [types]. [who] is the one who does the next step, for the words shown while reading.
 */
class UrScan(
    val types: Set<String>,
    val who: Int,
    val parts: PartsProgress? = null,
    val onMessage: (type: String, payload: ByteArray) -> Unit,
)

/**
 * A scan that reads a wallet's code that changes: every part whose text starts with "ur:" goes to the wallet
 * reader of the library, whatever its type, and [onWallet] is called with the descriptor text (as ASCII bytes, which
 * the callee wipes) when it is complete and valid. A first code that does not start with "ur:" is a plain code and
 * still goes to the scan's `onBytes`. The words while it reads are the reading words of [UrWords], which say how many
 * parts are in; [line] is the line under them for the parts read and the parts expected.
 */
class WalletScan(val who: Int, val line: (read: Int, of: Int) -> String, val onWallet: (text: ByteArray) -> Unit)

/**
 * The words and the progress line of a scan of parts that come in any order: [words] are shown while the set
 * is not complete, and [line] gives the line under them for the parts read and the parts that are expected.
 */
class PartsProgress(val words: SayWords, val line: (read: Int, of: Int) -> String)

/**
 * A scan that also takes a BBQr set: a code whose text starts with "B$" is a part, read in any order with
 * the joiner of the library, and [onMessage] is called with the kind and the file when every part is read and
 * the kind is one of [kinds]. [parts] says how far the reading is.
 */
class BbqrScan(val kinds: Set<Char>, val who: Int, val parts: PartsProgress, val onMessage: (kind: Char, payload: ByteArray) -> Unit)

/**
 * The scan, from the person's tap to the bytes of a code: asks for the camera permission only now (at
 * run time, when Scan is tapped), shows a page with a preview and a Cancel button, runs a [QrScanner]
 * and gives the bytes of the first code to the caller. Nothing is kept: no frame, no code. With a
 * [UrScan] the scan goes on after a code that starts with "ur:", feeds each part to one decoder of the
 * library, shows the progress in words and finishes when the message is complete; the decoder is dropped
 * when the scan finishes, is cancelled or stopped.
 *
 * Every call is on the main thread. A caller that clears its screen when the activity pauses calls [stop]
 * there, and holds back that clearing while [asking] is true, because the system's permission dialog
 * pauses the activity too; [askedAt] says since when, for the 60-second relock (SR-77).
 * [permissionResult] is the activity's `onRequestPermissionsResult`.
 */
class ScanFlow(private val act: Activity, private val ui: Ui) {
    private val main = Handler(Looper.getMainLooper())
    private var scanner: QrScanner? = null
    private var reading: UrReading? = null
    private var walletReading: WalletReading? = null
    private var joining: BbqrReading? = null
    private var pending: Request? = null
    private var generation = 0

    /** True while the system's permission dialog is open. */
    var asking = false
        private set

    /** When the permission dialog was opened, on the clock that counts since the phone started (SR-77). */
    var askedAt: Long? = null
        private set

    private class Request(
        val show: (View) -> Unit,
        val onBytes: (ByteArray) -> Unit,
        val onFailed: (ScanFailure) -> Unit,
        val onCancel: () -> Unit,
        val ur: UrScan?,
        val bbqr: BbqrScan?,
        val wallet: WalletScan?,
    )

    /**
     * Starts a scan. [show] puts a page on the screen. Exactly one of [onBytes] (the first code's bytes),
     * [onFailed], [onCancel] and, with [ur], its `onMessage` is called later, unless [stop] comes first.
     * With [ur] or [bbqr], a first code that does not start with "ur:" or "B$" still goes to [onBytes].
     */
    fun begin(
        show: (View) -> Unit,
        onBytes: (ByteArray) -> Unit,
        onFailed: (ScanFailure) -> Unit,
        onCancel: () -> Unit,
        ur: UrScan? = null,
        bbqr: BbqrScan? = null,
        wallet: WalletScan? = null,
    ) {
        stop()
        val request = Request(show, onBytes, onFailed, onCancel, ur, bbqr, wallet)
        if (act.checkSelfPermission(Manifest.permission.CAMERA) == PackageManager.PERMISSION_GRANTED) {
            scan(request)
            return
        }
        pending = request
        asking = true
        askedAt = SystemClock.elapsedRealtime()
        act.requestPermissions(arrayOf(Manifest.permission.CAMERA), REQUEST_CAMERA)
    }

    /** The answer of the permission dialog. Denied and "don't ask again" both arrive here as denied. */
    fun permissionResult(code: Int, results: IntArray) {
        if (code != REQUEST_CAMERA) return
        asking = false
        askedAt = null
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
        reading?.drop()
        reading = null
        joining?.drop()
        joining = null
        walletReading?.drop()
        walletReading = null
        pending = null
        asking = false
        askedAt = null
    }

    private fun scan(request: Request) {
        val mine = ++generation
        val status = LinearLayout(act).apply { orientation = LinearLayout.VERTICAL }
        var shown: Any? = null
        val ur = request.ur
        val bbqr = request.bbqr
        val wallet = request.wallet
        fun progress(key: Any, words: SayWords, line: String?): Boolean {
            if (key == shown) return false
            shown = key
            status.removeAllViews()
            status.addView(ui.words(WordsModel.of(words)))
            if (line != null) status.addView(ui.text(line, 16f, bold = true), ui.margins(top = 8))
            return true
        }
        val onCode: (ByteArray) -> Unit = { bytes ->
            if (mine == generation) {
                val current = reading
                val joined = joining
                val walleted = walletReading
                if (wallet != null && UrText.startsWithUr(bytes)) {
                    val r = walleted ?: WalletReading(UrCall.native(), wallet.who).also { walletReading = it }
                    when (val step = r.feed(String(bytes, Charsets.UTF_8))) {
                        is WalletStep.Show -> {
                            if (step.expected > 0) {
                                if (progress(step.received to step.expected, step.words, wallet.line(step.received, step.expected))) Feedback.scanPart(status)
                            } else {
                                progress(step.words, step.words, null)
                            }
                        }
                        is WalletStep.Done -> {
                            Feedback.scanDone(status)
                            stop()
                            wallet.onWallet(step.text)
                        }
                        is WalletStep.Fail -> {
                            stop()
                            request.onFailed(ScanFailure(false, null, step.words))
                        }
                        is WalletStep.Ignore -> {}
                    }
                } else if (ur != null && UrText.startsWithUr(bytes)) {
                    val r = current ?: UrReading(UrCall.native(), ur.types, ur.who).also { reading = it }
                    when (val step = r.feed(String(bytes, Charsets.UTF_8))) {
                        is UrStep.Show -> {
                            val parts = ur.parts
                            if (parts != null && step.expected > 0) {
                                if (progress(step.received to step.expected, parts.words, parts.line(step.received, step.expected))) Feedback.scanPart(status)
                            } else {
                                progress(step.words, step.words, null)
                            }
                        }
                        is UrStep.Done -> {
                            Feedback.scanDone(status)
                            stop()
                            ur.onMessage(step.type, step.payload)
                        }
                        is UrStep.Fail -> {
                            stop()
                            request.onFailed(ScanFailure(false, null, step.words))
                        }
                        is UrStep.Ignore -> {}
                    }
                } else if (bbqr != null && UrText.startsWithBbqr(bytes)) {
                    val r = joined ?: BbqrReading(UrCall.native(), bbqr.kinds, bbqr.who).also { joining = it }
                    when (val step = r.feed(String(bytes, Charsets.US_ASCII))) {
                        is BbqrStep.Show -> {
                            if (progress(step.have to step.total, bbqr.parts.words, bbqr.parts.line(step.have, step.total))) Feedback.scanPart(status)
                        }
                        is BbqrStep.Say -> progress(step.words, step.words, null)
                        is BbqrStep.Done -> {
                            Feedback.scanDone(status)
                            stop()
                            bbqr.onMessage(step.kind, step.payload)
                        }
                        is BbqrStep.Fail -> {
                            stop()
                            request.onFailed(ScanFailure(false, null, step.words))
                        }
                        is BbqrStep.Ignore -> {}
                    }
                } else if (current == null && joined == null && walleted == null) {
                    // A plain code: the scan is over.
                    Feedback.scanDone(status)
                    stop()
                    request.onBytes(bytes)
                }
            }
        }
        val s = QrScanner(
            act, main, onCode,
            { text ->
                if (mine == generation) {
                    stop()
                    request.onFailed(ScanFailure(false, text))
                }
            },
            ur != null || bbqr != null || wallet != null,
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
        val hint = if (ur != null || bbqr != null || wallet != null) {
            "Hold the phone still with the whole code in view. A code that changes is read part by part: keep the phone on it until it is done."
        } else {
            "Hold the phone still with the whole code in view. It is read when it is steady."
        }
        column.addView(ui.text(hint, 14f), ui.margins(bottom = 12))
        column.addView(status, ui.margins(bottom = 12))
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
