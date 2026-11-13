package org.openbmt.owner.app

import java.io.File
import java.io.IOException
import org.openbmt.app.DurableFile
import org.openbmt.app.Format
import org.openbmt.owner.FloatCoin
import org.openbmt.owner.FloatPaid
import org.openbmt.owner.FloatSealed
import org.openbmt.owner.FloatStatus
import org.openbmt.owner.FloatWalletView
import org.openbmt.owner.FloatWatchConfig
import org.openbmt.owner.StartConfig
import org.openbmt.owner.OwnerFloat
import org.openbmt.owner.OwnerException
import org.openbmt.owner.OwnerNode
import org.openbmt.owner.RefundRow
import org.openbmt.owner.StageKind

/** App limits follow the bitcoin supply and unhardened BIP-32 index domain. */
object FloatLimits {
    const val MAX_SATS: Long = 2_100_000_000_000_000L
    const val MAX_INDEX: Long = 2_147_483_647L
}

/** The binding surface used by the app. Kept injectable so model decisions can be exercised without JNI. */
interface FloatCore {
    /** FL-01, FL-03: the scanned Compact SeedQR bytes become the float's seed inside the library and are sealed there. */
    fun seal(scan: ByteArray): FloatSealed
    fun status(cap: Long, confirmed: Long, unconfirmedIncoming: Long): FloatStatus
    fun feeRate(recent: Long?, typed: Long?): Long

    /** FL-05: the float's coins from the node's watch of the float; with none given it must fail closed. */
    fun coinView(): FloatCoinView = FloatCoinView.Unavailable
}

/** Direct forwarding to the owner library; it does not retain or store the words. */
object LibraryFloat : FloatCore {
    override fun seal(scan: ByteArray) = OwnerFloat.seal(scan)

    /** FL-06: the scan of the float's words again, sealed anew only when it is the recorded float's wallet. */
    fun restore(scan: ByteArray, description: String) = OwnerFloat.restore(scan, description)

    override fun status(cap: Long, confirmed: Long, unconfirmedIncoming: Long) =
        OwnerFloat.status(cap, confirmed, unconfirmedIncoming)
    override fun feeRate(recent: Long?, typed: Long?) = OwnerFloat.feeRate(recent, typed)
}

/**
 * The library's float calls, with the coin view from the running node's watch of the float (FL-05). With no node, a node
 * that is not watching yet, or a view that does not read, the view is [FloatCoinView.Unavailable]: no balance is guessed.
 */
class NodeFloat(private val node: OwnerNode?) : FloatCore by LibraryFloat {
    /**
     * FL-05: tells the node to move its watch of the float to 20 past the indexes [record] holds, at once. A node that is not
     * watching the float, or is not there, is left as it is: the next start reads the same record (FloatModel.watch).
     */
    fun raiseWatch(record: FloatRecord) {
        val n = node ?: return
        val (receiveEnd, changeEnd) = FloatModel.watchEnds(record)
        try {
            n.floatWatch(receiveEnd, changeEnd)
        } catch (_: OwnerException) {
        } catch (_: LinkageError) {
        }
    }

    override fun coinView(): FloatCoinView {
        val n = node ?: return FloatCoinView.Unavailable
        return try {
            val held = n.floatCoins()
            FloatCoinView.Checked(held.coins, held.confirmed, held.unconfirmedIncoming)
        } catch (_: OwnerException) {
            FloatCoinView.Unavailable
        } catch (_: IllegalArgumentException) {
            FloatCoinView.Unavailable
        } catch (_: LinkageError) {
            FloatCoinView.Unavailable
        }
    }
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
    /** FL-05: the height the node starts its watch of the float from; 0 when the phone did not know the tip at setup. */
    val watchFrom: Long = 0,
    /** FL-35: when the float was turned off, in seconds; the node keeps watching it for a day after. */
    val offAt: Long? = null,
) {
    init {
        require(watchFrom in 0..FloatLimits.MAX_INDEX) { "float watch height is outside the supported range" }
        require(offAt == null || offAt >= 0) { "float off time is outside the supported range" }
        require(capSats in 1..FloatLimits.MAX_SATS) { "float cap is outside the supported range" }
        require(nextReceiveIndex in 0..FloatLimits.MAX_INDEX) { "float receive index is outside the supported range" }
        require(nextChangeIndex in 0..FloatLimits.MAX_INDEX) { "float change index is outside the supported range" }
        require(childIndex in 0..FloatLimits.MAX_INDEX) { "float child index is outside the supported range" }
        require(pendingPayId == null || TXID.matches(pendingPayId)) { "pending float payment ID does not read" }
        require(pendingSweepId == null || TXID.matches(pendingSweepId)) { "pending float sweep ID does not read" }
    }

    fun text(): String = "cap-sats: $capSats\nreceive-index: $nextReceiveIndex\nchange-index: $nextChangeIndex\n" +
        "state: ${state.name}\nchild-index: $childIndex\nwatch-from: $watchFrom\n" + (offAt?.let { "off-at: $it\n" } ?: "") + (pendingPayId?.let { "pending-pay-id: $it\n" } ?: "") +
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
            val allowed = setOf("cap-sats", "receive-index", "change-index", "state", "child-index", "watch-from", "off-at", "pending-pay-id", "pending-sweep-id")
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
                    if (fields.containsKey("watch-from")) number("watch-from", FloatLimits.MAX_INDEX) else 0,
                    if (fields.containsKey("off-at")) number("off-at", Long.MAX_VALUE) else null,
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
    fun finishSetup(draft: FloatRecord, wallet: FloatWalletView, childIndex: Long = 0, watchFrom: Long = 0): FloatRecord {
        if (!hasSealed() || !hasWrapped()) throw IllegalStateException("the float's key is not kept yet")
        if (!dir.isDirectory && !dir.mkdirs()) throw IOException("the float folder could not be made")
        DurableFile.write(dir, File(dir, WALLET), "${wallet.description}\n${wallet.firstAddress}\n".toByteArray(Charsets.UTF_8))
        val ready = FloatRecord(draft.capSats, 0, 0, null, null, FloatState.Ready, childIndex, watchFrom)
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
            latest.pendingPayId, latest.pendingSweepId, latest.state, latest.childIndex, latest.watchFrom, latest.offAt)
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

    fun clearPendingSweep(record: FloatRecord) = save(copy(current(record), sweep = null))

    fun clearPendingPay(record: FloatRecord) = save(copy(current(record), pay = null))

    /** FL-20: a payment with change used change index [used]; the next one starts after it, so no address repeats. */
    fun changeUsed(record: FloatRecord, used: Long) {
        val latest = current(record)
        if (used + 1 > latest.nextChangeIndex) save(copy(latest, change = used + 1))
    }

    /**
     * Mark disabled only after a known confirmation; an unknown depth never deletes the key. [now] is when the float went
     * off: the node watches it for a day after that (FL-35).
     */
    fun completeSweep(record: FloatRecord, confirmations: Long?, now: Long) {
        if (!FloatModel.sweepMayDeleteWords(confirmations)) throw IllegalStateException("the float move is not confirmed")
        save(copy(current(record), sweep = null, state = FloatState.Disabled, offAt = now))
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
            offAt: Long? = record.offAt,
        ) = FloatRecord(record.capSats, receive, change, pay, sweep, state, record.childIndex, record.watchFrom, offAt)
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
     * refund is ready and not larger than what it holds. The library makes the same checks when it signs, and the outbox again.
     */
    fun canPay(home: FloatHome, hasKey: Boolean, refund: RefundRow): Boolean =
        home.state == FloatState.Ready && hasKey && readyRefunds(listOf(refund)).isNotEmpty() && floatCovers(home, refund)

    /** Which buttons the float page enables, from what the phone knows. */
    fun actions(record: FloatRecord?, home: FloatHome, hasKey: Boolean, refunds: List<RefundRow>): FloatActions = FloatActions(
        setup = record != null && record.state == FloatState.NeedsSetup,
        refill = canRefill(home),
        pay = refunds.any { canPay(home, hasKey, it) },
        sweep = canSweep(home, hasKey),
    )

    /** FL-35: turning off needs the float's coins and its key, and something to move. */
    fun canSweep(home: FloatHome, hasKey: Boolean): Boolean =
        home.state == FloatState.Ready && hasKey && (home.heldSats ?: 0) > 0

    /** The next free receive index after a refill was queued to [record]'s current one (FL-30): no address is used twice. */
    fun afterRefill(record: FloatRecord): FloatRecord =
        FloatRecord(record.capSats, record.nextReceiveIndex + 1, record.nextChangeIndex, record.pendingPayId,
            record.pendingSweepId, record.state, record.childIndex, record.watchFrom, record.offAt)

    /** FL-05: where each chain's watch ends, one past the last index used or handed out; the node adds 20 (FL-05). */
    fun watchEnds(record: FloatRecord): Pair<Long, Long> = Pair(record.nextReceiveIndex + 1, record.nextChangeIndex + 1)

    /**
     * FL-06: the float's words can be scanned again when the phone has its record and its public wallet, whether or not
     * the key is there: the page offers it when the key was dropped.
     */
    fun canRestore(record: FloatRecord?, wallet: FloatWalletView?): Boolean =
        record != null && record.state == FloatState.Ready && wallet != null

    /** FL-35: how long the node keeps watching a float that was turned off, in seconds. */
    const val WATCH_AFTER_OFF_SECS = 86_400L

    /**
     * FL-05: what the node is started with to watch the float: a float that is on, and one that was turned off less than a day
     * ago. Each chain is watched to one past the last index used or handed out; the node adds 20. [fallbackBirthday] is the
     * shop's own start height, used when the phone did not know the tip at setup.
     */
    fun watch(record: FloatRecord?, wallet: FloatWalletView?, fallbackBirthday: Long, now: Long): FloatWatchConfig? {
        if (record == null || wallet == null) return null
        val live = when (record.state) {
            FloatState.Ready -> true
            FloatState.Disabled -> record.offAt != null && now < record.offAt + WATCH_AFTER_OFF_SECS
            FloatState.NeedsSetup -> false
        }
        if (!live) return null
        val (receiveEnd, changeEnd) = watchEnds(record)
        return FloatWatchConfig(
            wallet.description,
            receiveEnd,
            changeEnd,
            if (record.watchFrom > 0) record.watchFrom else fallbackBirthday,
        )
    }

    /** FL-05: [base] with the float's watch added, the rest of the start unchanged. */
    fun withWatch(base: StartConfig, float: FloatWatchConfig?): StartConfig = StartConfig(
        base.dir, base.network, base.now, base.ownerSecret, base.approverRoot, base.wallet, base.key1Account, base.key3Xpub,
        base.batchMinCount, base.refundDepth, base.birthday, base.proxy, base.fixedPeers, base.broadcastProxy, base.withoutTor,
        base.link, base.ownWallet, float,
    )

    /** FL-22: what the owner reads before proving they are there: the address in full, the amount and the fee rate. */
    fun beforeLines(address: String, sats: Long, feeRatePerKvb: Long): List<Pair<String, String>> =
        StarterPayModel.beforeLines(address, sats, feeRatePerKvb)

    /** FL-35: before the move, the fee rate and the words that it goes to the shop's own wallet. */
    fun sweepBeforeLines(feeRatePerKvb: Long): List<Pair<String, String>> = listOf(
        StarterWords.payFeeRate to "${feeRatePerKvb / 1000}.${(feeRatePerKvb % 1000).toString().padStart(3, '0')} sat/vB",
    )

    /** After it is signed and before it goes out: every address in full, every amount, the fee and the change. */
    fun afterLines(paid: FloatPaid): List<Pair<String, String>> {
        val out = ArrayList<Pair<String, String>>()
        out.add(StarterWords.payAddressLabel to paid.destination)
        out.add(StarterWords.payAmountLabel to Format.sats(paid.paid))
        out.add(StarterWords.payFeeLabel to Format.sats(paid.fee))
        paid.changeSats?.let { out.add(StarterWords.payChangeLabel to Format.sats(it)) }
        return out
    }
}
