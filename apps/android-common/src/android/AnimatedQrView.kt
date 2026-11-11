package org.openbmt.app

import android.content.Context
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Paint
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.view.View
import org.openbmt.common.QrNative

/** What [AnimatedQrView.make] gave: the view, or the words of why there is none. */
class AnimatedQrMade(val view: AnimatedQrView?, val words: SayWords?)

/**
 * A message as a QR code that changes: the parts of a UR, one after another, [AnimatedQr.FPS] a second,
 * round and again, each drawn black on white with a quiet zone of 4 modules and whole pixels for each
 * module, as large as fits (the width of the view, but never more than [maxHeightPx]). A message that
 * fits one part is one still code. It draws while it is on a window and stops when it is taken off, so
 * a screen that is cleared stops the codes. The mapping of modules to pixels is [AnimatedQr.box].
 */
class AnimatedQrView(context: Context, private val frames: AnimatedFrames, private val maxHeightPx: Int) : View(context) {
    private val black = Paint().apply { color = Color.BLACK; style = Paint.Style.FILL; isAntiAlias = false }
    private val handler = Handler(Looper.getMainLooper())
    private var start = 0L
    private var running = false

    private val tick = object : Runnable {
        override fun run() {
            if (!running) return
            invalidate()
            handler.postDelayed(this, 1000L / frames.fps)
        }
    }

    /** Whether the code changes: the message needed more than one part. */
    val animated: Boolean get() = frames.animated

    init {
        contentDescription = if (frames.animated) "Code that changes, for another phone to scan" else "Code for another phone to scan"
        filterTouchesWhenObscured = true
    }

    override fun onAttachedToWindow() {
        super.onAttachedToWindow()
        start = SystemClock.elapsedRealtime()
        if (frames.animated) {
            running = true
            handler.post(tick)
        }
    }

    override fun onDetachedFromWindow() {
        running = false
        handler.removeCallbacks(tick)
        super.onDetachedFromWindow()
    }

    override fun onMeasure(widthMeasureSpec: Int, heightMeasureSpec: Int) {
        val w = MeasureSpec.getSize(widthMeasureSpec)
        setMeasuredDimension(w, minOf(w, maxHeightPx))
    }

    override fun onDraw(canvas: Canvas) {
        canvas.drawColor(Color.WHITE)
        val index = frames.indexAt(SystemClock.elapsedRealtime() - start)
        val matrix = frames.matrix(index) ?: return
        val box = AnimatedQr.box(matrix.side, width, height)
        if (!box.fits) return
        val cell = box.cell.toFloat()
        for (y in 0 until matrix.side) {
            for (x in 0 until matrix.side) {
                if (matrix.get(x, y)) {
                    val left = box.x(x).toFloat()
                    val top = box.y(y).toFloat()
                    canvas.drawRect(left, top, left + cell, top + cell, black)
                }
            }
        }
    }

    companion object {
        /**
         * The view for [payload] as UR parts of [type] (`bytes` for OpenBMT's own objects, `crypto-psbt` for
         * a plain PSBT), or the words of why none could be made. Every part is drawn once here, so a part the
         * library cannot draw shows as words now and never as a blank frame later. [who] is the one who does
         * the next step.
         */
        fun make(
            context: Context,
            who: Int,
            type: String,
            payload: ByteArray,
            maxHeightPx: Int,
            maxFragment: Int = AnimatedQr.MAX_FRAGMENT,
        ): AnimatedQrMade {
            if (QrNative.load() != null) return AnimatedQrMade(null, UrWords.cannotShow(who))
            val call = UrCall.native()
            val parts = when (val outcome = call.urEncode(type, payload, maxFragment)) {
                is UrOutcome.Ok -> outcome.value
                else -> return AnimatedQrMade(null, UrWords.cannotShow(who))
            }
            return drawn(context, who, call, parts, maxHeightPx)
        }

        /**
         * The view for [payload] as a BBQr set of [kind] (`P` for a PSBT, `U` for a text), each part at most
         * [maxChars] characters; a file that fits one part is one still code.
         */
        fun makeBbqr(context: Context, who: Int, kind: Char, payload: ByteArray, maxHeightPx: Int, maxChars: Int): AnimatedQrMade {
            if (QrNative.load() != null) return AnimatedQrMade(null, UrWords.cannotShow(who))
            val call = UrCall.native()
            val parts = when (val outcome = call.bbqrEncode(kind, payload, maxChars)) {
                is UrOutcome.Ok -> outcome.value
                else -> return AnimatedQrMade(null, UrWords.cannotShow(who))
            }
            return drawn(context, who, call, parts, maxHeightPx)
        }

        /** The view for [text] as one plain code, for a text that fits one (a wallet's text file). */
        fun makePlain(context: Context, who: Int, text: String, maxHeightPx: Int): AnimatedQrMade {
            if (QrNative.load() != null) return AnimatedQrMade(null, UrWords.cannotShow(who))
            return drawn(context, who, UrCall.native(), listOf(text), maxHeightPx)
        }

        private fun drawn(context: Context, who: Int, call: UrCall, parts: List<String>, maxHeightPx: Int): AnimatedQrMade {
            val frames = AnimatedFrames(call, parts)
            if (frames.drawAll() != null) return AnimatedQrMade(null, UrWords.cannotShow(who))
            return AnimatedQrMade(AnimatedQrView(context, frames, maxHeightPx), null)
        }
    }
}
