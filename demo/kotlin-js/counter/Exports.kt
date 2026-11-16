package org.openbmt.counter.demo

import org.openbmt.app.Format
import org.openbmt.app.SayWords
import org.openbmt.app.WordsModel
import org.openbmt.counter.CounterException
import org.openbmt.counter.CounterNode
import org.openbmt.counter.LnMethod
import org.openbmt.counter.Native
import org.openbmt.counter.StartConfig
import org.openbmt.counter.app.AmountEntry
import org.openbmt.counter.app.AppWords
import org.openbmt.app.Colour
import org.openbmt.counter.Turn
import org.openbmt.counter.app.BillModel
import org.openbmt.counter.app.LightningFirstWords
import org.openbmt.counter.app.Notices
import org.openbmt.counter.app.OutcomePlays
import org.openbmt.counter.app.Money
import org.openbmt.counter.app.SaleModel
import org.openbmt.counter.app.SetupStatus
import org.openbmt.counter.app.ScreenModel
import org.openbmt.counter.app.Showing
import org.openbmt.counter.app.StarterModel
import org.openbmt.counter.app.TillModel
import org.openbmt.counter.app.WaitWords
import org.openbmt.counter.app.sayWords
import org.openbmt.counter.app.say

/**
 * What the page's script can call, as the first proof that the apps' own
 * model runs in the browser: the amounts of the counter's keypad, and the
 * one native call.
 */
@JsExport
object CounterModelDemo {
    /** [minor] in [code] as the counter writes it ("12.34 USD"), or null for a currency the keypad has not. */
    fun showAmount(code: String, minor: Double): String? = Money.of(code)?.show(minor.toLong())

    /** What a person typed, in minor units; -1 when it is not an amount. */
    fun parseAmount(code: String, text: String): Double = Money.of(code)?.parse(text)?.toDouble() ?: -1.0

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

/** What the home screen draws: the health card, its lines in order (with the key of each), the counter's line and the open requests. */
@JsExport
class HomeScreenData(
    val health: ShownWords,
    val lineKeys: Array<String>,
    val lines: Array<ShownWords>,
    val counterLine: String,
    val requestTitles: Array<String>,
    val requestSubs: Array<String>,
)

/** What the card of one request from the till shows (Controller.kt showHome, TillModel.lines): the till's sale as hex, and each line's label and value. */
@JsExport
class TillCardData(
    val sale: String,
    val labels: Array<String>,
    val values: Array<String>,
)

/**
 * What a sale's screen draws, as the controller works it out from the library's judgment of the sale. [qrBits]
 * holds [qrSize] times [qrSize] modules, most significant bit first, row by row (Payment.module).
 */
@JsExport
class SaleScreenData(
    val number: Int,
    val state: String,
    val says: ShownWords,
    val code: Boolean,
    val isPaid: Boolean,
    val amountBig: String,
    val amountSmall: String,
    val receivedLine: String,
    val outcomeLine: String,
    val outcomeHeadingSp: Double,
    val requote: Boolean,
    val cancel: Boolean,
    val override: Boolean,
    val extra: Array<ShownWords>,
    val paidLabels: Array<String>,
    val paidValues: Array<String>,
    val qrSize: Int,
    val qrBits: ByteArray?,
    val uri: String?,
    val copyUri: String?,
    /** The NDEF file the counter's tag holds for the sale (TP-01): what a phone that taps reads; null when no code shows. */
    val tag: ByteArray?,
    val address: String?,
    val needsSats: Double,
    val receivedSats: Double,
    /** CR-283: the code is the Lightning invoice alone; the screen adds the line and the button that show the on-chain code. */
    val lightningFirst: Boolean,
    /** CR-285: "Still to pay" over the large amount of a part paid sale; null for every other state. */
    val stillHeading: String?,
    /** CR-286: the seconds left of a wait that knows its end and the whole of it, for the ring; null when there is none or it is over. */
    val ringSecs: Double?,
    val ringTotal: Double,
    /** CR-286: the number in the ring ("5 s"), and for a wait for a block how long ago the payment was seen ("Seen 40 s ago"). */
    val ringLabel: String?,
    val seen: String?,
    /** CR-286: the mark beside the time pulses, only while the check is healthy. */
    val pulse: Boolean,
    /** CR-286: the seconds left of the wait for the Lightning invoice and the whole of it, for the ring beside the Lightning line; null when no invoice is waited for or the time is over. */
    val lnRingSecs: Double?,
    val lnRingTotal: Double,
    val lnRingLabel: String?,
    /** The place of the Lightning line among [extra], or -1 when the screen has none. */
    val lnRingAt: Int,
    /** One text of everything the screen says: it is drawn again only when this changes. */
    val signature: String,
)

/** What one notice of CR-284 shows (Notices.kt SaleNotice): the sale's receive index, whether it turned green, and the two lines. */
@JsExport
class NoticeData(
    val index: Double,
    val green: Boolean,
    val heading: String,
    val next: String,
)

/**
 * The counter app's own entry points, driven from the page's script: the node is started with
 * [CounterNode.start], a sale is opened with [CounterNode.open], and the sale's state is read with
 * [CounterNode.judge] and [SaleModel.isPaid], all through [Native.call], which the page answers with the
 * WebAssembly build of the counter's library. The screens' data is what Controller.kt works out from the same
 * calls (the page's controller draws it).
 */
@JsExport
object CounterFlowDemo {
    private var node: CounterNode? = null
    private var money: Money? = null
    private var entry: AmountEntry? = null
    private val amounts = HashMap<Long, Long>()

    // Controller.kt's notices (CR-284), the sounds its screens have played, and the sales that turned since the page last asked.
    private var notices = Notices()
    private var plays = OutcomePlays()
    private val turned = ArrayList<Turn>()

    private fun running(): CounterNode = node ?: error("the node is not started")

    /** The pretend price of the demo, in minor units of the dollar for one bitcoin: 73,554.18 USD. */
    const val DEMO_RATE = 7_355_418.0

    /** The source the sales name, in place of a price source. */
    const val DEMO_SOURCE = "demo"

    // The pretend Lightning node of the page (counter-host.mjs): the saved connection it starts the counter with, and
    // the answer it gives to a request the counter's driver hands out. Both are absent when the page has no such node.
    private val lightningRecord: dynamic = js("(function () { var f = globalThis.openbmtLightningRecord; return f ? f() : null; })")
    private val lightningAnswer: dynamic = js("(function (m, p, b) { var f = globalThis.openbmtLightningAnswer; return f ? f(m, p, b) : null; })")

    private fun bytesOf(answer: dynamic): ByteArray? {
        if (answer == null || answer == undefined) return null
        val size = answer.length as Int
        return ByteArray(size) { i -> (answer[i] as Int).toByte() }
    }

    /** Most requests of the Lightning driver answered in one turn of the pump. */
    private const val LIGHTNING_PER_PUMP = 8

    /**
     * Controller.kt's Lightning thread, in the pump: every request the counter's driver hands out (`lnNext`) goes to the
     * page's pretend node and its answer comes back with `lnReply`, so the counter itself makes the sale's code with the
     * invoice, asks whether it is paid, and judges the sale.
     */
    private fun serveLightning(n: CounterNode, now: Long) {
        repeat(LIGHTNING_PER_PUMP) {
            val request = n.lnNext(now) ?: return
            val method = if (request.method == LnMethod.Post) "POST" else "GET"
            val answer = bytesOf(lightningAnswer(method, request.path, request.body ?: ""))
            if (answer == null || answer.size < 2) return
            val status = (answer[0].toInt() and 0xff) or ((answer[1].toInt() and 0xff) shl 8)
            n.lnReply(request.id, status.toLong(), answer.copyOfRange(2, answer.size))
        }
    }

    /** Start the node on the folder [dir] from the pairing [record], and take its first events (the app pumps in a loop). */
    fun start(dir: String, now: Double, record: ByteArray) {
        val started = CounterNode.start(StartConfig(dir, now.toLong(), record, null, lightning = bytesOf(lightningRecord())))
        node = started
        money = Money.of("USD")
        amounts.clear()
        notices = Notices()
        plays = OutcomePlays()
        turned.clear()
        started.pump(now.toLong(), 0)
    }

    /** Open a sale of [amountMinor] in [currency] at [rate] minor units per bitcoin; the address the customer pays. */
    fun openSale(now: Double, amountMinor: Double, currency: String, rate: Double): String {
        val opened = running().open(now.toLong(), amountMinor.toLong(), currency, rate.toLong(), "demo", now.toLong())
        lastIndex = opened.index
        needsSats = opened.needsSats
        amounts[opened.index] = amountMinor.toLong()
        return opened.payment.address
    }

    private var lastIndex = 0L
    private var needsSats = 0L

    /** The sats the last sale needs. */
    fun satsNeeded(): Double = needsSats.toDouble()

    /** Take the heartbeat of the owner's phone ([bytes], signed); whether the counter verified it. */
    fun heartbeat(now: Double, bytes: ByteArray): Boolean = running().heartbeat(now.toLong(), bytes).verified

    /** Take the network's events; how many reports left. */
    fun pump(now: Double): Int {
        val n = running()
        serveLightning(n, now.toLong())
        val p = n.pump(now.toLong(), 0)
        turned.addAll(p.turns)
        return p.reports.size
    }

    private fun colourOf(name: String): Colour = if (name == Colour.Green.name) Colour.Green else if (name == Colour.Red.name) Colour.Red else Colour.Blue

    /** SaleModel.kt OutcomePlays.first: true the first time sale [index] shows the colour called [colour] and the colour plays a sound. */
    fun outcomeFirst(index: Double, colour: String): Boolean = plays.first(index.toLong(), colourOf(colour))

    /**
     * Controller.kt onTurns: the sales that turned green or red since the last call each get a notice, unless the screen of
     * sale [showing] (-1 when no sale shows) is theirs. The name of the colour that plays its sound now, or null when none does.
     */
    fun takeTurns(showing: Double): String? {
        val on = if (showing < 0) null else showing.toLong()
        var played: Colour? = null
        for (t in turned) {
            val colour = if (t.green) Colour.Green else Colour.Red
            if (notices.turned(t.index, colour, on, plays)) played = colour
        }
        turned.clear()
        return played?.name
    }

    /** Controller.kt drawNotices: the notices that show, newest first. */
    fun noticesShown(): Array<NoticeData> =
        notices.shown().map { NoticeData(it.index.toDouble(), it.colour == Colour.Green, it.heading, it.next) }.toTypedArray()

    /** Controller.kt drawNotices: the line after the notices that show, or null when there is none. */
    fun noticesMore(): String? = notices.more().let { if (it > 0) LightningFirstWords.moreLine(it) else null }

    /** Controller.kt drawNotices, showSale: the notice of sale [index] goes; true when there was one. */
    fun noticeRemove(index: Double): Boolean = notices.remove(index.toLong())

    /** CR-283: the line under a code that is the Lightning invoice alone. */
    fun lightningLine(): String = LightningFirstWords.LINE

    /** CR-283: the button that shows the on-chain code. */
    fun lightningButton(): String = LightningFirstWords.BUTTON

    /** Controller.kt payOnChainInstead: the cashier chose the on-chain code for the rest of sale [index]. A refusal is the library's own exception. */
    fun onChainInstead(index: Double) = running().saleOnChainInstead(index.toLong())

    /** The name of the last sale's state now (the library's own judgment), as the model has it. */
    fun saleState(now: Double): String = running().judge(now.toLong(), lastIndex).state.name

    /** Whether the sale screen's model counts the last sale as paid. */
    fun salePaid(now: Double): Boolean = SaleModel.isPaid(running().judge(now.toLong(), lastIndex).state)

    // ---- Controller.kt: the keypad of a new sale ----------------------------------------------------------------

    /** Controller.kt showNewSale: a new entry on the shop's keypad; what it shows. */
    fun entryNew(): String {
        val e = AmountEntry(money ?: error("the node is not started"))
        entry = e
        return e.show()
    }

    /** Controller.kt showNewSale, the keypad: a key's face ("C", the backspace or a digit) pressed; what the entry shows. */
    fun entryPress(face: String): String {
        val e = entry ?: error("no entry")
        when (face) {
            "C" -> e.clear()
            "\u232B" -> e.backspace()
            else -> e.press(face[0])
        }
        return e.show()
    }

    /** What the entry shows now. */
    fun entryShow(): String = (entry ?: error("no entry")).show()

    /** The entry in minor units. */
    fun entryMinor(): Double = (entry ?: error("no entry")).minor.toDouble()

    /** Controller.kt priceWords: the price line, the pretend price of the demo. */
    fun priceWords(): ShownWords = shown(AppWords.priceReady((money ?: error("the node is not started")).show(DEMO_RATE.toLong()), 3))

    /** Controller.kt chooseTip: how many tips the counter offers for [amountMinor]; 0 opens the sale at once. */
    fun tipChoices(amountMinor: Double): Int = running().tipChoices(amountMinor.toLong()).size

    /**
     * Controller.kt openSale: opens the sale for [amountMinor] at the pretend price, as the app does with the
     * price it has; the sale's receive index. A refusal is the library's own exception.
     */
    fun openEntered(now: Double, amountMinor: Double): Double {
        val m = money ?: error("the node is not started")
        val opened = running().open(now.toLong(), amountMinor.toLong(), m.code, DEMO_RATE.toLong(), DEMO_SOURCE, now.toLong(), null)
        amounts[opened.index] = amountMinor.toLong()
        lastIndex = opened.index
        needsSats = opened.needsSats
        return opened.index.toDouble()
    }

    // ---- Controller.kt: the requests from the till (CR-266 to CR-269) ---------------------------------------------

    private fun hex(bytes: ByteArray): String =
        bytes.joinToString("") { b -> "0123456789abcdef"[(b.toInt() shr 4) and 15].toString() + "0123456789abcdef"[b.toInt() and 15] }

    private fun waiting(now: Double) = try { TillModel.shown(running().tillWaiting(now.toLong())) } catch (e: CounterException) { emptyList() }

    /** Controller.kt showHome: the cards of the requests from the till that wait, oldest first, as TillModel makes them. */
    fun tillCards(now: Double): Array<TillCardData> {
        val m = money ?: return emptyArray()
        return waiting(now).map { r ->
            val lines = TillModel.lines(m, r)
            TillCardData(hex(r.sale), lines.map { it.label }.toTypedArray(), lines.map { it.value }.toTypedArray())
        }.toTypedArray()
    }

    /**
     * Controller.kt startTillSale: Start on the card of the till's sale [sale] (as hex): the counter opens the sale for
     * the request's amount at the pretend price; the sale's receive index. A refusal is the library's own exception.
     */
    fun tillStart(now: Double, sale: String): Double {
        val n = running()
        val request = waiting(now).first { hex(it.sale) == sale }
        val opened = n.tillStart(now.toLong(), request.sale, DEMO_RATE.toLong(), DEMO_SOURCE, now.toLong(), null)
        amounts[opened.index] = request.amountMinor
        lastIndex = opened.index
        needsSats = opened.needsSats
        return opened.index.toDouble()
    }

    /** Controller.kt showHome: Not now on the card of the till's sale [sale] (as hex). */
    fun tillNotNow(now: Double, sale: String) {
        val request = waiting(now).first { hex(it.sale) == sale }
        running().tillNotNow(now.toLong(), request.sale)
    }

    /** CR-268: the till's sale (as hex) that the sale [index] came from, or null when it was typed at the counter. */
    fun tillOfSale(index: Double): String? = running().tillOfSale(index.toLong()).sale?.let { hex(it) }

    // ---- Controller.kt: what the screens say ----------------------------------------------------------------------

    private fun healthWords(n: CounterNode): SayWords = try {
        val s = n.status()
        if (s.stopped) AppWords.stopped else s.health?.words?.say() ?: AppWords.starting
    } catch (e: CounterException) {
        e.sayWords()
    }

    /** Controller.kt showHome: the data of the home screen at [now]. */
    fun homeScreen(now: Double): HomeScreenData {
        val n = running()
        val m = money ?: error("the node is not started")
        val keys = ArrayList<String>()
        val lines = ArrayList<ShownWords>()
        // CR-116: the Lightning line is there only while a connection is saved.
        try {
            n.status().lightning?.words?.say()?.let { keys.add("lightning"); lines.add(shown(it)) }
        } catch (e: CounterException) {
            keys.add("lightning"); lines.add(shown(e.sayWords()))
        }
        // CR-273: a stop, or a line that cannot be checked, with the library's words.
        val starter = try { n.starterState(now.toLong()) } catch (e: CounterException) { null }
        StarterModel.homeLine(starter)?.let { keys.add("starter"); lines.add(shown(it)) }
        keys.add("price")
        lines.add(shown(AppWords.priceReady(m.show(DEMO_RATE.toLong()), 3)))
        // CR-215: the open requests, newest first.
        val open = try { n.requests(now.toLong()) } catch (e: CounterException) { emptyList() }
        return HomeScreenData(
            shown(healthWords(n)),
            keys.toTypedArray(),
            lines.toTypedArray(),
            ScreenModel.counterLine(Showing.Home, SetupStatus(true, true, true), null) ?: "",
            open.map { r -> r.note ?: m.show(r.amountMinor) }.toTypedArray(),
            open.map { r -> Format.localTime(r.expiresAt) }.toTypedArray(),
        )
    }

    /** Controller.kt showSale: the data of the sale [index]'s screen at [now], from the library's judgment. */
    fun saleScreen(now: Double, index: Double): SaleScreenData {
        val n = running()
        val m = money ?: error("the node is not started")
        val i = index.toLong()
        val s = n.judge(now.toLong(), i)
        val info = n.saleInfo(i)
        val request = info.requestDays > 0
        val says = if (request && s.state == org.openbmt.counter.SaleState.NotPaidYet) {
            AppWords.requestOpen(Format.localTime(info.expiresAt))
        } else {
            s.words.say()
        }
        val extra = ArrayList<SayWords>()
        s.check?.let { extra.add(it.say()) }
        for (f in s.flags) extra.add(f.say())
        // CR-113, CR-286: while the invoice is waited for the sale says so, and the line has the ring of the 6 seconds.
        val ln = s.lightning
        var lnRingAt = -1
        var lnLeft: Long? = null
        var lnTotal = 0L
        if (ln != null && ln.waiting) {
            lnRingAt = extra.size
            extra.add(AppWords.lightningWaiting)
            val left = ln.waitSecsLeft
            val total = ln.waitSecsTotal
            if (left != null && total != null) {
                lnLeft = left
                lnTotal = total
            }
        }
        val code = SaleModel.showsCode(s.state)
        val b = SaleModel.buttons(s.state, request)
        val amount = amounts[i]
        val payment = s.payment
        val labels = ArrayList<String>()
        val values = ArrayList<String>()
        if (!code && SaleModel.isPaid(s.state)) {
            for (line in BillModel.paidLines(i, info)) {
                labels.add(line.label)
                values.add(line.value)
            }
        }
        val shownSays = shown(says)
        val shownExtra = extra.map { shown(it) }
        // CR-285: a part paid sale leads with what is missing; CR-286: what a wait shows of its time.
        val still = if (s.state == org.openbmt.counter.SaleState.PartPaid) SaleModel.stillToPay(m, s.missingSats, s.missingMinor, s.receivedSats, s.needsSats, amount) else null
        val wait = SaleModel.wait(s.state, s.watchSecsLeft, s.watchSecsTotal, s.seenSecs, s.checkHealthy)
        val signature = listOf(
            s.state.name, shownSays.heading, shownSays.what, shownSays.next, s.receivedSats, s.needsSats, s.missingSats, s.missingMinor,
            wait.ringSecs != null, wait.seen != null, wait.pulse, lnLeft != null, payment?.copyUri,
            shownExtra.joinToString("\u0002") { it.heading + it.what + it.next }, s.lightningFirst, info.reference, info.billNumber,
        ).joinToString("\u0001")
        return SaleScreenData(
            number = (i + 1).toInt(),
            state = s.state.name,
            says = shownSays,
            code = code,
            isPaid = SaleModel.isPaid(s.state),
            amountBig = still?.big ?: SaleModel.amountBig(m, amount, s.needsSats),
            amountSmall = still?.toSend ?: SaleModel.amountSmall(m, amount, s.needsSats),
            receivedLine = still?.paid ?: SaleModel.receivedLine(s.receivedSats, s.needsSats),
            outcomeLine = SaleModel.outcomeLine(m, amount, s.needsSats),
            outcomeHeadingSp = SaleModel.outcomeHeadingSp(says.heading).toDouble(),
            requote = b.requote,
            cancel = b.cancel,
            override = b.override,
            extra = shownExtra.toTypedArray(),
            paidLabels = labels.toTypedArray(),
            paidValues = values.toTypedArray(),
            qrSize = if (code) payment?.qrSize?.toInt() ?: 0 else 0,
            qrBits = if (code) payment?.qrBits else null,
            uri = if (code) payment?.uri else null,
            copyUri = if (code) payment?.copyUri else null,
            tag = if (code) payment?.tag else null,
            address = payment?.address,
            needsSats = s.needsSats.toDouble(),
            receivedSats = s.receivedSats.toDouble(),
            lightningFirst = s.lightningFirst,
            stillHeading = still?.heading,
            ringSecs = wait.ringSecs?.toDouble(),
            ringTotal = wait.ringTotal.toDouble(),
            ringLabel = wait.ringSecs?.let { WaitWords.ring(it) },
            seen = wait.seen,
            pulse = wait.pulse,
            lnRingSecs = lnLeft?.toDouble(),
            lnRingTotal = lnTotal.toDouble(),
            lnRingLabel = lnLeft?.let { WaitWords.ring(it) },
            lnRingAt = lnRingAt,
            signature = signature,
        )
    }

    /** Stop the node. */
    fun stop() {
        node?.stop()
        node = null
    }
}
