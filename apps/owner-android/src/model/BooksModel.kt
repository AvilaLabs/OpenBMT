package org.openbmt.owner.app

import org.openbmt.app.SayWords
import org.openbmt.owner.Books

/**
 * What the Books page shows and saves (OA-103), made from what the library's books call gave. The days, their
 * amounts, the two files and every refusal are the library's; this only chooses the order of the page, the line of
 * a day when the library gives none, and the name of a file. No Android class is used.
 */
object BooksModel {
    /** One row of the page: the day as the title and its totals as the sub. */
    class Row(val date: String, val totals: String)

    /** A file the owner can save: the name the picker starts with, and exactly the bytes the library made. */
    class SaveFile(val name: String, val bytes: ByteArray)

    class Page(
        val rows: List<Row>,
        /** Words when there is nothing to list and nothing was refused. */
        val empty: SayWords?,
        /** The library's words when an export was not made. */
        val refusal: SayWords?,
        val accounting: SaveFile?,
        /** The name the full ledger is saved under; the file itself is made only after the warning is accepted (BK-19). */
        val ledger: String?,
    )

    /**
     * The warning before the full ledger (BK-19): the button opens it, [cancel] closes it with no call, and [accept] closes it
     * and makes the ledger with the answer true, once. The answer is not kept: the next save asks again.
     */
    class LedgerAsk(private val name: String, private val make: (Boolean) -> ByteArray) {
        var open = false
            private set

        val title = OwnerLook.BOOKS_LEDGER_WARN_TITLE
        val text = OwnerLook.BOOKS_LEDGER_WARN_TEXT
        val acceptLabel = OwnerLook.BOOKS_LEDGER_WARN_ACCEPT
        val cancelLabel = OwnerLook.BOOKS_LEDGER_WARN_CANCEL

        fun ask() {
            open = true
        }

        fun cancel() {
            open = false
        }

        /** The file to save, or null when the warning was not open. */
        fun accept(): SaveFile? {
            if (!open) return null
            open = false
            return SaveFile(name, make(true))
        }
    }

    /**
     * The library gives no words for the totals of a day, so the line is "{n} sales, {amount}" with the amount in
     * the shop's currency as the library wrote it ([code] is "SAT" for Bitcoin Standard).
     */
    fun totals(sales: Long, amount: String, code: String): String = "$sales ${if (sales == 1L) "sale" else "sales"}, $amount $code"

    /** `openbmt-accounting-2026-11.csv`: the kind of file and the month it is for. */
    fun fileName(kind: String, month: String): String = "openbmt-$kind-$month.csv"

    /**
     * The page for [b]: the days in the order the library gave them (newest first), and a file to save for each
     * export the library made. The month of a name is the newest month of the books.
     */
    fun page(b: Books): Page {
        val rows = b.days.map { Row(it.date, totals(it.sales, it.amount, b.currency)) }
        val month = b.accounting.lastOrNull()?.month ?: b.days.firstOrNull()?.date?.take(7)
        val accounting = b.accounting.lastOrNull()?.let { SaveFile(fileName("accounting", it.month), it.csv) }
        val refusal = b.refusal?.say()
        val ledger = if (month != null && refusal == null) fileName("ledger", month) else null
        val empty = if (rows.isEmpty() && refusal == null) AppWords.booksEmpty else null
        return Page(rows, empty, refusal, accounting, ledger)
    }
}
