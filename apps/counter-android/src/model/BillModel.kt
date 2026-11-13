package org.openbmt.counter.app

import org.openbmt.counter.Bill
import org.openbmt.counter.BillKind
import org.openbmt.counter.BillState
import org.openbmt.counter.PartWay
import org.openbmt.counter.SaleInfo

/**
 * CR-245 to CR-254: the labels of bills, tabs and the till's reference (CR-255, `docs/SCREEN-RULES.md`). A label
 * is under 28 characters.
 */
object WordsOfBills {
    /** CR-246: the title of the list of bills, and the button on Home that opens it. */
    const val BILLS = "Bills and tabs"

    /** CR-246: the button that opens a bill. */
    const val NEW_BILL = "New bill"

    /** CR-246: the button that opens a tab. */
    const val NEW_TAB = "New tab"

    /** CR-246: the line of the list when no bill is open. */
    const val NONE = "No bill or tab is open."

    /** CR-246: the label of the amount of a bill or a tab. */
    const val AMOUNT = "Amount"

    /** CR-246: the state of a bill that is open. */
    const val STATE_OPEN = "Open"

    /** CR-246: the state of a tab that is open. */
    const val STATE_TAB = "Tab open"

    /** CR-246: the state of a bill that is settled. */
    const val STATE_SETTLED = "Paid"

    /** CR-246: the state of a bill closed short. */
    const val STATE_SHORT = "Closed short"

    /** CR-246: the state of a bill cancelled. */
    const val STATE_CANCELLED = "Cancelled"

    /** CR-245: the label of the till's reference. */
    const val REFERENCE = "Till reference"

    /** CR-245: the hint of the reference field. */
    const val REFERENCE_HINT = "Check or receipt number"

    /** CR-245: the button that adds a reference to a sale or a bill that has none. */
    const val ADD_REFERENCE = "Add till reference"

    /** CR-249: the label of what a bill has received. */
    const val RECEIVED = "Paid so far"

    /** CR-249: the label of what is left of a bill. */
    const val LEFT = "Left to pay"

    /** CR-249: the label of the parts that are not paid yet. */
    const val PENDING = "On its way"

    /** CR-247: the button that takes a bitcoin part. */
    const val PART_BITCOIN = "Take a bitcoin payment"

    /** CR-247: the button that takes a part paid another way. */
    const val PART_OTHER = "Paid another way"

    /** CR-247: the labels of the ways a part was paid, in the order cash, card, other. */
    val WAYS = listOf("Cash", "Card", "Other")

    /** CR-248: the button that joins a sale, such as a paid request, to a bill. */
    const val JOIN = "Add a sale already taken"

    /** CR-248: the title of the list of sales that can be joined. */
    const val JOIN_PICK = "Pick the sale to add"

    /** CR-250: the button that splits what is left. */
    const val SPLIT = "Split what is left"

    /** CR-250: the label of the number of people. */
    const val PEOPLE = "Number of people"

    /** CR-250: the label of what each pays. */
    const val EACH = "Each pays"

    /** CR-251: the button that raises or lowers a tab. */
    const val CHANGE_TAB = "Change the tab"

    /** CR-251: the button that finishes a tab. */
    const val FINISH_TAB = "Finish the tab"

    /** CR-252: the button that closes a bill short. */
    const val CLOSE_SHORT = "Close short"

    /** CR-252: the button that cancels a bill. */
    const val CANCEL_BILL = "Cancel the bill"

    /** CR-252: the label of what was not paid when a bill was closed short. */
    const val UNPAID = "Not paid"

    /** CR-254: the label of the sale's number on the Paid screen. */
    const val SALE_NUMBER = "Sale number"

    /** CR-254: the label of the till's reference on the Paid screen. */
    const val SALE_REFERENCE = "Reference for the till"

    /** CR-254: the label of the number of the bill the sale is a part of, on the Paid screen. */
    const val SALE_BILL = "Bill number"

    /** Every label, for the test that checks them all. */
    fun all(): List<String> = listOf(
        BILLS, NEW_BILL, NEW_TAB, NONE, AMOUNT, STATE_OPEN, STATE_TAB, STATE_SETTLED, STATE_SHORT, STATE_CANCELLED, REFERENCE,
        REFERENCE_HINT, ADD_REFERENCE, RECEIVED, LEFT, PENDING, PART_BITCOIN, PART_OTHER, JOIN, JOIN_PICK, SPLIT, PEOPLE,
        EACH, CHANGE_TAB, FINISH_TAB, CLOSE_SHORT, CANCEL_BILL, UNPAID, SALE_NUMBER, SALE_REFERENCE, SALE_BILL,
    ) + WAYS
}

/** Which buttons the screen of a bill shows. */
class BillButtons(
    val bitcoinPart: Boolean,
    val otherPart: Boolean,
    val join: Boolean,
    val split: Boolean,
    val addReference: Boolean,
    val changeTab: Boolean,
    val finishTab: Boolean,
    val closeShort: Boolean,
    val cancel: Boolean,
)

/** One line of the Paid screen (CR-254): a label and the value to copy into the till's payment note. */
class PaidLine(val label: String, val value: String)

/** What the bill screens write, from what the library says. No Android class is used. */
object BillModel {
    /** The most people the split helper takes (CR-250). */
    const val MAX_PEOPLE = 20

    /** The fewest people the split helper takes (CR-250). */
    const val MIN_PEOPLE = 2

    /** The text of the state of [bill] (CR-246). */
    fun stateLabel(bill: Bill): String = when (bill.state) {
        BillState.Open -> if (bill.kind == BillKind.Tab) WordsOfBills.STATE_TAB else WordsOfBills.STATE_OPEN
        BillState.Settled -> WordsOfBills.STATE_SETTLED
        BillState.ClosedShort -> WordsOfBills.STATE_SHORT
        BillState.Cancelled -> WordsOfBills.STATE_CANCELLED
    }

    /** The label of a way a part was paid (CR-247). */
    fun wayLabel(way: PartWay): String = WordsOfBills.WAYS[way.ordinal]

    /**
     * CR-246 to CR-252: the buttons of a bill. An ended bill has none. A part is taken while the bill is open;
     * the split helper while something is left; a tab is changed and finished while it is open; a bill is closed
     * short while something is left; and it is cancelled only while nothing has been received and no part is
     * pending (the library decides again).
     */
    fun buttons(bill: Bill): BillButtons {
        val open = bill.state == BillState.Open
        val tab = bill.kind == BillKind.Tab
        val received = bill.receivedBitcoin + bill.receivedOther
        return BillButtons(
            bitcoinPart = open,
            otherPart = open,
            join = open,
            split = open && bill.left > 0,
            addReference = open && bill.reference == null,
            changeTab = open && tab,
            finishTab = open && tab,
            closeShort = open && bill.left > 0,
            cancel = open && received == 0L && bill.pending == 0L,
        )
    }

    /** The bill's number as the screens write it: the counter's own number, from 1 (CR-246). */
    fun numberText(bill: Bill): String = bill.number.toString()

    /** The line of a bill in the list: its number, its state, and what is left or what it came to (CR-246, CR-249). */
    fun listLine(money: Money, bill: Bill): String {
        val tail = when (bill.state) {
            BillState.Open -> "${WordsOfBills.LEFT} ${money.show(bill.left)}"
            BillState.ClosedShort -> "${WordsOfBills.UNPAID} ${money.show(bill.unpaidMinor)}"
            else -> money.show(bill.amountMinor)
        }
        val ref = bill.reference?.let { " $it" }.orEmpty()
        return "${numberText(bill)}$ref · ${stateLabel(bill)} · $tail"
    }

    /** The lines of what a bill has received and what is left (CR-249). */
    fun totals(money: Money, bill: Bill): List<PaidLine> {
        val lines = ArrayList<PaidLine>()
        lines.add(PaidLine(WordsOfBills.AMOUNT, money.show(bill.amountMinor)))
        lines.add(PaidLine(WordsOfBills.RECEIVED, money.show(bill.receivedBitcoin + bill.receivedOther)))
        if (bill.pending > 0) lines.add(PaidLine(WordsOfBills.PENDING, bill.pending.toString()))
        lines.add(PaidLine(WordsOfBills.LEFT, money.show(bill.left)))
        if (bill.state == BillState.ClosedShort) lines.add(PaidLine(WordsOfBills.UNPAID, money.show(bill.unpaidMinor)))
        return lines
    }

    /** The line of one part: a bitcoin part by its sale, a part paid another way by its way (CR-247). */
    fun partLine(money: Money, part: org.openbmt.counter.BillPart): String {
        val what = part.sale?.let { "${it + 1}" } ?: part.way?.let { wayLabel(it) }.orEmpty()
        return "$what · ${money.show(part.amountMinor)}"
    }

    /**
     * CR-250: the share of each of [people], from what the library returned, one per line; null when the number of
     * people is not one the helper takes. The last share is the one that takes what is left over.
     */
    fun splitLines(money: Money, shares: List<Long>): List<String> =
        shares.mapIndexed { i, s -> "${i + 1} · ${money.show(s)}" }

    /** Whether [people] is a number of people the split helper takes (CR-250). */
    fun peopleOk(people: Int): Boolean = people in MIN_PEOPLE..MAX_PEOPLE

    /**
     * CR-254: the lines of the Paid screen that carry the number and the reference, large enough to copy into the
     * till's payment note: the sale's number, the till's reference when it has one, and the number of the bill the
     * sale is a part of when it is one. [index] is the sale's receive index; its number is that plus one, as on
     * every screen of a sale.
     */
    fun paidLines(index: Long, info: SaleInfo): List<PaidLine> {
        val lines = ArrayList<PaidLine>()
        lines.add(PaidLine(WordsOfBills.SALE_NUMBER, (index + 1).toString()))
        info.reference?.let { lines.add(PaidLine(WordsOfBills.SALE_REFERENCE, it)) }
        info.billNumber?.let { lines.add(PaidLine(WordsOfBills.SALE_BILL, it.toString())) }
        return lines
    }

    /** CR-245: whether [text] fits the till's reference: 1 to 32 characters of `A-Z a-z 0-9 - _ / # .` (the library decides again). */
    fun referenceOk(text: String): Boolean =
        text.length in 1..32 && text.all { it in 'A'..'Z' || it in 'a'..'z' || it in '0'..'9' || it in "-_/#." }

    /** A reference typed, trimmed, or null when nothing was typed (the field is optional). */
    fun referenceFrom(text: String): String? = text.trim().takeIf { it.isNotEmpty() }

    /**
     * CR-248: the sales that can be joined to a bill: those that are paid or open, and are not parts of a bill
     * already. [bills] are the bills the library lists, [sales] the sales' receive indexes newest first.
     */
    fun joinable(sales: List<Long>, bills: List<Bill>): List<Long> {
        val taken = bills.flatMap { b -> b.parts.mapNotNull { it.sale } }.toSet()
        return sales.filter { it !in taken }
    }
}
