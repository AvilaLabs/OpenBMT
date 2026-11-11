package org.openbmt.key.app

import org.openbmt.app.Format
import org.openbmt.key.CheckView
import org.openbmt.key.Direction
import org.openbmt.key.LineDetail
import org.openbmt.key.LineKind
import org.openbmt.key.LineView
import org.openbmt.key.OutputKindView
import org.openbmt.key.OutputView
import org.openbmt.key.Quoted
import org.openbmt.key.Words

/**
 * One line of an approval session as text: what the Key app checked itself
 * in [rows], and what the owner's phone only claims in [claims], which this
 * app cannot check. Text typed on another device is in [quoted], never in
 * a row (SC-08).
 */
class LineText(
    val title: String,
    val rows: List<String>,
    val claims: List<String>,
    val quoted: List<Quoted>,
    /** Why the line cannot be approved, from the rule crates, or null. */
    val failure: String?,
    /** The library's words for [failure], when it has words of its own (AS-39); shown in place of the raw text. */
    val failureWords: Words? = null,
    val canApprove: Boolean,
    /**
     * What the screen shows larger than anything else, or null: for a pairing, the four session words
     * on one line, to compare with the counter's screen.
     */
    val prominent: String? = null,
    /** What to do with [prominent], or null. */
    val instruction: String? = null,
    /**
     * The settings changes that list identities (devices, counters, Boxes), each in full as 64 hex digits
     * in groups of four (AS-39), to be shown in monospace and wrapped, so that two identities can be compared.
     */
    val identities: List<String> = emptyList(),
)

object LineModel {
    /** The settings fields whose values are identities, shown in full (AS-39). */
    val IDENTITY_FIELDS = setOf("devices", "counters", "boxes")

    private fun kindName(kind: LineKind) = when (kind) {
        LineKind.Spend -> "Payment"
        LineKind.BatchRefill -> "New addresses for a counter"
        LineKind.Settings -> "Change of settings"
        LineKind.Pairing -> "Pairing a counter"
        LineKind.HealthCheck -> "Health check"
        LineKind.BoxClaim -> "Box found"
    }

    /** The four words of a pairing on one line, spaced so that each stands apart. */
    fun wordsLine(words: List<String>): String = words.joinToString("   ")

    private fun direction(d: Direction) = if (d == Direction.Loosening) "loosens" else "tightens"

    private fun checkRow(c: CheckView) = "${AppWords.check(c.status).heading}: ${c.name}"

    private fun outputRows(o: OutputView, claims: MutableList<String>, quoted: MutableList<Quoted>): String {
        val to = o.address ?: "an address this app cannot write out"
        val kind = when (o.kind) {
            is OutputKindView.Change -> "your own change address"
            is OutputKindView.Allowlisted -> "a saved address named by you on this phone"
            is OutputKindView.Other -> "an address that is not saved on this phone"
        }
        val label = o.ownerPhoneLabel
        if (label != null) {
            claims.add("The owner's phone says this output is a ${label.kind.name.lowercase()}.")
            label.saleIndex?.let { claims.add("The owner's phone says it is for sale number $it.") }
            label.shopValueMinor?.let {
                claims.add("The owner's phone says it is worth $it in minor units of ${label.shopCurrency ?: "an unnamed currency"}.")
            }
            label.cashier?.let { quoted.add(Quoted("The owner's phone says the cashier is", it)) }
            label.destination?.let { quoted.add(Quoted("The owner's phone says the destination is", it)) }
        }
        return "Output ${o.index + 1}: ${Format.sats(o.amountSats)} to $to ($kind)."
    }

    fun describe(line: LineView): LineText {
        val rows = ArrayList<String>()
        val claims = ArrayList<String>()
        val quoted = ArrayList<Quoted>()
        val identities = ArrayList<String>()
        var title = line.title.ifEmpty { kindName(line.kind) }
        var prominent: String? = null
        var instruction: String? = null
        when (val d = line.detail) {
            is LineDetail.Spend -> {
                val r = d.detail.review
                if (r == null) {
                    rows.add("No review of this payment could be made.")
                } else {
                    for (o in r.outputs) rows.add(outputRows(o, claims, quoted))
                    val change = r.changeSats
                    if (change != null) {
                        rows.add("Change back to your wallet: ${Format.sats(change)}.")
                    } else if (r.movesCoinsInsideWallet) {
                        rows.add("This payment only moves coins inside your wallet.")
                    }
                    rows.add("Fee: ${Format.sats(r.feeSats)}, ${r.feeRateSatPerVb} sats for each byte of size.")
                    rows.add("Coins spent: ${r.inputCount}, worth ${Format.sats(r.inputTotalSats)}.")
                    rows.add(
                        "Room left today for other addresses: ${Format.sats(r.remaining.otherDay)}; " +
                            "this week: ${Format.sats(r.remaining.otherWeek)}.",
                    )
                    rows.add(
                        "Room left today for saved addresses: ${Format.sats(r.remaining.allowlistedDay)}; " +
                            "this week: ${Format.sats(r.remaining.allowlistedWeek)}.",
                    )
                    if (r.alreadyCounted) rows.add("This payment is already counted in the totals.")
                    r.clockBehindBy?.let { rows.add("This phone's clock is behind the latest time it has seen.") }
                }
                if (d.detail.outputsWithoutLabel.isNotEmpty()) {
                    claims.add("The owner's phone gave no label for ${d.detail.outputsWithoutLabel.size} output(s).")
                }
                if (d.detail.labelsWithoutOutput.isNotEmpty()) {
                    claims.add("The owner's phone gave ${d.detail.labelsWithoutOutput.size} label(s) for outputs that do not exist.")
                }
                if (d.detail.claimedChangeNotProven.isNotEmpty()) {
                    claims.add("The owner's phone says ${d.detail.claimedChangeNotProven.size} output(s) are change; this app cannot prove it.")
                }
            }
            is LineDetail.Batch -> {
                val b = d.detail
                rows.add("Counter: ${b.counterWords.joinToString(" ")}.")
                rows.add("Address numbers ${b.start} to ${b.end ?: "open end"}, ${b.count} in all.")
                rows.add("Highest number this app has co-signed: ${b.receiveMark}.")
                for (c in b.checks) rows.add(checkRow(c))
            }
            is LineDetail.Settings -> {
                val s = d.detail
                rows.add("Version ${s.newVersion}${s.lastVersion?.let { ", after version $it" } ?: ""}.")
                for (c in s.changes) {
                    val row = "${c.field}: ${c.old ?: "not set"} to ${c.new} (${direction(c.direction)})."
                    if (c.field in IDENTITY_FIELDS) identities.add(row) else rows.add(row)
                }
                s.overall?.let { rows.add("Overall this change ${direction(it)} the settings.") }
                for (c in s.checks) rows.add(checkRow(c))
            }
            is LineDetail.Pairing -> {
                val p = d.detail
                // The name was typed on another device: it is cut and cleaned (SC-08) and shown in quotation
                // marks, and again below as text from elsewhere.
                title = "Pair counter \u201c${Format.cleanForeign(p.name)}\u201d"
                quoted.add(Quoted("The name the counter gives itself", p.name))
                val words = p.sessionWords
                if (words != null) {
                    prominent = wordsLine(words)
                    instruction = "Compare these four words with the four on the counter's screen. " +
                        "Approve only if all four are the same, in the same order."
                } else {
                    rows.add("The four words could not be worked out, so this pairing cannot be compared.")
                }
                rows.add("Settings version to sign: ${p.settingsVersionToSign}; the counter claims ${p.claimedSettingsVersion}.")
            }
            is LineDetail.BoxClaim -> {
                val b = d.detail
                // The Box's key is shown in full: the words are a short way to compare it, not the key.
                rows.add("The Box's key: ${BoxModel.hexGroups(b.boxKey)}.")
                for (c in b.checks) rows.add(checkRow(c))
                val words = b.words
                if (words != null) {
                    prominent = wordsLine(words)
                    instruction = AppWords.boxFound.next
                } else {
                    rows.add("The four words could not be worked out, so this Box cannot be compared.")
                }
            }
            is LineDetail.Health -> rows.add("The counter asks this app to show it is still answering.")
        }
        return LineText(
            title = title,
            rows = rows,
            claims = claims,
            quoted = quoted.map { Quoted(it.from, Format.cleanForeign(it.text)) },
            failure = line.failure,
            failureWords = line.failureWords,
            canApprove = line.canApprove,
            prominent = prominent,
            instruction = instruction,
            identities = identities,
        )
    }
}
