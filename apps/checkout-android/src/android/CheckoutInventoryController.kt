package org.openbmt.checkout

import android.app.Activity
import android.app.AlertDialog
import android.text.InputType
import android.view.View
import android.widget.Button
import android.widget.EditText
import android.widget.LinearLayout
import java.util.UUID
import org.openbmt.app.ButtonKind
import org.openbmt.app.LineStyle
import org.openbmt.app.ReceiptLine
import org.openbmt.app.Ui

/** TEST ONLY local stock and handoff screens. This records a local handoff command, not physical proof. */
class CheckoutInventoryController(
    private val activity: Activity,
    private val catalog: CheckoutCatalog,
    private val inventory: CheckoutInventory,
    private val ledger: CheckoutLedger,
    private val executeCapability: (
        capability: CheckoutAuthorityCapability,
        operation: () -> Unit,
        completion: (Exception?) -> Unit,
    ) -> Unit,
    private val render: (View, List<View>, List<Button>) -> Unit,
    private val onBack: () -> Unit,
) {
    private val ui = Ui(activity)
    private val fulfillmentIds = HashMap<String, String>()
    private var stockPageRevision = 0
    private var inventoryBlocked = false

    fun showStock() {
        if (inventoryBlocked) return showReopen()
        val revision = ++stockPageRevision
        val body = ArrayList<View>()
        body.add(ui.receipt(listOf(
            ReceiptLine("Local inventory", LineStyle.Strong),
            ReceiptLine("TEST ONLY · single device · no authenticated role", LineStyle.Small),
        )))
        body.add(ui.text("Counts are operator entered. They do not prove a physical count, purchase order, or inventory at another location.", 14f, colour = ui.pal.secondary))
        try {
            val stock = inventory.snapshot().products.associateBy { it.productId }
            val products = catalog.listProducts()
            if (products.isEmpty()) body.add(ui.text("Add products in the catalog before configuring stock.", 15f))
            products.forEach { p ->
                val item = stock[p.productId]
            body.add(ui.receipt(listOf(
                    ReceiptLine(p.name, LineStyle.Strong),
                    ReceiptLine(if (item == null) "Stock mode is not configured" else if (item.tracked) "Tracked · ${item.onHand} on hand" else "Manual · no stock count"),
                )))
                val buttons = LinearLayout(activity).apply { orientation = LinearLayout.VERTICAL }
                if (item == null) {
                    buttons.addView(ui.button("Configure stock mode", ButtonKind.Primary) { configureProduct(p.productId, p.name, revision) })
                } else {
                    buttons.addView(ui.button("Adjust count", ButtonKind.Secondary) { adjustProduct(p.productId, p.name, item.version, item.onHand, revision) })
                    buttons.addView(ui.button("Change tracked or manual mode", ButtonKind.Secondary) { changeMode(p.productId, p.name, item.tracked, item.version, revision) })
                }
                body.add(buttons)
            }
        } catch (_: Exception) {
            inventoryBlocked = true
            return showReopen()
        }
        render(ui.bar("Inventory", back = onBack, logo = true), body, listOf(ui.quiet("Done", onBack)))
    }

    fun showFulfillment(saleId: String, onReturn: () -> Unit = onBack) {
        if (inventoryBlocked) return showReopen()
        val body = ArrayList<View>()
        val sale = try { ledger.getSale(saleId) } catch (_: Exception) {
            body.add(ui.receipt(listOf(ReceiptLine("Sale unavailable", LineStyle.Strong), ReceiptLine("Open the sale from Sales and try again."))))
            render(ui.bar("Fulfillment", back = onReturn, logo = true), body, listOf(ui.quiet("Back", onReturn))); return
        }
        val saved = try { inventory.snapshot().fulfillments.firstOrNull { it.saleId == saleId } } catch (_: Exception) {
            inventoryBlocked = true
            return showReopen()
        }
        body.add(ui.receipt(listOf(
            ReceiptLine("Sale ${sale.saleId}", LineStyle.Strong),
            ReceiptLine("TEST ONLY · Local handoff record", LineStyle.Small),
        )))
        val manifest = aggregateManifest(sale.items.map { InventoryPurchaseLine(it.productId, it.quantity) })
        if (saved != null) {
            body.add(manifestView(saved.payload.map { it.productId to it.quantity }))
            body.add(ui.receipt(listOf(
                ReceiptLine("Handoff command recorded · ${saved.fulfillmentId}", LineStyle.Strong),
            ReceiptLine("Recorded at ${saved.completedAt}"),
                ReceiptLine("This durable local record does not prove the items were physically handed over."),
                ReceiptLine("Later payment or chain evidence does not automatically restock items."),
            )))
            render(ui.bar("Fulfillment record", back = onReturn, logo = true), body, listOf(ui.quiet("Back", onReturn))); return
        }
        val version = try { ledger.eventSnapshot().saleVersions[saleId] } catch (_: Exception) { null }
        if (version == null) {
            body.add(ui.text("Sale version is unavailable. Refresh Sales before creating a handoff record.", 14f))
            render(ui.bar("Fulfillment", back = onReturn, logo = true), body, listOf(ui.quiet("Back", onReturn))); return
        }
        val stock = try { inventory.snapshot().products.associateBy { it.productId } } catch (_: Exception) { emptyMap() }
        body.add(manifestView(manifest.map { line ->
            val product = stock[line.productId]
            val mode = when {
                product == null -> "inventory mode not configured"
                !product.tracked -> "manual mode · no stock decrement"
                else -> "tracked · ${product.onHand} on hand"
            }
            "${line.productId} · ${line.quantity} · $mode" to 0
        }, alreadyFormatted = true))
        body.add(ui.text("Payment observations alone do not authorize fulfillment. This action requires a paid sale with no unresolved, partial, unconfirmed, late, reorged, or review state.", 14f, colour = ui.pal.secondary))
        body.add(ui.text("The sale update is held steady while the local inventory write completes. Future payment or chain evidence may change sale review status; it will not automatically restock items.", 13f, colour = ui.pal.secondary))
        val fulfillmentId = fulfillmentIds.getOrPut(saleId) { UUID.randomUUID().toString() }
        val confirm = ui.primary("Record handoff") {
            showConfirmation(saleId, fulfillmentId, version, manifest, onReturn)
        }
        val missingMode = manifest.any { it.productId !in stock }
        val insufficient = manifest.firstOrNull { line -> stock[line.productId]?.let { it.tracked && it.onHand < line.quantity } == true }
        confirm.isEnabled = isEligibleForDisplay(sale) && manifest.isNotEmpty() && !missingMode && insufficient == null
        if (insufficient != null) body.add(ui.text("Insufficient tracked stock for ${insufficient.productId}: requested ${insufficient.quantity}, available ${stock[insufficient.productId]?.onHand ?: 0}. Adjust stock or set manual mode before handoff.", 14f))
        if (!confirm.isEnabled && insufficient == null) body.add(ui.text("This sale is not eligible for handoff, or a product's stock mode has not been configured.", 14f))
        render(ui.bar("Fulfillment", back = onReturn, logo = true), body, listOf(confirm, ui.quiet("Back", onReturn)))
    }

    private fun showConfirmation(saleId: String, fulfillmentId: String, expectedVersion: Long,
                                 manifest: List<InventoryPurchaseLine>, onReturn: () -> Unit) {
        val sale = try { ledger.getSale(saleId) } catch (_: Exception) { return showFulfillment(saleId, onReturn) }
        if (!isEligibleForDisplay(sale) || aggregateManifest(sale.items.map { InventoryPurchaseLine(it.productId, it.quantity) }) != manifest) {
            showFulfillment(saleId, onReturn); return
        }
        val lines = manifest.joinToString("\n") { "${it.quantity} × ${it.productId}" }
        AlertDialog.Builder(activity)
            .setTitle("Confirm handoff · TEST ONLY")
            .setMessage("Record that these items were handed to the customer?\n\n$lines\n\nThis saves a local command record. It does not prove physical handoff.")
            .setNegativeButton("Cancel", null)
            .setPositiveButton("Confirm handoff") { _, _ ->
                if (inventoryBlocked) return@setPositiveButton showReopen()
                executeCapability(CheckoutAuthorityCapability.INVENTORY_MANAGE, {
                    ledger.withStableSale(saleId, expectedVersion) { stableSale ->
                        require(aggregateManifest(stableSale.items.map { InventoryPurchaseLine(it.productId, it.quantity) }) == manifest) { "Sale lines changed; refresh before fulfillment." }
                        inventory.fulfill(stableSale, expectedVersion, expectedVersion, fulfillmentId, System.currentTimeMillis() / 1000L)
                    }
                }) { error ->
                    if (error == null) {
                    fulfillmentIds.remove(saleId)
                    showFulfillment(saleId, onReturn)
                    } else if (error is CheckoutAuthorityDeniedException || error is CheckoutAuthorityUnavailableException) {
                        Unit
                    } else if (error is InsufficientInventoryException) {
                    showMessage("Insufficient stock", "Stock is unchanged. Adjust the count or switch to manual mode, then review the sale again.", onReturn)
                    } else {
                    if (inventoryWriteUncertain()) showReopen()
                    else showMessage("Handoff not recorded", "The sale changed or inventory was insufficient. Return to Sales and review the current sale and stock before trying again.", onReturn)
                    }
                }
            }
            .show()
    }

    private fun configureProduct(productId: String, name: String, revision: Int) {
        AlertDialog.Builder(activity).setTitle("Choose stock mode")
            .setItems(arrayOf("Track an on-hand count", "Manual · do not track stock")) { _, which -> configureProductWithMode(productId, name, revision, which == 0) }
            .setNegativeButton("Cancel", null).show()
    }

    private fun configureProductWithMode(productId: String, name: String, revision: Int, tracked: Boolean) {
        val count = EditText(activity).apply {
            inputType = InputType.TYPE_CLASS_NUMBER; hint = "Opening count (0 or more)"
            if (!tracked) visibility = View.GONE
        }
        val reason = EditText(activity).apply { hint = "Reason for opening count" }
        val operationId = UUID.randomUUID().toString()
        AlertDialog.Builder(activity).setTitle("Configure $name")
            .setMessage(if (tracked) "Tracked mode decrements on hand at recorded fulfillment." else "Manual mode keeps no stock count and never decrements stock.")
            .setView(dialogFields(count, reason))
            .setPositiveButton("Save mode and opening count") { _, _ ->
                if (inventoryBlocked) return@setPositiveButton showReopen()
                val opening = if (tracked) count.text.toString().toIntOrNull() else 0
                val why = reason.text.toString().trim()
                if (opening == null || opening !in 0..999999 || why.isBlank()) return@setPositiveButton showMessage("Check the values", "Enter a whole number from 0 to 999999 and a reason.") { showStock() }
                executeCapability(CheckoutAuthorityCapability.INVENTORY_MANAGE, {
                    inventory.addProduct(productId, opening, tracked, why, operationId)
                }) { error -> handleInventoryWrite(error, revision, "Stock mode not saved", "Review the current stock mode before making another change.") }
            }.setNegativeButton("Cancel", null).show()
    }

    private fun adjustProduct(productId: String, name: String, expectedVersion: Long, currentCount: Int, revision: Int) {
        val delta = EditText(activity).apply { inputType = InputType.TYPE_CLASS_NUMBER or InputType.TYPE_NUMBER_FLAG_SIGNED; hint = "Signed change, for example 3 or -1" }
        val reason = EditText(activity).apply { hint = "Reason for count change" }
        val operationId = UUID.randomUUID().toString()
        AlertDialog.Builder(activity).setTitle("Adjust $name")
            .setMessage("Current count: $currentCount")
            .setView(dialogFields(delta, reason))
            .setPositiveButton("Save adjustment") { _, _ ->
                if (inventoryBlocked) return@setPositiveButton showReopen()
                val amount = delta.text.toString().toIntOrNull(); val why = reason.text.toString().trim()
                if (amount == null || amount == 0 || why.isBlank()) return@setPositiveButton showMessage("Check the values", "Enter a nonzero signed count change and a reason.") { showStock() }
                executeCapability(CheckoutAuthorityCapability.INVENTORY_MANAGE, {
                    inventory.adjustStock(productId, amount, why, operationId, expectedVersion)
                }) { error -> handleInventoryWrite(error, revision, "Count not changed", "Stock changed since you opened this form. Review the current count before making another adjustment.") }
            }.setNegativeButton("Cancel", null).show()
    }

    private fun changeMode(productId: String, name: String, isTracked: Boolean, expectedVersion: Long, revision: Int) {
        val target = !isTracked
        val reason = EditText(activity).apply { hint = "Reason for mode change" }
        val operationId = UUID.randomUUID().toString()
        AlertDialog.Builder(activity).setTitle(if (target) "Track $name" else "Use manual mode for $name")
            .setMessage(if (target) "Tracked mode will decrement counts during fulfillment. Enter a reason." else "Manual mode stops count decrements. Existing count is retained. Enter a reason.")
            .setView(reason)
            .setPositiveButton(if (target) "Track stock" else "Use manual mode") { _, _ ->
                if (inventoryBlocked) return@setPositiveButton showReopen()
                val why = reason.text.toString().trim()
                if (why.isBlank()) return@setPositiveButton showMessage("Reason required", "Enter a reason for this mode change.") { showStock() }
                executeCapability(CheckoutAuthorityCapability.INVENTORY_MANAGE, {
                    inventory.setTracked(productId, target, why, operationId, expectedVersion)
                }) { error -> handleInventoryWrite(error, revision, "Mode not changed", "The product mode changed since you opened this form. Review the current mode before making another change.") }
            }.setNegativeButton("Cancel", null).show()
    }

    private fun showMessage(title: String, message: String, next: (() -> Unit)? = null) {
        AlertDialog.Builder(activity).setTitle(title).setMessage(message).setPositiveButton("OK") { _, _ -> next?.invoke() }.show()
    }

    private fun handleInventoryWrite(error: Exception?, revision: Int, title: String, message: String) {
        when (error) {
            null -> if (revision == stockPageRevision) showStock()
            is CheckoutAuthorityDeniedException, is CheckoutAuthorityUnavailableException -> Unit
            else -> if (inventoryWriteUncertain()) showReopen() else showMessage(title, message) { showStock() }
        }
    }

    private fun inventoryWriteUncertain(): Boolean = try {
        inventory.snapshot(); false
    } catch (_: Exception) {
        inventoryBlocked = true; true
    }

    private fun showReopen() {
        inventoryBlocked = true
        val body = listOf(ui.text("A saved inventory write may have an uncertain outcome. Close and reopen this test app, then inspect saved stock and handoff history before continuing. Do not reset the data or repeat the action.", 15f))
        render(ui.bar("Saved inventory needs checking", back = { activity.finish() }, logo = true), body,
            listOf(ui.button("Close test app", ButtonKind.Danger) { activity.finish() }))
    }

    private fun dialogFields(vararg fields: EditText): LinearLayout = LinearLayout(activity).apply {
        orientation = LinearLayout.VERTICAL
        val margin = (16 * activity.resources.displayMetrics.density).toInt()
        setPadding(margin, 0, margin, 0)
        fields.forEach { addView(it) }
    }

    private fun manifestView(lines: List<Pair<String, Int>>, alreadyFormatted: Boolean = false): View = ui.receipt(
        lines.map { (productId, quantity) -> ReceiptLine(if (alreadyFormatted) productId else "$quantity × $productId") }
    )

    private fun isEligibleForDisplay(sale: Sale): Boolean = sale.paid && sale.remainingMinor == 0L && !sale.requiresReview &&
        !sale.lateBitcoinReceipt && sale.overpaidMinor == 0L && sale.overpaidSats == 0L && sale.refunds.isEmpty() &&
        sale.attempts.none { it.tender == Tender.BITCOIN && it.bitcoinEvidence.any { evidence -> evidence.chainState == BitcoinChainState.REORGED } } &&
        sale.attempts.all { attempt -> when (attempt.state) {
            PaymentState.PAID, PaymentState.CONFIRMED, PaymentState.CANCELED -> true
            PaymentState.PARTIAL -> when (attempt.tender) {
                Tender.CARD -> attempt.cardEvidence.maxByOrNull { it.observedAt }?.let { evidence ->
                    evidence.result == CardResult.COMPLETED && evidence.amountMinor > 0 && evidence.amountMinor < attempt.amountMinor && attempt.receivedMinor == evidence.amountMinor
                } == true
                Tender.BITCOIN -> attempt.bitcoinEvidence.filter { it.chainState == BitcoinChainState.ACTIVE && it.receivedSats > 0 }
                    .let { active -> active.isNotEmpty() && active.all { it.confirmations >= it.requiredConfirmations } && attempt.receivedMinor > 0 }
            }
            else -> false
        } } && sale.items.isNotEmpty()

    private fun aggregateManifest(lines: List<InventoryPurchaseLine>): List<InventoryPurchaseLine> {
        val quantities = linkedMapOf<String, Int>()
        for (line in lines) quantities[line.productId] = Math.addExact(quantities[line.productId] ?: 0, line.quantity)
        return quantities.map { InventoryPurchaseLine(it.key, it.value) }
    }
}
