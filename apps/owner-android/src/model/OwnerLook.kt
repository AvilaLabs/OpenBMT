package org.openbmt.owner.app

/**
 * What the home and settings screens of the new look write from words the app already has (OA-91). Every
 * sentence here is an existing one cut where it ends, or a label that is shown as it is; the only new words are
 * the titles of the four groups and of the waiting card. No Android class is used.
 */
object OwnerLook {
    // ---- home -----------------------------------------------------------------------------------

    const val WAITING_TITLE = "Open approvals and queues"
    const val MONEY = "Money"
    const val DEVICES = "Counters and devices"
    const val ALERTS = "Alerts and notes"
    const val SETTINGS = "Settings and this phone"

    /** What the settings row of home says under its title. */
    const val SETTINGS_SUB = "Settings, Change the PIN, About"

    /** The sentence of the balance cut at its full stop: the lead, the amount, and the line about the blocks. */
    class Balance(val lead: String, val amount: String, val checked: String) {
        /** The three parts as the one sentence they came from. */
        fun sentence(): String = "$lead $amount. $checked"
    }

    /** "The wallet can spend {amount}. Checked to block {checked} of {tip}." as three parts; [amount] is the amount as it is written. */
    fun balance(amount: String, checkedHeight: Long, tipHeight: Long): Balance =
        Balance("The wallet can spend", amount, "Checked to block $checkedHeight of $tipHeight.")

    /** The entries of the Money page, by title, in the order they are listed. Waiting to be delivered is last. */
    val moneyTitles: List<String> = listOf(
        "Receipts and merge", "Cash out", "Old addresses", "Refund notes", "Open approvals", "Refund queue", "Waiting to be delivered",
    )

    /** The Books row of the Money page: after the rows above, so the names of the first three do not change (OA-103). */
    const val BOOKS = "Books"
    const val BOOKS_SUB = "Sales, refunds and totals by day, and the files for your bookkeeper."
    const val BOOKS_NOTE = "The files follow US rules only and are not tax advice."
    const val BOOKS_SAVE_ACCOUNTING = "Save the accounting file"
    const val BOOKS_SAVE_LEDGER = "Save the full ledger"
    const val BOOKS_LEDGER_WARN_TITLE = "This file shows your payments"
    const val BOOKS_LEDGER_WARN_TEXT = "The full ledger lists every sale with its bitcoin transaction. Anyone who has the file can find those payments on the blockchain and follow the money from them. Share it only with people you trust with that, such as your bookkeeper."
    const val BOOKS_LEDGER_WARN_ACCEPT = "I understand, save it"
    const val BOOKS_LEDGER_WARN_CANCEL = "Not now"

    /** What the Money row says under its title: the names of its first three entries, and the payments line when payments are in progress. */
    fun moneySub(inProgress: Int, sendsLine: String): String {
        val names = moneyTitles.take(3).joinToString(", ")
        return if (inProgress > 0) "$names\n$sendsLine" else names
    }

    /** One row of the waiting card: its title and how many wait. Only a count above zero is a row. */
    class WaitingRow(val title: String, val count: Int)

    /** The rows of the waiting card from the counts the app has; a title with no count (null) or a count of zero is left out. */
    fun waitingRows(counts: List<Pair<String, Int?>>): List<WaitingRow> =
        counts.mapNotNull { (title, n) -> if (n != null && n > 0) WaitingRow(title, n) else null }

    // ---- settings -------------------------------------------------------------------------------

    const val INSTANT = "Instant sales"

    /** The table of the instant sales card: the two column titles, one row for each check, and the line of the tolerance. */
    class InstantTable(
        val columns: List<String>,
        val rows: List<Triple<String, String, String>>,
        val tolerance: Pair<String, String>?,
    )

    private val instantColumns = listOf("Largest instant sale", "Instant total")

    /** The first letter in upper case, the rest as it is. */
    fun capital(text: String): String = text.replaceFirstChar { it.uppercase() }

    /**
     * The lines "Largest instant sale, light check" and "Instant total, light check" (and those of the own node and
     * the Box) cut at their comma into a table: a row for each check named with its first letter in upper case, a
     * value under each column. The lines that are not part of it are [rest], in their order.
     */
    fun instantTable(lines: List<Pair<String, String>>): Pair<InstantTable, List<Pair<String, String>>> {
        val checks = ArrayList<String>()
        val values = HashMap<String, Array<String?>>()
        val rest = ArrayList<Pair<String, String>>()
        var tolerance: Pair<String, String>? = null
        for (line in lines) {
            val column = instantColumns.indexOfFirst { line.first.startsWith("$it, ") }
            when {
                column >= 0 -> {
                    val check = line.first.removePrefix(instantColumns[column] + ", ")
                    if (check !in values) {
                        checks.add(check)
                        values[check] = arrayOfNulls(2)
                    }
                    values[check]?.set(column, line.second)
                }
                line.first == TOLERANCE -> tolerance = line
                else -> rest.add(line)
            }
        }
        val rows = checks.map { c -> Triple(capital(c), values[c]?.get(0) ?: "", values[c]?.get(1) ?: "") }
        return InstantTable(instantColumns, rows, tolerance) to rest
    }

    const val TOLERANCE = "Underpayment still counted"

    /** The folded groups of the settings page: the title and the labels of the lines it holds, in order. */
    val settingsGroups: List<Pair<String, List<String>>> = listOf(
        "Currency" to listOf("Currency"),
        "Payments" to listOf("Blocks to settle a sale", "Blocks until a refund may be paid", "Smallest sale a counter opens"),
        "Tips" to listOf("Tips"),
        "Refunds and overrides" to listOf("Largest single refund", "Refunds per day", "Cashier's override per day", "Manager's override ceiling"),
        "Safety" to listOf("Longest heartbeat age for instant", "Tor", "Customers can join the Wi-Fi", "Devices that write records", "Boxes"),
        "Counters' shares" to listOf("Counters and their shares"),
        "Lightning node" to listOf("Lightning node"),
    )

    /** The title of the group of a line about the background watch, when the settings list one. */
    const val WATCH = "Watch in the background"

    /**
     * The lines that are not in the instant table, put in their groups; a group with no line is left out. The
     * version is the bar's sub. A line that no group names goes to Safety, and a line about the watch has a group
     * of its own.
     */
    fun settingsFolded(rest: List<Pair<String, String>>): List<Pair<String, List<Pair<String, String>>>> {
        val named = settingsGroups.flatMap { it.second }.toSet()
        val out = ArrayList<Pair<String, List<Pair<String, String>>>>()
        for ((title, labels) in settingsGroups) {
            val mine = ArrayList<Pair<String, String>>()
            for (line in rest) {
                if (line.first in labels) mine.add(line)
                if (title == "Safety" && line.first !in named && line.first != "Version" && !line.first.startsWith(WATCH)) mine.add(line)
            }
            if (mine.isNotEmpty()) out.add(title to mine)
        }
        val watch = rest.filter { it.first.startsWith(WATCH) }
        if (watch.isNotEmpty()) out.add(WATCH to watch)
        return out
    }

    /** The settings version of a held settings, for the bar's sub. */
    fun settingsSub(version: Long): String = "Settings version $version"
}
