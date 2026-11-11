package org.openbmt.counter.app

/**
 * Where a code of [modules] by [modules] squares is drawn in a box of
 * [width] by [height] pixels: [cell] whole pixels for each square, so that
 * the edges are sharp, as large as fits with the quiet zone of [quiet]
 * squares on every side (the zone is white, drawn by the view). [left] and
 * [top] are where the quiet zone starts; the code itself starts [quiet]
 * cells further in.
 */
class QrBox(val cell: Int, val left: Int, val top: Int, val modules: Int, val quiet: Int) {
    /** The side of the code and its quiet zone, in pixels. */
    val side: Int get() = cell * (modules + 2 * quiet)

    /** The left edge of the module in column [x], in pixels. */
    fun x(x: Int): Int = left + (quiet + x) * cell

    /** The top edge of the module in row [y], in pixels. */
    fun y(y: Int): Int = top + (quiet + y) * cell

    /** Whether a code can be drawn at all: a cell is at least 1 pixel. */
    val fits: Boolean get() = cell >= 1
}

object QrLayout {
    /** The quiet zone of a QR code, in modules. */
    const val QUIET = 4

    fun box(modules: Int, width: Int, height: Int, quiet: Int = QUIET): QrBox {
        require(modules > 0) { "a code has modules" }
        val total = modules + 2 * quiet
        val cell = minOf(width, height) / total
        val side = cell * total
        return QrBox(cell, (width - side) / 2, (height - side) / 2, modules, quiet)
    }
}
