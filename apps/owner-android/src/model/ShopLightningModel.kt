package org.openbmt.owner.app

import org.openbmt.app.Format
import org.openbmt.owner.LightningReport
import org.openbmt.owner.ShopLightningAction
import org.openbmt.owner.ShopLightningEditArgs
import org.openbmt.owner.ShopLightningHeld
import org.openbmt.owner.ShopLightningState
import org.openbmt.owner.ReceivingBackup
import org.openbmt.owner.ReceivingSnapshot

/**
 * The Shop Lightning page (`docs/SHOP-LIGHTNING-RULES.md` SL-45, SL-76 to SL-78): which page the state calls for, the
 * draft of the turn-on steps and the edit it becomes, and the facts of the newest report as labelled lines. The node
 * judges every value (AP-13, SL-71); only the shape of a typed number and of the scanned code is checked here. No
 * Android class is used, and no text of its own: the labels and values are the words of [ShopLightningWords] and
 * the numbers and times the library gave.
 */
object ShopLightningModel {
    /** The pages of the row. */
    enum class Page { Starter, Off, On }

    /** The steps of a turn-on, in order (SL-76). */
    enum class Step { Counter, Node, Provider, Limits, Approve }

    /** Where a step stands among the five, from the words' placeholders `{n}` and `{of}` (SL-76). */
    fun stepLine(step: Step): String =
        ShopLightningWords.stepOf.replace("{n}", (step.ordinal + 1).toString()).replace("{of}", Step.values().size.toString())

    /** One labelled fact of the page, with a plain line under it when it needs one. */
    class Line(val label: String, val value: String, val note: String? = null)

    /** What the steps have gathered. The app keeps it between screens and forgets it when the page is left. */
    class Draft {
        var counter: ByteArray? = null
        var nodeId: String = ""
        var providerKey: String = ""
        var providerAddr: String = ""
        var cap: String = ""
        var fee: String = ""
    }

    /** A shop on its starter wallet is not offered turning on: the wallet's key is on this phone (AP-15). */
    fun page(state: ShopLightningState, starter: Boolean = false): Page = when {
        state.on -> Page.On
        state.ownWallet && starter -> Page.Starter
        else -> Page.Off
    }

    /**
     * Turning on needs a counter to pick and a Box to keep the second copy (SL-73, SL-76). A shop with its own wallet
     * signs the change with the wallet (AP-14).
     */
    fun canTurnOn(state: ShopLightningState): Boolean = state.counters.isNotEmpty() && state.boxes.isNotEmpty()

    /** The row's second line: whether Shop Lightning is on. */
    fun rowSub(state: ShopLightningState?): String = if (state != null && state.on) ShopLightningWords.rowSubOn else ShopLightningWords.rowSubOff

    private val NODE_ID = Regex("[0-9a-f]{66}")

    /** The node's ID of a scanned code: 66 lower-case hexadecimal digits (SK-35). The library checks that they are a key. */
    fun nodeId(scanned: ByteArray): Form<String> {
        val text = String(scanned, Charsets.UTF_8).trim()
        return if (NODE_ID.matches(text)) Form.Ok(text) else Form.Refused(ShopLightningWords.nodeNotIdWords)
    }

    /** A whole number of satoshis above zero. */
    fun sats(text: String): Long? = Forms.number(text)?.takeIf { it > 0 }

    /** The fee may be zero: a provider that opens for nothing. */
    fun fee(text: String): Long? = Forms.number(text)

    /** The first and last eight digits of an ID and the rest left out, as the stick shows its code (SK-35). */
    fun short(hex: String): String = if (hex.length <= 20) hex else hex.take(8) + "…" + hex.takeLast(8)

    fun hex(bytes: ByteArray): String = bytes.joinToString("") { "%02x".format(it) }

    /** The turn-on edit from the steps' draft (SL-76). */
    fun turnOn(d: Draft): Form<ShopLightningEditArgs> {
        val counter = d.counter ?: return Form.Refused(ShopLightningWords.noCountersWords)
        if (!NODE_ID.matches(d.nodeId)) return Form.Refused(ShopLightningWords.nodeNotIdWords)
        if (d.providerKey.isBlank() || d.providerAddr.isBlank()) return Form.Refused(ShopLightningWords.providerMissingWords)
        val cap = sats(d.cap) ?: return Form.Refused(ShopLightningWords.numberInvalidWords)
        val fee = fee(d.fee) ?: return Form.Refused(ShopLightningWords.numberInvalidWords)
        return Form.Ok(
            ShopLightningEditArgs(
                ShopLightningAction.Set,
                counter = counter,
                nodeId = d.nodeId,
                providerNodeId = d.providerKey.trim(),
                providerAddr = d.providerAddr.trim(),
                capSats = cap,
                maxOpeningFeeSats = fee,
            ),
        )
    }

    /** The change of the cap and the most for one opening: only what differs from [held] is sent (AP-13). */
    fun limits(capText: String, feeText: String, held: ShopLightningHeld): Form<ShopLightningEditArgs> {
        val cap = sats(capText) ?: return Form.Refused(ShopLightningWords.numberInvalidWords)
        val fee = fee(feeText) ?: return Form.Refused(ShopLightningWords.numberInvalidWords)
        return Form.Ok(
            ShopLightningEditArgs(
                ShopLightningAction.Change,
                capSats = cap.takeIf { it != held.capSats },
                maxOpeningFeeSats = fee.takeIf { it != held.maxOpeningFeeSats },
            ),
        )
    }

    /** The change of the provider: both fields go together (SL-71). */
    fun provider(key: String, addr: String): Form<ShopLightningEditArgs> {
        if (key.isBlank() || addr.isBlank()) return Form.Refused(ShopLightningWords.providerMissingWords)
        return Form.Ok(ShopLightningEditArgs(ShopLightningAction.Change, providerNodeId = key.trim(), providerAddr = addr.trim()))
    }

    /** Turning off: the field is taken away, and the Lightning counter closes the channels (SL-77). */
    fun turnOff(): ShopLightningEditArgs = ShopLightningEditArgs(ShopLightningAction.Remove)

    /** SL-34: the line that offers the suggestion for the cap, or null when there is none. */
    fun suggestionLine(suggestion: Long?): String? =
        suggestion?.let { ShopLightningWords.capSuggestionWhat.replace("{sats}", Format.sats(it)) }

    /** SL-34: what the cap field holds once the suggestion is taken; the owner still confirms the change. */
    fun fillCap(suggestion: Long): String = suggestion.toString()

    /** SL-27: what the page says above the sales of a lost funding, when the report names any. */
    fun lostFundingNote(report: LightningReport?): String? =
        ShopLightningWords.lostFundingWhat.takeIf { report != null && report.lostFundingSales.isNotEmpty() }

    /** The facts of SL-45 as labelled lines: the settings' cap and provider, and the newest report's numbers (SL-78). */
    fun lines(
        held: ShopLightningHeld,
        report: LightningReport?,
        nowUnix: Long = System.currentTimeMillis() / 1000,
    ): List<Line> {
        val out = ArrayList<Line>()
        out.add(Line(ShopLightningWords.counterLabel, OwnerModel.counterLabel(held.counter)))
        out.add(Line(ShopLightningWords.nodeLabel, short(hex(held.nodeId))))
        out.add(Line(ShopLightningWords.capLabel, Format.sats(held.capSats)))
        out.add(Line(ShopLightningWords.providerLabel, short(hex(held.providerNodeId)) + " " + held.providerAddr))
        out.add(Line(ShopLightningWords.feeLabel, Format.sats(held.maxOpeningFeeSats)))
        out.add(Line(ShopLightningWords.boxLabel, short(hex(held.secondDevice))))
        if (report == null) {
            out.add(Line(ShopLightningWords.confirmedRoomLabel, ShopLightningWords.receivingUnknown))
            out.add(Line(ShopLightningWords.backupStateLabel, ShopLightningWords.backupUnknown, ShopLightningWords.receivingUnknown))
            out.add(Line(ShopLightningWords.amountCheckLabel, ShopLightningWords.amountCheckUnknown, ShopLightningWords.receivingQuote))
            out.add(Line(ShopLightningWords.cameLabel, ShopLightningWords.noReport))
            return out
        }
        out.add(Line(ShopLightningWords.balanceLabel, Format.sats(report.balanceSats)))
        // SL-35: wherever the room is shown, it says that a sweep does not give room back.
        out.add(Line(ShopLightningWords.roomLabel, Format.sats(report.receiveRoomSats), ShopLightningWords.roomNote))
        out.add(Line(ShopLightningWords.feesPaidLabel, Format.sats(report.openingFeesPaidSats)))
        out.add(Line(ShopLightningWords.roomAheadLabel, ShopLightningWords.roomAhead(report.lsps1)))
        ShopLightningWords.order(report.orderState)?.let { state ->
            out.add(Line(ShopLightningWords.orderLabel, state))
            ShopLightningWords.orderWhy(report.orderWhy)?.let { out.add(Line(ShopLightningWords.orderWhyLabel, it)) }
            if (report.orderFeeSats > 0) out.add(Line(ShopLightningWords.orderFeeLabel, Format.sats(report.orderFeeSats)))
        }
        out.add(Line(ShopLightningWords.lastStoredLabel, if (report.lastStoredAt == 0L) ShopLightningWords.neverStored else Format.localTime(report.lastStoredAt)))
        out.add(Line(ShopLightningWords.lastSweepLabel, if (report.lastSweepAt == 0L) ShopLightningWords.noSweep else Format.localTime(report.lastSweepAt)))
        out.add(Line(ShopLightningWords.whyLabel, ShopLightningWords.cause(report.notOffered)))
        out.addAll(receivingLines(report.receiving, report.takenAt, nowUnix))
        // SL-27: the sales paid through a channel whose funding was lost, as the numbers the counter gave.
        if (report.lostFundingSales.isNotEmpty()) out.add(Line(ShopLightningWords.lostFundingLabel, report.lostFundingSales.joinToString(", ")))
        // Past the most the report lists, it also says how many in all.
        if (report.lostFundingTotal > report.lostFundingSales.size) out.add(Line(ShopLightningWords.lostFundingTotalLabel, report.lostFundingTotal.toString()))
        out.add(Line(ShopLightningWords.cameLabel, Format.localTime(report.takenAt)))
        return out
    }

    /** A missing, stale, or future-dated snapshot remains unknown. The
     * counter's wall clock is shown as provenance, never treated as proof of
     * synchronized time or a route. */
    fun receivingLines(snapshot: ReceivingSnapshot?, reportTakenAt: Long, nowUnix: Long): List<Line> {
        if (snapshot == null || reportTakenAt <= 0 || snapshot.capturedAtUnix <= 0 ||
            nowUnix < reportTakenAt || nowUnix < snapshot.capturedAtUnix ||
            nowUnix - reportTakenAt > 30 || nowUnix - snapshot.capturedAtUnix > 30
        ) {
            return listOf(
                Line(ShopLightningWords.confirmedRoomLabel, ShopLightningWords.receivingUnknown),
                Line(ShopLightningWords.backupStateLabel, ShopLightningWords.backupUnknown, ShopLightningWords.receivingStale),
                Line(ShopLightningWords.amountCheckLabel, ShopLightningWords.amountCheckUnknown, ShopLightningWords.receivingQuote),
            )
        }
        val backup = when (snapshot.backupState) {
            ReceivingBackup.Unknown -> ShopLightningWords.backupUnknown
            ReceivingBackup.Away -> ShopLightningWords.backupAway
            ReceivingBackup.Behind -> ShopLightningWords.backupBehind
            ReceivingBackup.CaughtUp -> ShopLightningWords.backupCaughtUp
        }
        val room = Format.sats(snapshot.confirmedRoomMsat / 1000)
        return listOf(
            Line(ShopLightningWords.confirmedRoomLabel, room, ShopLightningWords.receivingCapacity),
            Line(ShopLightningWords.backupStateLabel, backup),
            Line(
                ShopLightningWords.backupPendingLabel,
                (snapshot.strictPendingWrites + snapshot.backgroundPendingWrites).toString(),
            ),
            Line(
                ShopLightningWords.lastDeviceStoreSnapshotLabel,
                if (snapshot.lastDeviceStoreUnix == 0L) ShopLightningWords.neverStored else Format.localTime(snapshot.lastDeviceStoreUnix),
            ),
            Line(ShopLightningWords.pendingChannelsLabel, snapshot.pendingOrUnknownChannels.toString()),
            Line(ShopLightningWords.confirmationDepthLabel, snapshot.confirmationDepth.toString()),
            Line(ShopLightningWords.snapshotTimeLabel, Format.localTime(snapshot.capturedAtUnix), ShopLightningWords.receivingClock),
            Line(ShopLightningWords.chainHealthLabel, if (snapshot.chainHealthy) ShopLightningWords.chainHealthy else ShopLightningWords.snapshotChainBehind),
            Line(
                ShopLightningWords.chainAtLabel,
                if (snapshot.chainHeight == 0L) ShopLightningWords.chainUnknown else snapshot.chainHeight.toString(),
            ),
            Line(ShopLightningWords.amountCheckLabel, ShopLightningWords.amountCheckUnknown, ShopLightningWords.receivingQuote),
        )
    }
}
