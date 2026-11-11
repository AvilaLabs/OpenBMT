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
    val canApprove: Boolean,
)

object LineModel {
    private fun kindName(kind: LineKind) = when (kind) {
        LineKind.Spend -> "Payment"
        LineKind.BatchRefill -> "New addresses for a counter"
        LineKind.Settings -> "Change of settings"
        LineKind.Pairing -> "Pairing a counter"
        LineKind.HealthCheck -> "Health check"
    }

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
                    rows.add("${c.field}: ${c.old ?: "not set"} to ${c.new} (${direction(c.direction)}).")
                }
                s.overall?.let { rows.add("Overall this change ${direction(it)} the settings.") }
                for (c in s.checks) rows.add(checkRow(c))
            }
            is LineDetail.Pairing -> {
                val p = d.detail
                quoted.add(Quoted("The name the counter gives itself", p.name))
                p.sessionWords?.let { rows.add("Words to compare with the counter: ${it.joinToString(" ")}.") }
                rows.add("Settings version to sign: ${p.settingsVersionToSign}; the counter claims ${p.claimedSettingsVersion}.")
            }
            is LineDetail.Health -> rows.add("The counter asks this app to show it is still answering.")
        }
        return LineText(
            title = line.title.ifEmpty { kindName(line.kind) },
            rows = rows,
            claims = claims,
            quoted = quoted.map { Quoted(it.from, Format.cleanForeign(it.text)) },
            failure = line.failure,
            canApprove = line.canApprove,
        )
    }
}
