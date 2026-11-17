package org.openbmt.checkout

import android.app.Activity
import android.app.AlertDialog
import android.graphics.Typeface
import android.text.InputType
import android.view.View
import android.widget.Button
import android.widget.EditText
import java.math.BigDecimal
import java.math.RoundingMode
import java.security.MessageDigest
import java.util.UUID
import org.openbmt.app.Ui

/**
 * TEST ONLY manual cash-session screens. Operator labels are audit text, not authentication.
 * The renderer and navigation belong to the host Activity; this controller owns no view tree.
 */
class CheckoutCashController(
    private val activity: Activity,
    private val store: CheckoutCashSessionStore,
    private val ledger: CheckoutLedger,
    private val render: (bar: View, body: List<View>, actions: List<Button>) -> Unit,
    private val onBack: () -> Unit,
) {
    private val ui = Ui(activity)
    private val drawerId = "main-drawer"
    private var blocked = false

    fun show() {
        if (blocked) return showBlocked()
        try { showHome() } catch (e: Exception) { handleError(e, "Cash session data could not be read.") }
    }

    private fun showHome(message: String? = null) {
        val sessions = store.sessions()
        val current = sessions.firstOrNull { it.drawerId == drawerId && it.status == CashSessionStatus.OPEN }
        val body = mutableListOf<View>()
        body += ui.text("TEST ONLY · MANUAL CASH SESSION", 13f, bold = true)
        body += ui.text("Cash and staff details are entered by the operator. Staff labels are not authenticated. No drawer hardware or physical cash verification is connected.", 13f)
        if (message != null) body += ui.text(message, 14f, bold = true, colour = ui.pal.secondary)
        if (current == null) {
            body += ui.text("Main drawer has no open session.", 17f, bold = true)
            body += ui.text("Opening float is entered explicitly in USD. This app does not infer a starting balance.", 14f, colour = ui.pal.secondary)
        } else {
            body += ui.receipt(listOf(
                org.openbmt.app.ReceiptLine("OPEN CASH SESSION", org.openbmt.app.LineStyle.Strong),
                org.openbmt.app.ReceiptLine("Drawer: ${current.drawerId}"),
                org.openbmt.app.ReceiptLine("Session: ${current.sessionId}"),
                org.openbmt.app.ReceiptLine("Opening float: ${money(current.openingFloatMinor)}"),
                org.openbmt.app.ReceiptLine("Expected cash now: ${money(current.expectedCashMinor)}", org.openbmt.app.LineStyle.Strong),
                org.openbmt.app.ReceiptLine("Cash payments linked: ${current.sales.size}"),
                org.openbmt.app.ReceiptLine("Cash movements: ${current.movements.size}"),
            ))
            body += ui.text("Expected cash = opening float + recorded cash in − recorded cash out + linked cash sale amounts after change.", 13f, colour = ui.pal.secondary)
            if (current.sales.isNotEmpty()) {
                body += ui.text("Linked cash payments", 14f, bold = true)
                current.sales.forEach { link ->
                    body += ui.text("${link.paymentId} · ${money(link.amountMinor)} · operator ${link.staffId ?: "Unknown (legacy record)"}", 13f)
                }
            }
        }
        val actions = mutableListOf<Button>()
        if (current == null) actions += ui.primary("Open main cash session") { showOpen() }
        else {
            actions += ui.primary("View cash count and close") { showClose(current.sessionId) }
            actions += ui.secondary("Record cash in") { showMovement(current, CashMovementKind.CASH_IN) }
            actions += ui.secondary("Record cash out") { showMovement(current, CashMovementKind.CASH_OUT) }
            actions += ui.secondary("Reconcile unlinked cash payments") { showReconcile(current) }
        }
        actions += ui.quiet("Closed session history") { showHistory() }
        actions += ui.quiet("Back") { onBack() }
        render(ui.bar("Cash drawer", sub = "TEST ONLY · USD", back = onBack), body, actions)
    }

    private fun showOpen() {
        val floatField = amountField("Opening float in USD", "0.00")
        val staffField = ui.field("Operator label (not authenticated)")
        val body = listOf(
            ui.text("Open the main drawer session", 18f, bold = true),
            ui.text("Enter starting cash and an operator label. This records your input only; it does not read a drawer.", 14f, colour = ui.pal.secondary),
            floatField, staffField,
        )
        val operationId = UUID.randomUUID().toString()
        val sessionId = UUID.randomUUID().toString()
        val submit = ui.primary("Open session") {
            val float = parseUsd(floatField, "Opening float") ?: return@primary
            val staff = staffField.text.toString().trim().ifEmpty { showInputError("Enter an operator label."); return@primary }
            perform("The session was not opened.") {
                store.openSession(operationId, sessionId, drawerId, staff, float, nowSeconds())
                showHome("Cash session opened. Record only observed operator entries.")
            }
        }
        render(ui.bar("Open cash session", back = { showHome() }), body,
            listOf(submit, ui.quiet("Cancel") { showHome() }))
    }

    private fun showMovement(session: CashSession, kind: CashMovementKind) {
        val title = if (kind == CashMovementKind.CASH_IN) "Cash in" else "Cash out"
        val amountField = amountField("Amount in USD", "")
        val reasonField = ui.field("Reason")
        val staffField = ui.field("Operator label (not authenticated)").apply { setText(session.staffId) }
        val body = listOf(
            ui.text("Record $title", 18f, bold = true),
            ui.text("Entry changes expected cash only after this explicit save. It does not move or verify physical cash.", 14f, colour = ui.pal.secondary),
            amountField, reasonField, staffField,
        )
        val operationId = UUID.randomUUID().toString()
        val submit = ui.primary("Save $title") {
            val amount = parseUsd(amountField, "Amount") ?: return@primary
            if (amount <= 0) { showInputError("Amount must be greater than $0.00."); return@primary }
            val reason = reasonField.text.toString().trim()
            val staff = staffField.text.toString().trim()
            if (reason.isEmpty() || staff.isEmpty()) { showInputError("Enter a reason and operator label."); return@primary }
            perform("The cash movement was not saved.") {
                store.recordMovement(session.sessionId, operationId, kind, amount, reason, staff, nowSeconds())
                showHome("$title saved to the manual cash journal.")
            }
        }
        render(ui.bar(title, back = { showHome() }), body,
            listOf(submit, ui.quiet("Cancel") { showHome() }))
    }

    private fun showClose(sessionId: String) {
        val session = store.getSession(sessionId)
        if (session.status != CashSessionStatus.OPEN) return showHistory()
        val countField = amountField("Counted cash in USD", "")
        val staffField = ui.field("Closing operator label (not authenticated)").apply { setText(session.staffId) }
        val body = listOf(
            ui.receipt(listOf(
                org.openbmt.app.ReceiptLine("EXPECTED CASH", org.openbmt.app.LineStyle.Strong),
                org.openbmt.app.ReceiptLine(money(session.expectedCashMinor), org.openbmt.app.LineStyle.Strong),
                org.openbmt.app.ReceiptLine("Opening float ${money(session.openingFloatMinor)} · ${session.sales.size} linked sales · ${session.movements.size} movements"),
            )),
            countField, staffField,
            ui.text("Closing stores an immutable count snapshot. A positive discrepancy is over; a negative discrepancy is short. No physical count is verified by this app.", 13f, colour = ui.pal.secondary),
        )
        val closeOperationId = UUID.randomUUID().toString()
        val countButton = ui.primary("Review and close session") {
            val count = parseUsd(countField, "Counted cash") ?: return@primary
            val staff = staffField.text.toString().trim()
            if (staff.isEmpty()) { showInputError("Enter a closing operator label."); return@primary }
            val discrepancy = runCatching { Math.subtractExact(count, session.expectedCashMinor) }.getOrElse {
                showInputError("The discrepancy is outside the supported amount range."); return@primary
            }
            AlertDialog.Builder(activity)
                .setTitle("Close cash session?")
                .setMessage("Expected: ${money(session.expectedCashMinor)}\nCounted: ${money(count)}\nDiscrepancy: ${signedMoney(discrepancy)}\n\nClosing cannot be edited or reopened.")
                .setNegativeButton("Cancel", null)
                .setPositiveButton("Close session") { _, _ ->
                    perform("The cash session was not closed.") {
                        store.closeSession(sessionId, closeOperationId, count, staff, nowSeconds())
                        showHistory()
                    }
                }.show()
        }
        render(ui.bar("Cash count", back = { showHome() }), body,
            listOf(countButton, ui.quiet("Back") { showHome() }))
    }

    private fun showHistory() {
        val sessions = store.sessions().filter { it.status == CashSessionStatus.CLOSED }
            .sortedWith(compareByDescending<CashSession> { it.closeout?.closedAt ?: 0L }.thenBy { it.sessionId })
        val body = mutableListOf<View>()
        body += ui.text("Closed sessions keep their original count, expected balance, discrepancy and staff label.", 14f, colour = ui.pal.secondary)
        if (sessions.isEmpty()) body += ui.text("No closed cash sessions yet.", 16f)
        sessions.forEach { session ->
            val closeout = session.closeout ?: return@forEach
            body += ui.receipt(listOf(
                org.openbmt.app.ReceiptLine("CLOSED · ${session.drawerId}", org.openbmt.app.LineStyle.Strong),
                org.openbmt.app.ReceiptLine("Session ${session.sessionId} · opened ${session.openedAt} · closed ${closeout.closedAt}"),
                org.openbmt.app.ReceiptLine("Expected ${money(closeout.expectedMinor)} · Counted ${money(closeout.countedMinor)}"),
                org.openbmt.app.ReceiptLine("Discrepancy ${signedMoney(closeout.discrepancyMinor)} · ${closeout.staffId}"),
            ))
            session.sales.forEach { link ->
                body += ui.text("${link.paymentId} · ${money(link.amountMinor)} · linked by ${link.staffId ?: "Unknown (legacy record)"}", 13f)
            }
        }
        render(ui.bar("Closed cash sessions", back = { showHome() }), body,
            listOf(ui.quiet("Back to cash drawer") { showHome() }, ui.quiet("Back") { onBack() }))
    }

    /** Explicit per-payment reconciliation. Date eligibility is based on the canonical recordedAt field. */
    private fun showReconcile(session: CashSession) {
        val allSessions = store.sessions()
        val linkedIds = allSessions.flatMap { it.sales }.map { it.paymentId }.toSet()
        val candidates = ledger.eventSnapshot().sales.asSequence()
            .filter { it.currency == "USD" }
            .flatMap { sale -> sale.cashPayments.asSequence().filter { it.recordedAt >= session.openedAt && it.paymentId !in linkedIds }
                .map { sale to it } }
            .sortedWith(compareBy<Pair<Sale, CashPayment>> { it.second.recordedAt }.thenBy { it.first.saleId }.thenBy { it.second.paymentId })
            .toList()
        val body = mutableListOf<View>()
        body += ui.text("Review each candidate before linking it to ${session.sessionId}. Only USD cash payments with recordedAt at or after this session opened are shown. A link does not collect cash or change the sale.", 14f, colour = ui.pal.secondary)
        body += ui.text("Drawer ${session.drawerId}. This screen does not reconcile payments from another open drawer automatically.", 13f, colour = ui.pal.secondary)
        if (candidates.isEmpty()) body += ui.text("No eligible unlinked cash payments were found.", 16f)
        val staffField = ui.field("Operator label (not authenticated)").apply { setText(session.staffId) }
        body += staffField
        val actions = mutableListOf<Button>()
        candidates.forEach { (sale, payment) ->
            body += ui.receipt(listOf(
                org.openbmt.app.ReceiptLine("CASH PAYMENT · ${sale.saleId}", org.openbmt.app.LineStyle.Strong),
                org.openbmt.app.ReceiptLine("Recorded ${payment.recordedAt} · applied ${money(payment.amountMinor)}"),
                org.openbmt.app.ReceiptLine("Tendered ${money(payment.tenderedMinor)} · change ${money(payment.changeMinor)}"),
            ))
            val operationId = deterministicOperationId(session.sessionId, payment.paymentId)
            actions += ui.secondary("Link this cash payment") {
                val staff = staffField.text.toString().trim()
                if (staff.isEmpty()) { showInputError("Enter an operator label."); return@secondary }
                perform("Payment was not linked. No cash was collected.") {
                    store.linkCashSale(session.sessionId, operationId, sale.saleId, payment.paymentId,
                        staff, payment.recordedAt, ledger)
                    showReconcile(store.getSession(session.sessionId))
                }
            }
        }
        actions += ui.quiet("Back to cash drawer") { showHome() }
        render(ui.bar("Reconcile cash payments", back = { showHome() }), body, actions)
    }

    private fun perform(failureMessage: String, action: () -> Unit) {
        if (blocked) return showBlocked()
        try { action() } catch (e: Exception) { handleError(e, failureMessage) }
    }

    private fun handleError(error: Exception, fallback: String) {
        val uncertain = error.message?.contains("uncertain", ignoreCase = true) == true
        if (uncertain || error.message?.contains("reopen before continuing", ignoreCase = true) == true) {
            blocked = true
            return showBlocked()
        }
        render(ui.bar("Cash entry not saved", back = onBack), listOf(
            ui.text(fallback, 16f, bold = true),
            ui.text(safeDetail(error), 14f, colour = ui.pal.secondary),
            ui.text("No successful save was confirmed. Check the cash journal before repeating any physical cash handling.", 13f),
        ), listOf(ui.secondary("Return to cash sessions") { show() }, ui.quiet("Back") { onBack() }))
    }

    private fun showBlocked() {
        val body = listOf(
            ui.text("CASH SESSION STORAGE NEEDS RECOVERY", 17f, bold = true),
            ui.text("A saved write may or may not have committed. This screen will not retry, reset, or collect cash again. Close and reopen the test app, then inspect the saved session and Sales before continuing.", 14f),
            ui.text("If the storage error remains after reopening, stop and report it. Do not repeat the cash handling operation.", 14f, bold = true),
        )
        render(ui.bar("Cash journal blocked", back = onBack), body, listOf(ui.quiet("Back") { onBack() }))
    }

    private fun amountField(label: String, initial: String): EditText = ui.field(label).apply {
        inputType = InputType.TYPE_CLASS_NUMBER or InputType.TYPE_NUMBER_FLAG_DECIMAL
        setSingleLine(true)
        setText(initial)
        typeface = Typeface.MONOSPACE
    }

    private fun parseUsd(field: EditText, label: String): Long? {
        val text = field.text.toString().trim()
        if (text.isEmpty()) { showInputError("Enter $label in USD."); return null }
        if (!text.matches(Regex("[0-9]+(?:\\.[0-9]{1,2})?"))) {
            showInputError("Enter a valid USD amount with no more than two decimal places.")
            return null
        }
        return try {
            BigDecimal(text).setScale(2, RoundingMode.UNNECESSARY).movePointRight(2).longValueExact().also {
                require(it >= 0) { "$label cannot be negative." }
            }
        } catch (_: Exception) {
            showInputError("Enter a valid USD amount with no more than two decimal places.")
            null
        }
    }

    private fun showInputError(message: String) {
        AlertDialog.Builder(activity).setTitle("Check cash entry").setMessage(message)
            .setPositiveButton("OK", null).show()
    }

    private fun money(minor: Long): String = "USD ${minor / 100}.${(minor % 100).toString().padStart(2, '0')}"
    private fun signedMoney(minor: Long): String = if (minor > 0) "+${money(minor)}" else if (minor < 0) "−${money(Math.negateExact(minor))}" else money(0)
    private fun nowSeconds(): Long = System.currentTimeMillis() / 1000L
    private fun safeDetail(error: Exception): String = error.message?.take(180) ?: "storage or validation error"

    private fun deterministicOperationId(sessionId: String, paymentId: String): String {
        val bytes = (sessionId + "\u0000" + paymentId).toByteArray(Charsets.UTF_8)
        val hash = MessageDigest.getInstance("SHA-256").digest(bytes).joinToString("") {
            (it.toInt() and 0xff).toString(16).padStart(2, '0')
        }
        return "cash-link-$hash"
    }
}
