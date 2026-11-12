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
        val ledger: SaveFile?,
    )

    /**
     * The library gives no words for the totals of a day, so the line is "{n} sales, {amount}" with the amount in
     * the shop's currency as the library wrote it ([code] is "SAT" for Bitcoin Standard).
     */
    fun totals(sales: Long, amount: String, code: String): String = "$sales sales, $amount $code"

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
        val ledger = if (month != null && b.ledger.isNotEmpty()) SaveFile(fileName("ledger", month), b.ledger) else null
        val refusal = b.refusal?.say()
        val empty = if (rows.isEmpty() && refusal == null) AppWords.booksEmpty else null
        return Page(rows, empty, refusal, accounting, ledger)
    }
}
