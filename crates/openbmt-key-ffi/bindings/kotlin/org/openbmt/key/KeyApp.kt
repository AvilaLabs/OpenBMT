package org.openbmt.key

import java.io.ByteArrayOutputStream
import java.nio.ByteBuffer
import java.nio.charset.CharacterCodingException
import java.nio.charset.CodingErrorAction

// The binding and OBM9 decoder harness compile and run on the JVM. This does
// not compile or load the Android JNI library or exercise a device UI.

/** A call that did not do what was asked. [rule] is the ID of the rule that refused. */
class KeyException(
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
 * What a person is shown for an error: the fields of the Rust `Words`, in the
 * order the response frame carries them. [tone] is one of the TONE_ numbers and
 * [who] one of the WHO_ numbers below. [at] is in seconds since 1970, for the
 * app to show on the person's own clock.
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

private const val OP_OPEN = 1
private const val OP_CLOSE_APP = 2
private const val OP_CLOSE_SESSION = 3
private const val OP_STATE = 10
private const val OP_CREATE_FROM_ENTROPY = 11
private const val OP_CREATE_APPROVER_ONLY = 12
private const val OP_KEY_WORDS = 13
private const val OP_KEY2_PUBLIC = 14
private const val OP_DEVICE_ROOT = 15
private const val OP_DEVICE_ROOT_WORDS = 16
private const val OP_ACCEPT_WALLET = 17
private const val OP_WALLET_VIEW = 18
private const val OP_PAIR_OFFER_BYTES = 19
private const val OP_CHECK_KIT = 20
private const val OP_OPEN_SESSION = 30
private const val OP_SESSION_ID = 31
private const val OP_LINES = 32
private const val OP_DECIDE = 33
private const val OP_FINISH = 34
private const val OP_CANCEL = 35
private const val OP_SIGNER_STATUS = 40
private const val OP_RECEIVE_MARK = 41
private const val OP_ENROLL_DESTINATION = 42
private const val OP_REMOVE_DESTINATION = 43
private const val OP_SET_CEILINGS = 44
private const val OP_SET_FEE_CEILINGS = 45
private const val OP_RAISE_CHANGE_MARK = 46
private const val OP_RAISE_RECEIVE_MARK = 47
private const val OP_CORRECT_CLOCK = 48
private const val OP_COIN_BITS = 60
private const val OP_DICE_BITS = 61
private const val OP_DICE_FACE_COUNTS = 62
private const val OP_WORD_SUGGESTIONS = 63
private const val OP_WORD_IS_VALID = 64
private const val OP_PARSE_DEPOSIT_CODE = 65
private const val OP_TAKE_KEY3 = 80
private const val OP_KEY3_VIEW = 81
private const val OP_PUBLIC_ANSWER = 82
private const val OP_ACCEPT_WALLET_SETUP = 83
private const val OP_CONFIRM_WALLET = 84
private const val OP_DISCARD_WALLET = 85

// ---- tag bytes of the enums (PROTOCOL.md) ------------------------------------------

private const val TAG_KEY_NETWORK_MAINNET = 0
private const val TAG_KEY_NETWORK_TESTNET = 1
private const val TAG_KEY_NETWORK_SIGNET = 2
private const val TAG_KEY_NETWORK_REGTEST = 3
private const val TAG_KEY_WORD_COUNT_TWELVE = 0
private const val TAG_KEY_WORD_COUNT_TWENTY_FOUR = 1
private const val TAG_ENTROPY_SOURCE_PLATFORM = 0
private const val TAG_ENTROPY_SOURCE_DICE_HASHED = 1
private const val TAG_ENTROPY_SOURCE_COIN_FLIPS = 2
private const val TAG_ENTROPY_SOURCE_DICE_SKEW_CORRECTED = 3
private const val TAG_ENTROPY_SOURCE_TYPED_WORDS = 4
private const val TAG_SETUP_STATE_EMPTY = 0
private const val TAG_SETUP_STATE_HAS_IDENTITY = 1
private const val TAG_SETUP_STATE_READY = 2
private const val TAG_SETUP_STATE_WALLET_PENDING = 3
private const val TAG_KIT_CHECK_SAME = 0
private const val TAG_KIT_CHECK_DIFFERENT = 1
private const val TAG_DECISION_APPROVE = 0
private const val TAG_DECISION_DECLINE = 1
private const val TAG_LINE_KIND_SPEND = 0
private const val TAG_LINE_KIND_BATCH_REFILL = 1
private const val TAG_LINE_KIND_SETTINGS = 2
private const val TAG_LINE_KIND_PAIRING = 3
private const val TAG_LINE_KIND_HEALTH_CHECK = 4
private const val TAG_LINE_DETAIL_SPEND = 0
private const val TAG_LINE_DETAIL_BATCH = 1
private const val TAG_LINE_DETAIL_SETTINGS = 2
private const val TAG_LINE_DETAIL_PAIRING = 3
private const val TAG_LINE_DETAIL_HEALTH = 4
private const val TAG_OUTPUT_KIND_VIEW_CHANGE = 0
private const val TAG_OUTPUT_KIND_VIEW_ALLOWLISTED = 1
private const val TAG_OUTPUT_KIND_VIEW_OTHER = 2
private const val TAG_OWNER_LABEL_KIND_REFUND = 0
private const val TAG_OWNER_LABEL_KIND_CASH_OUT = 1
private const val TAG_OWNER_LABEL_KIND_CHANGE = 2
private const val TAG_CHECK_STATUS_PASSED = 0
private const val TAG_CHECK_STATUS_FAILED = 1
private const val TAG_CHECK_STATUS_NOT_CHECKED = 2
private const val TAG_DIRECTION_TIGHTENING = 0
private const val TAG_DIRECTION_LOOSENING = 1

// ---- the views ----------------------------------------------------------------------
//
// A Rust u32 is a Long here and a u64 is a Long as well: a u64 above
// 2^63 - 1 reads as negative (no value of this API gets near it).

enum class KeyNetwork { Mainnet, Testnet, Signet, Regtest }

enum class KeyWordCount { Twelve, TwentyFour }

sealed class EntropySource {
    object Platform : EntropySource()
    class DiceHashed(val rolls: String) : EntropySource()
    class CoinFlips(val heads: List<Boolean>, val words: KeyWordCount) : EntropySource()
    class DiceSkewCorrected(val rolls: String, val words: KeyWordCount) : EntropySource()
    class TypedWords(val words: List<String>) : EntropySource()
}

sealed class SetupState {
    object Empty : SetupState()
    data class HasIdentity(val holdsKey2: Boolean, val wordsWaiting: Boolean) : SetupState()
    data class Ready(val holdsKey2: Boolean) : SetupState()

    /** A wallet setup passed every check and is kept, not registered, until confirmed or discarded. */
    data class WalletPending(val holdsKey2: Boolean) : SetupState()
}

data class Key2Public(
    val network: KeyNetwork,
    val masterFingerprint: String,
    val originPath: String,
    val accountXpub: String,
)

/** Key 3's public parts as the Key app keeps them, for the comparison screen. */
data class Key3View(
    val mainnet: Boolean,
    val masterFingerprint: String,
    val originPath: String,
    val accountXpub: String,
)

data class SlotView(val index: Long, val fingerprint: String, val xpub: String)

class WalletView(
    val checksumWords: List<String>,
    val walletId: ByteArray,
    val slots: List<SlotView>,
    val firstReceiveAddress: String,
    val ownRootWords: List<String>,
    val ownerRootWords: List<String>,
)

class WalletSetup(
    val description: String,
    val network: KeyNetwork,
    val ownerRoot: ByteArray,
    val key3Xpub: String?,
    val batchMinCount: Long?,
)

sealed class KitCheck {
    object Same : KitCheck()
    data class Different(
        val firstDifference: Long,
        val sameChecksumWords: Boolean,
        val parses: Boolean,
    ) : KitCheck()
}

sealed class Decision {
    object Approve : Decision()
    data class Decline(val reason: String) : Decision()
}

enum class LineKind { Spend, BatchRefill, Settings, Pairing, HealthCheck }

enum class CheckStatus { Passed, Failed, NotChecked }

enum class Direction { Tightening, Loosening }

enum class OwnerLabelKind { Refund, CashOut, Change }

data class CheckView(val name: String, val status: CheckStatus)

sealed class OutputKindView {
    data class Change(val index: Long) : OutputKindView()
    data class Allowlisted(val label: String) : OutputKindView()
    object Other : OutputKindView()
}

class OwnerLabelView(
    val isClaim: Boolean,
    val kind: OwnerLabelKind,
    val counter: ByteArray?,
    val saleIndex: Long?,
    val cashier: String?,
    val shopValueMinor: Long?,
    val shopCurrency: String?,
    val destination: String?,
)

class OutputView(
    val index: Long,
    val address: String?,
    val scriptHex: String,
    val amountSats: Long,
    val kind: OutputKindView,
    val ownerPhoneLabel: OwnerLabelView?,
)

data class CeilingsRemaining(
    val otherPerTx: Long,
    val otherDay: Long,
    val otherWeek: Long,
    val allowlistedPerTx: Long,
    val allowlistedDay: Long,
    val allowlistedWeek: Long,
)

class SpendReviewView(
    val outputs: List<OutputView>,
    val changeIndex: Long?,
    val changeSats: Long?,
    val movesCoinsInsideWallet: Boolean,
    val feeSats: Long,
    val feeRateSatPerVb: Long,
    val inputCount: Long,
    val inputTotalSats: Long,
    val otherTotalSats: Long,
    val allowlistedTotalSats: Long,
    val remaining: CeilingsRemaining,
    val alreadyCounted: Boolean,
    val clockBehindBy: Long?,
)

class SpendDetail(
    val review: SpendReviewView?,
    val outputsWithoutLabel: List<Long>,
    val labelsWithoutOutput: List<Long>,
    val claimedChangeNotProven: List<Long>,
)

class BatchDetail(
    val counter: ByteArray,
    val counterWords: List<String>,
    val start: Long,
    val end: Long?,
    val count: Long,
    val receiveMark: Long,
    val checks: List<CheckView>,
)

data class SettingChangeView(
    val field: String,
    val old: String?,
    val new: String,
    val direction: Direction,
)

class SettingsDetail(
    val newVersion: Long,
    val lastVersion: Long?,
    val changes: List<SettingChangeView>,
    val overall: Direction?,
    val checks: List<CheckView>,
)

class PairingDetail(
    val name: String,
    val counterKey: ByteArray,
    val sessionWords: List<String>?,
    val claimedSettingsVersion: Long,
    val settingsVersionToSign: Long,
)

class HealthDetail(val nonce: ByteArray)

sealed class LineDetail {
    class Spend(val detail: SpendDetail) : LineDetail()
    class Batch(val detail: BatchDetail) : LineDetail()
    class Settings(val detail: SettingsDetail) : LineDetail()
    class Pairing(val detail: PairingDetail) : LineDetail()
    class Health(val detail: HealthDetail) : LineDetail()
}

class LineView(
    val item: Long,
    val kind: LineKind,
    val title: String,
    val canApprove: Boolean,
    val failure: String?,
    val decision: Decision?,
    val detail: LineDetail,
)

data class CeilingsView(val perTx: Long, val perDay: Long, val perWeek: Long)

data class FeeCeilingsView(val satPerVb: Long, val absoluteSats: Long)

class AllowlistView(
    val address: String?,
    val scriptHex: String,
    val label: String,
    val enrolledAt: Long,
    val activeAt: Long?,
)

class SignerStatus(
    val ceilingsOther: CeilingsView,
    val ceilingsAllowlisted: CeilingsView,
    val feeCeilings: FeeCeilingsView,
    val maxFeeRateSatPerVb: Long,
    val maxFeeSats: Long,
    val allowlistWaitSecs: Long,
    val receiveMark: Long,
    val changeMark: Long,
    val latestTimeSeen: Long,
    val effectiveTimeNow: Long,
    val otherDay: Long,
    val otherWeek: Long,
    val allowlistedDay: Long,
    val allowlistedWeek: Long,
    val spendLogCount: Long,
    val allowlist: List<AllowlistView>,
)

data class BitProgress(val have: Long, val stillNeeded: Long)

data class DepositCode(
    val address: String,
    val amountSats: Long?,
    val label: String?,
    val ignored: List<String>,
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
        throw KeyException("response-malformed", "the library's response is not readable: $what", 4)

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

    /** A string with a u16 length, as in an error frame. */
    fun string16(): String = utf8(take(u16()))

    /** A response status, including success, as defined by OBM9. */
    fun status(): Int = u8().also { if (it !in 0..4) bad("a status is not 0 to 4") }

    /** The words after the rule and the text of an error frame. */
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
}

private fun Writer.network(v: KeyNetwork): Writer =
    u8(
        when (v) {
            KeyNetwork.Mainnet -> TAG_KEY_NETWORK_MAINNET
            KeyNetwork.Testnet -> TAG_KEY_NETWORK_TESTNET
            KeyNetwork.Signet -> TAG_KEY_NETWORK_SIGNET
            KeyNetwork.Regtest -> TAG_KEY_NETWORK_REGTEST
        },
    )

private fun Writer.wordCount(v: KeyWordCount): Writer =
    u8(
        when (v) {
            KeyWordCount.Twelve -> TAG_KEY_WORD_COUNT_TWELVE
            KeyWordCount.TwentyFour -> TAG_KEY_WORD_COUNT_TWENTY_FOUR
        },
    )

private fun Writer.source(v: EntropySource): Writer {
    when (v) {
        is EntropySource.Platform -> u8(TAG_ENTROPY_SOURCE_PLATFORM)
        is EntropySource.DiceHashed -> u8(TAG_ENTROPY_SOURCE_DICE_HASHED).string(v.rolls)
        is EntropySource.CoinFlips -> {
            u8(TAG_ENTROPY_SOURCE_COIN_FLIPS)
            list(v.heads) { bool(it) }
            wordCount(v.words)
        }
        is EntropySource.DiceSkewCorrected -> {
            u8(TAG_ENTROPY_SOURCE_DICE_SKEW_CORRECTED).string(v.rolls)
            wordCount(v.words)
        }
        is EntropySource.TypedWords -> {
            u8(TAG_ENTROPY_SOURCE_TYPED_WORDS)
            list(v.words) { string(it) }
        }
    }
    return this
}

private fun Writer.decision(v: Decision): Writer {
    when (v) {
        is Decision.Approve -> u8(TAG_DECISION_APPROVE)
        is Decision.Decline -> u8(TAG_DECISION_DECLINE).string(v.reason)
    }
    return this
}

private fun Writer.ceilings(v: CeilingsView): Writer = u64(v.perTx).u64(v.perDay).u64(v.perWeek)

private fun Reader.network(): KeyNetwork =
    when (u8()) {
        TAG_KEY_NETWORK_MAINNET -> KeyNetwork.Mainnet
        TAG_KEY_NETWORK_TESTNET -> KeyNetwork.Testnet
        TAG_KEY_NETWORK_SIGNET -> KeyNetwork.Signet
        TAG_KEY_NETWORK_REGTEST -> KeyNetwork.Regtest
        else -> tag("KeyNetwork")
    }

internal fun Reader.setupState(): SetupState =
    when (u8()) {
        TAG_SETUP_STATE_EMPTY -> SetupState.Empty
        TAG_SETUP_STATE_HAS_IDENTITY -> SetupState.HasIdentity(bool(), bool())
        TAG_SETUP_STATE_READY -> SetupState.Ready(bool())
        TAG_SETUP_STATE_WALLET_PENDING -> SetupState.WalletPending(bool())
        else -> tag("SetupState")
    }

private fun Reader.key2Public(): Key2Public =
    Key2Public(network(), string(), string(), string())

internal fun Reader.key3View(): Key3View = Key3View(bool(), string(), string(), string())

internal fun Reader.walletView(): WalletView =
    WalletView(
        checksumWords = strings(),
        walletId = bytes(),
        slots = list { SlotView(u32(), string(), string()) },
        firstReceiveAddress = string(),
        ownRootWords = strings(),
        ownerRootWords = strings(),
    )

private fun Reader.kitCheck(): KitCheck =
    when (u8()) {
        TAG_KIT_CHECK_SAME -> KitCheck.Same
        TAG_KIT_CHECK_DIFFERENT -> KitCheck.Different(u32(), bool(), bool())
        else -> tag("KitCheck")
    }

private fun Reader.decision(): Decision =
    when (u8()) {
        TAG_DECISION_APPROVE -> Decision.Approve
        TAG_DECISION_DECLINE -> Decision.Decline(string())
        else -> tag("Decision")
    }

private fun Reader.direction(): Direction =
    when (u8()) {
        TAG_DIRECTION_TIGHTENING -> Direction.Tightening
        TAG_DIRECTION_LOOSENING -> Direction.Loosening
        else -> tag("Direction")
    }

private fun Reader.checks(): List<CheckView> =
    list {
        CheckView(
            string(),
            when (u8()) {
                TAG_CHECK_STATUS_PASSED -> CheckStatus.Passed
                TAG_CHECK_STATUS_FAILED -> CheckStatus.Failed
                TAG_CHECK_STATUS_NOT_CHECKED -> CheckStatus.NotChecked
                else -> tag("CheckStatus")
            },
        )
    }

private fun Reader.ownerLabel(): OwnerLabelView =
    OwnerLabelView(
        isClaim = bool(),
        kind =
            when (u8()) {
                TAG_OWNER_LABEL_KIND_REFUND -> OwnerLabelKind.Refund
                TAG_OWNER_LABEL_KIND_CASH_OUT -> OwnerLabelKind.CashOut
                TAG_OWNER_LABEL_KIND_CHANGE -> OwnerLabelKind.Change
                else -> tag("OwnerLabelKind")
            },
        counter = option { bytes() },
        saleIndex = option { u32() },
        cashier = option { string() },
        shopValueMinor = option { u64() },
        shopCurrency = option { string() },
        destination = option { string() },
    )

private fun Reader.output(): OutputView =
    OutputView(
        index = u32(),
        address = option { string() },
        scriptHex = string(),
        amountSats = u64(),
        kind =
            when (u8()) {
                TAG_OUTPUT_KIND_VIEW_CHANGE -> OutputKindView.Change(u32())
                TAG_OUTPUT_KIND_VIEW_ALLOWLISTED -> OutputKindView.Allowlisted(string())
                TAG_OUTPUT_KIND_VIEW_OTHER -> OutputKindView.Other
                else -> tag("OutputKindView")
            },
        ownerPhoneLabel = option { ownerLabel() },
    )

private fun Reader.review(): SpendReviewView =
    SpendReviewView(
        outputs = list { output() },
        changeIndex = option { u32() },
        changeSats = option { u64() },
        movesCoinsInsideWallet = bool(),
        feeSats = u64(),
        feeRateSatPerVb = u64(),
        inputCount = u32(),
        inputTotalSats = u64(),
        otherTotalSats = u64(),
        allowlistedTotalSats = u64(),
        remaining = CeilingsRemaining(u64(), u64(), u64(), u64(), u64(), u64()),
        alreadyCounted = bool(),
        clockBehindBy = option { u64() },
    )

private fun Reader.lineDetail(): LineDetail =
    when (u8()) {
        TAG_LINE_DETAIL_SPEND ->
            LineDetail.Spend(
                SpendDetail(
                    review = option { review() },
                    outputsWithoutLabel = u32s(),
                    labelsWithoutOutput = u32s(),
                    claimedChangeNotProven = u32s(),
                ),
            )
        TAG_LINE_DETAIL_BATCH ->
            LineDetail.Batch(
                BatchDetail(
                    counter = bytes(),
                    counterWords = strings(),
                    start = u32(),
                    end = option { u32() },
                    count = u32(),
                    receiveMark = u32(),
                    checks = checks(),
                ),
            )
        TAG_LINE_DETAIL_SETTINGS ->
            LineDetail.Settings(
                SettingsDetail(
                    newVersion = u32(),
                    lastVersion = option { u32() },
                    changes = list { SettingChangeView(string(), option { string() }, string(), direction()) },
                    overall = option { direction() },
                    checks = checks(),
                ),
            )
        TAG_LINE_DETAIL_PAIRING ->
            LineDetail.Pairing(
                PairingDetail(
                    name = string(),
                    counterKey = bytes(),
                    sessionWords = option { strings() },
                    claimedSettingsVersion = u32(),
                    settingsVersionToSign = u32(),
                ),
            )
        TAG_LINE_DETAIL_HEALTH -> LineDetail.Health(HealthDetail(bytes()))
        else -> tag("LineDetail")
    }

private fun Reader.line(): LineView =
    LineView(
        item = u32(),
        kind =
            when (u8()) {
                TAG_LINE_KIND_SPEND -> LineKind.Spend
                TAG_LINE_KIND_BATCH_REFILL -> LineKind.BatchRefill
                TAG_LINE_KIND_SETTINGS -> LineKind.Settings
                TAG_LINE_KIND_PAIRING -> LineKind.Pairing
                TAG_LINE_KIND_HEALTH_CHECK -> LineKind.HealthCheck
                else -> tag("LineKind")
            },
        title = string(),
        canApprove = bool(),
        failure = option { string() },
        decision = option { decision() },
        detail = lineDetail(),
    )

private fun Reader.ceilings(): CeilingsView = CeilingsView(u64(), u64(), u64())

private fun Reader.signerStatus(): SignerStatus =
    SignerStatus(
        ceilingsOther = ceilings(),
        ceilingsAllowlisted = ceilings(),
        feeCeilings = FeeCeilingsView(u64(), u64()),
        maxFeeRateSatPerVb = u64(),
        maxFeeSats = u64(),
        allowlistWaitSecs = u64(),
        receiveMark = u32(),
        changeMark = u32(),
        latestTimeSeen = u64(),
        effectiveTimeNow = u64(),
        otherDay = u64(),
        otherWeek = u64(),
        allowlistedDay = u64(),
        allowlistedWeek = u64(),
        spendLogCount = u32(),
        allowlist =
            list { AllowlistView(option { string() }, string(), string(), u64(), option { u64() }) },
    )

private fun Reader.bitProgress(): BitProgress = BitProgress(u32(), u32())

private fun Reader.depositCode(): DepositCode =
    DepositCode(string(), option { u64() }, option { string() }, strings())

// ---- the call --------------------------------------------------------------------------------

private val REQUEST_MAGIC = byteArrayOf('O'.code.toByte(), 'B'.code.toByte(), 'M'.code.toByte(), '8'.code.toByte())
private val RESPONSE_MAGIC = byteArrayOf('O'.code.toByte(), 'B'.code.toByte(), 'M'.code.toByte(), '9'.code.toByte())

/**
 * One call: write the frame, call the library, check the response frame and
 * return a reader at the start of the result. The library runs one call at a
 * time, so calls are made one at a time here too.
 */
internal fun decodeResponse(response: ByteArray): Reader {
    val r = Reader(response)
    if (!r.take(4).contentEquals(RESPONSE_MAGIC)) {
        throw KeyException("response-magic", "the library's response is not an OBM9 frame", 4)
    }
    if (r.u8() != 1) {
        throw KeyException("response-version", "the library's response has a version that is not 1", 4)
    }
    val status = r.status()
    if (status == 0) return r
    val rule = r.string16()
    val text = r.string16()
    val words = r.words()
    r.end()
    throw KeyException(rule, text, status, words)
}

/** One request frame: the magic, version 1, the op, the handle and the fields [fill] writes. */
internal fun requestFrame(op: Int, handle: Long, fill: (Writer) -> Unit = {}): ByteArray {
    val w = Writer().raw(REQUEST_MAGIC).u8(1).u16(op).u64(handle)
    fill(w)
    return w.toByteArray()
}

/** The request of [KeyApp.takeKey3]. */
internal fun takeKey3Request(handle: Long, text: String): ByteArray =
    requestFrame(OP_TAKE_KEY3, handle) { it.string(text) }

/** The request of [KeyApp.acceptWalletSetup]. */
internal fun acceptWalletSetupRequest(handle: Long, setup: ByteArray): ByteArray =
    requestFrame(OP_ACCEPT_WALLET_SETUP, handle) { it.bytes(setup) }

private fun call(store: KeyStore, op: Int, handle: Long, fill: (Writer) -> Unit = {}): Reader {
    val response = synchronized(Native) { Native.call(store, requestFrame(op, handle, fill)) }
        ?: throw KeyException("response-null", "the library gave no response", 4)
    return decodeResponse(response)
}

/** A call whose result is nothing. */
private fun callUnit(store: KeyStore, op: Int, handle: Long, fill: (Writer) -> Unit = {}) {
    call(store, op, handle, fill).end()
}

/**
 * The Key app. [open] restores it from the store; every call is one op of
 * PROTOCOL.md. The [store] is used during each call only.
 */
class KeyApp private constructor(private val store: KeyStore, private val handle: Long) :
    AutoCloseable {
    companion object {
        /** Restore the Key app from what [store] holds (an empty app on a first start). */
        fun open(store: KeyStore): KeyApp {
            val r = call(store, OP_OPEN, 0)
            val handle = r.u64()
            r.end()
            return KeyApp(store, handle)
        }

        /** How far the coin flips are, as bits. */
        fun coinBits(store: KeyStore, heads: List<Boolean>, words: KeyWordCount): BitProgress {
            val r =
                call(store, OP_COIN_BITS, 0) {
                    it.list(heads) { h -> it.bool(h) }
                    it.wordCount(words)
                }
            return r.bitProgress().also { r.end() }
        }

        /** How far the dice rolls are, as bits. */
        fun diceBits(store: KeyStore, rolls: String, words: KeyWordCount): BitProgress {
            val r =
                call(store, OP_DICE_BITS, 0) {
                    it.string(rolls)
                    it.wordCount(words)
                }
            return r.bitProgress().also { r.end() }
        }

        /** How many times each face (1 to 6) came up. */
        fun diceFaceCounts(store: KeyStore, rolls: String): List<Long> {
            val r = call(store, OP_DICE_FACE_COUNTS, 0) { it.string(rolls) }
            return r.u32s().also { r.end() }
        }

        /** Up to 8 BIP39 words that start with [prefix], for the word picker. */
        fun wordSuggestions(store: KeyStore, prefix: String): List<String> {
            val r = call(store, OP_WORD_SUGGESTIONS, 0) { it.string(prefix) }
            return r.strings().also { r.end() }
        }

        /** Whether [word] is on the BIP39 English list. */
        fun wordIsValid(store: KeyStore, word: String): Boolean {
            val r = call(store, OP_WORD_IS_VALID, 0) { it.string(word) }
            return r.bool().also { r.end() }
        }

        /** Read a bare address or a BIP21 URI. Enrols nothing. */
        fun parseDepositCode(store: KeyStore, text: String, network: KeyNetwork): DepositCode {
            val r =
                call(store, OP_PARSE_DEPOSIT_CODE, 0) {
                    it.string(text)
                    it.network(network)
                }
            return r.depositCode().also { r.end() }
        }
    }

    fun state(): SetupState {
        val r = call(store, OP_STATE, handle)
        return r.setupState().also { r.end() }
    }

    fun createFromEntropy(source: EntropySource, network: KeyNetwork) =
        callUnit(store, OP_CREATE_FROM_ENTROPY, handle) {
            it.source(source)
            it.network(network)
        }

    fun createApproverOnly() = callUnit(store, OP_CREATE_APPROVER_ONLY, handle)

    /** The words of the key just made, once. */
    fun keyWords(): List<String> {
        val r = call(store, OP_KEY_WORDS, handle)
        return r.strings().also { r.end() }
    }

    fun key2Public(): Key2Public? {
        val r = call(store, OP_KEY2_PUBLIC, handle)
        return r.option { r.key2Public() }.also { r.end() }
    }

    fun deviceRoot(): ByteArray {
        val r = call(store, OP_DEVICE_ROOT, handle)
        return r.bytes().also { r.end() }
    }

    fun deviceRootWords(): List<String> {
        val r = call(store, OP_DEVICE_ROOT_WORDS, handle)
        return r.strings().also { r.end() }
    }

    fun acceptWallet(setup: WalletSetup): WalletView {
        val r =
            call(store, OP_ACCEPT_WALLET, handle) {
                it.string(setup.description)
                it.network(setup.network)
                it.bytes(setup.ownerRoot)
                it.option(setup.key3Xpub) { x -> it.string(x) }
                it.option(setup.batchMinCount) { n -> it.u32(n) }
            }
        return r.walletView().also { r.end() }
    }

    private fun callFrame(request: ByteArray): Reader {
        val response = synchronized(Native) { Native.call(store, request) }
            ?: throw KeyException("response-null", "the library gave no response", 4)
        return decodeResponse(response)
    }

    /** SR-64 to SR-67: read and keep key 3 from the text another wallet shows. */
    fun takeKey3(text: String): Key3View {
        val r = callFrame(takeKey3Request(handle, text))
        return r.key3View().also { r.end() }
    }

    /** Key 3 as kept, or null. */
    fun key3View(): Key3View? {
        val r = call(store, OP_KEY3_VIEW, handle)
        return r.option { r.key3View() }.also { r.end() }
    }

    /** The public answer (`OBMv`) the owner phone scans. */
    fun publicAnswer(): ByteArray {
        val r = call(store, OP_PUBLIC_ANSWER, handle)
        return r.bytes().also { r.end() }
    }

    /** SR-67: "they match". The pending wallet becomes the registered wallet. */
    fun confirmWallet() = callUnit(store, OP_CONFIRM_WALLET, handle)

    /** SR-67: "they differ". The pending wallet is dropped; key 2 and key 3 are kept. */
    fun discardWallet() = callUnit(store, OP_DISCARD_WALLET, handle)

    /** SR-67, SR-68: check the owner phone's wallet setup (`OBMw`) and keep it as pending. */
    fun acceptWalletSetup(setup: ByteArray): WalletView {
        val r = callFrame(acceptWalletSetupRequest(handle, setup))
        return r.walletView().also { r.end() }
    }

    fun walletView(): WalletView {
        val r = call(store, OP_WALLET_VIEW, handle)
        return r.walletView().also { r.end() }
    }

    fun pairOfferBytes(): ByteArray {
        val r = call(store, OP_PAIR_OFFER_BYTES, handle)
        return r.bytes().also { r.end() }
    }

    fun checkKit(scanned: String): KitCheck {
        val r = call(store, OP_CHECK_KIT, handle) { it.string(scanned) }
        return r.kitCheck().also { r.end() }
    }

    /** AS-20: read and check a request and open the session. One is open at a time. */
    fun openSession(request: ByteArray): KeySession {
        val r = call(store, OP_OPEN_SESSION, handle) { it.bytes(request) }
        val session = r.u64()
        r.end()
        return KeySession(store, session)
    }

    fun signerStatus(): SignerStatus {
        val r = call(store, OP_SIGNER_STATUS, handle)
        return r.signerStatus().also { r.end() }
    }

    fun receiveMark(): Long {
        val r = call(store, OP_RECEIVE_MARK, handle)
        return r.u32().also { r.end() }
    }

    fun enrollDestination(address: String, label: String) =
        callUnit(store, OP_ENROLL_DESTINATION, handle) {
            it.string(address)
            it.string(label)
        }

    fun removeDestination(address: String) =
        callUnit(store, OP_REMOVE_DESTINATION, handle) { it.string(address) }

    fun setCeilings(other: CeilingsView, allowlisted: CeilingsView) =
        callUnit(store, OP_SET_CEILINGS, handle) {
            it.ceilings(other)
            it.ceilings(allowlisted)
        }

    fun setFeeCeilings(fee: FeeCeilingsView) =
        callUnit(store, OP_SET_FEE_CEILINGS, handle) {
            it.u64(fee.satPerVb)
            it.u64(fee.absoluteSats)
        }

    fun raiseChangeMark(mark: Long) =
        callUnit(store, OP_RAISE_CHANGE_MARK, handle) { it.u32(mark) }

    fun raiseReceiveMark(mark: Long) =
        callUnit(store, OP_RAISE_RECEIVE_MARK, handle) { it.u32(mark) }

    fun correctClock() = callUnit(store, OP_CORRECT_CLOCK, handle)

    /** Close the app and its session. The stored bytes are not touched. */
    override fun close() = callUnit(store, OP_CLOSE_APP, handle)
}

/**
 * One approval session. Call [cancel] or [close] when the person leaves the
 * screen: until then a second session and every setup or own-screen change
 * is refused with the rule `session-open`.
 */
class KeySession internal constructor(private val store: KeyStore, private val handle: Long) :
    AutoCloseable {
    /** The session's ID, 32 bytes. */
    fun sessionId(): ByteArray {
        val r = call(store, OP_SESSION_ID, handle)
        return r.bytes().also { r.end() }
    }

    /** One line for each item, with the decisions made so far. */
    fun lines(): List<LineView> {
        val r = call(store, OP_LINES, handle)
        return r.list { r.line() }.also { r.end() }
    }

    fun decide(item: Long, decision: Decision) =
        callUnit(store, OP_DECIDE, handle) {
            it.u32(item)
            it.decision(decision)
        }

    /** Sign the approved lines, store the new state, then return the response bytes. */
    fun finish(): ByteArray {
        val r = call(store, OP_FINISH, handle)
        return r.bytes().also { r.end() }
    }

    /** Release the slot without finishing. Nothing is stored. */
    fun cancel() = callUnit(store, OP_CANCEL, handle)

    /** Cancel and drop the handle. */
    override fun close() = callUnit(store, OP_CLOSE_SESSION, handle)
}
