package org.openbmt.owner

import java.io.ByteArrayOutputStream
import java.nio.ByteBuffer
import java.nio.charset.CharacterCodingException
import java.nio.charset.CodingErrorAction

// The binding and OBMl decoder harness compile and run on the JVM. This does
// not compile or load the Android JNI library or exercise a device UI.

/** A call that did not do what was asked. [rule] is the ID of the rule that refused. */
class OwnerException(
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
private const val OP_LINK_STEP = 11
private const val OP_DELIVER = 12
private const val OP_STATUS = 20
private const val OP_RECEIPTS_LINE = 21
private const val OP_MERGE_OFFER = 22
private const val OP_SEND_STATES = 23
private const val OP_OLD_ADDRESSES = 24
private const val OP_REQUEST_DEEP_CHECK = 25
private const val OP_SETTINGS = 26
private const val OP_OUTBOX = 27
private const val OP_BUILD_REQUEST = 30
private const val OP_APPROVE_MERGE = 31
private const val OP_TAKE = 32
private const val OP_SESSIONS = 33
private const val OP_SESSION_REQUEST = 34
private const val OP_ABANDON_SESSION = 35
private const val OP_QUEUE = 40
private const val OP_QUEUE_REFUND = 41
private const val OP_CANCEL = 42
private const val OP_REQUEUE = 43
private const val OP_CASH_OUT = 44
private const val OP_SIGN_BATCH = 45
private const val OP_QUEUE_SETTINGS = 46
private const val OP_LINK_STATE = 50
private const val OP_CLEAR_ALERT = 51
private const val OP_REFUND_NOTES = 52
private const val OP_DISMISS_REFUND_NOTE = 53
private const val OP_NOTES = 60
private const val OP_RESCAN = 61
private const val OP_RECOVER_KEY1 = 62
private const val OP_ARCHIVE_PAID = 63

// ---- tag bytes of the enums (PROTOCOL.md) ------------------------------------------

private const val TAG_OWNER_NETWORK_MAINNET = 0
private const val TAG_OWNER_NETWORK_TESTNET = 1
private const val TAG_OWNER_NETWORK_SIGNET = 2
private const val TAG_OWNER_NETWORK_REGTEST = 3
private const val TAG_ROUTE_VIEW_DIRECT = 0
private const val TAG_ROUTE_VIEW_ONION = 1
private const val TAG_CASH_AMOUNT_VIEW_SATS = 0
private const val TAG_CASH_AMOUNT_VIEW_ALL = 1
private const val TAG_ALERT_KIND_MISSED_CHECK_IN = 0
private const val TAG_ALERT_KIND_DIVERGED = 1
private const val TAG_VIEW_KIND_READY = 0
private const val TAG_VIEW_KIND_RESCANNING = 1
private const val TAG_VIEW_KIND_DEEP_REORG = 2
private const val TAG_RECEIPTS_KIND_NOT_READY = 0
private const val TAG_RECEIPTS_KIND_NO_COINS = 1
private const val TAG_RECEIPTS_KIND_FEE_UNKNOWN = 2
private const val TAG_RECEIPTS_KIND_KNOWN = 3
private const val TAG_SEND_KIND_WAITING = 0
private const val TAG_SEND_KIND_SENT = 1
private const val TAG_SEND_KIND_SEEN_BACK = 2
private const val TAG_SEND_KIND_NOT_SEEN = 3
private const val TAG_SEND_KIND_CONFIRMED = 4
private const val TAG_SEND_KIND_DONE = 5
private const val TAG_SEND_KIND_REPLACED = 6
private const val TAG_SEND_KIND_UNRESOLVED = 7
private const val TAG_OUTCOME_KIND_DELIVERED = 0
private const val TAG_OUTCOME_KIND_RETURNED = 1
private const val TAG_OUTCOME_KIND_HEALTHY = 2
private const val TAG_STAGE_KIND_QUEUED = 0
private const val TAG_STAGE_KIND_IN_SESSION = 1
private const val TAG_STAGE_KIND_RETURNED = 2
private const val TAG_STAGE_KIND_SIGNED = 3
private const val TAG_STAGE_KIND_SENT = 4
private const val TAG_STAGE_KIND_PAID = 5
private const val TAG_ENTRY_KIND_VIEW_BATCH = 0
private const val TAG_ENTRY_KIND_VIEW_SETTINGS_CHANGE = 1
private const val TAG_ENTRY_KIND_VIEW_PAIRING = 2
private const val TAG_NOTE_KIND_VIEW_REFUSED = 0
private const val TAG_NOTE_KIND_VIEW_WITHDRAWAL_NOT_APPLIED = 1
private const val TAG_NOTE_KIND_VIEW_INTERRUPTED = 2
private const val TAG_NOTE_KIND_VIEW_CLAIMED_MORE_THAN_SEEN = 3
private const val TAG_OLD_KIND_NONE = 0
private const val TAG_OLD_KIND_FIRST_CHECK_WAITING = 1
private const val TAG_OLD_KIND_MONTHLY = 2
private const val TAG_OLD_KIND_CHECKING = 3
private const val TAG_OLD_KIND_FAILED = 4
private const val TAG_OUTBOX_KIND_TRANSACTION = 0
private const val TAG_OUTBOX_KIND_BATCH = 1
private const val TAG_OUTBOX_KIND_SETTINGS = 2
private const val TAG_OUTBOX_KIND_PAIRING = 3

// ---- the views ----------------------------------------------------------------------
//
// A Rust u32 is a Long here and a u64 is a Long as well: a u64 above
// 2^63 - 1 reads as negative (no value of this API gets near it).

enum class OwnerNetwork { Mainnet, Testnet, Signet, Regtest }

/** How the link reaches the relay. */
sealed class Route {
    /** An address and port on the local network, `host:port`. */
    class Direct(val addr: String) : Route()

    /** A Tor onion service by name, through a SOCKS5 proxy `host:port`. */
    class Onion(val proxy: String, val host: String, val port: Long) : Route()
}

/** The link to the counters through a relay. The keys are 32 bytes each. */
class LinkConfig(
    val route: Route,
    val relayKey: ByteArray,
    val counters: List<ByteArray>,
    val heartbeatEverySecs: Long,
    val checkInGraceSecs: Long,
)

/**
 * What [OwnerNode.start] is given. The owner's secret, the Key app's root, the
 * wallet and Key 1's account key are what the shop was set up with; the app
 * keeps them and hands them over at each start. [dir] is the app's own folder
 * (`noBackupFilesDir`). [key1Account] is the 78 bytes of the BIP 32
 * serialization of the private account key.
 */
class StartConfig(
    val dir: String,
    val network: OwnerNetwork,
    val now: Long,
    val ownerSecret: ByteArray,
    val approverRoot: ByteArray,
    val wallet: String,
    val key1Account: ByteArray,
    val key3Xpub: String?,
    val batchMinCount: Long,
    val refundDepth: Long,
    val birthday: Long,
    val proxy: String?,
    val fixedPeers: List<String>,
    val broadcastProxy: String?,
    val withoutTor: Boolean,
    val link: LinkConfig?,
)

/** A refund to queue. The facts it is queued on come from the wallet's own view. */
class RefundArgs(
    val counter: ByteArray,
    val sale: Long,
    val cashier: String,
    val address: String,
    val sats: Long,
)

/** What a cash-out moves. */
sealed class CashAmount {
    class Sats(val sats: Long) : CashAmount()

    object All : CashAmount()
}

/** A cash-out. The address is checked by the library, never by the app. */
class CashOutArgs(val address: String, val amount: CashAmount, val label: String)

class BatchArgs(val counter: ByteArray, val start: Long, val count: Long)

enum class AlertKind { MissedCheckIn, Diverged }

/** An alert as [OwnerNode.linkState] showed it. */
class ClearArgs(val kind: AlertKind, val counter: ByteArray, val value: Long)

class Pump(val notes: List<String>, val stopped: Boolean)

class LinkStep(
    val notes: List<String>,
    val queued: Long,
    val refused: Long,
    val withdrawn: Long,
    val held: Long,
    val waiting: Words?,
    val stoppedEarly: String?,
    val reached: Boolean,
)

enum class ViewKind { Ready, Rescanning, DeepReorg }

class Halted(val text: String, val words: Words)

class Health(val known: Boolean, val healthy: Boolean, val behind: Boolean, val words: Words)

class Status(
    val view: ViewKind,
    val rescanTo: Long?,
    val viewWords: Words,
    val receiptsWords: Words,
    val checkedHeight: Long,
    val tipHeight: Long,
    val halted: Halted?,
    val health: Health,
    val balanceSats: Long,
    val recoveryPending: Boolean,
    val recoveryWords: Words?,
    val stopped: Boolean,
)

enum class ReceiptsKind { NotReady, NoCoins, FeeUnknown, Known }

class Receipts(
    val kind: ReceiptsKind,
    val payments: Long,
    val costSats: Long,
    val ratePerKvb: Long,
    val words: Words,
)

class MergeOffer(
    val coins: Long,
    val of: Long,
    val ratePerKvb: Long,
    val costSats: Long,
    val usualCostSats: Long,
    val savingSats: Long,
    val words: Words,
)

enum class SendKind { Waiting, Sent, SeenBack, NotSeen, Confirmed, Done, Replaced, Unresolved }

class SendRow(val txid: String, val state: SendKind, val n: Long, val words: Words)

class Skipped(val id: Long, val reason: String, val words: Words)

/** An approval session built for the Key app: [request] is what it takes. */
class Built(val sessionId: ByteArray, val request: ByteArray, val skipped: List<Skipped>)

enum class OutcomeKind { Delivered, Returned, Healthy }

class Outcome(
    val item: Long,
    val kind: OutcomeKind,
    val finished: Long,
    val reason: String?,
    val words: Words,
)

class Taken(val sessionId: ByteArray, val outcomes: List<Outcome>, val sends: List<SendRow>)

enum class StageKind { Queued, InSession, Returned, Signed, Sent, Paid }

class Stage(val kind: StageKind, val detail: String?, val words: Words)

class RefundRow(
    val id: Long,
    val counter: ByteArray,
    val sale: Long,
    val cashier: String,
    val address: String,
    val sats: Long,
    val stage: Stage,
    val waitingForDepth: Boolean?,
    val receivedSats: Long?,
)

class CashOutRow(
    val id: Long,
    val address: String,
    val amount: CashAmount,
    val label: String,
    val stage: Stage,
)

enum class EntryKind { Batch, SettingsChange, Pairing }

class EntryRow(val id: Long, val kind: EntryKind, val stage: Stage)

class Queue(val refunds: List<RefundRow>, val cashOuts: List<CashOutRow>, val others: List<EntryRow>)

class QueuedRefund(val id: Long, val receivedSats: Long, val reachedRefundDepth: Boolean)

class SettingsQueued(val tightening: Boolean, val id: Long, val signed: ByteArray?)

class Deliver(val sent: Long, val waiting: Long, val notes: List<String>)

class Alert(val kind: AlertKind, val counter: ByteArray, val value: Long, val words: Words)

class CounterState(
    val counter: ByteArray,
    val records: Long,
    val head: ByteArray,
    val checkInSeq: Long?,
    val checkInHeld: Boolean?,
    val lastCheckInAt: Long?,
    val words: Words,
)

class LinkState(
    val reachable: Boolean,
    val refused: Long,
    val alerts: List<Alert>,
    val counters: List<CounterState>,
)

enum class NoteKind { Refused, WithdrawalNotApplied, Interrupted, ClaimedMoreThanSeen }

class RefundNote(
    val index: Long,
    val counter: ByteArray,
    val seq: Long,
    val number: Long,
    val sale: Long,
    val kind: NoteKind,
    val withdrawn: Boolean,
    val words: Words,
)

enum class OldKind { None, FirstCheckWaiting, Monthly, Checking, Failed }

/** The live-watch line about the old addresses that are checked once a month. */
class OldAddresses(
    val kind: OldKind,
    val count: Long,
    val daysAgo: Long,
    val next: Long?,
    val running: Boolean,
    val words: Words?,
)

class DeepRequest(val asked: Boolean, val words: Words)

/** One limit of the shop's settings: what is enforced, in satoshis, and the whole dollars it was shown as. */
class Limit(val sats: Long, val displayUsd: Long)

/** A counter's share of every budget, in basis points (10,000 is the whole budget). */
class CounterShare(val counter: ByteArray, val bps: Long)

/** The shop's settings the owner phone holds as in force, decoded, for a read-only view. */
class SettingsHeld(
    val version: Long,
    val lightPerSale: Limit,
    val lightBudget: Limit,
    val nodePerSale: Limit,
    val nodeBudget: Limit,
    val boxPerSale: Limit,
    val boxBudget: Limit,
    val underpaymentToleranceBps: Long,
    val refundCap: Limit,
    val refundDailyCap: Limit,
    val cashierOverrideDailyCap: Limit,
    val managerOverrideCeiling: Limit,
    val minConfirmations: Int,
    val finalConfirmations: Int,
    val heartbeatMaxAgeSecs: Long,
    val torOn: Boolean,
    val customerWifi: Boolean,
    val minSaleSats: Long,
    val devices: List<ByteArray>,
    val counters: List<CounterShare>,
    val boxes: List<ByteArray>,
)

enum class OutboxKind { Transaction, Batch, Settings, Pairing }

/**
 * A signed object that waits to be delivered. [signedAt] is known for a batch
 * only (seconds since 1970); the node keeps no time for the others.
 */
class OutboxRow(
    val id: Long,
    val kind: OutboxKind,
    val txid: String?,
    val counter: ByteArray?,
    val version: Long?,
    val signedAt: Long?,
    val size: Long,
    val words: Words,
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

    fun bool(v: Boolean): Writer = u8(if (v) 1 else 0)

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
        throw OwnerException("response-malformed", "the library's response is not readable: $what", 4)

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

    /** A response status, including success, as defined by OBMl. */
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

    fun end() {
        if (remaining() != 0) bad("bytes are left over")
    }

    fun tag(what: String): Nothing = bad("a tag of $what is not defined")

    fun fail(what: String): Nothing = bad(what)
}

private fun Writer.network(v: OwnerNetwork): Writer =
    u8(
        when (v) {
            OwnerNetwork.Mainnet -> TAG_OWNER_NETWORK_MAINNET
            OwnerNetwork.Testnet -> TAG_OWNER_NETWORK_TESTNET
            OwnerNetwork.Signet -> TAG_OWNER_NETWORK_SIGNET
            OwnerNetwork.Regtest -> TAG_OWNER_NETWORK_REGTEST
        },
    )

private fun Writer.route(v: Route): Writer {
    when (v) {
        is Route.Direct -> u8(TAG_ROUTE_VIEW_DIRECT).string(v.addr)
        is Route.Onion -> u8(TAG_ROUTE_VIEW_ONION).string(v.proxy).string(v.host).u32(v.port)
    }
    return this
}

private fun Writer.link(v: LinkConfig): Writer =
    route(v.route).bytes(v.relayKey).list(v.counters) { bytes(it) }.u64(v.heartbeatEverySecs)
        .u64(v.checkInGraceSecs)

private fun Writer.start(c: StartConfig): Writer =
    string(c.dir).network(c.network).u64(c.now).bytes(c.ownerSecret).bytes(c.approverRoot)
        .string(c.wallet).bytes(c.key1Account).option(c.key3Xpub) { string(it) }
        .u32(c.batchMinCount).u32(c.refundDepth).u32(c.birthday).option(c.proxy) { string(it) }
        .list(c.fixedPeers) { string(it) }.option(c.broadcastProxy) { string(it) }
        .bool(c.withoutTor).option(c.link) { link(it) }

private fun Writer.cashAmount(v: CashAmount): Writer {
    when (v) {
        is CashAmount.Sats -> u8(TAG_CASH_AMOUNT_VIEW_SATS).u64(v.sats)
        is CashAmount.All -> u8(TAG_CASH_AMOUNT_VIEW_ALL)
    }
    return this
}

private fun Writer.alertKind(v: AlertKind): Writer =
    u8(
        when (v) {
            AlertKind.MissedCheckIn -> TAG_ALERT_KIND_MISSED_CHECK_IN
            AlertKind.Diverged -> TAG_ALERT_KIND_DIVERGED
        },
    )

private fun Reader.alertKind(): AlertKind =
    when (u8()) {
        TAG_ALERT_KIND_MISSED_CHECK_IN -> AlertKind.MissedCheckIn
        TAG_ALERT_KIND_DIVERGED -> AlertKind.Diverged
        else -> tag("AlertKind")
    }

private fun Reader.cashAmount(): CashAmount =
    when (u8()) {
        TAG_CASH_AMOUNT_VIEW_SATS -> CashAmount.Sats(u64())
        TAG_CASH_AMOUNT_VIEW_ALL -> CashAmount.All
        else -> tag("CashAmountView")
    }

private fun Reader.viewKind(): ViewKind =
    when (u8()) {
        TAG_VIEW_KIND_READY -> ViewKind.Ready
        TAG_VIEW_KIND_RESCANNING -> ViewKind.Rescanning
        TAG_VIEW_KIND_DEEP_REORG -> ViewKind.DeepReorg
        else -> tag("ViewKind")
    }

private fun Reader.receiptsKind(): ReceiptsKind =
    when (u8()) {
        TAG_RECEIPTS_KIND_NOT_READY -> ReceiptsKind.NotReady
        TAG_RECEIPTS_KIND_NO_COINS -> ReceiptsKind.NoCoins
        TAG_RECEIPTS_KIND_FEE_UNKNOWN -> ReceiptsKind.FeeUnknown
        TAG_RECEIPTS_KIND_KNOWN -> ReceiptsKind.Known
        else -> tag("ReceiptsKind")
    }

private fun Reader.sendKind(): SendKind =
    when (u8()) {
        TAG_SEND_KIND_WAITING -> SendKind.Waiting
        TAG_SEND_KIND_SENT -> SendKind.Sent
        TAG_SEND_KIND_SEEN_BACK -> SendKind.SeenBack
        TAG_SEND_KIND_NOT_SEEN -> SendKind.NotSeen
        TAG_SEND_KIND_CONFIRMED -> SendKind.Confirmed
        TAG_SEND_KIND_DONE -> SendKind.Done
        TAG_SEND_KIND_REPLACED -> SendKind.Replaced
        TAG_SEND_KIND_UNRESOLVED -> SendKind.Unresolved
        else -> tag("SendKind")
    }

private fun Reader.outcomeKind(): OutcomeKind =
    when (u8()) {
        TAG_OUTCOME_KIND_DELIVERED -> OutcomeKind.Delivered
        TAG_OUTCOME_KIND_RETURNED -> OutcomeKind.Returned
        TAG_OUTCOME_KIND_HEALTHY -> OutcomeKind.Healthy
        else -> tag("OutcomeKind")
    }

private fun Reader.stageKind(): StageKind =
    when (u8()) {
        TAG_STAGE_KIND_QUEUED -> StageKind.Queued
        TAG_STAGE_KIND_IN_SESSION -> StageKind.InSession
        TAG_STAGE_KIND_RETURNED -> StageKind.Returned
        TAG_STAGE_KIND_SIGNED -> StageKind.Signed
        TAG_STAGE_KIND_SENT -> StageKind.Sent
        TAG_STAGE_KIND_PAID -> StageKind.Paid
        else -> tag("StageKind")
    }

private fun Reader.entryKind(): EntryKind =
    when (u8()) {
        TAG_ENTRY_KIND_VIEW_BATCH -> EntryKind.Batch
        TAG_ENTRY_KIND_VIEW_SETTINGS_CHANGE -> EntryKind.SettingsChange
        TAG_ENTRY_KIND_VIEW_PAIRING -> EntryKind.Pairing
        else -> tag("EntryKindView")
    }

private fun Reader.noteKind(): NoteKind =
    when (u8()) {
        TAG_NOTE_KIND_VIEW_REFUSED -> NoteKind.Refused
        TAG_NOTE_KIND_VIEW_WITHDRAWAL_NOT_APPLIED -> NoteKind.WithdrawalNotApplied
        TAG_NOTE_KIND_VIEW_INTERRUPTED -> NoteKind.Interrupted
        TAG_NOTE_KIND_VIEW_CLAIMED_MORE_THAN_SEEN -> NoteKind.ClaimedMoreThanSeen
        else -> tag("NoteKindView")
    }

private fun Reader.oldKind(): OldKind =
    when (u8()) {
        TAG_OLD_KIND_NONE -> OldKind.None
        TAG_OLD_KIND_FIRST_CHECK_WAITING -> OldKind.FirstCheckWaiting
        TAG_OLD_KIND_MONTHLY -> OldKind.Monthly
        TAG_OLD_KIND_CHECKING -> OldKind.Checking
        TAG_OLD_KIND_FAILED -> OldKind.Failed
        else -> tag("OldKind")
    }

internal fun Reader.pump(): Pump = Pump(strings(), bool())

private fun Reader.linkStep(): LinkStep =
    LinkStep(
        strings(), u64(), u64(), u64(), u64(), option { words() }, option { string() }, bool(),
    )

private fun Reader.health(): Health = Health(bool(), bool(), bool(), words())

internal fun Reader.statusView(): Status =
    Status(
        viewKind(), option { u32() }, words(), words(), u32(), u32(),
        option { Halted(string(), words()) }, health(), u64(), bool(), option { words() }, bool(),
    )

private fun Reader.receipts(): Receipts =
    Receipts(receiptsKind(), u64(), u64(), u64(), words())

private fun Reader.mergeOffer(): MergeOffer =
    MergeOffer(u64(), u64(), u64(), u64(), u64(), u64(), words())

private fun Reader.sendRow(): SendRow = SendRow(string(), sendKind(), u32(), words())

private fun Reader.built(): Built =
    Built(bytes(), bytes(), list { Skipped(u32(), string(), words()) })

private fun Reader.outcome(): Outcome =
    Outcome(u32(), outcomeKind(), u32(), option { string() }, words())

internal fun Reader.taken(): Taken =
    Taken(bytes(), list { outcome() }, list { sendRow() })

private fun Reader.stage(): Stage = Stage(stageKind(), option { string() }, words())

private fun Reader.refundRow(): RefundRow =
    RefundRow(
        u32(), bytes(), u32(), string(), string(), u64(), stage(), option { bool() },
        option { u64() },
    )

private fun Reader.cashOutRow(): CashOutRow =
    CashOutRow(u32(), string(), cashAmount(), string(), stage())

internal fun Reader.queue(): Queue =
    Queue(
        list { refundRow() }, list { cashOutRow() }, list { EntryRow(u32(), entryKind(), stage()) },
    )

private fun Reader.queuedRefund(): QueuedRefund = QueuedRefund(u32(), u64(), bool())

private fun Reader.settingsQueued(): SettingsQueued =
    SettingsQueued(bool(), u32(), option { bytes() })

private fun Reader.deliver(): Deliver = Deliver(u32(), u32(), strings())

private fun Reader.alert(): Alert = Alert(alertKind(), bytes(), u64(), words())

private fun Reader.counterState(): CounterState =
    CounterState(
        bytes(), u64(), bytes(), option { u64() }, option { bool() }, option { u64() }, words(),
    )

internal fun Reader.linkState(): LinkState =
    LinkState(bool(), u64(), list { alert() }, list { counterState() })

private fun Reader.refundNote(): RefundNote =
    RefundNote(u32(), bytes(), u64(), u64(), u32(), noteKind(), bool(), words())

internal fun Reader.oldAddresses(): OldAddresses =
    OldAddresses(oldKind(), u64(), u64(), option { u64() }, bool(), option { words() })

private fun Reader.deepRequest(): DeepRequest = DeepRequest(bool(), words())

private fun Reader.outboxKind(): OutboxKind =
    when (u8()) {
        TAG_OUTBOX_KIND_TRANSACTION -> OutboxKind.Transaction
        TAG_OUTBOX_KIND_BATCH -> OutboxKind.Batch
        TAG_OUTBOX_KIND_SETTINGS -> OutboxKind.Settings
        TAG_OUTBOX_KIND_PAIRING -> OutboxKind.Pairing
        else -> tag("OutboxKind")
    }

private fun Reader.limit(): Limit = Limit(u64(), u32())

internal fun Reader.settingsHeld(): SettingsHeld =
    SettingsHeld(
        u32(), limit(), limit(), limit(), limit(), limit(), limit(), u32(),
        limit(), limit(), limit(), limit(), u8(), u8(), u32(), bool(), bool(), u64(),
        list { bytes() }, list { CounterShare(bytes(), u32()) }, list { bytes() },
    )

internal fun Reader.outboxRow(): OutboxRow =
    OutboxRow(
        u32(), outboxKind(), option { string() }, option { bytes() }, option { u32() },
        option { u64() }, u32(), words(),
    )

// ---- the call ----------------------------------------------------------------------------

private val REQUEST_MAGIC = byteArrayOf('O'.code.toByte(), 'B'.code.toByte(), 'M'.code.toByte(), 'k'.code.toByte())
private val RESPONSE_MAGIC = byteArrayOf('O'.code.toByte(), 'B'.code.toByte(), 'M'.code.toByte(), 'l'.code.toByte())

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
 * throw the [OwnerException] it carries.
 */
internal fun decodeResponse(response: ByteArray): Reader {
    val r = Reader(response)
    if (!r.take(4).contentEquals(RESPONSE_MAGIC)) {
        throw OwnerException("response-magic", "the library's response is not an OBMl frame", 4)
    }
    if (r.u8() != 1) {
        throw OwnerException("response-version", "the library's response has a version that is not 1", 4)
    }
    val status = r.status()
    if (status == 0) return r
    val rule = r.string16()
    val text = r.string16()
    val words = r.words()
    r.end()
    throw OwnerException(rule, text, status, words)
}

// No lock is taken here: the library is called from the pump thread and the
// screen thread at the same time, and takes care of that itself.
private fun call(op: Int, handle: Long, fill: (Writer) -> Unit = {}): Reader {
    val response = Native.call(requestFrame(op, handle, fill))
        ?: throw OwnerException("response-null", "the library gave no response", 4)
    return decodeResponse(response)
}

/** A call whose result is nothing. */
private fun callUnit(op: Int, handle: Long, fill: (Writer) -> Unit = {}) {
    call(op, handle, fill).end()
}

/**
 * A running owner node. [start] makes one; every call is one op of
 * PROTOCOL.md. Call [pump] from one background thread in a loop (it waits for
 * at most 250 ms) and every other method from the screen thread: they may run
 * at the same time. Every time is in seconds since 1970 and is the app's clock.
 */
class OwnerNode private constructor(private val handle: Long) : AutoCloseable {
    companion object {
        /** Start the node on the folder in [config]. */
        fun start(config: StartConfig): OwnerNode {
            val r = call(OP_START, 0) { it.start(config) }
            val handle = r.u64()
            r.end()
            return OwnerNode(handle)
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
        } catch (_: OwnerException) {
            // Already stopped, or never running.
        }
    }

    /** Take the network's events for up to [waitMs] (at most 250) and return the lines they logged. */
    fun pump(now: Long, waitMs: Long): Pump {
        val r = call(OP_PUMP, handle) { it.u64(now).u32(waitMs) }
        return r.pump().also { r.end() }
    }

    /** One turn of the link, and the refund requests it brought. */
    fun linkStep(now: Long): LinkStep {
        val r = call(OP_LINK_STEP, handle) { it.u64(now) }
        return r.linkStep().also { r.end() }
    }

    /** Hand the signed batches in the outbox to the link. */
    fun deliver(now: Long): Deliver {
        val r = call(OP_DELIVER, handle) { it.u64(now) }
        return r.deliver().also { r.end() }
    }

    fun status(): Status {
        val r = call(OP_STATUS, handle)
        return r.statusView().also { r.end() }
    }

    /** The line about the payments received and what spending them costs. */
    fun receiptsLine(): Receipts {
        val r = call(OP_RECEIPTS_LINE, handle)
        return r.receipts().also { r.end() }
    }

    /** The offer to merge the wallet's coins, when there is one. */
    fun mergeOffer(): MergeOffer? {
        val r = call(OP_MERGE_OFFER, handle)
        return r.option { r.mergeOffer() }.also { r.end() }
    }

    /** Where each finished payment is. */
    fun sendStates(): List<SendRow> {
        val r = call(OP_SEND_STATES, handle)
        return r.list { r.sendRow() }.also { r.end() }
    }

    /** The live-watch line about the old addresses; its words are null when none is set aside. */
    fun oldAddresses(): OldAddresses {
        val r = call(OP_OLD_ADDRESSES, handle)
        return r.oldAddresses().also { r.end() }
    }

    /** Ask for a deep check of the old addresses now. */
    fun requestDeepCheck(now: Long): DeepRequest {
        val r = call(OP_REQUEST_DEEP_CHECK, handle) { it.u64(now) }
        return r.deepRequest().also { r.end() }
    }

    /** The shop's settings the node holds as in force, or null when it holds none yet. */
    fun settings(): SettingsHeld? {
        val r = call(OP_SETTINGS, handle)
        return r.option { r.settingsHeld() }.also { r.end() }
    }

    /** The signed objects that wait to be delivered, by kind. */
    fun outbox(): List<OutboxRow> {
        val r = call(OP_OUTBOX, handle)
        return r.list { r.outboxRow() }.also { r.end() }
    }

    /** Build an approval for the Key app at [feeRate] satoshis per 1000 virtual bytes. */
    fun buildRequest(now: Long, feeRate: Long): Built {
        val r = call(OP_BUILD_REQUEST, handle) { it.u64(now).u64(feeRate) }
        return r.built().also { r.end() }
    }

    /** Build the approval for the merge offer. */
    fun approveMerge(now: Long): Built {
        val r = call(OP_APPROVE_MERGE, handle) { it.u64(now) }
        return r.built().also { r.end() }
    }

    /** Take the Key app's response; Key 1's state is kept in the data folder. */
    fun take(now: Long, response: ByteArray): Taken {
        val r = call(OP_TAKE, handle) { it.u64(now).bytes(response) }
        return r.taken().also { r.end() }
    }

    /** The open approvals' IDs. */
    fun sessions(): List<ByteArray> {
        val r = call(OP_SESSIONS, handle)
        return r.list { r.bytes() }.also { r.end() }
    }

    /** The request of an open approval, to show to the Key app again. */
    fun sessionRequest(id: ByteArray): ByteArray {
        val r = call(OP_SESSION_REQUEST, handle) { it.bytes(id) }
        return r.bytes().also { r.end() }
    }

    fun abandonSession(id: ByteArray) = callUnit(OP_ABANDON_SESSION, handle) { it.bytes(id) }

    fun queue(): Queue {
        val r = call(OP_QUEUE, handle)
        return r.queue().also { r.end() }
    }

    /** Queue a refund on the wallet's own facts (OC-14). */
    fun queueRefund(now: Long, args: RefundArgs): QueuedRefund {
        val r =
            call(OP_QUEUE_REFUND, handle) {
                it.u64(now).bytes(args.counter).u32(args.sale).string(args.cashier)
                    .string(args.address).u64(args.sats)
            }
        return r.queuedRefund().also { r.end() }
    }

    fun cancel(id: Long) = callUnit(OP_CANCEL, handle) { it.u32(id) }

    /** Put an entry that was sent back in the queue again. */
    fun requeue(id: Long) = callUnit(OP_REQUEUE, handle) { it.u32(id) }

    /** Queue a cash-out; returns the entry's number. */
    fun cashOut(args: CashOutArgs): Long {
        val r =
            call(OP_CASH_OUT, handle) {
                it.string(args.address).cashAmount(args.amount).string(args.label)
            }
        return r.u32().also { r.end() }
    }

    /** Sign a batch of addresses for a counter and queue it; returns the entry's number. */
    fun signBatch(now: Long, args: BatchArgs): Long {
        val r =
            call(OP_SIGN_BATCH, handle) {
                it.u64(now).bytes(args.counter).u32(args.start).u32(args.count)
            }
        return r.u32().also { r.end() }
    }

    /** Queue a change of the shop's settings; [signedSettings] is the bytes of the settings. */
    fun queueSettings(signedSettings: ByteArray): SettingsQueued {
        val r = call(OP_QUEUE_SETTINGS, handle) { it.bytes(signedSettings) }
        return r.settingsQueued().also { r.end() }
    }

    /** The link to the counters: the alerts and what each counter's copy holds. */
    fun linkState(): LinkState {
        val r = call(OP_LINK_STATE, handle)
        return r.linkState().also { r.end() }
    }

    fun clearAlert(args: ClearArgs) =
        callUnit(OP_CLEAR_ALERT, handle) {
            it.alertKind(args.kind).bytes(args.counter).u64(args.value)
        }

    fun refundNotes(): List<RefundNote> {
        val r = call(OP_REFUND_NOTES, handle)
        return r.list { r.refundNote() }.also { r.end() }
    }

    fun dismissRefundNote(index: Long) = callUnit(OP_DISMISS_REFUND_NOTE, handle) { it.u32(index) }

    /** The lines the node and the link logged since the last call. */
    fun notes(): List<String> {
        val r = call(OP_NOTES, handle)
        return r.strings().also { r.end() }
    }

    /** Check again from the wallet's first block. */
    fun rescan() = callUnit(OP_RESCAN, handle)

    /** Roll a committed approval forward into Key 1's state. */
    fun recoverKey1() = callUnit(OP_RECOVER_KEY1, handle)

    /** Archive the paid entries that no reorganization can reopen; returns how many. */
    fun archivePaid(): Long {
        val r = call(OP_ARCHIVE_PAID, handle)
        return r.u32().also { r.end() }
    }
}
