package org.openbmt.owner.app

import org.openbmt.app.Format
import org.openbmt.app.SayWords
import org.openbmt.app.Tones
import org.openbmt.app.Who
import org.openbmt.owner.CashAmount
import org.openbmt.owner.CashOutRow
import org.openbmt.owner.MergeOffer
import org.openbmt.owner.OwnSpendItem
import org.openbmt.owner.OwnSpendSession
import org.openbmt.owner.Queue
import org.openbmt.owner.RefundRow
import org.openbmt.owner.StageKind

/** What a queued payment of "Send from your wallet" is. */
enum class OwnSendKind { Refund, CashOut }

/** One line of the list the owner picks from (OA-115). */
class OwnSendRow(val kind: OwnSendKind, val id: Long, val title: String, val detail: String)

/** Where a payment of the wallet is (OA-117). */
enum class OwnSendState {
    /** Prepared, and the file was not given to the wallet yet. */
    NotFiled,
    /** The file was given: the wallet may sign it, or send it itself and never return it. */
    Waiting,
}

/**
 * The owner phone of a shop with its own wallet sends from it (`docs/OWNER-APP-RULES.md` OA-115 to OA-118,
 * `docs/SESSION-RULES.md` AS-100 to AS-107): what is listed, what is picked, what the wallet's screen must show and
 * how the file reaches the wallet. The library freezes, files and judges; this is what the person sees and picks.
 */
object OwnSpendModel {
    /** OA-116: the ways a file reaches the wallet and comes back are those of a hardware signer (OA-70). */
    val transports: List<TransportChoice> get() = HardwareModel.transports

    /** OA-115: the refunds that are queued, then the cash-outs that are queued, as the owner picks them. */
    fun rows(queue: Queue): List<OwnSendRow> =
        queue.refunds.filter { it.stage.kind == StageKind.Queued }.map { refundRow(it) } +
            queue.cashOuts.filter { it.stage.kind == StageKind.Queued }.map { cashOutRow(it) }

    private fun refundRow(r: RefundRow) =
        OwnSendRow(OwnSendKind.Refund, r.id, "Refund ${Format.sats(r.sats)}", "To ${r.address}, for ${r.cashier}.")

    private fun cashOutRow(c: CashOutRow) = OwnSendRow(
        OwnSendKind.CashOut, c.id,
        when (val a = c.amount) {
            is CashAmount.Sats -> "Cash-out ${Format.sats(a.sats)}"
            CashAmount.All -> "Cash-out of everything"
        },
        "To ${c.address}, ${c.label}.",
    )

    /** OA-115: the owner picks or un-picks one; the order is the queue's. */
    fun toggle(picked: List<Long>, id: Long): List<Long> = if (id in picked) picked - id else picked + id

    /** OA-115: "Prepare" works on at least one picked payment, and only on ones that are listed. */
    fun canPrepare(rows: List<OwnSendRow>, picked: List<Long>): Boolean =
        picked.isNotEmpty() && picked.all { p -> rows.any { it.id == p } }

    /** OA-115: whether the offer to merge is shown here: when it holds (OC-19) and nothing else is waiting to be sent. */
    fun showMerge(offer: MergeOffer?, rows: List<OwnSendRow>): Boolean = offer != null && rows.isEmpty()

    /** OA-115, OA-73: what the wallet's screen must show for a payment, one to a line, for the owner to compare. */
    fun compareLines(item: OwnSpendItem): List<String> = HardwareModel.compareLines(
        item.payments.map { it.address to it.sats },
        item.change?.let { it.address to it.sats },
        item.fee,
    )

    /** OA-115: the transaction's lines, each payment a refund or a cash-out of the queue. */
    fun itemTitle(item: OwnSpendItem, position: Int, of: Int): String =
        "Payment $position of $of: " + when {
            item.cashOut != null && item.refunds.isEmpty() -> "a cash-out"
            item.cashOut == null && item.refunds.size == 1 -> "a refund"
            item.cashOut == null -> "${item.refunds.size} refunds"
            else -> "${item.refunds.size} refunds and a cash-out"
        }

    /** OA-117: where a payment is. */
    fun state(item: OwnSpendItem): OwnSendState = if (item.filed) OwnSendState.Waiting else OwnSendState.NotFiled

    /** OA-117: the open payments that have been given to the wallet and wait, across every open session. */
    fun waiting(sessions: List<OwnSpendSession>): List<Pair<OwnSpendSession, OwnSpendItem>> =
        sessions.flatMap { s -> s.items.filter { it.filed }.map { s to it } }

    /** OA-117: the words under a payment that waits: a wallet that sent it itself needs nothing more. */
    fun waitingWords(state: OwnSendState): SayWords = when (state) {
        OwnSendState.NotFiled -> OwnSpendWords.notGiven
        OwnSendState.Waiting -> OwnSpendWords.waiting
    }

    /** OA-117: the screen shows the waiting words with a payment given to the wallet and not yet taken. */
    fun showsWaiting(item: OwnSpendItem): Boolean = item.filed

    /** OA-117: after a refusal nothing else changes: the same picks, the same session, the same file. */
    fun afterRefusal(picked: List<Long>, session: ByteArray?): Pair<List<Long>, ByteArray?> = picked to session
}

/**
 * The words of "Send from your wallet" (OA-115 to OA-118). Refusals come from the library with their own words
 * (`docs/screens/owner.md`: AS-100 to AS-104); these are the app's own steps.
 */
object OwnSpendWords {
    private fun w(tone: Int, heading: String, what: String, who: Int, next: String) =
        SayWords(tone, who, heading, what, next, null, null, null, null)

    val list = w(
        Tones.INFO, "Send from your wallet",
        "These refunds and cash-outs wait to be paid. They are paid from the shop's own wallet, which signs them.",
        Who.OWNER,
        "Pick the ones to send now, then tap Prepare.",
    )

    val empty = w(
        Tones.GOOD, "Nothing to send",
        "No refund or cash-out is waiting to be paid from the shop's wallet.",
        Who.NOBODY,
        "Nothing to do.",
    )


    val compare = w(
        Tones.INFO, "What the wallet will show",
        "Each payment below is one transaction. The wallet shows each address, each amount and the fee.",
        Who.OWNER,
        "Keep this screen at hand. Approve in the wallet only if every line there matches these.",
    )

    val transport = w(
        Tones.INFO, "Give it to the wallet",
        "The payment goes to the wallet as moving codes on this screen or as a file. Nothing on this phone can sign it.",
        Who.OWNER,
        "Scan the code or open the file in the wallet, check every line, and sign there.",
    )

    val returnNeeded = w(
        Tones.INFO, "Bring the payment back",
        "The wallet has the payment. What it signed has to come back to this phone unless the wallet sent the payment itself.",
        Who.OWNER,
        "Scan the code the wallet shows, or open the file it saved. A payment the wallet finished works too.",
    )

    val taken = w(
        Tones.GOOD, "Payment taken",
        "The signed payment matches the one prepared here, and it goes out to the network.",
        Who.NOBODY,
        "Nothing to do. The queue shows it as paid once it is in a block.",
    )

    val waiting = w(
        Tones.INFO, "Waiting for the wallet",
        "This payment was given to the wallet. If the wallet sends it itself, this phone sees it and marks it paid once it is in a block.",
        Who.OWNER,
        "Bring back what the wallet signed, or do nothing if the wallet sent it.",
    )

    val notGiven = w(
        Tones.INFO, "Not given to the wallet",
        "This payment is prepared, but its file has not been given to the wallet yet.",
        Who.OWNER,
        "Open it and give the file to the wallet, or give it up to put it back in the queue.",
    )

    val abandoned = w(
        Tones.INFO, "Payment given up",
        "The prepared payment was given up, and its refunds and cash-outs are back in the queue.",
        Who.NOBODY,
        "Nothing to do. If the wallet still sends the copy it signed, they are marked paid, and a new payment for them can never pay them twice.",
    )

    /** OA-118: every words of this section, by name, for the checks and docs/screens/owner-app.md. */
    fun all(): List<Pair<String, SayWords>> = listOf(
        "ownSpendList" to list,
        "ownSpendEmpty" to empty,
        "ownSpendCompare" to compare,
        "ownSpendTransport" to transport,
        "ownSpendReturn" to returnNeeded,
        "ownSpendTaken" to taken,
        "ownSpendWaiting" to waiting,
        "ownSpendNotGiven" to notGiven,
        "ownSpendAbandoned" to abandoned,
    )
}
