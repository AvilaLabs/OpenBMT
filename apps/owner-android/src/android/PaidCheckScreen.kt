package org.openbmt.owner.app

import android.view.View
import org.openbmt.app.RowItem
import org.openbmt.app.SayWords
import org.openbmt.owner.OwnerNode

/**
 * The counter checks as the views of the pages that show them (`docs/OWNER-COINS-RULES.md`, OC-60 to OC-64). The
 * check runs in the library each time a page asks; what it says is the library's, and the words are
 * [PaidCheckWords]'s.
 */

/**
 * The alarms of the Alerts page: each sale a counter showed paid that this phone does not see, with a button that
 * clears it (OC-62), the lines of payments that arrived since, and the sales waiting for a block, too old to check or not checkable while the receipt log is full
 * (OC-61), and the counters whose records cannot be read. When the checks cannot be kept or are full, the page says so and offers to clear them, and still shows what it found (OC-64). [again] shows the page
 * again, with the words of what was just done.
 */
internal fun Controller.paidCheckViews(n: OwnerNode, msg: Slot, again: () -> Unit): List<View> {
    val check = attempt(msg) { n.paidCheck(nowSecs()) } ?: return emptyList()
    val out = ArrayList<View>()
    // Marks that are not kept or full do not stop the check: the alarms still show below (OC-64).
    if (!check.kept || check.full) {
        out.add(
            stack(
                ui.words(if (check.kept) PaidCheckWords.full else PaidCheckWords.notKept),
                ui.button(PaidCheckWords.clearButton) { if (attempt(msg) { n.paidCheckReset() } != null) again() },
                gap = 10,
            ),
        )
    }
    for (c in check.unreadable) out.add(ui.words(PaidCheckWords.unreadable(c)))
    for (a in check.alarms) {
        out.add(
            stack(
                ui.words(PaidCheckWords.mismatch(a)),
                ui.button(PaidCheckWords.clearButton) { if (attempt(msg) { n.paidClear(nowSecs(), a.counter, a.sale) } != null) again() },
                gap = 10,
            ),
        )
    }
    for (a in check.arrived) out.add(line(PaidCheckWords.arrived(a)))
    if (check.waiting.isNotEmpty()) {
        out.add(ui.words(PaidCheckWords.waiting))
        out.add(ui.rows(check.waiting.map { RowItem(PaidCheckWords.saleTitle(it), PaidCheckWords.saleSub(it)) }))
    }
    if (check.notCheckable.isNotEmpty()) {
        out.add(ui.words(PaidCheckWords.notCheckable))
        out.add(ui.rows(check.notCheckable.map { RowItem(PaidCheckWords.saleTitle(it), PaidCheckWords.saleSub(it)) }))
        if (check.notCheckableMore > 0) out.add(line("+ ${check.notCheckableMore}", muted = true))
    }
    if (check.logFull.isNotEmpty()) {
        out.add(ui.words(PaidCheckWords.logFull))
        out.add(ui.rows(check.logFull.map { RowItem(PaidCheckWords.saleTitle(it), PaidCheckWords.saleSub(it)) }))
        if (check.logFullMore > 0) out.add(line("+ ${check.logFullMore}", muted = true))
    }
    return out
}

/**
 * The day summary of the Books page (OC-63): a line for each counter that has a sale paid on chain today, in the
 * phone's time zone. A counter whose total is not what was seen is named in strong type. Nothing when there is
 * no such sale, (a check that cannot be kept still gives its days; the Alerts page says so).
 */
internal fun Controller.paidDayViews(n: OwnerNode, msg: Slot): List<View> {
    val offsetSecs = java.util.TimeZone.getDefault().getOffset(System.currentTimeMillis()) / 1000
    val days = attempt(msg) { n.paidDay(nowSecs(), offsetSecs) } ?: return emptyList()
    return days.map { line(PaidCheckWords.day(it), bold = it.differs) }
}
