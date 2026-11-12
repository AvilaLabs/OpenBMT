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
        OwnSendState.NotFiled -> placeholder(WORDS_NEEDED_OA_117_NOT_GIVEN)
        OwnSendState.Waiting -> placeholder(WORDS_NEEDED_OA_117_WAITING)
    }

    /** OA-117: the screen shows the waiting words with a payment given to the wallet and not yet taken. */
    fun showsWaiting(item: OwnSpendItem): Boolean = item.filed

    /** OA-117: after a refusal nothing else changes: the same picks, the same session, the same file. */
    fun afterRefusal(picked: List<Long>, session: ByteArray?): Pair<List<Long>, ByteArray?> = picked to session

    // The words are not written yet (`docs/SCREEN-RULES.md`): each state names the rule it needs (OA-118).
    const val WORDS_NEEDED_OA_115_LIST = "WORDS_NEEDED_OA_115_LIST"
    const val WORDS_NEEDED_OA_115_EMPTY = "WORDS_NEEDED_OA_115_EMPTY"
    const val WORDS_NEEDED_OA_115_MERGE = "WORDS_NEEDED_OA_115_MERGE"
    const val WORDS_NEEDED_OA_115_COMPARE = "WORDS_NEEDED_OA_115_COMPARE"
    const val WORDS_NEEDED_OA_116_TRANSPORT = "WORDS_NEEDED_OA_116_TRANSPORT"
    const val WORDS_NEEDED_OA_116_RETURN = "WORDS_NEEDED_OA_116_RETURN"
    const val WORDS_NEEDED_OA_116_TAKEN = "WORDS_NEEDED_OA_116_TAKEN"
    const val WORDS_NEEDED_OA_117_WAITING = "WORDS_NEEDED_OA_117_WAITING"
    const val WORDS_NEEDED_OA_117_NOT_GIVEN = "WORDS_NEEDED_OA_117_NOT_GIVEN"
    const val WORDS_NEEDED_OA_117_ABANDON = "WORDS_NEEDED_OA_117_ABANDON"

    /** OA-118: every placeholder this section uses, in the order of the rules. */
    val PLACEHOLDERS: List<String> = listOf(
        WORDS_NEEDED_OA_115_LIST, WORDS_NEEDED_OA_115_EMPTY, WORDS_NEEDED_OA_115_MERGE, WORDS_NEEDED_OA_115_COMPARE,
        WORDS_NEEDED_OA_116_TRANSPORT, WORDS_NEEDED_OA_116_RETURN, WORDS_NEEDED_OA_116_TAKEN,
        WORDS_NEEDED_OA_117_WAITING, WORDS_NEEDED_OA_117_NOT_GIVEN, WORDS_NEEDED_OA_117_ABANDON,
    )

    /** OA-118: the words of a state whose words are not written: the constant stands for each part. */
    fun placeholder(name: String): SayWords =
        SayWords(Tones.INFO, Who.OWNER, name, "$name.", "$name.", null, null, null, null)
}
