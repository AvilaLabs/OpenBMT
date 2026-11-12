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
private const val OP_LINK_STEP = 24
private const val OP_LINK_STATE = 25
private const val OP_STATUS = 30
private const val OP_NOTES = 31
private const val OP_OWN_NODE_RETRY = 32
private const val OP_RATE_SOURCES = 40
private const val OP_RATE_PARSE = 41
private const val OP_RATE_COMBINE = 42
private const val OP_PAIRING_READ = 43
private const val OP_PAIR_BEGIN = 44
private const val OP_PAIR_MATCH = 45
private const val OP_PAIR_DIFFER = 46
private const val OP_PAIR_CANCEL = 47
private const val OP_PAIR_FINISH = 48
private const val OP_OWN_NODE_CHECK = 49
private const val OP_LN_SETUP = 50
private const val OP_LN_REPLY = 51
private const val OP_LN_FAILED = 52
private const val OP_LN_PIN = 53
private const val OP_LN_NEXT = 54
private const val OP_LN_REMOVE = 55
private const val OP_TAKE_RELAY = 56
private const val OP_STAFF_RECORD = 57
private const val OP_DAY_TOTALS = 58
private const val OP_TIP_CHOICES = 59
private const val OP_OPEN_WITH_TIP = 60
private const val OP_OPEN_REQUEST = 61
private const val OP_ADD_NOTE = 62
private const val OP_SALE_INFO = 63
private const val OP_REQUESTS = 64
private const val OP_REQUEST_LINK = 65
private const val OP_SWAP_NEXT = 66
private const val OP_SWAP_REPLY = 67
private const val OP_SWAP_FAILED = 68
private const val OP_PAIR_ADDRESS_SAME = 69
private const val OP_PAIR_ADDRESS_DIFFERENT = 70
private const val OP_PENDING_CHANGE = 71
private const val OP_CONFIRM_CHANGE = 72

// ---- tag bytes of the enums (PROTOCOL.md) ------------------------------------------

private const val TAG_COUNTER_NETWORK_MAINNET = 0
private const val TAG_COUNTER_NETWORK_TESTNET = 1
private const val TAG_COUNTER_NETWORK_SIGNET = 2
private const val TAG_COUNTER_NETWORK_REGTEST = 3
private const val TAG_RELAY_KIND_NONE = 0
private const val TAG_RELAY_KIND_DIRECT = 1
private const val TAG_RELAY_KIND_ONION = 2
private const val TAG_OWN_WALLET_KIND_NONE = 0
private const val TAG_OWN_WALLET_KIND_SINGLE_KEY = 1
private const val TAG_OWN_WALLET_KIND_TWO_OF_THREE = 2
private const val TAG_CHANGE_VALUE_KIND_UNITS = 0
private const val TAG_CHANGE_VALUE_KIND_SATS = 1
private const val TAG_CHANGE_VALUE_KIND_BPS = 2
private const val TAG_CHANGE_VALUE_KIND_COUNT = 3
private const val TAG_CHANGE_VALUE_KIND_SECONDS = 4
private const val TAG_CHANGE_VALUE_KIND_FLAG = 5
private const val TAG_CHANGE_VALUE_KIND_CODE = 6
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
private const val TAG_STATE_VIEW_PAID_LIGHTNING = 11
private const val TAG_LN_KIND_LND = 0
private const val TAG_LN_KIND_CLN = 1
private const val TAG_LN_METHOD_GET = 0
private const val TAG_LN_METHOD_POST = 1
private const val TAG_LN_TRANSPORT_VIEW_PIN_REFUSED = 0
private const val TAG_LN_TRANSPORT_VIEW_UNREACHABLE = 1
private const val TAG_LN_TRANSPORT_VIEW_TIMED_OUT = 2
private const val TAG_LN_TRANSPORT_VIEW_TOO_BIG = 3
private const val TAG_LN_TRANSPORT_VIEW_REDIRECTED = 4
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

/** What kind of relay a pairing record holds (CR-131): an onion relay needs Orbot. */
enum class RelayKind { None, Direct, Onion }

/** The kind of wallet an offer from the shop's own wallet names (CR-228). */
enum class OwnWalletKind { None, SingleKey, TwoOfThree }

/** Who acts at the counter. The app has checked the person's PIN. */
enum class ActorRole { Cashier, Manager }

class Actor(val role: ActorRole, val name: String)

/**
 * What [CounterNode.start] is given. [record] is the pairing record that
 * [Pairing.finish] returned (the identity secret, the two roots, the wallet
 * check, the network, the signed settings, the first batch and the peers);
 * the app keeps it sealed and hands it over at each start. [dir] is the
 * app's own folder (`noBackupFilesDir`). [ownNode] is the own node's address
 * as [OwnNodeSetting.check] made it, or null (CR-100): an address that is
 * refused, or that comes with no [proxy], starts the node without an own node
 * and the status says why. [lightning] is the saved Lightning connection as
 * [CounterNode.lnReply] returned it when the test passed, kept sealed by the
 * app (CR-115), or null. [linkProxy] is Orbot's address for the link to the
 * owner when the relay is an onion service and the settings have Tor off (CR-134);
 * give it only when the proxy answers.
 */
class StartConfig(
    val dir: String,
    val now: Long,
    val record: ByteArray,
    val proxy: String?,
    val ownNode: String? = null,
    val lightning: ByteArray? = null,
    val linkProxy: String? = null,
)

/** What [Pairing.begin] returns: the request to show as a code, and the four words. */
/**
 * What [Pairing.begin] and [Pairing.addressSame] return. For the Key app's offer: the request and the
 * four words. For an offer from the shop's own wallet, [Pairing.begin] returns the first address and
 * the [wallet] kind with no request and no words (CR-228); [Pairing.addressSame] then returns the
 * request and the words (CR-229) with an empty [address].
 */
class PairStarted(val request: ByteArray, val words: List<String>, val address: String, val wallet: OwnWalletKind)

/** What the app needs from a saved pairing record before the node runs. */
class PairingInfo(val network: CounterNetwork, val settings: ShopSettings, val relay: RelayKind)

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

/** A count with the sums counted: minor units of the shop's currency and satoshis. */
class Tally(val count: Long, val minor: Long, val sats: Long)

/** What one person did in the day. */
class PersonLine(
    val role: ActorRole,
    val name: String,
    val sales: Long,
    val overrides: Long,
    val refunds: Long,
    val cashRefunds: Long,
    /** The tips of the paid sales this person opened, in minor units (CR-191). */
    val tipsMinor: Long = 0,
)

/** The tips of the paid sales of a day: how many carried one, and their sum in minor units (CR-191). */
class TipTally(val count: Long, val minor: Long)

/** A tip the counter offers for a sale: a suggested percent and what it comes to in minor units (CR-188). */
class TipChoice(val percent: Int, val tipMinor: Long)

/** How a value of a field of a change to the settings is to be written (CR-240). */
enum class ChangeValueKind { Units, Sats, Bps, Count, Seconds, Flag, Code }

/**
 * One value of a field, now or after a change (CR-240). [number] holds the amount for every kind but
 * [ChangeValueKind.Code], which has its three letters in [text]; a [ChangeValueKind.Flag] is 1 for on.
 * [ChangeValueKind.Units] are whole units of the shop's currency, 0 being off.
 */
class ChangeValue(val kind: ChangeValueKind, val number: Long, val text: String)

/** One field a change loosens: its name as the settings call it, its value now and its value after (CR-240). */
class ChangeField(val field: String, val now: ChangeValue, val after: ChangeValue)

/**
 * The change to the shop's settings that waits for a counter's confirmation (CR-239, CR-240): its [version], the
 * four [words] to compare with the owner phone, each [fields] it loosens, when it [arrived] and when it is dropped
 * ([expiresAt]) if nobody confirms it, and whether this counter [confirmed] it since it last started.
 */
class PendingChange(
    val version: Long,
    val words: List<String>,
    val fields: List<ChangeField>,
    val arrived: Long,
    val expiresAt: Long,
    val confirmed: Boolean,
)

/** What [CounterNode.pendingChange] returns: the change that waits, and the words of one that came to nothing (CR-244). */
class ChangeState(val pending: PendingChange?, val notice: Words?)

/** What a sale carries besides its amount: its note, and the days of a request to send (0 for a sale shown at the till) (CR-205, CR-206). */
class SaleInfo(val note: String?, val requestDays: Int, val expiresAt: Long, val cashRefundOnly: Boolean)

/** One open request to send, for the list on Home (CR-215). */
class OpenRequest(
    val index: Long,
    val note: String?,
    val amountMinor: Long,
    val openedAt: Long,
    val expiresAt: Long,
    val needsSats: Long,
)

/** The totals of one day from the counter's own records (CR-153). */
class DayTotals(
    val from: Long,
    val to: Long,
    val currency: String?,
    val paid: Tally,
    val onChain: Tally,
    val lightning: Tally,
    val waiting: Tally,
    val stopped: Long,
    val ended: Long,
    val overrides: Tally,
    val refunds: Tally,
    val refundsWithdrawn: Long,
    val cashRefunds: Tally,
    val tips: TipTally,
    val unnamedSales: Long,
    val people: List<PersonLine>,
    val peopleLeftOut: Long,
)

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
    DoNotHandOver, Expired, Cancelled, NeedsAPerson, PaidLightning,
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
    /** CR-105: which check was done, for a payment that waits; null otherwise. */
    val check: Words?,
    val receivedSats: Long,
    val needsSats: Long,
    val stuck: Boolean,
    val late: Boolean,
    val reopened: Boolean,
    val conflictSeen: Boolean,
    val reports: List<Report>,
    /** What the customer scans; the code of LN-18 when [lightning] says there is an invoice. */
    val payment: Payment?,
    /** CR-113: what the sale shows of Lightning; null when Lightning is off. */
    val lightning: LightningSale?,
)

/**
 * CR-113: what a sale shows of Lightning. [unavailable] is the one line, with the reason under its details.
 * Through a swap provider, [notForThisSale] is the small line of CR-219 and [failed] the words of CR-223.
 */
class LightningSale(
    val invoice: Boolean,
    val waiting: Boolean,
    val unavailable: Words?,
    val notForThisSale: Boolean,
    val failed: Words?,
)

/** CR-116: the Lightning line of the status. */
class LightningStatus(val ready: Boolean, val words: Words)

/** The kind of node a connection is for (LN-01). */
enum class LnKind { Lnd, Cln }

enum class LnMethod { Get, Post }

/** Why a request to the node got no answer (CR-114). */
enum class LnTransport { PinRefused, Unreachable, TimedOut, TooBig, Redirected }

/**
 * One request for the app to send to the shop's node (CR-114): HTTPS to
 * [hostPort] only, no redirect, the certificate pinned with
 * [CounterNode.lnPin]. [headerValue] is a secret: never log it.
 */
class LnRequest(
    val id: Long,
    val hostPort: String,
    val method: LnMethod,
    val path: String,
    val headerName: String,
    val headerValue: String,
    val body: String?,
) {
    override fun toString(): String = "LnRequest(id=$id, $method $path)"
}

/**
 * One request for the app to send to the swap provider (CR-225): plain HTTP/1.1 to the onion name [hostPort]
 * through the counter's own Tor, never around it. [bytes] is the whole request.
 */
class SwapRequest(val id: Long, val hostPort: String, val bytes: ByteArray) {
    override fun toString(): String = "SwapRequest(id=$id, ${bytes.size} bytes)"
}

/** What an answer to a setup request led to: the next request, or the record to seal. */
class LnStep(val next: LnRequest?, val record: ByteArray?)

class Pump(val reports: List<Report>, val notes: List<String>, val stopped: Boolean)

class RefundRow(
    val sale: Long,
    val number: Long?,
    val askedSats: Long,
    val valueMinor: Long?,
    /** How much of the refund is the tip, in minor units of the shop's currency (CR-192). */
    val tipMinor: Long,
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

/** CR-107: the own node's health line. [usable] says it can be used as a second check now. */
class OwnNodeStatus(val usable: Boolean, val words: Words)

class Status(
    val checkedHeight: Long,
    val tipHeight: Long,
    val health: Health?,
    /** Null when no own node is set. */
    val ownNode: OwnNodeStatus?,
    val sales: List<Long>,
    val refusedBelowMinimum: Long,
    val stopped: Boolean,
    /** CR-116: null when no Lightning credential is saved. */
    val lightning: LightningStatus?,
)

/**
 * CR-136: the link to the owner. [words] is the line for the home screen
 * ("Connected to the Box", or why not); [since] is when the Box was first not
 * reachable, while it is not. [settingsVersion] is the version of the
 * settings in force; [restartNeeded] is set when the settings changed Tor or
 * the Lightning node, so the app starts the node again when no sale screen is
 * open (CR-164); [settingsLine] is what Home says about the settings, if
 * anything (CR-164, CR-165).
 */
class Link(
    val words: Words,
    val connected: Boolean,
    val since: Long?,
    val refused: Boolean,
    val heartbeatsTaken: Long,
    val ownerRecords: Long,
    val lastCheckInAt: Long?,
    val settingsVersion: Long,
    val restartNeeded: Boolean,
    val settingsLine: Words?,
)

/** What [CounterNode.linkStep] returns: the link after the step, and what the link logged. */
class LinkStep(val link: Link, val notes: List<String>)

/** One source of the price table (RT-01): where the app fetches, and what it asks for. */
class RateSource(val id: String, val url: String, val currency: String, val initial: String)

/**
 * One price from one source (RT-45): [fetchedAt] is the app's own clock when
 * the whole reply had been read, [minorPerBtc] is in minor units of the
 * source's currency.
 */
data class Sample(val source: String, val fetchedAt: Long, val minorPerBtc: Long)

/**
 * What the app needs from the shop's settings before the node runs. [currency] is the shop's currency
 * (CR-193): three capital letters, or "SAT" for Bitcoin Standard, where an amount is satoshis and no
 * price is asked (CR-197). [tipsOn] and [tipPercents] are the tips the counters ask for (CR-187).
 */
class ShopSettings(
    val version: Long,
    val torOn: Boolean,
    val customerWifi: Boolean,
    val minSaleSats: Long,
    val currency: String = "USD",
    val tipsOn: Boolean = false,
    val tipPercents: List<Int> = listOf(10, 15, 20),
)

/** The rate a sale is opened with, and its source as the counter takes it (CR-57). */
class Rate(val rate: Long, val sourceLabel: String, val sourceFetchedAt: Long)

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

private fun Reader.network(): CounterNetwork =
    when (u8()) {
        TAG_COUNTER_NETWORK_MAINNET -> CounterNetwork.Mainnet
        TAG_COUNTER_NETWORK_TESTNET -> CounterNetwork.Testnet
        TAG_COUNTER_NETWORK_SIGNET -> CounterNetwork.Signet
        TAG_COUNTER_NETWORK_REGTEST -> CounterNetwork.Regtest
        else -> tag("CounterNetwork")
    }

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
    string(c.dir).u64(c.now).bytes(c.record).option(c.proxy) { string(it) }
        .option(c.ownNode) { string(it) }.option(c.lightning) { bytes(it) }.option(c.linkProxy) { string(it) }


internal fun Reader.ownWalletKind(): OwnWalletKind =
    when (val t = u8()) {
        TAG_OWN_WALLET_KIND_NONE -> OwnWalletKind.None
        TAG_OWN_WALLET_KIND_SINGLE_KEY -> OwnWalletKind.SingleKey
        TAG_OWN_WALLET_KIND_TWO_OF_THREE -> OwnWalletKind.TwoOfThree
        else -> tag("OwnWalletKind $t")
    }

internal fun Reader.relayKind(): RelayKind =
    when (val t = u8()) {
        TAG_RELAY_KIND_NONE -> RelayKind.None
        TAG_RELAY_KIND_DIRECT -> RelayKind.Direct
        TAG_RELAY_KIND_ONION -> RelayKind.Onion
        else -> tag("RelayKind $t")
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
        TAG_STATE_VIEW_PAID_LIGHTNING -> SaleState.PaidLightning
        else -> tag("StateView")
    }

private fun Reader.tally(): Tally = Tally(u32(), u64(), u64())

private fun Reader.personLine(): PersonLine =
    PersonLine(role(), string(), u32(), u32(), u32(), u32(), u64())

internal fun Reader.dayTotals(): DayTotals =
    DayTotals(
        u64(), u64(), option { string() }, tally(), tally(), tally(), tally(), u32(), u32(),
        tally(), tally(), u32(), tally(), TipTally(u32(), u64()), u32(), list { personLine() }, u32(),
    )

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

private fun Reader.changeValueKind(): ChangeValueKind =
    when (val t = u8()) {
        TAG_CHANGE_VALUE_KIND_UNITS -> ChangeValueKind.Units
        TAG_CHANGE_VALUE_KIND_SATS -> ChangeValueKind.Sats
        TAG_CHANGE_VALUE_KIND_BPS -> ChangeValueKind.Bps
        TAG_CHANGE_VALUE_KIND_COUNT -> ChangeValueKind.Count
        TAG_CHANGE_VALUE_KIND_SECONDS -> ChangeValueKind.Seconds
        TAG_CHANGE_VALUE_KIND_FLAG -> ChangeValueKind.Flag
        TAG_CHANGE_VALUE_KIND_CODE -> ChangeValueKind.Code
        else -> tag("ChangeValueKind $t")
    }

private fun Reader.changeValue(): ChangeValue = ChangeValue(changeValueKind(), u64(), string())

private fun Reader.changeField(): ChangeField = ChangeField(string(), changeValue(), changeValue())

private fun Reader.pendingChange(): PendingChange =
    PendingChange(u32(), strings(), list { changeField() }, u64(), u64(), bool())

internal fun Reader.changeState(): ChangeState = ChangeState(option { pendingChange() }, option { words() })

internal fun Reader.saleInfo(): SaleInfo = SaleInfo(option { string() }, u8(), u64(), bool())

internal fun Reader.openRequest(): OpenRequest =
    OpenRequest(u32(), option { string() }, u64(), u64(), u64(), u64())

internal fun Reader.screen(): Screen =
    Screen(
        saleState(), words(), list { words() }, option { words() }, u64(), u64(), bool(), bool(),
        bool(), bool(), reports(), option { payment() }, option { lightningSale() },
    )

private fun Reader.lightningSale(): LightningSale =
    LightningSale(bool(), bool(), option { words() }, bool(), option { words() })

internal fun Reader.swapRequest(): SwapRequest = SwapRequest(u64(), string(), bytes())

private fun Reader.lnMethod(): LnMethod =
    when (u8()) {
        TAG_LN_METHOD_GET -> LnMethod.Get
        TAG_LN_METHOD_POST -> LnMethod.Post
        else -> tag("LnMethod")
    }

internal fun Reader.lnRequest(): LnRequest =
    LnRequest(u64(), string(), lnMethod(), string(), string(), string(), option { string() })

internal fun Reader.lnStep(): LnStep = LnStep(option { lnRequest() }, option { bytes() })

internal fun Reader.pump(): Pump = Pump(reports(), strings(), bool())

private fun Reader.refundRow(): RefundRow =
    RefundRow(
        u32(), option { u64() }, u64(), option { u64() }, u64(), refundState(), role(), string(), u64(),
        words(),
    )

internal fun Reader.rateSource(): RateSource = RateSource(string(), string(), string(), string())

internal fun Reader.sample(): Sample = Sample(string(), u64(), u64())

internal fun Reader.shopSettings(): ShopSettings = ShopSettings(u32(), bool(), bool(), u64(), string(), bool(), bytes().map { it.toInt() and 0xff })

internal fun Reader.rate(): Rate = Rate(u64(), string(), u64())

internal fun Reader.linkView(): Link =
    Link(
        words(), bool(), option { u64() }, bool(), u64(), u64(), option { u64() },
        u32(), bool(), option { words() },
    )

internal fun Reader.linkStepView(): LinkStep = LinkStep(linkView(), strings())

private fun Reader.heartbeatOutcome(): HeartbeatOutcome =
    HeartbeatOutcome(bool(), option { u64() }, words(), reports())

internal fun Reader.statusView(): Status =
    Status(
        u32(), u32(), option { Health(bool(), bool(), words()) },
        option { OwnNodeStatus(bool(), words()) }, u32s(), u64(), bool(),
        option { LightningStatus(bool(), words()) },
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

/** The request frame of `ln_reply`: the answer of the node to request [id]. */
internal fun lnReplyFrame(handle: Long, id: Long, status: Long, body: ByteArray): ByteArray =
    requestFrame(OP_LN_REPLY, handle) { it.u64(id).u32(status).bytes(body) }

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
private fun send(request: ByteArray): ByteArray =
    Native.call(request) ?: throw CounterException("response-null", "the library gave no response", 4)

private fun call(op: Int, handle: Long, fill: (Writer) -> Unit = {}): Reader =
    decodeResponse(send(requestFrame(op, handle, fill)))

/** A call whose result is nothing. */
private fun callUnit(op: Int, handle: Long, fill: (Writer) -> Unit = {}) {
    call(op, handle, fill).end()
}

/** The request frame of `rate_parse`. */
internal fun rateParseFrame(source: String, response: ByteArray, fetchedAt: Long): ByteArray =
    requestFrame(OP_RATE_PARSE, 0) { it.string(source).bytes(response).u64(fetchedAt) }

/** The request frame of `rate_combine`. */
internal fun rateCombineFrame(samples: List<Sample>, now: Long): ByteArray =
    requestFrame(OP_RATE_COMBINE, 0) {
        it.list(samples) { s -> it.string(s.source).u64(s.fetchedAt).u64(s.minorPerBtc) }.u64(now)
    }

/**
 * The price ops work on no node: the app fetches the replies itself on the
 * schedule of RT-40 to RT-45 and hands the bytes over. A refusal is a
 * [CounterException] with the words of `openbmt-rates`.
 */
object Rates {
    /** The table of sources (RT-01). */
    fun sources(): List<RateSource> {
        val r = call(OP_RATE_SOURCES, 0)
        return r.list { r.rateSource() }.also { r.end() }
    }

    /** The price in the reply of the source [source], stamped [fetchedAt] (RT-45). */
    fun parse(source: String, response: ByteArray, fetchedAt: Long): Sample {
        val r = decodeResponse(rateParseFrame(source, response, fetchedAt).let(::send))
        return r.sample().also { r.end() }
    }

    /** The one rate of the [samples] at [now] (RT-30 to RT-38). */
    fun combine(samples: List<Sample>, now: Long): Rate {
        val r = decodeResponse(send(rateCombineFrame(samples, now)))
        return r.rate().also { r.end() }
    }
}

/**
 * A look at the shop's signed settings before the node starts, to know
 * whether Tor is on. The settings are read, not verified: `start` verifies
 * the same bytes.
 */
object Shop {
    /**
     * The pairing record read for display. With the counter's data folder [dir] the settings are the ones a
     * start there would use, which can differ from the record's after the owner changed Tor (CR-163).
     */
    fun pairing(record: ByteArray, dir: String = ""): PairingInfo {
        val r = decodeResponse(send(requestFrame(OP_PAIRING_READ, 0) { it.bytes(record).string(dir) }))
        return PairingInfo(r.network(), r.shopSettings(), r.relayKind()).also { r.end() }
    }
}

/**
 * The own node's address (CR-100 to CR-102). It works on no node: [check]
 * reads what a manager typed and returns the canonical text the app keeps
 * and hands to [StartConfig.ownNode]; a refusal is a [CounterException] with
 * its words (a home-network address, a name that is not a Tor address, a
 * port that is not one, or no Tor: "turn on Orbot first").
 */
object OwnNodeSetting {
    /** [text] as typed, on [network], with [tor] saying whether the counter has a Tor proxy. */
    fun check(text: String, network: CounterNetwork, tor: Boolean): String {
        val r = decodeResponse(
            send(requestFrame(OP_OWN_NODE_CHECK, 0) { it.string(text).network(network).u8(if (tor) 1 else 0) }),
        )
        return r.string().also { r.end() }
    }
}

/**
 * The pairing ceremony of the counter (CR-91 to CR-96). It works on no node
 * and lives in the library, in memory only: [begin] takes the Key app's offer
 * and returns the counter's request and the four words; the person compares
 * the words and calls [match] or [differ]; [finish] takes the owner phone's
 * bundle and returns the pairing record the app keeps sealed. A refusal is a
 * [CounterException] with its words.
 */
object Pairing {
    /** Begin from the offer's bytes; [now] is the app's clock. */
    fun begin(offer: ByteArray, name: String, now: Long): PairStarted {
        val r = decodeResponse(send(requestFrame(OP_PAIR_BEGIN, 0) { it.bytes(offer).string(name).u64(now) }))
        return PairStarted(r.bytes(), r.strings(), r.string(), r.ownWalletKind()).also { r.end() }
    }

    /**
     * CR-229: the person says the address on the counter is the same as the wallet's. Only now does
     * the counter make its key; returns the request and the four words.
     */
    fun addressSame(now: Long, name: String): PairStarted {
        val r = decodeResponse(send(requestFrame(OP_PAIR_ADDRESS_SAME, 0) { it.u64(now).string(name) }))
        return PairStarted(r.bytes(), r.strings(), r.string(), r.ownWalletKind()).also { r.end() }
    }

    /** CR-228: the person says the address is different: the ceremony is forgotten; these are the words to show. */
    fun addressDifferent(): Words {
        val r = call(OP_PAIR_ADDRESS_DIFFERENT, 0)
        return r.words().also { r.end() }
    }

    /** The person says the words match. */
    fun match(now: Long) {
        callUnit(OP_PAIR_MATCH, 0) { it.u64(now) }
    }

    /** The person says the words differ: the ceremony is forgotten; these are the words to show. */
    fun differ(): Words {
        val r = call(OP_PAIR_DIFFER, 0)
        return r.words().also { r.end() }
    }

    /** The person leaves: the ceremony is forgotten. */
    fun cancel() {
        callUnit(OP_PAIR_CANCEL, 0)
    }

    /**
     * CR-132: take the owner's signed relay for counters (the code the owner's phone shows) into
     * the pairing [record]; returns the new record for the app to keep in place of the old. The
     * link starts from it at the next start. A refusal is a [CounterException] with its words.
     */
    fun takeRelay(record: ByteArray, frame: ByteArray): ByteArray {
        val r = decodeResponse(send(requestFrame(OP_TAKE_RELAY, 0) { it.bytes(record).bytes(frame) }))
        return r.bytes().also { r.end() }
    }

    /** Finish with the owner phone's bundle; returns the pairing record. */
    fun finish(now: Long, bundle: ByteArray): ByteArray {
        val r = decodeResponse(send(requestFrame(OP_PAIR_FINISH, 0) { it.u64(now).bytes(bundle) }))
        return r.bytes().also { r.end() }
    }
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
        actor: Actor? = null,
    ): Opened {
        val r =
            call(OP_OPEN, handle) {
                it.u64(now).u64(amountMinor).string(currency).u64(rate).string(sourceLabel)
                    .u64(sourceFetchedAt).option(actor) { a -> it.actor(a) }
            }
        return r.opened().also { r.end() }
    }

    /** The tips the settings offer for a sale of [amountMinor], one for each suggested percent; none when tips are off (CR-188). */
    fun tipChoices(amountMinor: Long): List<TipChoice> {
        val r = call(OP_TIP_CHOICES, handle) { it.u64(amountMinor) }
        return r.list { TipChoice(r.u8(), r.u64()) }.also { r.end() }
    }

    /**
     * Open a sale with a tip (CR-189, CR-190): the payment is for [amountMinor] and [tipMinor] together. A tip above
     * the sale is a [CounterException] with its words.
     */
    fun openWithTip(
        now: Long,
        amountMinor: Long,
        tipMinor: Long,
        currency: String,
        rate: Long,
        sourceLabel: String,
        sourceFetchedAt: Long,
        actor: Actor? = null,
    ): Opened {
        val r =
            call(OP_OPEN_WITH_TIP, handle) {
                it.u64(now).u64(amountMinor).u64(tipMinor).string(currency).u64(rate).string(sourceLabel)
                    .u64(sourceFetchedAt).option(actor) { a -> it.actor(a) }
            }
        return r.opened().also { r.end() }
    }

    /**
     * Make a request to send (CR-205): a sale that lasts [days] (1, 3 or 7) with one quote fixed for all of them, with
     * the [note] if the cashier wrote one. A note the library refuses is a [CounterException] with its words and no
     * request is made.
     */
    fun openRequest(
        now: Long,
        amountMinor: Long,
        currency: String,
        rate: Long,
        sourceLabel: String,
        sourceFetchedAt: Long,
        actor: Actor?,
        note: String?,
        days: Int,
    ): Opened {
        val r =
            call(OP_OPEN_REQUEST, handle) {
                it.u64(now).u64(amountMinor).string(currency).u64(rate).string(sourceLabel).u64(sourceFetchedAt)
                    .option(actor) { a -> it.actor(a) }.option(note) { n -> it.string(n) }.u8(days)
            }
        return r.opened().also { r.end() }
    }

    /** Add a note to a sale that has none (CR-206). */
    fun addNote(now: Long, index: Long, note: String) {
        val r = call(OP_ADD_NOTE, handle) { it.u64(now).u32(index).string(note) }
        r.end()
    }

    /** The note of a sale, and the days of a request to send (CR-205, CR-206). */
    fun saleInfo(index: Long): SaleInfo {
        val r = call(OP_SALE_INFO, handle) { it.u32(index) }
        return r.saleInfo().also { r.end() }
    }

    /**
     * The change to the shop's settings that waits for a counter's confirmation, with each field it loosens, its
     * version and the four words, and the words of a change that was dropped or refused (CR-239, CR-240, CR-244).
     */
    fun pendingChange(now: Long): ChangeState {
        val r = call(OP_PENDING_CHANGE, handle) { it.u64(now) }
        return r.changeState().also { r.end() }
    }

    /**
     * CR-241: sign the confirmation of the change that waits, as the [version] the manager was shown, and write it
     * as a record of the counter's chain; returns the words to show. The app asks only after the manager's PIN was
     * right (CR-84): the library checks no PIN.
     */
    fun confirmChange(now: Long, version: Long): Words {
        val r = call(OP_CONFIRM_CHANGE, handle) { it.u64(now).u32(version) }
        return r.words().also { r.end() }
    }

    /** The open requests, newest first (CR-215). */
    fun requests(now: Long): List<OpenRequest> {
        val r = call(OP_REQUESTS, handle) { it.u64(now) }
        return r.list { r.openRequest() }.also { r.end() }
    }

    /** The payment link of a request to send, with its amount and [label] (CR-214). */
    fun requestLink(index: Long, label: String): String {
        val r = call(OP_REQUEST_LINK, handle) { it.u32(index).string(label) }
        return r.string().also { r.end() }
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

    /** The person at the till cancels a sale with nothing received. */
    fun cancel(now: Long, index: Long, actor: Actor? = null) =
        callUnit(OP_CANCEL, handle) { it.u64(now).u32(index).option(actor) { a -> it.actor(a) } }

    /** Write a change to the cashiers as a record for the owner (CR-143). */
    fun staffRecord(now: Long, added: Boolean, name: String, by: String) =
        callUnit(OP_STAFF_RECORD, handle) {
            it.u64(now).u8(if (added) 1 else 0).string(name).string(by)
        }

    /** The totals of the day from [from] (counted) to [to] (not counted) (CR-152). */
    fun dayTotals(from: Long, to: Long, people: Boolean): DayTotals {
        val r = call(OP_DAY_TOTALS, handle) { it.u64(from).u64(to).u8(if (people) 1 else 0) }
        return r.dayTotals().also { r.end() }
    }

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

    /**
     * CR-135: one turn of the link to the owner; call it from a thread of its own every five
     * seconds. It may wait for the Box as long as the Box's timeouts say, and it does not hold up
     * the other calls while it does. With [again], the Box is called now (the manager's Try
     * again). Refused when the pairing has no relay.
     */
    fun linkStep(now: Long, again: Boolean = false): LinkStep {
        val r = call(OP_LINK_STEP, handle) { it.u64(now).u8(if (again) 1 else 0) }
        return r.linkStepView().also { r.end() }
    }

    /** CR-136: the link as the screen shows it, or null when the pairing has no relay. */
    fun linkState(): Link? {
        val r = call(OP_LINK_STATE, handle)
        return r.option { r.linkView() }.also { r.end() }
    }

    /** CR-107: ask the driver to dial the own node again ("Try again"). */
    fun ownNodeRetry() = callUnit(OP_OWN_NODE_RETRY, handle)

    // ---- Lightning through the shop's own node (CR-112 to CR-116) ----

    /**
     * Begin the test of a connection a manager scanned or typed (CR-115):
     * for LND, [address] is the `lndconnect://` text and [secret] and [cert]
     * are empty; for Core Lightning, [address] is `host:port`, [secret] the
     * rune and [cert] the certificate as PEM or a SHA-256 fingerprint. Returns
     * the first request of the test, or throws with the words (among them
     * that the owner has not named the node yet).
     */
    fun lnSetup(kind: LnKind, address: String, secret: String, cert: String): LnRequest {
        val r = call(OP_LN_SETUP, handle) {
            it.u8(
                when (kind) {
                    LnKind.Lnd -> TAG_LN_KIND_LND
                    LnKind.Cln -> TAG_LN_KIND_CLN
                },
            ).string(address).string(secret).string(cert)
        }
        return r.lnRequest().also { r.end() }
    }

    /**
     * The answer to a request: the node's own, or a setup's. For a setup
     * it is the next request or, when the test passed, the record to seal in
     * `noBackupFilesDir` and hand to [StartConfig.lightning].
     */
    fun lnReply(id: Long, status: Long, body: ByteArray): LnStep {
        val r = decodeResponse(send(lnReplyFrame(handle, id, status, body)))
        return r.lnStep().also { r.end() }
    }

    /** The request got no answer. A setup's request throws the words of why the test stopped. */
    fun lnFailed(id: Long, why: LnTransport) =
        callUnit(OP_LN_FAILED, handle) {
            it.u64(id).u8(
                when (why) {
                    LnTransport.PinRefused -> TAG_LN_TRANSPORT_VIEW_PIN_REFUSED
                    LnTransport.Unreachable -> TAG_LN_TRANSPORT_VIEW_UNREACHABLE
                    LnTransport.TimedOut -> TAG_LN_TRANSPORT_VIEW_TIMED_OUT
                    LnTransport.TooBig -> TAG_LN_TRANSPORT_VIEW_TOO_BIG
                    LnTransport.Redirected -> TAG_LN_TRANSPORT_VIEW_REDIRECTED
                },
            )
        }

    /** Whether [der], the certificate the node showed, is the pinned one for request [id] (CR-114). */
    fun lnPin(id: Long, der: ByteArray): Boolean {
        val r = call(OP_LN_PIN, handle) { it.u64(id).bytes(der) }
        return r.bool().also { r.end() }
    }

    /** The next request for the node to send, or null when none is due (CR-112). */
    fun lnNext(now: Long): LnRequest? {
        val r = call(OP_LN_NEXT, handle) { it.u64(now) }
        return r.option { r.lnRequest() }.also { r.end() }
    }

    /** The next request for the swap provider, or null when none is due (CR-219, CR-225). */
    fun swapNext(now: Long): SwapRequest? {
        val r = call(OP_SWAP_NEXT, handle) { it.u64(now) }
        return r.option { r.swapRequest() }.also { r.end() }
    }

    /** The bytes the provider sent for request [id], everything until it closed the connection (CR-225). */
    fun swapReply(id: Long, bytes: ByteArray) = callUnit(OP_SWAP_REPLY, handle) { it.u64(id).bytes(bytes) }

    /** The request to the provider got no answer (CR-225). */
    fun swapFailed(id: Long) = callUnit(OP_SWAP_FAILED, handle) { it.u64(id) }

    /** Remove the Lightning connection at once (CR-115). */
    fun lnRemove() = callUnit(OP_LN_REMOVE, handle)

    /** The lines the node and the driver logged since the last call. */
    fun notes(): List<String> {
        val r = call(OP_NOTES, handle)
        return r.strings().also { r.end() }
    }
}
