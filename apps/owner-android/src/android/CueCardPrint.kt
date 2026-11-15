package org.openbmt.owner.app

import android.content.Context
import android.graphics.Canvas
import android.graphics.Paint
import android.graphics.Typeface
import android.os.Bundle
import android.os.CancellationSignal
import android.os.ParcelFileDescriptor
import android.print.PageRange
import android.print.PrintAttributes
import android.print.PrintDocumentAdapter
import android.print.PrintDocumentInfo
import android.print.PrintManager
import android.print.pdf.PrintedPdfDocument
import android.text.Layout
import android.text.StaticLayout
import android.text.TextPaint
import java.io.FileOutputStream
import java.io.IOException

// The staff cue card on paper (DESIGN 2.1 step 11): one page drawn with Canvas and Paint only, black on white, handed to
// the system's print service. No WebView and no library; nothing leaves the phone but what the owner sends to a printer.

/** One page: the title large and bold, the steps numbered, and the last line (the doubt) bold and unnumbered. */
internal class CueCardAdapter(
    private val context: Context,
    private val page: CueCardPage,
    private val onPrinted: () -> Unit,
    private val onFailed: () -> Unit,
) : PrintDocumentAdapter() {
    private var attributes: PrintAttributes? = null
    private var written = false

    override fun onLayout(
        old: PrintAttributes?,
        next: PrintAttributes,
        signal: CancellationSignal?,
        callback: LayoutResultCallback,
        extras: Bundle?,
    ) {
        if (signal?.isCanceled == true) return callback.onLayoutCancelled()
        attributes = next
        val info = PrintDocumentInfo.Builder("cue-card.pdf")
            .setContentType(PrintDocumentInfo.CONTENT_TYPE_DOCUMENT)
            .setPageCount(1)
            .build()
        callback.onLayoutFinished(info, old != next)
    }

    override fun onWrite(pages: Array<out PageRange>?, destination: ParcelFileDescriptor, signal: CancellationSignal?, callback: WriteResultCallback) {
        val attrs = attributes ?: return callback.onWriteFailed(FirstRunWords.cardPrintFailed)
        val doc = PrintedPdfDocument(context, attrs)
        try {
            if (signal?.isCanceled == true) return callback.onWriteCancelled()
            val p = doc.startPage(0)
            draw(p.canvas, page)
            doc.finishPage(p)
            FileOutputStream(destination.fileDescriptor).use { doc.writeTo(it) }
            written = true
            callback.onWriteFinished(arrayOf(PageRange.ALL_PAGES))
        } catch (_: IOException) {
            callback.onWriteFailed(FirstRunWords.cardPrintFailed)
            onFailed()
        } finally {
            doc.close()
        }
    }

    override fun onFinish() {
        if (written) onPrinted()
    }

    companion object {
        private const val MARGIN = 36f
        private const val TITLE_SIZE = 26f
        private const val TEXT_SIZE = 16f

        /** Draws [page] on [canvas]; the lines are wrapped to the width inside the margins. */
        fun draw(canvas: Canvas, page: CueCardPage) {
            val width = (canvas.width - 2 * MARGIN).toInt().coerceAtLeast(1)
            var y = MARGIN
            fun block(text: String, size: Float, bold: Boolean, gap: Float) {
                val paint = TextPaint(Paint.ANTI_ALIAS_FLAG).apply {
                    color = 0xFF000000.toInt()
                    textSize = size
                    typeface = if (bold) Typeface.DEFAULT_BOLD else Typeface.DEFAULT
                }
                val layout = StaticLayout.Builder.obtain(text, 0, text.length, paint, width)
                    .setAlignment(Layout.Alignment.ALIGN_NORMAL)
                    .setLineSpacing(0f, 1.15f)
                    .build()
                canvas.save()
                canvas.translate(MARGIN, y)
                layout.draw(canvas)
                canvas.restore()
                y += layout.height + gap
            }
            canvas.drawColor(0xFFFFFFFF.toInt())
            block(page.title, TITLE_SIZE, true, 24f)
            val steps = page.lines.dropLast(1)
            steps.forEachIndexed { i, s -> block("${i + 1}. $s", TEXT_SIZE, false, 14f) }
            page.lines.lastOrNull()?.let { block(it, TEXT_SIZE, true, 0f) }
        }
    }
}

/** Sends [page] to the system's print screen; false when the phone has no print service to ask. */
internal fun Controller.printCueCard(page: CueCardPage, onPrinted: () -> Unit, onFailed: () -> Unit): Boolean {
    val manager = act.getSystemService(Context.PRINT_SERVICE) as? PrintManager ?: return false
    return try {
        manager.print(FirstRunWords.cardScreenTitle, CueCardAdapter(act, page, onPrinted, onFailed), null)
        true
    } catch (_: RuntimeException) {
        false
    }
}
