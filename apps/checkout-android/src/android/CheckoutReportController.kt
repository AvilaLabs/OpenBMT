package org.openbmt.checkout

import android.app.Activity
import android.app.AlertDialog
import android.app.DatePickerDialog
import android.content.ClipData
import android.content.ClipboardManager
import android.content.Intent
import android.net.Uri
import android.view.View
import android.widget.Button
import android.widget.LinearLayout
import java.time.LocalDate
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.util.concurrent.atomic.AtomicReference
import org.openbmt.app.Ui

/** Native, read-only reporting screen. The Activity supplies page installation and back navigation. */
class CheckoutReportController(
    private val activity: Activity,
    private val ui: Ui,
    private val ledger: CheckoutLedger,
    private val executeCapability: (
        capability: CheckoutAuthorityCapability,
        operation: () -> Unit,
        completion: (Exception?) -> Unit,
    ) -> Unit,
    private val render: (bar: View, body: List<View>, actions: List<Button>) -> Unit,
    private val onBack: () -> Unit,
) {
    private val zone: ZoneId = ZoneId.systemDefault()
    private var fromDate: LocalDate = LocalDate.now(zone)
    private var throughDate: LocalDate = fromDate
    private var report: CheckoutReport? = null
    private var faultText: String? = null
    private var loading = false
    private var readRevision = 0L
    private var pendingExportCsv: String? = null
    private var pendingExportUri: Uri? = null
    private var exportInFlight = false

    /** Render the selected local-date cohort. Re-reads an immutable ledger event snapshot each time. */
    fun show() {
        val revision = ++readRevision
        val fromDateSnapshot = fromDate
        val throughDateSnapshot = throughDate
        val nextReport = AtomicReference<CheckoutReport?>()
        report = null
        faultText = null
        loading = true
        pendingExportCsv = null
        renderReport()

        executeCapability(CheckoutAuthorityCapability.REPORT_READ, {
            val from = fromDateSnapshot.atStartOfDay(zone).toEpochSecond()
            val until = throughDateSnapshot.plusDays(1).atStartOfDay(zone).toEpochSecond()
            require(from < until) { "invalid date range" }
            val snapshot = ledger.eventSnapshot()
            nextReport.set(CheckoutReporting.build(snapshot.sales, CheckoutReportRange(from, until)))
        }) { error ->
            if (revision != readRevision) return@executeCapability
            loading = false
            when (error) {
                null -> {
                    report = nextReport.get()
                    faultText = if (report == null) "The report could not be created." else null
                }
                is CheckoutAuthorityDeniedException -> {
                    report = null
                    pendingExportCsv = null
                    faultText = "Report access denied. Sign in with a role that can view reports."
                }
                is CheckoutAuthorityUnavailableException -> {
                    report = null
                    pendingExportCsv = null
                    faultText = "Report access is unavailable. Sign in again before viewing reports."
                }
                else -> {
                    report = null
                    pendingExportCsv = null
                    faultText = safeMessage(error)
                }
            }
            renderReport()
        }
    }

    /** Forward the matching document-picker result from the hosting Activity. */
    fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?): Boolean {
        if (requestCode != REQUEST_CREATE_DOCUMENT) return false
        val csv = pendingExportCsv
        if (csv == null) {
            message("Export interrupted", "The reviewed CSV snapshot was not retained while the file picker was open. No report content was written. Review the current report before exporting again; the selected destination may be empty.")
            return true
        }
        if (resultCode != Activity.RESULT_OK) {
            pendingExportCsv = null
            pendingExportUri = null
            return true
        }
        val uri = data?.data
        if (uri == null) {
            pendingExportCsv = null
            pendingExportUri = null
            message("CSV not saved", "The document picker returned no destination. The checkout data was not changed.")
            return true
        }
        // The picker normally backgrounds the Activity, which locks authority. Retain this exact
        // reviewed snapshot and destination only in memory; Activity resumes it after fresh sign-in.
        pendingExportUri = uri
        return true
    }

    /** Continue an exact picker result only after Activity has published a fresh signed-in state. */
    fun resumePendingExportAfterSignIn(): Boolean {
        val csv = pendingExportCsv ?: return exportInFlight
        val uri = pendingExportUri ?: return false
        if (exportInFlight) return true
        exportInFlight = true
        executeCapability(CheckoutAuthorityCapability.REPORT_EXPORT, {
            val stream = activity.contentResolver.openOutputStream(uri, "w")
                ?: throw IllegalStateException("The selected destination could not be opened.")
            stream.bufferedWriter(Charsets.UTF_8).use { writer -> writer.write(csv) }
        }) { error ->
            exportInFlight = false
            when (error) {
                null -> {
                    pendingExportCsv = null
                    pendingExportUri = null
                    show()
                    message("CSV saved", "The reviewed report snapshot was written after fresh manager authorization.")
                }
                is CheckoutAuthorityDeniedException, is CheckoutAuthorityUnavailableException -> Unit
                else -> {
                    pendingExportCsv = null
                    pendingExportUri = null
                    show()
                    message("CSV not saved", "The selected destination could not be fully written. A partial file may remain there. The checkout data was not changed. ${safeMessage(error)}")
                }
            }
        }
        return true
    }

    private fun renderReport() {
        render(buildBar(), buildBody(), buildActions())
    }

    private fun buildBar(): View = ui.bar("Sales report", "Created in range · ${zone.id}", back = onBack)

    private fun buildBody(): List<View> {
        val body = ArrayList<View>()
        if (loading) {
            body += ui.text("Checking report access…", bold = true)
            body += ui.text("The saved report will appear after access is confirmed.")
            return body
        }
        body += ui.text("Date range", bold = true)
        body += ui.text("Sales are grouped by their creation date. Refund values show current cumulative requests and payments for those sales, not refund cash flow by day.")
        body += ui.text("Timezone basis: ${zone.id}. Local-day boundaries use the device timezone, including daylight-saving changes.")
        body += ui.text("${fromDate.format(DATE)} through ${throughDate.format(DATE)} (inclusive)")
        body += ui.button("Choose start date") { chooseDate(fromDate) { chosen ->
            fromDate = chosen
            if (throughDate.isBefore(fromDate)) throughDate = fromDate
            show()
        } }
        body += ui.button("Choose end date") { chooseDate(throughDate) { chosen ->
            throughDate = if (chosen.isBefore(fromDate)) fromDate else chosen
            show()
        } }

        val value = report
        if (value == null) {
            body += ui.text("Report unavailable", bold = true)
            body += ui.text("No cached report is displayed. Resolve the access or storage issue shown below before trying again.")
            body += ui.text(faultText ?: "Unknown report fault")
            return body
        }
        body += ui.text("USD sales · ${value.usdSaleCount}", bold = true)
        body += ui.text("Sales total: ${usd(value.usdSalesMinor)}")
        body += ui.text("Received / observed: ${usd(value.usdObservedMinor)}")
        body += ui.text("Settled / applied: ${usd(value.usdAppliedMinor)}")
        body += ui.text("Cash applied: ${usd(value.usdCashAppliedMinor)}")
        body += ui.text("Card observed / applied: ${usd(value.usdCardObservedMinor)} / ${usd(value.usdCardAppliedMinor)}")
        body += ui.text("Bitcoin observed / applied: ${usd(value.usdBitcoinObservedMinor)} / ${usd(value.usdBitcoinAppliedMinor)}")
        body += ui.text("Bitcoin observed / applied: ${sats(value.usdBitcoinObservedSats)} / ${sats(value.usdBitcoinAppliedSats)}")
        body += ui.text("Refund requested / paid: ${usd(value.usdRefundRequestedMinor)} / ${usd(value.usdRefundPaidMinor)}")
        body += ui.text("Bitcoin refund requested / paid: ${sats(value.usdRefundRequestedSats)} / ${sats(value.usdRefundPaidSats)}")
        body += ui.text("Frozen pricing · subtotal ${usd(value.usdSubtotalMinor)} · discount ${usd(value.usdDiscountMinor)} · tax ${usd(value.usdTaxMinor)} · tip ${usd(value.usdTipMinor)}")
        body += ui.text("${value.usdBreakdownUnknownCount} USD sale(s) have legacy pricing breakdowns that are unknown; those amounts are excluded from the breakdown totals.")
        val reviewCount = value.rows.count { it.status == "REVIEW" }
        val uncertainCount = value.rows.count { row -> row.paymentStates.split('|').any { it in UNCERTAIN_STATES } }
        body += ui.text("Sales requiring review: $reviewCount · sales with uncertain, partial, canceled, or reorged payment states: $uncertainCount")
        if (value.nonUsdCountsByCurrency.isNotEmpty()) {
            body += ui.text("Other currencies (separate; no FX conversion):", bold = true)
            value.nonUsdCountsByCurrency.forEach { (currency, count) -> body += ui.text("$currency · $count sale(s)") }
        } else body += ui.text("Other currencies: none")
        body += ui.text("This is a sale-created-date cohort report, not a cashbook, full shift close, or accounting-compliance report.")
        return body
    }

    private fun buildActions(): List<Button> {
        val value = report ?: return listOf(ui.button("Back", onBack))
        return listOf(
            ui.button("Copy CSV") { confirmExport(value.toCsv(), save = false) },
            ui.button("Save CSV") { confirmExport(value.toCsv(), save = true) },
            ui.button("Back", onBack),
        )
    }

    private fun chooseDate(current: LocalDate, picked: (LocalDate) -> Unit) {
        DatePickerDialog(activity, { _, year, month, day -> picked(LocalDate.of(year, month + 1, day)) },
            current.year, current.monthValue - 1, current.dayOfMonth).show()
    }

    private fun confirmExport(csv: String, save: Boolean) {
        AlertDialog.Builder(activity)
            .setTitle(if (save) "Save report CSV?" else "Copy report CSV?")
            .setMessage("The export contains sale IDs, timestamps, amounts, payment states, and refund totals. Keep the file or clipboard private. Payment provider references and evidence IDs are excluded.")
            .setNegativeButton("Cancel", null)
            .setPositiveButton(if (save) "Choose file" else "Copy CSV") { _, _ ->
                if (save) launchSave(csv) else copy(csv)
            }
            .show()
    }

    private fun launchSave(csv: String) {
        executeCapability(CheckoutAuthorityCapability.REPORT_EXPORT, { Unit }) { error ->
            when (error) {
                null -> {
                    // Retain this exact reviewed snapshot until the picker returns; never rebuild from a newer ledger state.
                    pendingExportCsv = csv
                    pendingExportUri = null
                    val intent = Intent(Intent.ACTION_CREATE_DOCUMENT).apply {
                        addCategory(Intent.CATEGORY_OPENABLE)
                        type = "text/csv"
                        putExtra(Intent.EXTRA_TITLE, "sales-${fromDate.format(FILE_DATE)}-through-${throughDate.format(FILE_DATE)}.csv")
                    }
                    try {
                        activity.startActivityForResult(intent, REQUEST_CREATE_DOCUMENT)
                    } catch (e: Exception) {
                        pendingExportCsv = null
                        message("CSV not saved", "A file picker could not be opened. No checkout data was changed. ${safeMessage(e)}")
                    }
                }
                is CheckoutAuthorityDeniedException, is CheckoutAuthorityUnavailableException ->
                    clearReportForAuthorityFailure(error)
                else -> message("CSV not saved", safeMessage(error))
            }
        }
    }

    private fun copy(csv: String) {
        executeCapability(CheckoutAuthorityCapability.REPORT_EXPORT, {
            val clipboard = activity.getSystemService(Activity.CLIPBOARD_SERVICE) as ClipboardManager
            clipboard.setPrimaryClip(ClipData.newPlainText("Sales report CSV", csv))
        }) { error ->
            when (error) {
                null -> message("CSV copied", "The reviewed report snapshot was copied. The clipboard may be visible to other apps.")
                is CheckoutAuthorityDeniedException, is CheckoutAuthorityUnavailableException ->
                    clearReportForAuthorityFailure(error)
                else -> message("CSV not copied", safeMessage(error))
            }
        }
    }

    private fun clearReportForAuthorityFailure(error: Exception) {
        report = null
        pendingExportCsv = null
        pendingExportUri = null
        exportInFlight = false
        loading = false
        faultText = when (error) {
            is CheckoutAuthorityDeniedException -> "Report export access denied. The report was cleared; sign in with an authorized role and review it again."
            else -> "Report export access is unavailable. The report was cleared; sign in again and review it before exporting."
        }
        renderReport()
    }

    private fun message(title: String, body: String) {
        AlertDialog.Builder(activity).setTitle(title).setMessage(body).setPositiveButton("OK", null).show()
    }

    private fun usd(minor: Long): String = "USD ${minor / 100}.${(minor % 100).toString().padStart(2, '0')}"
    private fun sats(value: Long): String = "$value sats"
    private fun safeMessage(e: Throwable): String = (e.message ?: e.javaClass.simpleName)
        .filterNot { it.isISOControl() }.take(180)

    companion object {
        const val REQUEST_CREATE_DOCUMENT = 0x4C31
        private val DATE = DateTimeFormatter.ISO_LOCAL_DATE
        private val FILE_DATE = DateTimeFormatter.BASIC_ISO_DATE
        private val UNCERTAIN_STATES = setOf("UNKNOWN", "AWAITING", "UNCONFIRMED", "PARTIAL", "CANCELED", "REORGED")
    }
}
