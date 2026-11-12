package org.openbmt.owner.app

import android.view.View
import android.widget.Button
import org.openbmt.app.RowItem
import org.openbmt.owner.OwnerNode

/**
 * The Books page (OA-103): the days of the sales and refunds the node holds, newest first, and the two files for a
 * bookkeeper. What is in the days and in the files is the library's; each file is written, through the system's
 * save picker, exactly as the library made it.
 */
internal fun Controller.showBooks(save: BooksModel.SaveFile? = null) {
    val n = node
    if (n == null) {
        begin()
        return
    }
    val msg = Slot(act)
    val offsetMinutes = java.util.TimeZone.getDefault().getOffset(System.currentTimeMillis()) / 60_000
    val books = attempt(msg) { n.books(offsetMinutes) }
    val views = ArrayList<View>()
    val footer = ArrayList<Button>()
    if (books == null) {
        views.add(msg)
    } else {
        val page = BooksModel.page(books)
        page.empty?.let { views.add(ui.words(it)) }
        page.refusal?.let { views.add(ui.words(it)) }
        if (page.rows.isNotEmpty()) {
            views.add(ui.rows(page.rows.map { RowItem(it.date, it.totals) }))
            views.add(line(OwnerLook.BOOKS_NOTE, muted = true))
        }
        views.add(msg)
        page.accounting?.let { file -> footer.add(ui.primary(OwnerLook.BOOKS_SAVE_ACCOUNTING) { saveBooks(file, msg) }) }
        page.ledger?.let { name -> footer.add(ui.button(OwnerLook.BOOKS_SAVE_LEDGER) { showLedgerWarning(n, name) }) }
    }
    screen(OwnerLook.BOOKS, views, footer, back = { showHome() })
    save?.let { saveBooks(it, msg) }
}

/** The warning before the full ledger (BK-19). Accepting makes the ledger and opens the save picker; every save asks again. */
private fun Controller.showLedgerWarning(n: OwnerNode, name: String) {
    val msg = Slot(act)
    val ask = BooksModel.LedgerAsk(name) { n.booksLedger(it) }
    ask.ask()
    val accept = ui.primary(ask.acceptLabel) {
        attempt(msg) { ask.accept() }?.let { showBooks(it) }
    }
    val leave = {
        ask.cancel()
        showBooks()
    }
    screen(ask.title, listOf(para(ask.text), msg), listOf(accept, ui.button(ask.cancelLabel) { leave() }), back = leave)
}

private fun Controller.saveBooks(file: BooksModel.SaveFile, msg: Slot) {
    saveBytes(
        file.name, file.bytes, "text/csv",
        cancelled = { msg.show() },
        saved = { msg.show(ui.words(AppWords.booksSaved)) },
        failed = { msg.show(ui.words(it)) },
    )
}
