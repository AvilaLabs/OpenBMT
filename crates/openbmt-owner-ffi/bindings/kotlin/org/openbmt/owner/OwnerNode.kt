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
private const val OP_SETTINGS_PREVIEW = 47
private const val OP_LINK_STATE = 50
private const val OP_CLEAR_ALERT = 51
private const val OP_REFUND_NOTES = 52
private const val OP_DISMISS_REFUND_NOTE = 53
private const val OP_NOTES = 60
private const val OP_RESCAN = 61
private const val OP_RECOVER_KEY1 = 62
private const val OP_ARCHIVE_PAID = 63
private const val OP_POS_STATE = 70
private const val OP_POS_SETUP = 71
private const val OP_POS_REMOVE = 72
private const val OP_POS_REQUEST = 73
private const val OP_POS_REPLY = 74
private const val OP_POS_ACKNOWLEDGE = 75
private const val OP_CEREMONY_MAKE_KEY1 = 80
private const val OP_CEREMONY_READ_ANSWER = 81
private const val OP_CEREMONY_BUILD_WALLET = 82
private const val OP_CEREMONY_FINISH = 83
private const val OP_CEREMONY_OPEN = 84
private const val OP_CEREMONY_STAGE = 85
private const val OP_CEREMONY_DEFAULTS = 86
private const val OP_COUNTER_SCAN = 90
private const val OP_COUNTER_CONFIRM = 91
private const val OP_COUNTERS_WAITING = 92
private const val OP_COUNTER_BUNDLE = 93
private const val OP_COUNTER_DELIVERED = 94
private const val OP_COUNTER_RELAY = 95
private const val OP_SETUP_LOCK_FORM = 100
private const val OP_SETUP_LOCK = 101
private const val OP_SETUP_UNLOCK = 102
private const val OP_SETUP_CHANGE_PIN = 103
private const val OP_BOX_FETCH_CLAIM = 104
private const val OP_BOX_QUEUE = 105
private const val OP_BOXES_WAITING = 106
private const val OP_BOX_DELIVER = 107
private const val OP_BOX_OWN_INVITATION = 108
private const val OP_BOX_OWN_CHECK = 109
private const val OP_BOXES_PENDING = 110
private const val OP_BOX_FORGET = 111
private const val OP_SIGNER_REGISTRATION = 120
private const val OP_SIGNER_BUILD = 121
private const val OP_SIGNER_EXPORT = 122
private const val OP_SIGNER_FILE = 123
private const val OP_SIGNER_TAKE = 124
private const val OP_FIRST_ADDRESS_RECORD = 125
private const val OP_FIRST_ADDRESS_READ = 126

// ---- tag bytes of the enums (PROTOCOL.md) ------------------------------------------

private const val TAG_OWNER_NETWORK_MAINNET = 0
private const val TAG_OWNER_NETWORK_TESTNET = 1
private const val TAG_OWNER_NETWORK_SIGNET = 2
private const val TAG_OWNER_NETWORK_REGTEST = 3
private const val TAG_ROUTE_VIEW_DIRECT = 0
private const val TAG_ROUTE_VIEW_ONION = 1
private const val TAG_CASH_AMOUNT_VIEW_SATS = 0
private const val TAG_CASH_AMOUNT_VIEW_ALL = 1
private const val TAG_LIGHTNING_ACTION_KEEP = 0
private const val TAG_LIGHTNING_ACTION_REMOVE = 1
private const val TAG_LIGHTNING_ACTION_SET = 2
private const val TAG_ALERT_KIND_MISSED_CHECK_IN = 0
private const val TAG_ALERT_KIND_DIVERGED = 1
private const val TAG_ALERT_KIND_BATCH_UNDELIVERED = 2
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
private const val TAG_FIRST_ADDRESS_KIND_CHECKED = 1
private const val TAG_FIRST_ADDRESS_KIND_SKIPPED = 2
private const val TAG_OLD_KIND_NONE = 0
private const val TAG_OLD_KIND_FIRST_CHECK_WAITING = 1
private const val TAG_OLD_KIND_MONTHLY = 2
private const val TAG_OLD_KIND_CHECKING = 3
private const val TAG_OLD_KIND_FAILED = 4
private const val TAG_OUTBOX_KIND_TRANSACTION = 0
private const val TAG_OUTBOX_KIND_BATCH = 1
private const val TAG_OUTBOX_KIND_SETTINGS = 2
private const val TAG_OUTBOX_KIND_PAIRING = 3
private const val TAG_OUTBOX_KIND_BOX_WELCOME = 4
private const val TAG_POS_VENDOR_VIEW_SQUARE = 0
private const val TAG_POS_VENDOR_VIEW_CLOVER = 1
private const val TAG_POS_METHOD_VIEW_GET = 0
private const val TAG_POS_METHOD_VIEW_POST = 1
private const val TAG_POS_ALERT_KIND_TENDER_WITHOUT_PAYMENT = 0
private const val TAG_POS_ALERT_KIND_CLOSED_BEFORE_PAID = 1
private const val TAG_POS_ALERT_KIND_PAYMENT_WITHOUT_TENDER = 2
private const val TAG_POS_ALERT_KIND_AMOUNT_DIFFERS = 3
private const val TAG_POS_ALERT_KIND_TENDER_ON_UNPAID_SALE = 4
private const val TAG_POS_ALERT_KIND_VOIDED_AFTER_PAYMENT = 5
private const val TAG_POS_ALERT_KIND_DUPLICATE_REFERENCE = 6
private const val TAG_POS_ALERT_KIND_BITCOIN_TIP = 7
private const val TAG_POS_ALERT_KIND_UNKNOWN_CURRENCY = 8
private const val TAG_POS_SUBJECT_VIEW_TENDER = 0
private const val TAG_POS_SUBJECT_VIEW_SALE = 1

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

enum class AlertKind { MissedCheckIn, Diverged, BatchUndelivered }

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

/** One address and amount, as the signer's screen shows it. */
class SignerLine(val address: String, val sats: Long)

/** The approval for the Key app, and the session of the spends for a hardware signer (null if none). */
class SignerBuilt(val built: Built, val signerSession: ByteArray?)

/** The file for a hardware signer: the PSBT key 1 signed, and what its screen must show. */
class SignerFile(
    val psbt: ByteArray,
    val txid: ByteArray,
    val fileName: String,
    val payments: List<SignerLine>,
    val change: SignerLine?,
    val fee: Long,
    val words: Words,
)

/** What taking the signer's answer did. */
class SignerTaken(val id: Long, val txid: ByteArray, val fee: Long, val sends: List<SendRow>)

/** The wallet in the forms a signer takes (AS-97). */
class Registration(
    val name: String,
    val textFile: String,
    val descriptorBoth: String,
    val descriptorReceive: String,
    val descriptorChange: String,
    val outputCbor: ByteArray,
    val firstAddress: String,
    val textFitsPlainQr: Boolean,
    val descriptorFitsPlainQr: Boolean,
)

/** The first address check: [checked] is false when it was skipped (AS-99). */
class FirstAddress(val checked: Boolean, val at: Long)

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

/** New addresses that wait for a counter to hold them (LK-70, LK-73). */
class BatchWaiting(
    val counter: ByteArray,
    val batches: Long,
    val since: Long,
    val words: Words,
)

class LinkState(
    val reachable: Boolean,
    val refused: Long,
    val alerts: List<Alert>,
    val counters: List<CounterState>,
    val batches: List<BatchWaiting>,
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
    /** The 33-byte key of the shop's own Lightning node, when the settings name one. */
    val lightningNode: ByteArray?,
)

/** What an edit does to the Lightning node. */
enum class LightningAction { Keep, Remove, Set }

/**
 * A change of the settings for [OwnerNode.previewSettings]: a null field (an empty [shares]) is kept.
 * Dollars are whole dollars and 0 is off; [underpaymentToleranceBps] is in basis points and
 * [heartbeatMaxAgeSecs] in seconds. [lightningText] is read only when [lightning] is Set.
 */
data class SettingsEditArgs(
    /** Today's price of one bitcoin in whole dollars; a changed limit needs it. */
    val usdPerBtc: Long? = null,
    val lightPerSale: Long? = null,
    val lightBudget: Long? = null,
    val nodePerSale: Long? = null,
    val nodeBudget: Long? = null,
    val boxPerSale: Long? = null,
    val boxBudget: Long? = null,
    val underpaymentToleranceBps: Long? = null,
    val refundCap: Long? = null,
    val refundDailyCap: Long? = null,
    val cashierOverrideDailyCap: Long? = null,
    val managerOverrideCeiling: Long? = null,
    val minConfirmations: Int? = null,
    val finalConfirmations: Int? = null,
    val heartbeatMaxAgeSecs: Long? = null,
    val torOn: Boolean? = null,
    val customerWifi: Boolean? = null,
    val minSaleSats: Long? = null,
    val shares: List<CounterShare> = emptyList(),
    val lightning: LightningAction = LightningAction.Keep,
    val lightningText: String = "",
)

/** One changed field in the words of a screen; [counter] is set for a counter's share. */
class SettingChange(
    val field: String,
    val label: String,
    val counter: ByteArray?,
    val old: String,
    val new: String,
    val loosening: Boolean,
)

/** What [OwnerNode.previewSettings] found; [settings] are the bytes [OwnerNode.queueSettings] takes. */
class SettingsPreview(
    val version: Long,
    val changes: List<SettingChange>,
    val needsKeyApp: Boolean,
    val nothingChanged: Boolean,
    val sharesTotalBps: Long,
    /** The price of a bitcoin in dollars the changed limits were made at, when any was. */
    val usdPerBtc: Long?,
    /** The satoshis to a dollar that price gives. */
    val satsPerUsd: Long?,
    val settings: ByteArray,
)

enum class OutboxKind { Transaction, Batch, Settings, Pairing, BoxWelcome }

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

/**
 * A counter's request that was queued: [sessionWords] are the four words the
 * counter and the Key app show, [needsRate] says the phone holds no settings
 * yet (confirming then needs a price), [windowSecs] is how long the counter
 * waits for its code, and [shares] are the counters' shares once it is added.
 * [name] is text from the counter.
 */
class CounterScan(
    val pairing: Long,
    val counter: ByteArray,
    val name: String,
    val sessionWords: List<String>,
    val needsRate: Boolean,
    val windowSecs: Long,
    val shares: List<CounterShare>,
    val words: Words,
)

/** The settings and the first batch queued for a counter. */
class CounterQueued(
    val settings: Long,
    val version: Long,
    val batch: Long,
    val batchCount: Long,
    val shares: List<CounterShare>,
    val words: Words,
)

/** A counter whose finished pairing waits to be handed over. [name] is text from the counter. */
class WaitingCounter(val counter: ByteArray, val name: String)

/** Where a Box is: `host:port` on the local network, or a v3 `.onion` name and port through [proxy]. */
class BoxLink(val address: String, val proxy: String)

/** The claim a Box offered. [needsRate] says the phone holds no settings, so queuing needs today's price. */
class BoxFound(val claim: ByteArray, val boxKey: ByteArray, val needsRate: Boolean)

/** A Box's claim and the settings that add it, queued. [words] are the four words of the Box's key. */
class BoxQueued(
    val claimId: Long,
    val settingsId: Long,
    val settingsVersion: Long,
    val boxKey: ByteArray,
    val words: List<String>,
)

/** A Box whose welcome the Key app approved and that waits to be handed it. */
class WaitingBox(val boxKey: ByteArray)

/** A Box with a claim queued or a welcome waiting. [welcomeReady] says the welcome can be handed over; [words] are the four words of its key. */
class PendingBox(val boxKey: ByteArray, val words: List<String>, val welcomeReady: Boolean)

/** A Box that took its welcome: the link is made for [boxKey] and these [counters]. */
class BoxDelivered(val boxKey: ByteArray, val counters: List<ByteArray>, val settingsVersion: Long)

/** An invitation this phone made for itself, with no Key app (BX-17). */
class BoxOwnInvitation(val text: String, val expiresAt: Long)

/** A claim that answers this phone's own invitation. */
class BoxOwnCheck(val boxKey: ByteArray, val words: List<String>)

/** The code a counter takes: [code] is the bundle to show, [withBatch] says the first addresses are in it. */
class CounterBundle(val code: ByteArray, val withBatch: Boolean, val words: Words)

enum class PosVendor { Square, Clover }

/** The owner's settings for the till: the vendor, its location id, the names of the Bitcoin tender and the shop's currencies. */
class PosSetup(
    val vendor: PosVendor,
    val location: String,
    val tenderNames: List<String>,
    val currencies: List<String>,
)

enum class PosMethod { Get, Post }

/** The request to send to the till: no sign-in in it; the app adds its own. */
class PosRequest(val method: PosMethod, val url: String, val body: ByteArray)

enum class PosAlertKind {
    TenderWithoutPayment,
    ClosedBeforePaid,
    PaymentWithoutTender,
    AmountDiffers,
    TenderOnUnpaidSale,
    VoidedAfterPayment,
    DuplicateReference,
    BitcoinTip,
    UnknownCurrency,
}

/** What a till alert is about. */
sealed class PosSubject {
    class Tender(val order: String, val tender: String) : PosSubject()

    class Sale(val counter: ByteArray, val index: Long) : PosSubject()
}

/** An alert's identity, as `posState` showed it. */
class PosAlertKey(val kind: PosAlertKind, val subject: PosSubject)

class PosAlert(val key: PosAlertKey, val raisedAt: Long, val words: Words)

class PosReply(
    val again: Boolean,
    val moreLeft: Boolean,
    val pages: Long,
    val ordersRead: Long,
    val newMatches: Long,
    val newAlerts: List<PosAlert>,
    val cleared: Long,
    val retired: Long,
)

class PosState(
    val setup: PosSetup?,
    val lastPollAt: Long?,
    val openAlerts: List<PosAlert>,
    val acknowledgedAlerts: Long,
    val trackedOrders: Long,
    val matches: Long,
    val matchesByOrder: Long,
    val retiredMatches: Long,
)


// ---- the key ceremony on this phone (ops 80 to 86, no node) --------------------------------

/** What [OwnerCeremony.makeKey1] is given. By default key 1 has no words to show. */
class Key1Args(val network: OwnerNetwork, val showWords: Boolean)

/** A key's public parts, for a screen. */
class PartView(val fingerprint: String, val originPath: String, val accountXpub: String)

/**
 * Key 1 made. [record] is the sealed record at stage 1: it holds secrets, and the app seals it before
 * it shows the next screen. [words] are key 1's 24 words, only when asked for.
 */
class Key1Made(
    val record: ByteArray,
    val key1: PartView,
    val ownerRoot: ByteArray,
    val ownerRootWords: List<String>,
    val words: List<String>?,
    val stateWords: Words,
)

/** The Key app's answer, checked. [key2] is null when the Key app holds none. */
class AnswerView(
    val network: OwnerNetwork,
    val key2: PartView?,
    val key3: PartView,
    val keyAppRoot: ByteArray,
    val keyAppRootWords: List<String>,
)

/** What [OwnerCeremony.buildWallet] is given. [key2Text] only when the answer holds no key 2. */
class BuildArgs(val record: ByteArray, val answer: ByteArray, val key2Text: String?, val batchMinCount: Long)

/** One pair of keys and who holds each: 1 the owner's phone, 2 the Key app, 3 another signer, 4 another wallet app. */
class PairView(val keyA: Long, val keyB: Long, val holderA: Int, val holderB: Int)

/**
 * The wallet built. [record] is the sealed record at stage 2. [walletSetup] is the bytes the Key app takes,
 * to show as a code. The checksum words, the fingerprints and the first address are for the owner to compare
 * with the Key app's.
 */
class WalletBuiltView(
    val record: ByteArray,
    val walletSetup: ByteArray,
    val description: String,
    val checksumWords: List<String>,
    val fingerprints: List<String>,
    val firstReceiveAddress: String,
    val pairs: List<PairView>,
    val pairsWords: Words,
    val stateWords: Words,
)

/** What [OwnerCeremony.finish] is given. */
class FinishArgs(val record: ByteArray, val refundDepth: Long, val birthday: Long)

/** The setup finished: [record] is the sealed record at stage 3. */
class FinishedView(val record: ByteArray, val stateWords: Words)

/** The values `start` is made from. They hold the owner's secret and key 1's account key. */
class SetupFields(
    val network: OwnerNetwork,
    val ownerSecret: ByteArray,
    val approverRoot: ByteArray,
    val wallet: String,
    val key1Account: ByteArray,
    val key3Xpub: String?,
    val batchMinCount: Long,
    val refundDepth: Long,
    val birthday: Long,
)

/** Where the setup is: 1 key 1 made, 2 wallet built and not yet confirmed, 3 finished. */
class SetupStageView(val stage: Int, val words: Words)

/** The values the last two steps are given besides the owner's word, as the library holds them. */
class CeremonyDefaults(val batchMinCount: Long, val refundDepth: Long, val birthday: Long)

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
            AlertKind.BatchUndelivered -> TAG_ALERT_KIND_BATCH_UNDELIVERED
        },
    )

private fun Reader.alertKind(): AlertKind =
    when (u8()) {
        TAG_ALERT_KIND_MISSED_CHECK_IN -> AlertKind.MissedCheckIn
        TAG_ALERT_KIND_DIVERGED -> AlertKind.Diverged
        TAG_ALERT_KIND_BATCH_UNDELIVERED -> AlertKind.BatchUndelivered
        else -> tag("AlertKind")
    }

private fun Writer.posSetup(v: PosSetup): Writer =
    u8(
        when (v.vendor) {
            PosVendor.Square -> TAG_POS_VENDOR_VIEW_SQUARE
            PosVendor.Clover -> TAG_POS_VENDOR_VIEW_CLOVER
        },
    ).string(v.location).list(v.tenderNames) { string(it) }.list(v.currencies) { string(it) }

private fun Writer.posAlertKind(v: PosAlertKind): Writer =
    u8(
        when (v) {
            PosAlertKind.TenderWithoutPayment -> TAG_POS_ALERT_KIND_TENDER_WITHOUT_PAYMENT
            PosAlertKind.ClosedBeforePaid -> TAG_POS_ALERT_KIND_CLOSED_BEFORE_PAID
            PosAlertKind.PaymentWithoutTender -> TAG_POS_ALERT_KIND_PAYMENT_WITHOUT_TENDER
            PosAlertKind.AmountDiffers -> TAG_POS_ALERT_KIND_AMOUNT_DIFFERS
            PosAlertKind.TenderOnUnpaidSale -> TAG_POS_ALERT_KIND_TENDER_ON_UNPAID_SALE
            PosAlertKind.VoidedAfterPayment -> TAG_POS_ALERT_KIND_VOIDED_AFTER_PAYMENT
            PosAlertKind.DuplicateReference -> TAG_POS_ALERT_KIND_DUPLICATE_REFERENCE
            PosAlertKind.BitcoinTip -> TAG_POS_ALERT_KIND_BITCOIN_TIP
            PosAlertKind.UnknownCurrency -> TAG_POS_ALERT_KIND_UNKNOWN_CURRENCY
        },
    )

private fun Writer.posAlertKey(v: PosAlertKey): Writer {
    posAlertKind(v.kind)
    when (val subject = v.subject) {
        is PosSubject.Tender ->
            u8(TAG_POS_SUBJECT_VIEW_TENDER).string(subject.order).string(subject.tender)
        is PosSubject.Sale -> u8(TAG_POS_SUBJECT_VIEW_SALE).bytes(subject.counter).u32(subject.index)
    }
    return this
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

private fun Reader.posVendor(): PosVendor =
    when (u8()) {
        TAG_POS_VENDOR_VIEW_SQUARE -> PosVendor.Square
        TAG_POS_VENDOR_VIEW_CLOVER -> PosVendor.Clover
        else -> tag("PosVendorView")
    }

private fun Reader.posMethod(): PosMethod =
    when (u8()) {
        TAG_POS_METHOD_VIEW_GET -> PosMethod.Get
        TAG_POS_METHOD_VIEW_POST -> PosMethod.Post
        else -> tag("PosMethodView")
    }

private fun Reader.posAlertKind(): PosAlertKind =
    when (u8()) {
        TAG_POS_ALERT_KIND_TENDER_WITHOUT_PAYMENT -> PosAlertKind.TenderWithoutPayment
        TAG_POS_ALERT_KIND_CLOSED_BEFORE_PAID -> PosAlertKind.ClosedBeforePaid
        TAG_POS_ALERT_KIND_PAYMENT_WITHOUT_TENDER -> PosAlertKind.PaymentWithoutTender
        TAG_POS_ALERT_KIND_AMOUNT_DIFFERS -> PosAlertKind.AmountDiffers
        TAG_POS_ALERT_KIND_TENDER_ON_UNPAID_SALE -> PosAlertKind.TenderOnUnpaidSale
        TAG_POS_ALERT_KIND_VOIDED_AFTER_PAYMENT -> PosAlertKind.VoidedAfterPayment
        TAG_POS_ALERT_KIND_DUPLICATE_REFERENCE -> PosAlertKind.DuplicateReference
        TAG_POS_ALERT_KIND_BITCOIN_TIP -> PosAlertKind.BitcoinTip
        TAG_POS_ALERT_KIND_UNKNOWN_CURRENCY -> PosAlertKind.UnknownCurrency
        else -> tag("PosAlertKind")
    }

private fun Reader.posSubject(): PosSubject =
    when (u8()) {
        TAG_POS_SUBJECT_VIEW_TENDER -> PosSubject.Tender(string(), string())
        TAG_POS_SUBJECT_VIEW_SALE -> PosSubject.Sale(bytes(), u32())
        else -> tag("PosSubjectView")
    }

private fun Reader.posSetup(): PosSetup = PosSetup(posVendor(), string(), strings(), strings())

private fun Reader.posAlert(): PosAlert =
    PosAlert(PosAlertKey(posAlertKind(), posSubject()), u64(), words())

internal fun Reader.posRequest(): PosRequest = PosRequest(posMethod(), string(), bytes())

internal fun Reader.posReply(): PosReply =
    PosReply(bool(), bool(), u32(), u32(), u32(), list { posAlert() }, u32(), u64())

internal fun Reader.posState(): PosState =
    PosState(
        option { posSetup() }, option { u64() }, list { posAlert() }, u32(), u32(), u32(), u32(),
        u64(),
    )

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

private fun Reader.signerLine(): SignerLine = SignerLine(string(), u64())

private fun Reader.signerFile(): SignerFile =
    SignerFile(
        bytes(), bytes(), string(), list { signerLine() }, option { signerLine() }, u64(), words(),
    )

private fun Reader.firstAddressKind(): Boolean =
    when (u8()) {
        TAG_FIRST_ADDRESS_KIND_CHECKED -> true
        TAG_FIRST_ADDRESS_KIND_SKIPPED -> false
        else -> tag("FirstAddressKind")
    }

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

private fun Reader.batchWaiting(): BatchWaiting = BatchWaiting(bytes(), u32(), u64(), words())

internal fun Reader.linkState(): LinkState =
    LinkState(bool(), u64(), list { alert() }, list { counterState() }, list { batchWaiting() })

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
        TAG_OUTBOX_KIND_BOX_WELCOME -> OutboxKind.BoxWelcome
        else -> tag("OutboxKind")
    }

private fun Reader.limit(): Limit = Limit(u64(), u32())

internal fun Reader.counterScan(): CounterScan =
    CounterScan(
        u32(), bytes(), string(), strings(), bool(), u32(),
        list { CounterShare(bytes(), u32()) }, words(),
    )

internal fun Reader.counterQueued(): CounterQueued =
    CounterQueued(u32(), u32(), u32(), u32(), list { CounterShare(bytes(), u32()) }, words())

internal fun Reader.waitingCounter(): WaitingCounter = WaitingCounter(bytes(), string())

internal fun Reader.boxFound(): BoxFound = BoxFound(bytes(), bytes(), bool())

internal fun Reader.boxQueued(): BoxQueued = BoxQueued(u32(), u32(), u32(), bytes(), strings())

internal fun Reader.waitingBox(): WaitingBox = WaitingBox(bytes())

internal fun Reader.pendingBox(): PendingBox = PendingBox(bytes(), strings(), bool())

internal fun Reader.boxDelivered(): BoxDelivered = BoxDelivered(bytes(), list { bytes() }, u32())

internal fun Reader.counterBundle(): CounterBundle = CounterBundle(bytes(), bool(), words())

internal fun Reader.settingsHeld(): SettingsHeld =
    SettingsHeld(
        u32(), limit(), limit(), limit(), limit(), limit(), limit(), u32(),
        limit(), limit(), limit(), limit(), u8(), u8(), u32(), bool(), bool(), u64(),
        list { bytes() }, list { CounterShare(bytes(), u32()) }, list { bytes() },
        option { bytes() },
    )

private fun Reader.settingChange(): SettingChange =
    SettingChange(string(), string(), option { bytes() }, string(), string(), bool())

internal fun Reader.settingsPreview(): SettingsPreview =
    SettingsPreview(
        u32(), list { settingChange() }, bool(), bool(), u32(), option { u64() }, option { u64() }, bytes(),
    )

private fun Writer.lightningAction(v: LightningAction): Writer =
    u8(
        when (v) {
            LightningAction.Keep -> TAG_LIGHTNING_ACTION_KEEP
            LightningAction.Remove -> TAG_LIGHTNING_ACTION_REMOVE
            LightningAction.Set -> TAG_LIGHTNING_ACTION_SET
        },
    )

internal fun Writer.settingsEdit(a: SettingsEditArgs): Writer {
    option(a.usdPerBtc) { u64(it) }
    for (usd in listOf(
        a.lightPerSale, a.lightBudget, a.nodePerSale, a.nodeBudget, a.boxPerSale, a.boxBudget,
    )) {
        option(usd) { u32(it) }
    }
    option(a.underpaymentToleranceBps) { u32(it) }
    for (usd in listOf(
        a.refundCap, a.refundDailyCap, a.cashierOverrideDailyCap, a.managerOverrideCeiling,
    )) {
        option(usd) { u32(it) }
    }
    option(a.minConfirmations) { u8(it) }
    option(a.finalConfirmations) { u8(it) }
    option(a.heartbeatMaxAgeSecs) { u32(it) }
    option(a.torOn) { bool(it) }
    option(a.customerWifi) { bool(it) }
    option(a.minSaleSats) { u64(it) }
    list(a.shares) { bytes(it.counter).u32(it.bps) }
    return lightningAction(a.lightning).string(a.lightningText)
}

internal fun Reader.outboxRow(): OutboxRow =
    OutboxRow(
        u32(), outboxKind(), option { string() }, option { bytes() }, option { u32() },
        option { u64() }, u32(), words(),
    )


private fun Reader.network(): OwnerNetwork =
    when (u8()) {
        TAG_OWNER_NETWORK_MAINNET -> OwnerNetwork.Mainnet
        TAG_OWNER_NETWORK_TESTNET -> OwnerNetwork.Testnet
        TAG_OWNER_NETWORK_SIGNET -> OwnerNetwork.Signet
        TAG_OWNER_NETWORK_REGTEST -> OwnerNetwork.Regtest
        else -> tag("OwnerNetwork")
    }

private fun Reader.partView(): PartView = PartView(string(), string(), string())

internal fun Reader.key1Made(): Key1Made =
    Key1Made(bytes(), partView(), bytes(), strings(), option { strings() }, words())

internal fun Reader.answerView(): AnswerView =
    AnswerView(network(), option { partView() }, partView(), bytes(), strings())

internal fun Reader.walletBuiltView(): WalletBuiltView =
    WalletBuiltView(
        bytes(), bytes(), string(), strings(), strings(), string(),
        list { PairView(u32(), u32(), u8(), u8()) }, words(), words(),
    )

internal fun Reader.finishedView(): FinishedView = FinishedView(bytes(), words())

internal fun Reader.setupFields(): SetupFields =
    SetupFields(network(), bytes(), bytes(), string(), bytes(), option { string() }, u32(), u32(), u32())

internal fun Reader.setupStageView(): SetupStageView = SetupStageView(u8(), words())

internal fun Reader.ceremonyDefaults(): CeremonyDefaults = CeremonyDefaults(u32(), u32(), u32())

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

    /** AS-97: the wallet in the forms a hardware signer takes. */
    fun signerRegistration(shop: String): Registration {
        val r = call(OP_SIGNER_REGISTRATION, handle) { it.string(shop) }
        return Registration(
            r.string(), r.string(), r.string(), r.string(), r.string(), r.bytes(), r.string(),
            r.bool(), r.bool(),
        ).also { r.end() }
    }

    /** AS-90: build the approval for the Key app, with the spends apart for the signer. */
    fun signerBuild(now: Long, feeRate: Long): SignerBuilt {
        val r = call(OP_SIGNER_BUILD, handle) { it.u64(now).u64(feeRate) }
        return SignerBuilt(r.built(), r.option { r.bytes() }).also { r.end() }
    }

    /** AS-91: sign item [number] of the spends' session with key 1 and make the signer's file. */
    fun signerExport(now: Long, session: ByteArray, number: Long): SignerFile {
        val r = call(OP_SIGNER_EXPORT, handle) { it.u64(now).bytes(session).u32(number) }
        return r.signerFile().also { r.end() }
    }

    /** AS-92: the signer's file again, as it was made. */
    fun signerFile(session: ByteArray, number: Long): SignerFile {
        val r = call(OP_SIGNER_FILE, handle) { it.bytes(session).u32(number) }
        return r.signerFile().also { r.end() }
    }

    /** AS-93 to AS-96: take what the signer returned (a PSBT or a finished transaction, as bytes). */
    fun signerTake(now: Long, session: ByteArray, number: Long, returned: ByteArray): SignerTaken {
        val r = call(OP_SIGNER_TAKE, handle) {
            it.u64(now).bytes(session).u32(number).bytes(returned)
        }
        return SignerTaken(r.u32(), r.bytes(), r.u64(), r.list { r.sendRow() }).also { r.end() }
    }

    /** AS-99: the record of the first address check, to store. */
    fun firstAddressRecord(checked: Boolean, at: Long): ByteArray {
        val r = call(OP_FIRST_ADDRESS_RECORD, handle) {
            it.u8(if (checked) TAG_FIRST_ADDRESS_KIND_CHECKED else TAG_FIRST_ADDRESS_KIND_SKIPPED)
                .u64(at)
        }
        return r.bytes().also { r.end() }
    }

    /** AS-99: read the stored record back. */
    fun firstAddressRead(record: ByteArray): FirstAddress {
        val r = call(OP_FIRST_ADDRESS_READ, handle) { it.bytes(record) }
        return FirstAddress(r.firstAddressKind(), r.u64()).also { r.end() }
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

    /**
     * What a change of the settings would do: each changed field in words, whether the Key app must
     * co-sign, and the bytes [queueSettings] takes. Nothing is stored.
     */
    fun previewSettings(edit: SettingsEditArgs): SettingsPreview {
        val r = call(OP_SETTINGS_PREVIEW, handle) { it.settingsEdit(edit) }
        return r.settingsPreview().also { r.end() }
    }

    /**
     * Read a counter's request (the bytes of its code) and queue its pairing; the words and the
     * shares come back for the owner to check before [counterConfirm].
     */
    fun counterScan(request: ByteArray): CounterScan {
        val r = call(OP_COUNTER_SCAN, handle) { it.bytes(request) }
        return r.counterScan().also { r.end() }
    }

    /**
     * Queue the settings that add the counter and its first batch. [usdPerBtc], today's price of one
     * bitcoin in whole dollars, starts the settings when the phone holds none, and is ignored when it does.
     */
    fun counterConfirm(now: Long, pairing: Long, usdPerBtc: Long): CounterQueued {
        val r = call(OP_COUNTER_CONFIRM, handle) { it.u64(now).u32(pairing).u64(usdPerBtc) }
        return r.counterQueued().also { r.end() }
    }

    /** The counters whose approved pairing waits for the counter to take its code. */
    fun countersWaiting(): List<WaitingCounter> {
        val r = call(OP_COUNTERS_WAITING, handle)
        return r.list { r.waitingCounter() }.also { r.end() }
    }

    /** The code the counter takes, from what the Key app approved. Nothing is changed. */
    fun counterBundle(counter: ByteArray): CounterBundle {
        val r = call(OP_COUNTER_BUNDLE, handle) { it.bytes(counter) }
        return r.counterBundle().also { r.end() }
    }

    /** The counter shows that it is paired: what waited for it is marked delivered. */
    fun counterDelivered(counter: ByteArray) =
        callUnit(OP_COUNTER_DELIVERED, handle) { it.bytes(counter) }

    /**
     * CR-132: the owner's signed relay for counters, made from the link's settings, for a counter
     * that was paired before the shop had a Box (the counter takes it with its own `takeRelay`).
     * Refused, with its words, when the link has no relay a counter can reach.
     */
    fun counterRelay(): ByteArray {
        val r = call(OP_COUNTER_RELAY, handle)
        return r.bytes().also { r.end() }
    }

    /**
     * Ask the Box at [link] for its claim. The call is made off the node's thread and takes up to
     * about a minute when the Box does not answer.
     */
    fun boxFetchClaim(link: BoxLink): BoxFound {
        val r = call(OP_BOX_FETCH_CLAIM, handle) { it.string(link.address).string(link.proxy) }
        return r.boxFound().also { r.end() }
    }

    /**
     * Queue the Box's claim and the settings that add it. [usdPerBtc] starts the settings when the
     * phone holds none and is ignored when it does.
     */
    fun boxQueue(claim: ByteArray, usdPerBtc: Long): BoxQueued {
        val r = call(OP_BOX_QUEUE, handle) { it.bytes(claim).u64(usdPerBtc) }
        return r.boxQueued().also { r.end() }
    }

    /** The Boxes whose approved welcome waits to be handed over. */
    fun boxesWaiting(): List<WaitingBox> {
        val r = call(OP_BOXES_WAITING, handle)
        return r.list { r.waitingBox() }.also { r.end() }
    }

    /** The Boxes with a claim queued or a welcome waiting, each once: the ones that can be forgotten. */
    fun boxesPending(): List<PendingBox> {
        val r = call(OP_BOXES_PENDING, handle)
        return r.list { r.pendingBox() }.also { r.end() }
    }

    /**
     * Forget a Box that was not handed its welcome: its queued claim and settings leave the queue in
     * one write. A Box in an open session is refused until the session is abandoned.
     */
    fun boxForget(boxKey: ByteArray) = callUnit(OP_BOX_FORGET, handle) { it.bytes(boxKey) }

    /**
     * Hand the Box its welcome and the settings. Only when the Box took them are they taken out of
     * the outbox; a refusal changes nothing.
     */
    fun boxDeliver(link: BoxLink, boxKey: ByteArray): BoxDelivered {
        val r = call(OP_BOX_DELIVER, handle) { it.string(link.address).string(link.proxy).bytes(boxKey) }
        return r.boxDelivered().also { r.end() }
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

    /** What the till screen shows: the connection, the open alerts and the counts. */
    fun posState(): PosState {
        val r = call(OP_POS_STATE, handle)
        return r.posState().also { r.end() }
    }

    /** Connect the till, or change its settings (the sign-in token is the app's, never the library's). */
    fun posSetup(setup: PosSetup) = callUnit(OP_POS_SETUP, handle) { it.posSetup(setup) }

    /** Take the till off: its settings and everything kept about it are removed. */
    fun posRemove() = callUnit(OP_POS_REMOVE, handle)

    /** The next request to send to the till; the app adds the sign-in and sends it. */
    fun posRequest(now: Long): PosRequest {
        val r = call(OP_POS_REQUEST, handle) { it.u64(now) }
        return r.posRequest().also { r.end() }
    }

    /** Hand over the till's answer to the request just made: its status and the body bytes. */
    fun posReply(now: Long, status: Long, body: ByteArray): PosReply {
        val r = call(OP_POS_REPLY, handle) { it.u64(now).u32(status).bytes(body) }
        return r.posReply().also { r.end() }
    }

    /** Acknowledge an open alert, in this phone's own ledger only. */
    fun posAcknowledge(now: Long, key: PosAlertKey) =
        callUnit(OP_POS_ACKNOWLEDGE, handle) { it.u64(now).posAlertKey(key) }
}

/**
 * The owner phone's part of the key ceremony (`docs/OWNER-COINS-RULES.md`, OC-40 to OC-46). No node is
 * running while it happens, so these calls name no handle. The sealed records the calls return hold secrets:
 * the app seals each before it shows the next screen and hands it back with the next call.
 */
/** Pairing a Box on this phone alone, with no Key app (BX-17). They work on no node. */
object OwnerBoxes {
    /** Make an invitation good for 30 minutes from [now]. The app keeps it in memory only. */
    fun ownInvitation(now: Long): BoxOwnInvitation {
        val r = call(OP_BOX_OWN_INVITATION, 0) { it.u64(now) }
        return BoxOwnInvitation(r.string(), r.u64()).also { r.end() }
    }

    /** Check a Box's claim against this phone's own invitation. */
    fun ownCheck(invitation: String, claim: ByteArray): BoxOwnCheck {
        val r = call(OP_BOX_OWN_CHECK, 0) { it.string(invitation).bytes(claim) }
        return BoxOwnCheck(r.bytes(), r.strings()).also { r.end() }
    }
}

object OwnerCeremony {
    /** Make key 1 and the owner's root on this phone. Takes a moment: the app draws a page first. */
    fun makeKey1(args: Key1Args): Key1Made {
        val r = call(OP_CEREMONY_MAKE_KEY1, 0) { it.network(args.network).bool(args.showWords) }
        return r.key1Made().also { r.end() }
    }

    /** Read the Key app's answer (the bytes of its code) and check it. */
    fun readAnswer(answer: ByteArray): AnswerView {
        val r = call(OP_CEREMONY_READ_ANSWER, 0) { it.bytes(answer) }
        return r.answerView().also { r.end() }
    }

    /** Build the 2-of-3 wallet from key 1, key 2 and key 3. Takes a moment: the app draws a page first. */
    fun buildWallet(args: BuildArgs): WalletBuiltView {
        val r = call(OP_CEREMONY_BUILD_WALLET, 0) {
            it.bytes(args.record).bytes(args.answer).option(args.key2Text) { t -> it.string(t) }.u32(args.batchMinCount)
        }
        return r.walletBuiltView().also { r.end() }
    }

    /** The owner confirmed that the words and fingerprints are the same on both phones. */
    fun finish(args: FinishArgs): FinishedView {
        val r = call(OP_CEREMONY_FINISH, 0) { it.bytes(args.record).u32(args.refundDepth).u32(args.birthday) }
        return r.finishedView().also { r.end() }
    }

    /** The values `start` is made from, out of a finished record. */
    fun open(record: ByteArray): SetupFields {
        val r = call(OP_CEREMONY_OPEN, 0) { it.bytes(record) }
        return r.setupFields().also { r.end() }
    }

    /** Where the setup is, from a record. */
    fun stage(record: ByteArray): SetupStageView {
        val r = call(OP_CEREMONY_STAGE, 0) { it.bytes(record) }
        return r.setupStageView().also { r.end() }
    }

    /** The batch minimum, the refund depth and the first block the last two steps are given, on [network]. */
    fun defaults(network: OwnerNetwork): CeremonyDefaults {
        val r = call(OP_CEREMONY_DEFAULTS, 0) { it.network(network) }
        return r.ceremonyDefaults().also { r.end() }
    }
}

/**
 * The lock on the setup text the app stores (`docs/OWNER-APP-RULES.md` OA-60 to OA-62, `docs/LOCK-RULES.md`).
 * The setup text holds the owner's secret and key 1's account key: the app stores the bytes these calls
 * return (inside the Keystore's seal) and keeps no PIN. No node runs while they are called, so they name no
 * handle. Opening takes about a second (Argon2id): the app draws a page first.
 */
object OwnerLock {
    /** Whether stored bytes are a locked setup. A setup saved before the lock existed is the bare text. */
    fun isLocked(stored: ByteArray): Boolean {
        val r = call(OP_SETUP_LOCK_FORM, 0) { it.bytes(stored) }
        return r.bool().also { r.end() }
    }

    /** The setup text locked under [pin]: 6 digits or more, or a passphrase of 8 characters or more. */
    fun lock(pin: String, text: ByteArray): ByteArray {
        val r = call(OP_SETUP_LOCK, 0) { it.string(pin).bytes(text) }
        return r.bytes().also { r.end() }
    }

    /** The setup text inside [locked]. A wrong PIN is refused with the rule `pin-wrong` and changes nothing. */
    fun unlock(locked: ByteArray, pin: String): ByteArray {
        val r = call(OP_SETUP_UNLOCK, 0) { it.bytes(locked).string(pin) }
        return r.bytes().also { r.end() }
    }

    /** [locked] under a new PIN, in one call; the current PIN is needed. */
    fun changePin(locked: ByteArray, current: String, new: String): ByteArray {
        val r = call(OP_SETUP_CHANGE_PIN, 0) { it.bytes(locked).string(current).string(new) }
        return r.bytes().also { r.end() }
    }
}
