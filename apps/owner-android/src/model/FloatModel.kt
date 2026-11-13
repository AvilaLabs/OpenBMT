package org.openbmt.owner.app

import java.io.File
import java.io.IOException
import org.openbmt.app.DurableFile
import org.openbmt.owner.FloatCoin
import org.openbmt.owner.FloatPaid
import org.openbmt.owner.FloatSealed
import org.openbmt.owner.FloatStatus
import org.openbmt.owner.FloatWalletView
import org.openbmt.owner.OwnerFloat
import org.openbmt.owner.OwnerNetwork
import org.openbmt.owner.RefundRow
import org.openbmt.owner.StageKind

/** App limits follow the bitcoin supply and unhardened BIP-32 index domain. */
object FloatLimits {
    const val MAX_SATS: Long = 2_100_000_000_000_000L
    const val MAX_INDEX: Long = 2_147_483_647L
}

/** The binding surface used by the app. Kept injectable so model decisions can be exercised without JNI. */
interface FloatCore {
    fun open(words: String): FloatWalletView

    /** FL-01, FL-03: the scanned Compact SeedQR bytes become the float's seed inside the library and are sealed there. */
    fun seal(scan: ByteArray): FloatSealed
    fun status(cap: Long, confirmed: Long, unconfirmedIncoming: Long): FloatStatus
    fun feeRate(recent: Long?, typed: Long?): Long
    fun pay(
        words: String,
        network: OwnerNetwork,
        address: String,
        sats: Long,
        feeRate: Long,
        changeIndex: Long,
        minConfirmations: Long,
        lockHeight: Long,
        coins: List<FloatCoin>,
    ): FloatPaid
    fun sweep(
        words: String,
        shop: String,
        shopIndex: Long,
        feeRate: Long,
        minConfirmations: Long,
        lockHeight: Long,
        coins: List<FloatCoin>,
    ): FloatPaid

    /** The current binding has no checked float coin-view call, so it must fail closed here. */
    fun coinView(): FloatCoinView = FloatCoinView.Unavailable
}

/** Direct forwarding to the owner library; it does not retain or store the words. */
object LibraryFloat : FloatCore {
    override fun open(words: String) = OwnerFloat.open(words)
    override fun seal(scan: ByteArray) = OwnerFloat.seal(scan)
    override fun status(cap: Long, confirmed: Long, unconfirmedIncoming: Long) =
        OwnerFloat.status(cap, confirmed, unconfirmedIncoming)
    override fun feeRate(recent: Long?, typed: Long?) = OwnerFloat.feeRate(recent, typed)
    override fun pay(
        words: String, network: OwnerNetwork, address: String, sats: Long, feeRate: Long,
        changeIndex: Long, minConfirmations: Long, lockHeight: Long, coins: List<FloatCoin>,
    ) = OwnerFloat.pay(words, network, address, sats, feeRate, changeIndex, minConfirmations, lockHeight, coins)
    override fun sweep(
        words: String, shop: String, shopIndex: Long, feeRate: Long,
        minConfirmations: Long, lockHeight: Long, coins: List<FloatCoin>,
    ) = OwnerFloat.sweep(words, shop, shopIndex, feeRate, minConfirmations, lockHeight, coins)
}

/** A coin view is either authoritative and checked by its provider, or unavailable. */
sealed class FloatCoinView {
    object Unavailable : FloatCoinView()

    /** Construct only from the node's checked float-wallet coin view; never from a typed balance or shop wallet. */
    class Checked(
        val coins: List<FloatCoin>,
        val confirmedSats: Long,
        val unconfirmedIncomingSats: Long,
    ) : FloatCoinView() {
        init {
            require(confirmedSats in 0..FloatLimits.MAX_SATS)
            require(unconfirmedIncomingSats in 0..FloatLimits.MAX_SATS)
            require(confirmedSats + unconfirmedIncomingSats <= FloatLimits.MAX_SATS) {
                "float balance exceeds the supported range"
            }
        }
    }
}

enum class FloatState { NeedsSetup, Ready, Disabled }

/** Public phone record only: it never contains the float's child words or another secret. */
class FloatRecord(
    val capSats: Long,
    val nextReceiveIndex: Long = 0,
    val nextChangeIndex: Long = 0,
    val pendingPayId: String? = null,
    val pendingSweepId: String? = null,
    val state: FloatState = FloatState.NeedsSetup,
    /** FL-04: the BIP-85 index of the float's words, kept so that the phone can say which one to make next. */
    val childIndex: Long = 0,
) {
    init {
        require(capSats in 1..FloatLimits.MAX_SATS) { "float cap is outside the supported range" }
        require(nextReceiveIndex in 0..FloatLimits.MAX_INDEX) { "float receive index is outside the supported range" }
        require(nextChangeIndex in 0..FloatLimits.MAX_INDEX) { "float change index is outside the supported range" }
        require(childIndex in 0..FloatLimits.MAX_INDEX) { "float child index is outside the supported range" }
        require(pendingPayId == null || TXID.matches(pendingPayId)) { "pending float payment ID does not read" }
        require(pendingSweepId == null || TXID.matches(pendingSweepId)) { "pending float sweep ID does not read" }
    }

    fun text(): String = "cap-sats: $capSats\nreceive-index: $nextReceiveIndex\nchange-index: $nextChangeIndex\n" +
        "state: ${state.name}\nchild-index: $childIndex\n" + (pendingPayId?.let { "pending-pay-id: $it\n" } ?: "") +
        (pendingSweepId?.let { "pending-sweep-id: $it\n" } ?: "")

    companion object {
        private val TXID = Regex("[0-9a-fA-F]{64}")

        fun parse(text: String): FloatRecord {
            val fields = HashMap<String, String>()
            for (line in text.lineSequence()) {
                if (line.isBlank()) continue
                val split = line.indexOf(": ")
                if (split <= 0) throw IOException("the float record does not read")
                val key = line.substring(0, split)
                if (fields.put(key, line.substring(split + 2)) != null) throw IOException("the float record repeats a field")
            }
            val allowed = setOf("cap-sats", "receive-index", "change-index", "state", "child-index", "pending-pay-id", "pending-sweep-id")
            if (fields.keys.any { it !in allowed }) throw IOException("the float record has an unknown field")
            fun number(name: String, max: Long): Long = fields[name]?.toLongOrNull()
                ?.takeIf { it in 0..max } ?: throw IOException("the float record has no valid $name")
            val cap = fields["cap-sats"]?.toLongOrNull()?.takeIf { it in 1..FloatLimits.MAX_SATS }
                ?: throw IOException("the float record has no valid cap")
            val state = fields["state"]?.let { value -> FloatState.values().firstOrNull { it.name == value } }
                ?: throw IOException("the float record has no valid state")
            return try {
                FloatRecord(
                    cap,
                    number("receive-index", FloatLimits.MAX_INDEX),
                    number("change-index", FloatLimits.MAX_INDEX),
                    fields["pending-pay-id"], fields["pending-sweep-id"], state,
                    if (fields.containsKey("child-index")) number("child-index", FloatLimits.MAX_INDEX) else 0,
                )
            } catch (e: IllegalArgumentException) {
                throw IOException("the float record does not read", e)
            }
        }
    }
}

/** A separate atomic file for public float state. Words and key material are not accepted by this store. */
class FloatStore(base: File) {
    val dir = File(base, DIR)
    private val recordFile get() = File(dir, RECORD)

    /** FL-03: the sealed seed and the wrapped data key, kept as the starter wallet's are (SS-20, SS-22), in the float's own folder. */
    private val keys = StarterStore(base, DIR)

    fun hasSealed() = keys.hasSealed()
    fun hasWrapped() = keys.hasWrapped()
    fun saveSealed(bytes: ByteArray) = keys.saveSealed(bytes)
    fun loadSealed(): ByteArray? = keys.loadSealed()
    fun saveWrapped(bytes: ByteArray) = keys.saveWrapped(bytes)
    fun loadWrapped(): ByteArray? = keys.loadWrapped()

    /** Overwrites and deletes the sealed seed and the wrapped key (FL-35, and a setup that did not finish). */
    fun deleteKey() = keys.deleteKey()

    /** The public side of the float: its description and first address, one to a line. It holds no secret. */
    fun loadWallet(): FloatWalletView? {
        val lines = File(dir, WALLET).takeIf { it.isFile }?.readBytes()?.toString(Charsets.UTF_8)?.lines() ?: return null
        if (lines.size < 2 || lines[0].isEmpty() || lines[1].isEmpty()) throw IOException("the float wallet does not read")
        return FloatWalletView(lines[0], lines[1])
    }

    /**
     * FL-03, FL-04: the last step of the setup. Only once the sealed seed and the wrapped key are both kept does the
     * public wallet go down, and then the record, last, as Ready at child index [childIndex] with receive index 0:
     * a Ready record never exists without its key.
     */
    fun finishSetup(draft: FloatRecord, wallet: FloatWalletView, childIndex: Long = 0): FloatRecord {
        if (!hasSealed() || !hasWrapped()) throw IllegalStateException("the float's key is not kept yet")
        if (!dir.isDirectory && !dir.mkdirs()) throw IOException("the float folder could not be made")
        DurableFile.write(dir, File(dir, WALLET), "${wallet.description}\n${wallet.firstAddress}\n".toByteArray(Charsets.UTF_8))
        val ready = FloatRecord(draft.capSats, 0, 0, null, null, FloatState.Ready, childIndex)
        save(ready)
        return ready
    }

    fun load(): FloatRecord? = recordFile.takeIf { it.isFile }
        ?.readBytes()?.toString(Charsets.UTF_8)?.let { FloatRecord.parse(it) }

    fun save(record: FloatRecord) {
        if (!dir.isDirectory && !dir.mkdirs()) throw IOException("the float folder could not be made")
        DurableFile.write(dir, recordFile, record.text().toByteArray(Charsets.UTF_8))
    }

    fun updateCap(record: FloatRecord, capSats: Long): FloatRecord {
        require(capSats in 1..FloatLimits.MAX_SATS)
        val latest = current(record)
        val updated = FloatRecord(capSats, latest.nextReceiveIndex, latest.nextChangeIndex,
            latest.pendingPayId, latest.pendingSweepId, latest.state, latest.childIndex)
        save(updated)
        return updated
    }

    fun pendingPay(record: FloatRecord, txid: String) {
        require(TXID.matches(txid))
        save(copy(current(record), pay = txid))
    }

    fun pendingSweep(record: FloatRecord, txid: String) {
        require(TXID.matches(txid))
        save(copy(current(record), sweep = txid))
    }

    fun clearPendingPay(record: FloatRecord) = save(copy(current(record), pay = null))

    /** Mark disabled only after a known confirmation; an unknown depth never deletes the key. */
    fun completeSweep(record: FloatRecord, confirmations: Long?) {
        if (!FloatModel.sweepMayDeleteWords(confirmations)) throw IllegalStateException("the float move is not confirmed")
        save(copy(current(record), sweep = null, state = FloatState.Disabled))
    }

    private fun current(fallback: FloatRecord): FloatRecord = load() ?: fallback

    companion object {
        const val DIR = "owner-float"
        const val RECORD = "record.txt"
        const val WALLET = "wallet.txt"

        /** The Keystore key of the float, apart from the starter's and every other key of the app. */
        const val KEY_ALIAS = "openbmt-owner-float-v1"
        private val TXID = Regex("[0-9a-fA-F]{64}")

        private fun copy(
            record: FloatRecord,
            receive: Long = record.nextReceiveIndex,
            change: Long = record.nextChangeIndex,
            pay: String? = record.pendingPayId,
            sweep: String? = record.pendingSweepId,
            state: FloatState = record.state,
        ) = FloatRecord(record.capSats, receive, change, pay, sweep, state, record.childIndex)
    }
}

/** Which of the float page's action buttons are enabled. */
class FloatActions(val setup: Boolean, val refill: Boolean, val pay: Boolean, val sweep: Boolean)

class FloatHome(
    val state: FloatState,
    val heldSats: Long?,
    val low: Boolean?,
    val refillSats: Long?,
    val message: String,
)

/** Pure policy around the library. An unavailable coin view never becomes an estimated or typed balance. */
object FloatModel {
    fun cap(text: String): Long? {
        val value = text.trim()
        if (!Regex("(?:[0-9]+|[0-9]{1,3}(?:,[0-9]{3})+)").matches(value)) return null
        return value.replace(",", "").toLongOrNull()?.takeIf { it in 1..FloatLimits.MAX_SATS }
    }

    fun setupRecord(capText: String): FloatRecord? = cap(capText)?.let { FloatRecord(it, state = FloatState.NeedsSetup) }

    fun home(core: FloatCore, record: FloatRecord?, view: FloatCoinView = core.coinView()): FloatHome {
        if (record == null) return FloatHome(FloatState.NeedsSetup, null, null, null, FloatWords.setupTitle)
        if (record.state == FloatState.Disabled) return FloatHome(FloatState.Disabled, null, null, null, FloatWords.floatUnavailable)
        if (record.state == FloatState.NeedsSetup) return FloatHome(FloatState.NeedsSetup, null, null, null, FloatWords.scanUnavailable)
        if (view !is FloatCoinView.Checked) return FloatHome(FloatState.Ready, null, null, null, FloatWords.unavailable)
        val status = core.status(record.capSats, view.confirmedSats, view.unconfirmedIncomingSats)
        return FloatHome(
            FloatState.Ready, status.held, status.low, status.refill,
            if (status.low) FloatWords.statusLow else FloatWords.statusReady,
        )
    }

    fun feeRate(core: FloatCore, recent: Long?, typed: Long?): Long = core.feeRate(recent, typed)

    /** Unknown and zero confirmations both fail closed. */
    fun sweepMayDeleteWords(confirmations: Long?): Boolean = confirmations != null && confirmations >= 1

    /**
     * FL-20, SS-30: the queued refunds that are ready, as the starter wallet's list reads them: queued and not waiting
     * for depth. The library makes the same check when the payment is made and again at the outbox.
     */
    fun readyRefunds(refunds: List<RefundRow>): List<RefundRow> =
        refunds.filter { it.stage.kind == StageKind.Queued && it.waitingForDepth == false }

    /** FL-11, FL-30: a refill is offered only for a float that is on and whose coins the node has given, and only with room under the cap. */
    fun canRefill(home: FloatHome): Boolean = home.state == FloatState.Ready && home.refillSats != null && home.refillSats > 0

    /** FL-12: the home screen offers the refill when the float is low and a refill is possible. */
    fun offersRefill(home: FloatHome): Boolean = home.low == true && canRefill(home)

    /** FL-23: a refund larger than what the float holds is not split; it stays in the queue for the shop's wallet. */
    fun floatCovers(home: FloatHome, refund: RefundRow): Boolean = home.heldSats != null && refund.sats <= home.heldSats

    /**
     * FL-20: the float pays a refund only when it is on, its key is on the phone, the node has given its coins and the
     * refund is ready and not larger than what it holds. Paying is not wired to the library yet (see `FloatScreens.kt`),
     * so no screen uses this to enable a button.
     */
    fun canPay(home: FloatHome, hasKey: Boolean, refund: RefundRow): Boolean =
        home.state == FloatState.Ready && hasKey && readyRefunds(listOf(refund)).isNotEmpty() && floatCovers(home, refund)

    /**
     * Whether the library can sign for the float on this phone. `float_pay` and `float_sweep` still take the words as text,
     * which the phone must never hold (FL-03), and the node does not watch the float's coins or take its payments
     * (see the report of the float wiring): no pay or turn-off button is enabled until that is built.
     */
    const val SIGNING_WIRED = false

    /** Which buttons the float page enables, from what the phone knows. */
    fun actions(record: FloatRecord?, home: FloatHome, hasKey: Boolean, refunds: List<RefundRow>): FloatActions = FloatActions(
        setup = record != null && record.state == FloatState.NeedsSetup,
        refill = canRefill(home),
        pay = SIGNING_WIRED && refunds.any { canPay(home, hasKey, it) },
        sweep = SIGNING_WIRED && canSweep(home, hasKey),
    )

    /** FL-35: turning off needs the float's coins and its key, and something to move. */
    fun canSweep(home: FloatHome, hasKey: Boolean): Boolean =
        home.state == FloatState.Ready && hasKey && (home.heldSats ?: 0) > 0

    /** The next free receive index after a refill was queued to [record]'s current one (FL-30): no address is used twice. */
    fun afterRefill(record: FloatRecord): FloatRecord =
        FloatRecord(record.capSats, record.nextReceiveIndex + 1, record.nextChangeIndex, record.pendingPayId,
            record.pendingSweepId, record.state, record.childIndex)
}
