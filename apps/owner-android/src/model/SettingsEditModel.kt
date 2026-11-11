package org.openbmt.owner.app

import org.openbmt.app.Format
import org.openbmt.app.SayWords
import org.openbmt.owner.CounterShare
import org.openbmt.owner.LightningAction
import org.openbmt.owner.SettingsEditArgs
import org.openbmt.owner.SettingsHeld
import org.openbmt.owner.SettingsPreview
import org.openbmt.owner.ShopCurrency

/** The kinds of value a field of the editor holds. */
enum class FieldKind { Dollars, Percent, Blocks, Minutes, Sats, Switch, TipPercent }

/**
 * One field the owner can change (OA-33). [read] is the value the settings hold in the field's own unit (whole
 * dollars, basis points, blocks, seconds, satoshis, or 1 and 0 for a switch, or a suggested tip's whole percent, 0 for
 * none); [put] sets the field of an edit
 * from a value in that unit. Everything that has a meaning is the library's: the app judges the shape of what
 * was typed and nothing more (OA-07).
 */
class EditField(
    val id: String,
    val kind: FieldKind,
    val read: (SettingsHeld) -> Long,
    val put: (SettingsEditArgs, Long) -> SettingsEditArgs,
) {
    val words: SayWords get() = AppWords.editField(id)
    val label: String get() = words.heading

    /** The field's words in the shop's currency: its name stands where "dollars" stood. */
    fun wordsIn(c: ShopCurrency): SayWords = AppWords.editField(id, c)
}

/** One screen of the editor: its words and the fields on it. [wordsIn] gives the words in a currency. */
class EditGroup(val words: SayWords, val fields: List<EditField>, val wordsIn: (ShopCurrency) -> SayWords = { words })

/** What the owner chose for the Lightning node (OA-33). */
enum class NodeChoice { Keep, Remove, Set }

/**
 * What the owner has typed so far, in memory only: for each field its text, only when it is not what the settings
 * hold. Nothing is stored and nothing is sent until the owner presses "Queue the change" (OA-34).
 */
class SettingsDraft {
    /** Typed text by field id; a switch is "1" or "0". A field that was left as it was is not here. */
    val typed: MutableMap<String, String> = LinkedHashMap()

    /** Typed percent by the counter's identity in hex; a share left as it was is not here. */
    val shares: MutableMap<String, String> = LinkedHashMap()

    var node: NodeChoice = NodeChoice.Keep
    var nodeText: String = ""

    /** The currency chosen, only when it is not the one the settings hold (CR-198). */
    var currency: ShopCurrency? = null

    /**
     * Today's price of one bitcoin in dollars, typed when the library asked for one (AS-75). It is never offered
     * again: it is forgotten when the review is left, so a price is typed afresh each time.
     */
    var price: String = ""

    val isEmpty: Boolean get() = typed.isEmpty() && shares.isEmpty() && node == NodeChoice.Keep && currency == null

    /**
     * Choose a currency. A currency other than the one [held] is a new currency: what was typed as a limit and the
     * price were typed in another currency, so they are forgotten and every limit is typed again (CR-198).
     */
    fun chooseCurrency(c: ShopCurrency, held: ShopCurrency) {
        val next = if (c == held) null else c
        if (next == currency) return
        currency = next
        typed.keys.removeAll { id -> SettingsEditModel.fields.any { it.id == id && it.kind == FieldKind.Dollars } }
        price = ""
    }
}

/** What was typed, turned into the arguments of the preview call, or the app's words of why not. */
object SettingsEditModel {
    private fun usd(id: String, read: (SettingsHeld) -> Long, put: (SettingsEditArgs, Long) -> SettingsEditArgs) =
        EditField(id, FieldKind.Dollars, read, put)

    val currency = EditGroup(AppWords.editGroupCurrency, emptyList())

    val instant = EditGroup(
        AppWords.editGroupInstant,
        listOf(
            usd("light_per_sale", { it.lightPerSale.displayUsd }, { a, v -> a.copy(lightPerSale = v) }),
            usd("light_budget", { it.lightBudget.displayUsd }, { a, v -> a.copy(lightBudget = v) }),
            usd("node_per_sale", { it.nodePerSale.displayUsd }, { a, v -> a.copy(nodePerSale = v) }),
            usd("node_budget", { it.nodeBudget.displayUsd }, { a, v -> a.copy(nodeBudget = v) }),
            usd("box_per_sale", { it.boxPerSale.displayUsd }, { a, v -> a.copy(boxPerSale = v) }),
            usd("box_budget", { it.boxBudget.displayUsd }, { a, v -> a.copy(boxBudget = v) }),
        ),
        AppWords::editGroupInstantIn,
    )

    val payments = EditGroup(
        AppWords.editGroupPayments,
        listOf(
            EditField("underpayment_tolerance_bps", FieldKind.Percent, { it.underpaymentToleranceBps }, { a, v -> a.copy(underpaymentToleranceBps = v) }),
            EditField("min_confirmations", FieldKind.Blocks, { it.minConfirmations.toLong() }, { a, v -> a.copy(minConfirmations = v.toInt()) }),
            EditField("final_confirmations", FieldKind.Blocks, { it.finalConfirmations.toLong() }, { a, v -> a.copy(finalConfirmations = v.toInt()) }),
            EditField("min_sale_sats", FieldKind.Sats, { it.minSaleSats }, { a, v -> a.copy(minSaleSats = v) }),
        ),
    )

    /** One of the three suggested tips. They are put together in [args], so a field of its own sets nothing. */
    private fun tip(id: String, at: Int) =
        EditField(id, FieldKind.TipPercent, { (it.tipPercents.getOrNull(at) ?: 0).toLong() }, { a, _ -> a })

    private val tipFields = listOf(tip("tip_1", 0), tip("tip_2", 1), tip("tip_3", 2))

    val tips = EditGroup(
        AppWords.editGroupTips,
        listOf(
            EditField("tips_on", FieldKind.Switch, { if (it.tipsOn) 1 else 0 }, { a, v -> a.copy(tipsOn = v == 1L) }),
        ) + tipFields,
    )

    val refunds = EditGroup(
        AppWords.editGroupRefunds,
        listOf(
            usd("refund_cap", { it.refundCap.displayUsd }, { a, v -> a.copy(refundCap = v) }),
            usd("refund_daily_cap", { it.refundDailyCap.displayUsd }, { a, v -> a.copy(refundDailyCap = v) }),
            usd("cashier_override_daily_cap", { it.cashierOverrideDailyCap.displayUsd }, { a, v -> a.copy(cashierOverrideDailyCap = v) }),
            usd("manager_override_ceiling", { it.managerOverrideCeiling.displayUsd }, { a, v -> a.copy(managerOverrideCeiling = v) }),
        ),
        AppWords::editGroupRefundsIn,
    )

    val safety = EditGroup(
        AppWords.editGroupSafety,
        listOf(
            EditField("heartbeat_max_age_secs", FieldKind.Minutes, { it.heartbeatMaxAgeSecs }, { a, v -> a.copy(heartbeatMaxAgeSecs = v) }),
            EditField("tor_on", FieldKind.Switch, { if (it.torOn) 1 else 0 }, { a, v -> a.copy(torOn = v == 1L) }),
            EditField("customer_wifi", FieldKind.Switch, { if (it.customerWifi) 1 else 0 }, { a, v -> a.copy(customerWifi = v == 1L) }),
        ),
    )

    /** The groups of the menu, in order: the currency, then the screens of fields. The shares and the node have screens of their own. */
    val groups: List<EditGroup> = listOf(currency, instant, payments, tips, refunds, safety)

    /** Every field of the screens of fields, in the order of the groups. */
    val fields: List<EditField> = groups.flatMap { it.fields }

    // ---- what was typed -----------------------------------------------------------------------------------------

    /** Whole dollars: digits, with a "$" in front and commas between thousands allowed; at most what a u32 holds. */
    fun dollars(text: String): Long? =
        Forms.number32(text.trim().removePrefix("$").replace(",", "").trim())

    /** A percent with up to two decimals, 0 to 100, as basis points: "0.5" and "0.50%" are 50, "33.33" is 3333. */
    fun percentBps(text: String): Long? {
        val m = Regex("^(\\d{1,3})(?:\\.(\\d{1,2}))?$").matchEntire(text.trim().removeSuffix("%").trim()) ?: return null
        val whole = m.groupValues[1].toLong()
        val frac = m.groupValues[2].padEnd(2, '0').toLong()
        val bps = whole * 100 + frac
        return bps.takeIf { it <= 10_000 }
    }

    /** Whole minutes, as seconds. */
    fun minutesToSecs(text: String): Long? = Forms.number(text.trim())?.takeIf { it <= 0xffffffffL / 60 }?.let { it * 60 }

    /** A whole number of blocks that fits a byte. */
    fun blocks(text: String): Long? = Forms.number(text.trim())?.takeIf { it <= 255 }

    /** A suggested tip: nothing (0, no tip), or a whole percent from 1 to 50. */
    fun tipPercent(text: String): Long? {
        val t = text.trim().removeSuffix("%").trim()
        if (t.isEmpty()) return 0L
        if (t.any { it == ',' || it == '.' }) return null
        return Forms.number(t)?.takeIf { it in 1..50 }
    }

    /** Whole satoshis. */
    fun sats(text: String): Long? = Forms.number(text.trim())

    /** The text of a node's key or address as typed or scanned: trimmed, and never empty. The library judges the rest (AS-76). */
    fun nodeText(text: String): Form<String> {
        val t = text.trim()
        return if (t.isEmpty()) Form.Refused(AppWords.editNodeNeeded) else Form.Ok(t)
    }

    /** The value of a field from what was typed, in the field's own unit, or null when it is not that kind of number. */
    fun parse(kind: FieldKind, text: String): Long? = when (kind) {
        FieldKind.Dollars -> dollars(text)
        FieldKind.Percent -> percentBps(text)
        FieldKind.Blocks -> blocks(text)
        FieldKind.Minutes -> minutesToSecs(text)
        FieldKind.Sats -> sats(text)
        FieldKind.TipPercent -> tipPercent(text)
        FieldKind.Switch -> when (text) {
            "1" -> 1L
            "0" -> 0L
            else -> null
        }
    }

    // ---- what the settings hold, for the screens -------------------------------------------------------------------

    /** A percent from basis points, without a sign: 50 is "0.5", 1000 is "10", 3333 is "33.33". */
    fun percentInput(bps: Long): String {
        val whole = bps / 100
        val frac = bps % 100
        return when {
            frac == 0L -> "$whole"
            frac % 10 == 0L -> "$whole.${frac / 10}"
            else -> "$whole.${"%02d".format(frac)}"
        }
    }

    /** A percent as the screens write it: "0.5%". */
    fun percentText(bps: Long): String = percentInput(bps) + "%"

    /** Whole dollars as the screens write them: "$25", "$1,250", or "off". */
    fun dollarsText(usd: Long): String = moneyText(ShopCurrency.Usd, usd)

    /** Whole units of a currency as the screens write them: "$1,250" for dollars, "1,250 euros", or "off". */
    fun moneyText(c: ShopCurrency, n: Long): String =
        if (n == 0L) "off" else if (c == ShopCurrency.Usd) "$" + grouped(n) else grouped(n) + " " + c.plural

    /** Whole units of a currency next to a limit's satoshis, without grouping: "$25", "25 euros". */
    fun unitsText(c: ShopCurrency, n: Long): String = if (c == ShopCurrency.Usd) "$$n" else "$n ${c.plural}"
    private fun grouped(n: Long): String {
        val digits = n.toString()
        val out = StringBuilder()
        for ((i, c) in digits.withIndex()) {
            if (i > 0 && (digits.length - i) % 3 == 0) out.append(',')
            out.append(c)
        }
        return out.toString()
    }

    /** The text a field starts with, from what the settings hold. */
    fun input(f: EditField, held: SettingsHeld, draft: SettingsDraft? = null): String {
        // A limit of a new currency starts empty: the old number is not a value in it (CR-198).
        if (f.kind == FieldKind.Dollars && draft?.currency != null) return ""
        val v = f.read(held)
        return when (f.kind) {
            FieldKind.Percent -> percentInput(v)
            FieldKind.Minutes -> (v / 60).toString()
            FieldKind.TipPercent -> if (v == 0L) "" else v.toString()
            else -> v.toString()
        }
    }

    /** What the settings hold in a field, in words, for the line "Now:". */
    fun now(f: EditField, held: SettingsHeld): String {
        val v = f.read(held)
        return when (f.kind) {
            FieldKind.Dollars -> moneyText(held.currency, v)
            FieldKind.Percent -> percentText(v)
            FieldKind.Blocks -> if (v == 1L) "1 block" else "$v blocks"
            FieldKind.Minutes -> if (v % 60 == 0L) (if (v == 60L) "1 minute" else "${v / 60} minutes") else "$v seconds"
            FieldKind.Sats -> if (v == 0L) "none" else Format.sats(v)
            FieldKind.TipPercent -> if (v == 0L) "none" else "$v%"
            FieldKind.Switch -> when (f.id) {
                "customer_wifi" -> if (v == 1L) "yes or unsure" else "no"
                else -> if (v == 1L) "on" else "off"
            }
        }
    }

    /** The suggested tips as the settings view writes them: "Off" with tips off, else "10%, 15%, 20%" ("none" with no percents). */
    fun tipsNow(held: SettingsHeld): String = when {
        !held.tipsOn -> "Off"
        held.tipPercents.isEmpty() -> "none"
        else -> held.tipPercents.joinToString(", ") { "$it%" }
    }

    /** The key of the node in full, 66 digits, or "none". */
    fun nodeNow(held: SettingsHeld): String =
        held.lightningNode?.joinToString("") { "%02x".format(it) } ?: "none"

    /** What the shares add up to and what remains, as one line; over the whole says by how much. */
    fun sharesTotalLine(bps: List<Long>): String {
        val total = bps.sum()
        return when {
            total > 10_000 -> "Total ${percentText(total)}. Over the whole by ${percentText(total - 10_000)}."
            else -> "Total ${percentText(total)}. Left over: ${percentText(10_000 - total)}."
        }
    }

    /** The total line for the texts typed in the shares' fields, or a line that says some are not percents yet. */
    fun sharesLine(texts: List<String>): String {
        val bps = texts.map { percentBps(it) }
        return if (bps.any { it == null }) "Some share is not a percent yet." else sharesTotalLine(bps.filterNotNull())
    }

    /** One line for a counter's share and what it holds now. */
    fun shareNow(s: CounterShare): String = "${OwnerModel.counterLabel(s.counter)}: now ${percentText(s.bps)}"

    /** A counter's identity in hex, to key the typed shares. */
    fun counterKey(counter: ByteArray): String = counter.joinToString("") { "%02x".format(it) }

    // ---- the draft, turned into a call ---------------------------------------------------------------------------

    /**
     * The arguments of the preview call for what was typed: only fields that were changed are in them. A text that
     * is not the kind of number its field takes, or a share that is not a percent, is refused with words and
     * nothing is called (OA-33). [held] gives the counters whose shares were typed.
     */
    fun args(draft: SettingsDraft, held: SettingsHeld): Form<SettingsEditArgs> {
        var a = SettingsEditArgs()
        for (f in fields) {
            val text = draft.typed[f.id] ?: continue
            val v = parse(f.kind, text)
                ?: return Form.Refused(AppWords.editValueNotRead(f.kind, f.label, draft.currency ?: held.currency))
            a = f.put(a, v)
        }
        // The three suggested tips are one list: a field not typed keeps what the settings hold, and an empty one is left out.
        if (tipFields.any { draft.typed.containsKey(it.id) }) {
            val percents = ArrayList<Int>()
            for (f in tipFields) {
                val v = draft.typed[f.id]?.let { parse(f.kind, it) } ?: f.read(held)
                if (v > 0) percents.add(v.toInt())
            }
            a = a.copy(tipPercents = percents)
        }
        a = a.copy(currency = draft.currency)
        val shares = ArrayList<CounterShare>()
        for (s in held.counters) {
            val text = draft.shares[counterKey(s.counter)] ?: continue
            val bps = percentBps(text) ?: return Form.Refused(AppWords.editSharesNotPercent)
            shares.add(CounterShare(s.counter, bps))
        }
        a = a.copy(shares = shares)
        a = when (draft.node) {
            NodeChoice.Keep -> a
            NodeChoice.Remove -> a.copy(lightning = LightningAction.Remove)
            NodeChoice.Set -> when (val t = nodeText(draft.nodeText)) {
                is Form.Refused -> return t
                is Form.Ok -> a.copy(lightning = LightningAction.Set, lightningText = t.value)
            }
        }
        if (draft.price.isNotBlank()) {
            when (val r = CounterModel.price(draft.price, draft.currency ?: held.currency)) {
                is Form.Refused -> return r
                is Form.Ok -> a = a.copy(usdPerBtc = r.value)
            }
        }
        return Form.Ok(a)
    }

    // ---- the review ----------------------------------------------------------------------------------------------

    /** One line of the review: what changes, from what to what, and whether it needs the Key app. */
    class ReviewLine(val text: String, val loosening: Boolean)

    /** Every change of the preview as a line "label: old → new"; a counter's share carries the counter. */
    fun reviewLines(p: SettingsPreview): List<ReviewLine> = p.changes.map { c ->
        val label = c.counter?.let { "Share of " + OwnerModel.counterLabel(it) } ?: c.label
        ReviewLine("$label: ${c.old} → ${c.new}", c.loosening)
    }

    /** The lines that need the Key app, which are the ones the library marked as loosening. */
    fun needKeyApp(p: SettingsPreview): List<String> = reviewLines(p).filter { it.loosening }.map { it.text }

    /** The lines that take effect when delivered. */
    fun whenDelivered(p: SettingsPreview): List<String> = reviewLines(p).filter { !it.loosening }.map { it.text }

    /** The words above the review: nothing changed, the Key app is needed, or the owner alone is enough. */
    fun reviewWords(p: SettingsPreview): SayWords = when {
        p.nothingChanged -> AppWords.editNothingChanged
        p.needsKeyApp -> AppWords.editNeedsKeyApp
        else -> AppWords.editTakesEffect
    }

    /** The shares line for the review when counters' shares changed, else null. */
    fun reviewShares(p: SettingsPreview): String? =
        if (p.changes.any { it.counter != null }) sharesTotalLine(listOf(p.sharesTotalBps)) else null
}
