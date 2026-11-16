package org.openbmt.owner.demo

import org.openbmt.app.Format
import org.openbmt.app.SayWords
import org.openbmt.app.WordsModel
import org.openbmt.owner.BooksDay
import org.openbmt.owner.Books
import org.openbmt.owner.Native
import org.openbmt.owner.OwnerNetwork
import org.openbmt.owner.OwnerNode
import org.openbmt.owner.PosAlert
import org.openbmt.owner.PosAlertKey
import org.openbmt.owner.PosAlertKind
import org.openbmt.owner.PosSetup
import org.openbmt.owner.PosState
import org.openbmt.owner.PosSubject
import org.openbmt.owner.PosVendor
import org.openbmt.owner.Words
import org.openbmt.owner.StartConfig
import org.openbmt.owner.ViewKind
import org.openbmt.owner.app.AppWords
import org.openbmt.owner.app.BooksModel
import org.openbmt.owner.app.OwnerLook
import org.openbmt.owner.app.OwnerModel
import org.openbmt.owner.app.PosLines
import org.openbmt.owner.app.RequestForm
import org.openbmt.owner.app.RequestModel
import org.openbmt.owner.app.RequestWords
import org.openbmt.owner.app.StarterChecks
import org.openbmt.owner.app.StarterHome
import org.openbmt.owner.app.StarterSetup
import org.openbmt.owner.app.say

/**
 * What the page's script can call, as the proof that the owner app's own
 * model runs in the browser: the books page over a few days, the dates and
 * limits of the setup and the request form (which run on the calendar and
 * big-number shims), and the one native call.
 */
@JsExport
object OwnerModelDemo {
    /** The rows of the books page for the days given as parallel arrays, one "date|totals" text each. */
    fun booksRows(currency: String, dates: Array<String>, sales: Array<Double>, amounts: Array<String>): Array<String> {
        val days = dates.indices.map { BooksDay(dates[it], sales[it].toLong(), 0, amounts[it]) }
        return BooksModel.page(Books(currency, days, emptyList(), null)).rows.map { "${it.date}|${it.totals}" }.toTypedArray()
    }

    /** The limit typed, in satoshis at [pricePerBtc]; -1 when it does not read. */
    fun limitSats(text: String, pricePerBtc: Double): Double = StarterSetup.limitSats(text, pricePerBtc.toLong())?.toDouble() ?: -1.0

    /** The date the setup suggests, 30 days on from [now] (seconds). */
    fun suggestedDate(now: Double): String = StarterSetup.suggestedDate(now.toLong())

    /** The second the typed day begins (UTC), or -1. */
    fun moveBy(text: String, now: Double): Double = StarterSetup.moveBy(text, now.toLong())?.toDouble() ?: -1.0

    /** Whether the security-patch date is more than six months before [today] (yyyy-mm-dd). */
    fun patchIsOld(patch: String, today: String): Boolean = StarterChecks.patchIsOld(patch, StarterChecks.parseDate(today)!!)

    /** The day of the shop's time zone at [now], as the form writes it. */
    fun shopDay(now: Double, offsetMinutes: Int): String = RequestModel.today(now.toLong(), offsetMinutes).toString()

    /** "yes", "no" or "bad": whether the typed due day is from [today] to 366 days on. */
    fun dueDay(text: String, today: String): String {
        val due = RequestForm.day(text) ?: return "bad"
        return if (RequestForm.inRange(due, StarterChecks.parseDate(today)!!)) "yes" else "no"
    }

    /** The date of [secs] as the starter home writes it. */
    fun homeDate(secs: Double): String = StarterHome.date(secs.toLong())

    /** The request bytes through [Native.call] and the answer's length, or -1 when the page has no answer. */
    fun nativeAnswerLength(request: ByteArray): Int = Native.call(request)?.size ?: -1
}

/** What a words shows (WordsModel): the colour's name, the three lines, who as a sentence, the quoted text and the details. */
@JsExport
class ShownWords(
    val colour: String,
    val heading: String,
    val what: String,
    val next: String,
    val who: String,
    val quotedLabel: String?,
    val quotedText: String?,
    val details: Array<String>,
)

private fun shown(words: SayWords): ShownWords {
    val m = WordsModel.of(words)
    return ShownWords(m.colour.name, m.heading, m.what, m.next, m.who, m.quotedLabel, m.quotedText, m.details.toTypedArray())
}

/**
 * What the owner's home draws (Screens.kt showHome): the status of the network and the lines under it, the balance as
 * its three parts, the waiting card's rows (a title and a count each), the four groups' titles and the line under each,
 * and the footer's buttons. [signature] changes when anything on it does.
 */
@JsExport
class OwnerHomeData(
    val status: ShownWords,
    val extra: Array<ShownWords>,
    val lead: String,
    val amount: String,
    val checked: String,
    val spend: String,
    val footer: Array<String>,
    val waitingTitle: String,
    val waitingTitles: Array<String>,
    val waitingCounts: Array<Int>,
    val groupTitles: Array<String>,
    val groupSubs: Array<String>,
    val signature: String,
)

/**
 * The owner app's own entry points, driven from the page's script: the node is started with [OwnerNode.start] on
 * the demo shop's wallet with the Key app's keys, and what it holds is read with [OwnerNode.status],
 * [OwnerNode.receiptsLine] and [OwnerNode.books], all through [Native.call], which the page answers with the
 * WebAssembly build of the owner's library.
 */
@JsExport
object OwnerFlowDemo {
    private var node: OwnerNode? = null

    private fun running(): OwnerNode = node ?: error("the node is not started")

    /**
     * Start the node on the folder [dir] for the demo shop: its wallet text [wallet] (the 2-of-3 wallet the counter takes
     * its payments into), the approver's root and Key 1's account key, which the module gives; and take its first
     * events (the app pumps in a loop).
     */
    fun start(dir: String, now: Double, wallet: String, approverRoot: ByteArray, key1Account: ByteArray) {
        val config = StartConfig(
            dir = dir,
            network = OwnerNetwork.Regtest,
            now = now.toLong(),
            ownerSecret = ByteArray(32) { 1 },
            approverRoot = approverRoot,
            wallet = wallet,
            key1Account = key1Account,
            key3Xpub = null,
            batchMinCount = 1000,
            refundDepth = 6,
            birthday = 0,
            proxy = null,
            fixedPeers = emptyList(),
            broadcastProxy = null,
            withoutTor = true,
            link = null,
            ownWallet = null,
        )
        val started = OwnerNode.start(config)
        node = started
        started.pump(now.toLong(), 0)
    }

    /** Take the network's events; how many lines they logged. */
    fun pump(now: Double): Int = running().pump(now.toLong(), 0).notes.size

    /** What the wallet holds, in satoshis, as the node's coin view counts it. */
    fun balanceSats(): Double = running().status().balanceSats.toDouble()

    /** The height of the chain the node has checked. */
    fun checkedHeight(): Double = running().status().checkedHeight.toDouble()

    /** Whether the node says the chain it watches is healthy. */
    fun healthy(): Boolean = running().status().health.healthy

    /** The kind of the line about the payments received: NotReady, NoCoins, FeeUnknown or Known. */
    fun receiptsKind(): String = running().receiptsLine().kind.name

    /** The number of payments the receipts line counts. */
    fun receiptsPayments(): Double = running().receiptsLine().payments.toDouble()

    /** The currency of the books page and the days it has, as "currency|days". */
    fun booksSummary(): String {
        val books = running().books(0)
        return "${books.currency}|${books.days.size}"
    }

    /**
     * What the owner's home draws, from the node's own state, as Controller.showHome reads it: the network's words,
     * the lines under them when the view is not ready, the balance, the waiting card's counts (the app gives none today),
     * and the groups' lines. The shop is paid to its own wallet, so Request a payment is in the footer (RQ-35).
     */
    fun homeScreen(): OwnerHomeData {
        val n = running()
        val s = n.status()
        val status = when {
            s.stopped -> AppWords.stopped
            else -> s.health.words.say()
        }
        val extra = ArrayList<ShownWords>()
        s.halted?.let { extra.add(shown(it.words.say())) }
        if (s.view != ViewKind.Ready) extra.add(shown(s.viewWords.say()))
        val balance = OwnerLook.balance(Format.sats(s.balanceSats), s.checkedHeight, s.tipHeight)
        val counts = listOf<Pair<String, Int?>>("Open approvals" to null, "Waiting to be delivered" to null, "Refund queue" to null)
        val rows = OwnerLook.waitingRows(counts)
        val inProgress = OwnerModel.sendsInProgress(n.sendStates())
        // The demo's phone was set up without a link to counters, as the app's own `setup?.link` says.
        val subs = arrayOf(
            OwnerLook.moneySub(inProgress, OwnerModel.sendsLine(inProgress)),
            OwnerModel.linkLine(null, 0),
            OwnerModel.alertsLine(0),
            OwnerLook.SETTINGS_SUB,
        )
        val titles = arrayOf(OwnerLook.MONEY, OwnerLook.DEVICES, OwnerLook.ALERTS, OwnerLook.SETTINGS)
        val footer = if (RequestModel.offered(true)) arrayOf(RequestWords.homeButton) else emptyArray()
        val signature = listOf(status.toString(), s.balanceSats, s.checkedHeight, s.tipHeight, extra.size, rows.size, subs.joinToString("|")).joinToString("/")
        return OwnerHomeData(
            shown(status), extra.toTypedArray(), balance.lead, balance.amount, balance.checked, "Spend", footer,
            OwnerLook.WAITING_TITLE, rows.map { it.title }.toTypedArray(), rows.map { it.count }.toTypedArray(), titles, subs, signature,
        )
    }

    /**
     * PosScreen.kt showTill: the two lines of the till screen that say what was read, as PosLines writes them: when the
     * till was last read, and the counts of matches and alerts. The line that names the till's vendor is not here: the
     * demo's register is no vendor's. [lastReadAt] is 0 for never.
     */
    fun tillLines(now: Double, lastReadAt: Double, matches: Double, byOrder: Double, alerts: Int): Array<String> {
        val setup = PosSetup(PosVendor.Square, "", listOf("Bitcoin"), listOf("USD"))
        val none = Words(0, 0, "", "", "", null, null, null, null)
        val open = List(alerts) { PosAlert(PosAlertKey(PosAlertKind.TenderWithoutPayment, PosSubject.Tender("", "")), 0L, none) }
        val at = if (lastReadAt > 0.0) lastReadAt.toLong() else null
        val state = PosState(setup, at, open, 0L, 0L, matches.toLong(), byOrder.toLong(), 0L)
        return arrayOf(PosLines.lastRead(now.toLong(), at), PosLines.counts(state))
    }

    /** Stop the node. */
    fun stop() {
        node?.stop()
        node = null
    }
}
