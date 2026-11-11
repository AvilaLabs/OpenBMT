package org.openbmt.counter

import java.io.ByteArrayOutputStream
import java.nio.ByteBuffer
import java.nio.charset.CharacterCodingException
import java.nio.charset.CodingErrorAction

// The binding and OBMn decoder harness compile and run on the JVM. This does
// not compile or load the Android JNI library or exercise a device UI.

/** A call that did not do what was asked. [rule] is the ID of the rule that refused. */
class CounterException(
    val rule: String,
    val text: String,
    /** 1 refused, 2 store, 3 state, 4 internal (a panic was caught). */
    val status: Int = 1,
    /** What the person is shown. Null for an error the Kotlin side made itself. */
    val words: Words? = null,
) : Exception(text)

/** Text from another device or a scanned code: [from] says where it came from. */
data class Quoted(val from: String, val text: String)

/**
 * What a person is shown, for an error and for every state a result carries:
 * the fields of the Rust `Words`, in the order the frames carry them. [tone]
 * is one of the TONE_ numbers and [who] one of the WHO_ numbers below. [at] is
 * in seconds since 1970, for the app to show on the person's own clock. The
 * app shows [heading], [what] and [next], and [code] and [detail] only under
 * "Details".
 */
data class Words(
    val tone: Int,
    val who: Int,
    val heading: String,
    val what: String,
    val next: String,
    val code: String?,
    val at: Long?,
    val quoted: Quoted?,
    val detail: String?,
)

// ---- tone and who numbers of the words (PROTOCOL.md) --------------------------------

private const val TONE_INFO = 0
private const val TONE_GOOD = 1
private const val TONE_STOP = 2
private const val TONE_ENDED = 3
private const val WHO_NOBODY = 0
private const val WHO_CUSTOMER = 1
private const val WHO_CASHIER = 2
private const val WHO_MANAGER = 3
private const val WHO_OWNER = 4
private const val WHO_KEY_HOLDER = 5
private const val WHO_BOX_KEEPER = 6

// ---- op numbers (PROTOCOL.md; never reused) -------------------------------------

private const val OP_START = 1
private const val OP_STOP = 2
private const val OP_PUMP = 10
private const val OP_OPEN = 11
private const val OP_JUDGE = 12
private const val OP_REQUOTE = 13
private const val OP_CANCEL = 14
private const val OP_OVERRIDE_SALE = 15
private const val OP_REQUEST_REFUND = 16
private const val OP_WITHDRAW_REFUND = 17
private const val OP_REFUND_IN_CASH = 18
private const val OP_REFUNDS = 19
private const val OP_HEARTBEAT = 20
private const val OP_COMMAND = 21
private const val OP_CHECK_IN = 22
private const val OP_ADD_BATCH = 23
private const val OP_STATUS = 30
private const val OP_NOTES = 31

// ---- tag bytes of the enums (PROTOCOL.md) ------------------------------------------

private const val TAG_COUNTER_NETWORK_MAINNET = 0
private const val TAG_COUNTER_NETWORK_TESTNET = 1
private const val TAG_COUNTER_NETWORK_SIGNET = 2
private const val TAG_COUNTER_NETWORK_REGTEST = 3
private const val TAG_ACTOR_ROLE_CASHIER = 0
private const val TAG_ACTOR_ROLE_MANAGER = 1
private const val TAG_REFUND_ASK_VIEW_SATS = 0
private const val TAG_REFUND_ASK_VIEW_VALUE = 1
private const val TAG_STATE_VIEW_NOT_PAID_YET = 0
private const val TAG_STATE_VIEW_PART_PAID = 1
private const val TAG_STATE_VIEW_WAITING = 2
private const val TAG_STATE_VIEW_PAID_INSTANT = 3
private const val TAG_STATE_VIEW_CONFIRMED = 4
private const val TAG_STATE_VIEW_FINAL = 5
private const val TAG_STATE_VIEW_OVERRIDDEN = 6
private const val TAG_STATE_VIEW_DO_NOT_HAND_OVER = 7
private const val TAG_STATE_VIEW_EXPIRED = 8
private const val TAG_STATE_VIEW_CANCELLED = 9
private const val TAG_STATE_VIEW_NEEDS_A_PERSON = 10
private const val TAG_REFUND_STATE_VIEW_REQUESTED = 0
private const val TAG_REFUND_STATE_VIEW_WAITING_FOR_DEPTH = 1
private const val TAG_REFUND_STATE_VIEW_WITHDRAWN = 2
private const val TAG_REFUND_STATE_VIEW_PAID_IN_CASH = 3
private const val TAG_REPORT_VIEW_POS_PAID = 0
private const val TAG_REPORT_VIEW_POS_UNPAID = 1
private const val TAG_REPORT_VIEW_BOOKS_FINAL = 2
private const val TAG_REPORT_VIEW_SURPLUS_OWED = 3
private const val TAG_REPORT_VIEW_LATE_PAYMENT = 4
private const val TAG_REPORT_VIEW_ALARM = 5
private const val TAG_REPORT_VIEW_DUST = 6

// ---- the views ----------------------------------------------------------------------
//
// A Rust u32 is a Long here and a u64 is a Long as well: a u64 above
// 2^63 - 1 reads as negative (no value of this API gets near it).

enum class CounterNetwork { Mainnet, Testnet, Signet, Regtest }

/** Who acts at the counter. The app has checked the person's PIN. */
enum class ActorRole { Cashier, Manager }

class Actor(val role: ActorRole, val name: String)

/**
 * What [CounterNode.start] is given. The identity secret, the two roots, the
 * wallet check, the signed settings and the first batch are what the counter
 * was paired with; the app keeps them and hands them over at each start.
 * [dir] is the app's own folder (`noBackupFilesDir`).
 */
class StartConfig(
    val dir: String,
    val network: CounterNetwork,
    val now: Long,
    val identitySecret: ByteArray,
    val ownerRoot: ByteArray,
    val approverRoot: ByteArray,
    val walletChecksum: ByteArray,
    val batchMinCount: Long,
    val settings: ByteArray,
    val firstBatch: ByteArray?,
    val proxy: String?,
    val fixedPeers: List<String>,
)

/** How a refund is asked for. */
sealed class RefundAsk {
    class Sats(val sats: Long) : RefundAsk()
    class Value(
        val minor: Long,
        val rate: Long,
        val sourceLabel: String,
        val sourceFetchedAt: Long,
    ) : RefundAsk()
}

/**
 * What the customer scans for one sale. [qrBits] holds [qrSize] times
 * [qrSize] modules, dark as 1, in one stream most significant bit first, row
 * by row; [module] reads one. [uri] is the text the code holds; [copyUri] is
 * the same request in lower case, for "copy".
 */
class Payment(
    val address: String,
    val uri: String,
    val copyUri: String,
    val qrSize: Long,
    val qrBits: ByteArray,
) {
    fun module(x: Int, y: Int): Boolean {
        val n = y.toLong() * qrSize + x
        return (qrBits[(n / 8).toInt()].toInt() and (0x80 shr (n % 8).toInt())) != 0
    }
}

class Opened(
    val index: Long,
    val needsSats: Long,
    val quoteRate: Long,
    val quoteFrom: Long,
    val quoteUntil: Long,
    val payment: Payment,
)

enum class SaleState {
    NotPaidYet, PartPaid, Waiting, PaidInstant, Confirmed, Final, Overridden,
    DoNotHandOver, Expired, Cancelled, NeedsAPerson,
}

enum class RefundState { Requested, WaitingForDepth, Withdrawn, PaidInCash }

/** Something to tell the POS, the books or the owner, once. */
sealed class Report {
    data class PosPaid(val index: Long) : Report()
    data class PosUnpaid(val index: Long) : Report()
    data class BooksFinal(
        val index: Long,
        val amountMinor: Long,
        val currency: String,
        val receivedSats: Long,
    ) : Report()
    data class SurplusOwed(val index: Long, val sats: Long) : Report()
    data class LatePayment(val index: Long, val sats: Long) : Report()
    data class Alarm(val index: Long) : Report()
    data class Dust(val index: Long, val sats: Long) : Report()
}

/** The screen of a sale: the state, the words for it and what leaves the counter. */
class Screen(
    val state: SaleState,
    val words: Words,
    val flags: List<Words>,
    val receivedSats: Long,
    val needsSats: Long,
    val stuck: Boolean,
    val late: Boolean,
    val reopened: Boolean,
    val conflictSeen: Boolean,
    val reports: List<Report>,
    val payment: Payment?,
)

class Pump(val reports: List<Report>, val notes: List<String>, val stopped: Boolean)

class RefundRow(
    val sale: Long,
    val number: Long?,
    val askedSats: Long,
    val valueMinor: Long?,
    val state: RefundState,
    val byRole: ActorRole,
    val byName: String,
    val at: Long,
    val words: Words,
)

class HeartbeatOutcome(
    val verified: Boolean,
    val sentAt: Long?,
    val words: Words,
    val reports: List<Report>,
)

class Health(val healthy: Boolean, val behind: Boolean, val words: Words)

class Status(
    val checkedHeight: Long,
    val tipHeight: Long,
    val health: Health?,
    val sales: List<Long>,
    val refusedBelowMinimum: Long,
    val stopped: Boolean,
)

// ---- the encoding ----------------------------------------------------------------------

/** Writes the fields of a request: integers little-endian, lengths as u32. */
internal class Writer {
    private val out = ByteArrayOutputStream()

    fun raw(bytes: ByteArray): Writer {
        out.write(bytes, 0, bytes.size)
        return this
    }

    fun u8(v: Int): Writer {
        out.write(v and 0xff)
        return this
    }

    fun u16(v: Int): Writer = u8(v).u8(v shr 8)

    fun u32(v: Long): Writer {
        for (i in 0 until 4) u8((v shr (8 * i)).toInt())
        return this
    }

    fun u64(v: Long): Writer {
        for (i in 0 until 8) u8((v shr (8 * i)).toInt())
        return this
    }

    fun bytes(v: ByteArray): Writer = u32(v.size.toLong()).raw(v)

    fun string(v: String): Writer = bytes(v.toByteArray(Charsets.UTF_8))

    fun <T> option(v: T?, put: (T) -> Unit): Writer {
        if (v == null) {
            u8(0)
        } else {
            u8(1)
            put(v)
        }
        return this
    }

    fun <T> list(v: List<T>, put: (T) -> Unit): Writer {
        u32(v.size.toLong())
        for (x in v) put(x)
        return this
    }

    fun toByteArray(): ByteArray = out.toByteArray()
}

/** Reads a response strictly: a short buffer, a flag or tag that is not defined,
 *  a length past the end and bytes left over are all errors. */
internal class Reader(private val data: ByteArray) {
    private var pos = 0

    private fun bad(what: String): Nothing =
        throw CounterException("response-malformed", "the library's response is not readable: $what", 4)

    fun remaining(): Int = data.size - pos

    fun take(n: Int): ByteArray {
        if (n < 0 || n > remaining()) bad("it ends early")
        val out = data.copyOfRange(pos, pos + n)
        pos += n
        return out
    }

    fun u8(): Int = take(1)[0].toInt() and 0xff

    fun u16(): Int = u8() or (u8() shl 8)

    fun u32(): Long {
        var v = 0L
        for (i in 0 until 4) v = v or (u8().toLong() shl (8 * i))
        return v
    }

    fun u64(): Long {
        var v = 0L
        for (i in 0 until 8) v = v or (u8().toLong() shl (8 * i))
        return v
    }

    fun bool(): Boolean =
        when (u8()) {
            0 -> false
            1 -> true
            else -> bad("a bool is not 0 or 1")
        }

    /** A length or a count: never more than the bytes that are left. */
    fun count(): Int {
        val n = u32()
        if (n > remaining()) bad("a length is past the end")
        return n.toInt()
    }

    fun bytes(): ByteArray = take(count())

    private fun utf8(bytes: ByteArray): String = try {
        Charsets.UTF_8.newDecoder()
            .onMalformedInput(CodingErrorAction.REPORT)
            .onUnmappableCharacter(CodingErrorAction.REPORT)
            .decode(ByteBuffer.wrap(bytes))
            .toString()
    } catch (_: CharacterCodingException) {
        bad("a string is not valid UTF-8")
    }

    fun string(): String = utf8(bytes())

    /** A string with a u16 length, as in the words. */
    fun string16(): String = utf8(take(u16()))

    /** A response status, including success, as defined by OBMn. */
    fun status(): Int = u8().also { if (it !in 0..4) bad("a status is not 0 to 4") }

    /** The words: after the rule and the text of an error frame, and in a result. */
    fun words(): Words {
        val tone = u8().also { if (it !in 0..3) bad("a tone is not 0 to 3") }
        val who = u8().also { if (it !in 0..6) bad("a person is not 0 to 6") }
        val heading = string16()
        val what = string16()
        val next = string16()
        val code = option { string16() }
        val at = option { u64() }
        val quoted = option { Quoted(string16(), string16()) }
        val detail = option { string16() }
        return Words(tone, who, heading, what, next, code, at, quoted, detail)
    }

    fun <T> option(read: () -> T): T? =
        when (u8()) {
            0 -> null
            1 -> read()
            else -> bad("an option flag is not 0 or 1")
        }

    fun <T> list(read: () -> T): List<T> {
        val n = count()
        val out = ArrayList<T>(minOf(n, 1024))
        for (i in 0 until n) out.add(read())
        return out
    }

    fun strings(): List<String> = list { string() }

    fun u32s(): List<Long> = list { u32() }

    fun end() {
        if (remaining() != 0) bad("bytes are left over")
    }

    fun tag(what: String): Nothing = bad("a tag of $what is not defined")

    fun fail(what: String): Nothing = bad(what)
}

private fun Writer.network(v: CounterNetwork): Writer =
    u8(
        when (v) {
            CounterNetwork.Mainnet -> TAG_COUNTER_NETWORK_MAINNET
            CounterNetwork.Testnet -> TAG_COUNTER_NETWORK_TESTNET
            CounterNetwork.Signet -> TAG_COUNTER_NETWORK_SIGNET
            CounterNetwork.Regtest -> TAG_COUNTER_NETWORK_REGTEST
        },
    )

private fun Writer.actor(v: Actor): Writer =
    u8(
        when (v.role) {
            ActorRole.Cashier -> TAG_ACTOR_ROLE_CASHIER
            ActorRole.Manager -> TAG_ACTOR_ROLE_MANAGER
        },
    ).string(v.name)

private fun Writer.refundAsk(v: RefundAsk): Writer {
    when (v) {
        is RefundAsk.Sats -> u8(TAG_REFUND_ASK_VIEW_SATS).u64(v.sats)
        is RefundAsk.Value ->
            u8(TAG_REFUND_ASK_VIEW_VALUE).u64(v.minor).u64(v.rate).string(v.sourceLabel)
                .u64(v.sourceFetchedAt)
    }
    return this
}

private fun Writer.start(c: StartConfig): Writer =
    string(c.dir).network(c.network).u64(c.now).bytes(c.identitySecret).bytes(c.ownerRoot)
        .bytes(c.approverRoot).bytes(c.walletChecksum).u32(c.batchMinCount).bytes(c.settings)
        .option(c.firstBatch) { bytes(it) }.option(c.proxy) { string(it) }
        .list(c.fixedPeers) { string(it) }

private fun Reader.saleState(): SaleState =
    when (u8()) {
        TAG_STATE_VIEW_NOT_PAID_YET -> SaleState.NotPaidYet
        TAG_STATE_VIEW_PART_PAID -> SaleState.PartPaid
        TAG_STATE_VIEW_WAITING -> SaleState.Waiting
        TAG_STATE_VIEW_PAID_INSTANT -> SaleState.PaidInstant
        TAG_STATE_VIEW_CONFIRMED -> SaleState.Confirmed
        TAG_STATE_VIEW_FINAL -> SaleState.Final
        TAG_STATE_VIEW_OVERRIDDEN -> SaleState.Overridden
        TAG_STATE_VIEW_DO_NOT_HAND_OVER -> SaleState.DoNotHandOver
        TAG_STATE_VIEW_EXPIRED -> SaleState.Expired
        TAG_STATE_VIEW_CANCELLED -> SaleState.Cancelled
        TAG_STATE_VIEW_NEEDS_A_PERSON -> SaleState.NeedsAPerson
        else -> tag("StateView")
    }

private fun Reader.refundState(): RefundState =
    when (u8()) {
        TAG_REFUND_STATE_VIEW_REQUESTED -> RefundState.Requested
        TAG_REFUND_STATE_VIEW_WAITING_FOR_DEPTH -> RefundState.WaitingForDepth
        TAG_REFUND_STATE_VIEW_WITHDRAWN -> RefundState.Withdrawn
        TAG_REFUND_STATE_VIEW_PAID_IN_CASH -> RefundState.PaidInCash
        else -> tag("RefundStateView")
    }

private fun Reader.role(): ActorRole =
    when (u8()) {
        TAG_ACTOR_ROLE_CASHIER -> ActorRole.Cashier
        TAG_ACTOR_ROLE_MANAGER -> ActorRole.Manager
        else -> tag("ActorRole")
    }

private fun Reader.report(): Report =
    when (u8()) {
        TAG_REPORT_VIEW_POS_PAID -> Report.PosPaid(u32())
        TAG_REPORT_VIEW_POS_UNPAID -> Report.PosUnpaid(u32())
        TAG_REPORT_VIEW_BOOKS_FINAL -> Report.BooksFinal(u32(), u64(), string(), u64())
        TAG_REPORT_VIEW_SURPLUS_OWED -> Report.SurplusOwed(u32(), u64())
        TAG_REPORT_VIEW_LATE_PAYMENT -> Report.LatePayment(u32(), u64())
        TAG_REPORT_VIEW_ALARM -> Report.Alarm(u32())
        TAG_REPORT_VIEW_DUST -> Report.Dust(u32(), u64())
        else -> tag("ReportView")
    }

private fun Reader.reports(): List<Report> = list { report() }

private fun Reader.payment(): Payment = Payment(string(), string(), string(), u32(), bytes()).also {
    if (it.qrBits.size.toLong() != (it.qrSize * it.qrSize + 7) / 8) {
        fail("the modules of a payment are not qrSize squared bits")
    }
}

internal fun Reader.opened(): Opened =
    Opened(u32(), u64(), u64(), u64(), u64(), payment())

internal fun Reader.screen(): Screen =
    Screen(
        saleState(), words(), list { words() }, u64(), u64(), bool(), bool(), bool(), bool(),
        reports(), option { payment() },
    )

internal fun Reader.pump(): Pump = Pump(reports(), strings(), bool())

private fun Reader.refundRow(): RefundRow =
    RefundRow(
        u32(), option { u64() }, u64(), option { u64() }, refundState(), role(), string(), u64(),
        words(),
    )

private fun Reader.heartbeatOutcome(): HeartbeatOutcome =
    HeartbeatOutcome(bool(), option { u64() }, words(), reports())

private fun Reader.statusView(): Status =
    Status(
        u32(), u32(), option { Health(bool(), bool(), words()) }, u32s(), u64(), bool(),
    )

// ---- the call ----------------------------------------------------------------------------

private val REQUEST_MAGIC = byteArrayOf('O'.code.toByte(), 'B'.code.toByte(), 'M'.code.toByte(), 'm'.code.toByte())
private val RESPONSE_MAGIC = byteArrayOf('O'.code.toByte(), 'B'.code.toByte(), 'M'.code.toByte(), 'n'.code.toByte())

/** The request frame of one op: the header, then the fields [fill] writes. */
internal fun requestFrame(op: Int, handle: Long, fill: (Writer) -> Unit = {}): ByteArray {
    val w = Writer().raw(REQUEST_MAGIC).u8(1).u16(op).u64(handle)
    fill(w)
    return w.toByteArray()
}

/** The request frame of `start`. */
internal fun startFrame(config: StartConfig): ByteArray =
    requestFrame(OP_START, 0) { it.start(config) }

/**
 * Check a response frame and return a reader at the start of its result, or
 * throw the [CounterException] it carries.
 */
internal fun decodeResponse(response: ByteArray): Reader {
    val r = Reader(response)
    if (!r.take(4).contentEquals(RESPONSE_MAGIC)) {
        throw CounterException("response-magic", "the library's response is not an OBMn frame", 4)
    }
    if (r.u8() != 1) {
        throw CounterException("response-version", "the library's response has a version that is not 1", 4)
    }
    val status = r.status()
    if (status == 0) return r
    val rule = r.string16()
    val text = r.string16()
    val words = r.words()
    r.end()
    throw CounterException(rule, text, status, words)
}

// No lock is taken here: the library is called from the pump thread and the
// screen thread at the same time, and takes care of that itself.
private fun call(op: Int, handle: Long, fill: (Writer) -> Unit = {}): Reader {
    val response = Native.call(requestFrame(op, handle, fill))
        ?: throw CounterException("response-null", "the library gave no response", 4)
    return decodeResponse(response)
}

/** A call whose result is nothing. */
private fun callUnit(op: Int, handle: Long, fill: (Writer) -> Unit = {}) {
    call(op, handle, fill).end()
}

/**
 * A running counter node. [start] makes one; every call is one op of
 * PROTOCOL.md. Call [pump] from one background thread in a loop (it waits for
 * at most 250 ms) and every other method from the screen thread: they may run
 * at the same time. Every time is in seconds since 1970 and is the app's clock.
 */
class CounterNode private constructor(private val handle: Long) : AutoCloseable {
    companion object {
        /** Start the node on the folder in [config]. */
        fun start(config: StartConfig): CounterNode {
            val r = call(OP_START, 0) { it.start(config) }
            val handle = r.u64()
            r.end()
            return CounterNode(handle)
        }
    }

    /** Stop the node and let go of its folder; what the last events reported. */
    fun stop(): Pump {
        val r = call(OP_STOP, handle)
        return r.pump().also { r.end() }
    }

    /** Stops the node, and keeps what it reported to nobody. */
    override fun close() {
        try {
            stop()
        } catch (_: CounterException) {
            // Already stopped, or never running.
        }
    }

    /** Take the network's events for up to [waitMs] (at most 250) and return what must leave. */
    fun pump(now: Long, waitMs: Long): Pump {
        val r = call(OP_PUMP, handle) { it.u64(now).u32(waitMs) }
        return r.pump().also { r.end() }
    }

    /** Open a sale; the payment is what the customer scans. */
    fun open(
        now: Long,
        amountMinor: Long,
        currency: String,
        rate: Long,
        sourceLabel: String,
        sourceFetchedAt: Long,
    ): Opened {
        val r =
            call(OP_OPEN, handle) {
                it.u64(now).u64(amountMinor).string(currency).u64(rate).string(sourceLabel)
                    .u64(sourceFetchedAt)
            }
        return r.opened().also { r.end() }
    }

    /** Work out the state of a sale now. */
    fun judge(now: Long, index: Long): Screen {
        val r = call(OP_JUDGE, handle) { it.u64(now).u32(index) }
        return r.screen().also { r.end() }
    }

    /** Give a sale a new quote. */
    fun requote(now: Long, index: Long, rate: Long, sourceLabel: String, sourceFetchedAt: Long): Opened {
        val r =
            call(OP_REQUOTE, handle) {
                it.u64(now).u32(index).u64(rate).string(sourceLabel).u64(sourceFetchedAt)
            }
        return r.opened().also { r.end() }
    }

    /** The cashier cancels a sale with nothing received. */
    fun cancel(now: Long, index: Long) = callUnit(OP_CANCEL, handle) { it.u64(now).u32(index) }

    /** A person accepts a sale; the app has checked the PIN. */
    fun overrideSale(now: Long, index: Long, actor: Actor): List<Report> {
        val r = call(OP_OVERRIDE_SALE, handle) { it.u64(now).u32(index).actor(actor) }
        return r.reports().also { r.end() }
    }

    /** Ask for a refund; returns the request number. */
    fun requestRefund(
        now: Long,
        index: Long,
        ask: RefundAsk,
        destination: String,
        actor: Actor,
        reason: String,
    ): Long {
        val r =
            call(OP_REQUEST_REFUND, handle) {
                it.u64(now).u32(index).refundAsk(ask).string(destination).actor(actor).string(reason)
            }
        return r.u64().also { r.end() }
    }

    fun withdrawRefund(now: Long, index: Long, number: Long, actor: Actor) =
        callUnit(OP_WITHDRAW_REFUND, handle) { it.u64(now).u32(index).u64(number).actor(actor) }

    /** A refund paid in cash from the till, [minor] in the shop's currency. */
    fun refundInCash(now: Long, index: Long, actor: Actor, minor: Long, reason: String) =
        callUnit(OP_REFUND_IN_CASH, handle) {
            it.u64(now).u32(index).actor(actor).u64(minor).string(reason)
        }

    /** The refunds of a sale, with the words for each. */
    fun refunds(index: Long): List<RefundRow> {
        val r = call(OP_REFUNDS, handle) { it.u32(index) }
        return r.list { r.refundRow() }.also { r.end() }
    }

    /** Take the bytes of a heartbeat the owner's watcher signed. */
    fun heartbeat(now: Long, bytes: ByteArray): HeartbeatOutcome {
        val r = call(OP_HEARTBEAT, handle) { it.u64(now).bytes(bytes) }
        return r.heartbeatOutcome().also { r.end() }
    }

    /** Take the bytes of a command the owner signed. */
    fun command(now: Long, bytes: ByteArray) =
        callUnit(OP_COMMAND, handle) { it.u64(now).bytes(bytes) }

    /** Append a check-in record; returns its bytes, for the owner. */
    fun checkIn(now: Long): ByteArray {
        val r = call(OP_CHECK_IN, handle) { it.u64(now) }
        return r.bytes().also { r.end() }
    }

    /** Take a new signed batch of addresses. */
    fun addBatch(now: Long, bytes: ByteArray) =
        callUnit(OP_ADD_BATCH, handle) { it.u64(now).bytes(bytes) }

    fun status(): Status {
        val r = call(OP_STATUS, handle)
        return r.statusView().also { r.end() }
    }

    /** The lines the node and the driver logged since the last call. */
    fun notes(): List<String> {
        val r = call(OP_NOTES, handle)
        return r.strings().also { r.end() }
    }
}
