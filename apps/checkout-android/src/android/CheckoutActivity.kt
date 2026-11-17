package org.openbmt.checkout

import android.app.Activity
import android.app.AlertDialog
import android.content.Intent
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Paint
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.view.Gravity
import android.view.View
import android.view.ViewGroup
import android.view.WindowManager
import android.widget.Button
import android.widget.EditText
import android.widget.FrameLayout
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.TextView
import android.widget.Toast
import android.text.InputType
import android.text.InputFilter
import android.text.TextWatcher
import android.text.Editable
import android.view.inputmethod.EditorInfo
import java.io.ByteArrayOutputStream
import java.io.File
import java.io.IOException
import java.math.BigDecimal
import java.math.BigInteger
import java.math.RoundingMode
import java.security.MessageDigest
import java.util.UUID
import java.util.concurrent.locks.ReentrantLock
import kotlin.concurrent.withLock
import org.openbmt.app.AnimatedQrView
import org.openbmt.app.ButtonKind
import org.openbmt.app.DurableFile
import org.openbmt.app.KeystoreSealer
import org.openbmt.app.LineStyle
import org.openbmt.app.ReceiptLine
import org.openbmt.app.ScanFailure
import org.openbmt.app.ScanFlow
import org.openbmt.app.SealedFile
import org.openbmt.app.TorLease
import org.openbmt.app.TorRunner
import org.openbmt.app.Ui
import org.openbmt.app.UrScan
import org.openbmt.app.Who
import org.openbmt.counter.CounterNetwork
import org.openbmt.counter.CounterNode
import org.openbmt.counter.Opened
import org.openbmt.counter.PairStarted
import org.openbmt.counter.Pairing
import org.openbmt.counter.Payment
import org.openbmt.counter.SaleState
import org.openbmt.counter.Shop
import org.openbmt.counter.StartConfig

/** The test-only native checkout host. The Square Sandbox build may subclass this Activity. */
open class CheckoutActivity : Activity() {
    private val main = Handler(Looper.getMainLooper())
    private lateinit var scan: ScanFlow
    private lateinit var ledger: CheckoutLedger
    private lateinit var catalog: CheckoutCatalog
    private lateinit var cashSessions: CheckoutCashSessionStore
    private lateinit var inventory: CheckoutInventory
    private lateinit var cashController: CheckoutCashController
    private lateinit var inventoryController: CheckoutInventoryController
    private lateinit var reportController: CheckoutReportController
    private lateinit var authorityFlow: AndroidCheckoutAuthorityFlow
    private lateinit var authorityUi: CheckoutAuthorityUi
    private var catalogReady = false
    private var cashSessionsReady = false
    private var inventoryReady = false
    private var catalogFault: String? = null
    private var cashSessionsFault: String? = null
    private var inventoryFault: String? = null
    private lateinit var pairingFile: SealedFile
    private lateinit var ui: Ui
    private val nodeDir by lazy { File(noBackupFilesDir, "counter-node") }
    private var pairing: ByteArray? = null
    private var pendingPairingImport: ByteArray? = null
    private var pendingPairingFixture: File? = null
    private var pairingImportInFlight = false
    private var node: CounterNode? = null
    private var torLease: TorLease? = null
    private var pumpThread: Thread? = null
    private var generation = 0
    private var visible = false
    // Fair locking prevents the event-pump worker from reacquiring the FFI gate
    // ahead of a UI operation that is already waiting.
    private val nativeLock = ReentrantLock(true)
    private var activeSaleId: String? = null
    private var activeAttemptId: String? = null
    private var config: org.openbmt.counter.PairingInfo? = null
    private var latestCounterLine = "Counter is starting."
    private var lastNativeRefreshAt = 0L
    private var activePayment: Payment? = null
    private var activeNativeScreen: org.openbmt.counter.Screen? = null
    private var activeUri: String? = null
    private var activeSaleShowing = false
    private var activeSaleRootShown = false
    private var lastActiveSaleRenderKey: String? = null
    private var receiptToResume: String? = null
    private var navigationRevision = 0L
    private var homeStatusView: TextView? = null
    private var homePrimaryButton: Button? = null
    private var homePrimaryAllowed = false
    private var tenderStatusView: TextView? = null
    private var tenderBitcoinButton: Button? = null
    private var historyQueryText = ""
    private var historyTenderFilter: HistoryTender? = null
    private var historyStatusFilter: HistoryStatus? = null
    private val cartQuantities = linkedMapOf<String, Int>()
    private data class CartSelection(val lineId: String, val productId: String, val variantId: String,
                                    val productVersion: Long, val name: String, val unitMinor: Long, val quantity: Int,
                                    val modifiers: List<SelectedModifier> = emptyList())
    private val cartSelections = linkedMapOf<String, CartSelection>()
    private var catalogSearchDraft = ""
    private var catalogWriteBlocked = false
    private var discountDraft = ""
    private var taxRateDraft = ""
    private var tipDraft = ""
    private var businessStoresInitialized = false
    private var businessStartupFault: String? = null
    private var pendingSavedUiState: Bundle? = null

    /** The plain APK has no Square class on its class path. The Sandbox project overrides this. */
    protected open fun createCardCheckoutBridge(): CardCheckoutBridge? = null

    private val cardBridge: CardCheckoutBridge? get() = createCardCheckoutBridge()

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        ui = Ui(this)
        scan = ScanFlow(this, ui)
        // Take this snapshot before any ledger/catalog/session initializer can create a file.
        // It is the fail-closed fresh-install evidence used by manager bootstrap.
        val initialBusinessFiles = BUSINESS_FILE_NAMES.map { File(noBackupFilesDir, it).exists() }
        pendingSavedUiState = savedInstanceState
        authorityUi = CheckoutAuthorityUi(this)
        authorityFlow = AndroidCheckoutAuthorityFlow(
            this,
            businessFilesWereAbsentAtLaunch = initialBusinessFiles.none { it },
            onState = ::onAuthorityState,
        )
        renderAuthorityGate()
        authorityFlow.start()
    }

    override fun onStart() {
        super.onStart()
        visible = true
        if (::authorityFlow.isInitialized) {
            authorityFlow.refresh()
            // refresh publishes Loading synchronously; replace the previous checkout page before
            // returning to the event loop rather than waiting for the posted state callback.
            renderAuthorityGate(CheckoutAccessState.Loading)
        }
    }

    override fun onStop() {
        visible = false
        stopCounterForAuthorityLock()
        if (::authorityFlow.isInitialized) authorityFlow.lock()
        super.onStop()
    }

    private fun stopCounterForAuthorityLock() {
        generation++
        if (!scan.asking) scan.stop()
        pumpThread?.interrupt()
        pumpThread = null
        nativeLock.withLock {
            node?.close()
            node = null
        }
        torLease?.close()
        torLease = null
    }

    override fun onDestroy() {
        scan.stop()
        pendingPairingImport?.fill(0)
        pendingPairingImport = null
        if (::authorityFlow.isInitialized) authorityFlow.destroy()
        super.onDestroy()
    }

    private fun onAuthorityState(state: CheckoutAccessState) {
        if (state !is CheckoutAccessState.SignedIn) {
            // Timeout, denial, recovery and storage faults must stop the payment observer just as
            // backgrounding does. Native state remains durable and is reconciled after sign-in.
            stopCounterForAuthorityLock()
            renderAuthorityGate(state)
            return
        }
        window.clearFlags(WindowManager.LayoutParams.FLAG_SECURE)
        initializeBusinessStores()
        if (businessStartupFault != null) {
            show("Saved test data needs checking", "No saved checkout data was reset. A startup storage operation failed, so checkout and the Regtest counter remain unavailable.", listOf("Close test app" to { finish() }), businessStartupFault)
            return
        }
        if (!businessStoresInitialized) return
        if (reportController.resumePendingExportAfterSignIn()) {
            if (visible && pairing != null && node == null) startCounter()
            return
        }
        if (commitPendingPairingImport()) return
        showInitialCheckoutRoute()
        if (visible && pairing != null && node == null) startCounter()
    }

    private fun renderAuthorityGate(state: CheckoutAccessState = authorityFlow.state()) {
        if (!::authorityUi.isInitialized || authorityFlow.isSignedIn()) return
        window.addFlags(WindowManager.LayoutParams.FLAG_SECURE)
        authorityUi.render(state, object : CheckoutAuthorityUi.Actions {
            override fun refresh() = authorityFlow.refresh()
            override fun prepareManagerEnrollment() = authorityFlow.prepareManagerEnrollment()
            override fun confirmRecoveryCode(value: CharArray) = authorityFlow.confirmRecoveryCode(value)
            override fun enrollManager(name: String, pin: CharArray) = authorityFlow.enrollManager(name, pin)
            override fun signIn(actorId: String, pin: CharArray) = authorityFlow.signIn(actorId, pin)
            override fun beginManagerRecovery() = authorityFlow.beginManagerRecovery()
            override fun prepareManagerRecovery(currentCode: CharArray, newPin: CharArray) =
                authorityFlow.prepareManagerRecovery(currentCode, newPin)
            override fun confirmNextRecoveryCode(value: CharArray) = authorityFlow.confirmNextRecoveryCode(value)
        })
    }

    /** Business stores are opened only after local authority has produced a signed-in actor. */
    private fun initializeBusinessStores() {
        if (businessStoresInitialized || !authorityFlow.isSignedIn()) return
        businessStoresInitialized = true
        val noBackup = noBackupFilesDir
        val renderer: (View, List<View>, List<Button>) -> Unit = { bar, body, actions -> showUiPage(bar, body, actions) }
        try {
            pairingFile = SealedFile(noBackup, KeystoreSealer(PAIRING_KEY_ALIAS), PAIRING_FILE)
            // A damaged ledger is a blocking fault. Never replace it with an empty ledger on startup.
            ledger = CheckoutLedger(AndroidLedgerStorage(noBackup, LEDGER_FILE))
            ledger.initializeEvents()
            pairing = pairingFile.load()
        } catch (e: Exception) {
            businessStartupFault = safeDetail(e)
            return
        }
        try {
            catalog = CheckoutCatalog(AndroidLedgerStorage(noBackup, CATALOG_FILE))
            catalogReady = true
        } catch (e: Exception) {
            catalogFault = safeDetail(e)
            catalogWriteBlocked = catalogFault!!.contains("uncertain", true) || catalogFault!!.contains("poisoned", true)
        }
        try {
            cashSessions = authorityFlow.cashSessionStore(
                AndroidLedgerStorage(noBackup, CASH_SESSIONS_FILE), MAIN_CASH_DRAWER_ID,
            )
            cashController = CheckoutCashController(
                this, cashSessions, ledger, authorityFlow::executeCommand, renderer,
            ) { showHome() }
            cashSessionsReady = true
        } catch (e: Exception) {
            cashSessionsFault = safeDetail(e)
        }
        try {
            inventory = CheckoutInventory(AndroidLedgerStorage(noBackup, INVENTORY_FILE))
            inventoryReady = true
        } catch (e: Exception) {
            inventoryFault = safeDetail(e)
        }
        if (catalogReady && inventoryReady) try {
            inventoryController = CheckoutInventoryController(
                this, catalog, inventory, ledger, authorityFlow::executeCapability, renderer,
            ) { showCatalogManagement(true) }
        } catch (e: Exception) {
            inventoryReady = false
            inventoryFault = safeDetail(e)
        }
        reportController = CheckoutReportController(
            this, ui, ledger, authorityFlow::executeCapability, renderer,
        ) { showHome() }
        restoreSavedUiState(pendingSavedUiState)
        pendingSavedUiState = null
        if (pairing == null) consumePrivateFixture()
    }

    private fun showInitialCheckoutRoute() {
        if (pairing == null) showSetup() else {
            val savedReceipt = receiptToResume
            if (savedReceipt == null) showHome() else try { showReceipt(savedReceipt) } catch (_: Exception) {
                receiptToResume = null
                showSalesIfPossible()
            }
        }
    }

    override fun onSaveInstanceState(outState: Bundle) {
        outState.putStringArrayList(CART_IDS_KEY, ArrayList(cartQuantities.keys))
        outState.putIntArray(CART_QUANTITIES_KEY, cartQuantities.values.toIntArray())
        val savedLines = cartSelections.values.toList()
        outState.putStringArrayList(CART_LINE_IDS_KEY, ArrayList(savedLines.map { it.lineId }))
        outState.putStringArrayList(CART_PRODUCT_IDS_KEY, ArrayList(savedLines.map { it.productId }))
        outState.putStringArrayList(CART_VARIANT_IDS_KEY, ArrayList(savedLines.map { it.variantId }))
        outState.putLongArray(CART_PRODUCT_VERSIONS_KEY, savedLines.map { it.productVersion }.toLongArray())
        outState.putStringArrayList(CART_LINE_NAMES_KEY, ArrayList(savedLines.map { it.name }))
        outState.putLongArray(CART_LINE_PRICES_KEY, savedLines.map { it.unitMinor }.toLongArray())
        outState.putIntArray(CART_LINE_QUANTITIES_KEY, savedLines.map { it.quantity }.toIntArray())
        val savedModifiers = savedLines.flatMap { line -> line.modifiers.map { line.lineId to it } }
        outState.putStringArrayList(CART_MODIFIER_LINE_IDS_KEY, ArrayList(savedModifiers.map { it.first }))
        outState.putStringArrayList(CART_MODIFIER_IDS_KEY, ArrayList(savedModifiers.map { it.second.modifierId }))
        outState.putStringArrayList(CART_MODIFIER_NAMES_KEY, ArrayList(savedModifiers.map { it.second.name }))
        outState.putLongArray(CART_MODIFIER_PRICES_KEY, savedModifiers.map { it.second.amountMinor }.toLongArray())
        outState.putString(CATALOG_SEARCH_KEY, catalogSearchDraft)
        outState.putString(DISCOUNT_DRAFT_KEY, discountDraft)
        outState.putString(TAX_DRAFT_KEY, taxRateDraft)
        outState.putString(TIP_DRAFT_KEY, tipDraft)
        outState.putString(RECEIPT_KEY, receiptToResume)
        outState.putString(HISTORY_TEXT_KEY, historyQueryText)
        outState.putString(HISTORY_TENDER_KEY, historyTenderFilter?.name)
        outState.putString(HISTORY_STATUS_KEY, historyStatusFilter?.name)
        super.onSaveInstanceState(outState)
    }

    private fun restoreSavedUiState(saved: Bundle?) {
        discountDraft = saved?.getString(DISCOUNT_DRAFT_KEY).orEmpty().take(12)
        taxRateDraft = saved?.getString(TAX_DRAFT_KEY).orEmpty().take(12)
        tipDraft = saved?.getString(TIP_DRAFT_KEY).orEmpty().take(12)
        receiptToResume = saved?.getString(RECEIPT_KEY)
        historyQueryText = saved?.getString(HISTORY_TEXT_KEY).orEmpty().take(128)
        historyTenderFilter = saved?.getString(HISTORY_TENDER_KEY)?.let { value -> runCatching { HistoryTender.valueOf(value) }.getOrNull() }
        historyStatusFilter = saved?.getString(HISTORY_STATUS_KEY)?.let { value -> runCatching { HistoryStatus.valueOf(value) }.getOrNull() }
        val ids = saved?.getStringArrayList(CART_IDS_KEY)
        val quantities = saved?.getIntArray(CART_QUANTITIES_KEY)
        if (ids != null && quantities != null) {
            cartQuantities.clear()
            for (index in 0 until minOf(ids.size, quantities.size)) {
                val id = ids[index]
                val quantity = quantities[index]
                if ((id == "latte" || id == "bagel") && quantity in 1..MAX_CART_QUANTITY) cartQuantities[id] = quantity
            }
        }
        saved?.let(::restoreFrozenCart)
        migrateLegacySampleCart()
        catalogSearchDraft = saved?.getString(CATALOG_SEARCH_KEY).orEmpty().take(128)
    }

    private fun restoreFrozenCart(saved: Bundle) {
        val lineIds = saved.getStringArrayList(CART_LINE_IDS_KEY) ?: return
        val productIds = saved.getStringArrayList(CART_PRODUCT_IDS_KEY) ?: return
        val variantIds = saved.getStringArrayList(CART_VARIANT_IDS_KEY) ?: return
        val versions = saved.getLongArray(CART_PRODUCT_VERSIONS_KEY) ?: return
        val names = saved.getStringArrayList(CART_LINE_NAMES_KEY) ?: return
        val prices = saved.getLongArray(CART_LINE_PRICES_KEY) ?: return
        val quantities = saved.getIntArray(CART_LINE_QUANTITIES_KEY) ?: return
        val modifierLines = saved.getStringArrayList(CART_MODIFIER_LINE_IDS_KEY).orEmpty()
        val modifierIds = saved.getStringArrayList(CART_MODIFIER_IDS_KEY).orEmpty()
        val modifierNames = saved.getStringArrayList(CART_MODIFIER_NAMES_KEY).orEmpty()
        val modifierPrices = saved.getLongArray(CART_MODIFIER_PRICES_KEY) ?: LongArray(0)
        val modifiersByLine = linkedMapOf<String, MutableList<SelectedModifier>>()
        for (i in 0 until minOf(modifierLines.size, modifierIds.size, modifierNames.size, modifierPrices.size, MAX_CART_LINES * MAX_SAVED_MODIFIERS)) {
            if (modifierIds[i].isNotBlank() && modifierNames[i].isNotBlank() && modifierPrices[i] >= 0) {
                modifiersByLine.getOrPut(modifierLines[i]) { mutableListOf() }.add(SelectedModifier(modifierIds[i], modifierNames[i].take(120), modifierPrices[i]))
            }
        }
        val count = minOf(lineIds.size, productIds.size, variantIds.size, versions.size, names.size, prices.size, quantities.size, MAX_CART_LINES)
        cartSelections.clear()
        for (i in 0 until count) {
            if (lineIds[i].isNotBlank() && productIds[i].isNotBlank() && variantIds[i].isNotBlank() && names[i].isNotBlank() && prices[i] >= 0 && quantities[i] in 1..MAX_CART_QUANTITY) {
                cartSelections[lineIds[i]] = CartSelection(lineIds[i], productIds[i], variantIds[i], versions[i], names[i].take(320), prices[i], quantities[i], modifiersByLine[lineIds[i]].orEmpty().take(MAX_SAVED_MODIFIERS))
            }
        }
        cartQuantities.clear()
    }

    private fun migrateLegacySampleCart() {
        if (!catalogReady || cartSelections.isNotEmpty() || cartQuantities.isEmpty()) return
        for ((productId, quantity) in cartQuantities.toMap()) {
            val product = catalog.getProduct(productId) ?: continue
            val variant = product.variants.firstOrNull() ?: continue
            addCatalogSelection(product, variant, amount = quantity)
        }
        cartQuantities.clear()
    }

    override fun onRequestPermissionsResult(requestCode: Int, permissions: Array<out String>, grantResults: IntArray) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults)
        scan.permissionResult(requestCode, grantResults)
    }

    @Deprecated("The Android document picker returns its URI here on the minimum supported API.")
    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        super.onActivityResult(requestCode, resultCode, data)
        if (::reportController.isInitialized && reportController.onActivityResult(requestCode, resultCode, data)) {
            if (authorityFlow.isSignedIn()) reportController.resumePendingExportAfterSignIn()
            return
        }
        if (requestCode != PICK_REGTEST_RECORD || resultCode != RESULT_OK) return
        val uri = data?.data ?: return
        try {
            val stream = contentResolver.openInputStream(uri) ?: throw IOException("The selected file could not be opened.")
            stream.use { input ->
                val bytes = readLimited(input, MAX_PAIRING_BYTES)
                queuePairingImport(bytes)
            }
        } catch (e: Exception) {
            showSetup("The selected file was not saved. It must be a valid Regtest pairing record. Details: ${safeDetail(e)}")
        }
    }

    private fun consumePrivateFixture() {
        if (applicationInfo.flags and android.content.pm.ApplicationInfo.FLAG_DEBUGGABLE == 0) return
        val pending = File(filesDir, FIXTURE_FILE)
        if (!pending.isFile) return
        try {
            if (pending.length() > MAX_PAIRING_BYTES) throw IOException("The private fixture is too large.")
            queuePairingImport(pending.readBytes(), pending)
        } catch (e: Exception) {
            showSetup("The private fixture was not accepted. It must be a Regtest pairing record. Details: ${safeDetail(e)}")
        }
    }

    private fun queuePairingImport(bytes: ByteArray, fixture: File? = null) {
        pendingPairingImport?.fill(0)
        pendingPairingImport = bytes.copyOf()
        pendingPairingFixture = fixture
        if (authorityFlow.isSignedIn()) commitPendingPairingImport()
    }

    /** Persist receive configuration only after a fresh manager capability check. */
    private fun commitPendingPairingImport(): Boolean {
        val bytes = pendingPairingImport ?: return pairingImportInFlight
        if (pairingImportInFlight) return true
        pairingImportInFlight = true
        var imported: org.openbmt.counter.PairingInfo? = null
        authorityFlow.executeCapability(CheckoutAuthorityCapability.WALLET_CONFIGURE, {
            val info = Shop.pairing(bytes, nodeDir.path)
            require(info.network == CounterNetwork.Regtest) { "Only Regtest records are accepted by this test app." }
            require(info.settings.currency == "USD" || info.settings.currency == "SAT") { "This test app accepts USD or SAT pricing." }
            pairingFile.save(bytes)
            imported = info
        }) { error ->
            pairingImportInFlight = false
            if (error is CheckoutAuthorityDeniedException || error is CheckoutAuthorityUnavailableException) return@executeCapability
            if (error != null) {
                pendingPairingImport?.fill(0)
                pendingPairingImport = null
                pendingPairingFixture = null
                showSetup("The pairing record was not saved. Details: ${safeDetail(error)}")
                return@executeCapability
            }
            val info = imported ?: return@executeCapability showSetup("The pairing record was not saved. Reopen setup and try again.")
            pairing = bytes.copyOf()
            config = info
            pendingPairingImport?.fill(0)
            pendingPairingImport = null
            pendingPairingFixture?.delete()
            pendingPairingFixture = null
            showMessage("Regtest setup saved", "The app sealed the watch-only pairing after fresh manager authorization. This is a test environment; do not accept real funds or hand over goods.", "Network: Regtest · Pricing: ${info.settings.currency}") {
                showHome()
                if (visible) startCounter()
            }
        }
        return true
    }

    private fun showSetup(message: String? = null) {
        receiptToResume = null
        val text = buildString {
            append("TEST ONLY · REGTEST\n\n")
            append("Pair with the public wallet offer shown by the owner device, or import the explicitly labelled developer fixture. This checkout keeps no wallet spending key.\n\n")
            append("The optional Sandbox card path is absent from this base build. No real funds or goods handover.")
            if (message != null) append("\n\n$message")
        }
        show("Set up test checkout", text, listOf(
            "Pair public wallet" to { scanOffer() },
            "Import developer Regtest pairing file" to { pickFixture() },
        ))
    }

    private fun pickFixture() {
        val intent = Intent(Intent.ACTION_OPEN_DOCUMENT).apply {
            addCategory(Intent.CATEGORY_OPENABLE)
            type = "application/octet-stream"
        }
        startActivityForResult(intent, PICK_REGTEST_RECORD)
    }

    private fun scanOffer() {
        scan.begin(
            show = { showView("Pair public wallet", it) },
            onBytes = { offer -> readOffer(offer) },
            onFailed = { failure -> failedScan(failure) },
            onCancel = { showSetup() },
            ur = UrScan(setOf(TYPE_BYTES), Who.MANAGER) { _, payload -> readOffer(payload) },
        )
    }

    private fun readOffer(offer: ByteArray) {
        try {
            val started = Pairing.begin(offer, "Regtest checkout", nowSecs())
            if (started.wallet != org.openbmt.counter.OwnWalletKind.None) {
                showAddressCheck(started)
            } else {
                showMatch(started)
            }
        } catch (e: Exception) {
            endPairing()
            showSetup("The wallet offer was not accepted. Details: ${safeDetail(e)}")
        }
    }

    private fun showAddressCheck(started: PairStarted) {
        show(
            "Check the wallet address",
            "Compare this first receive address with the wallet's own screen. The shop should be a Regtest Bitcoin Standard or USD test wallet.\n\n${groups(started.address)}\n\n${started.wallet}",
            listOf(
                "Same address" to {
                    try {
                        showMatch(Pairing.addressSame(nowSecs(), "Regtest checkout"))
                    } catch (e: Exception) {
                        endPairing(); showSetup("The address check did not finish. Details: ${safeDetail(e)}")
                    }
                },
                "Different address" to { Pairing.addressDifferent(); showSetup("The addresses differed. Nothing was saved.") },
                "Cancel" to { endPairing(); showSetup() },
            ),
        )
    }

    private fun showMatch(started: PairStarted) {
        val made = AnimatedQrView.make(this, Who.MANAGER, TYPE_BYTES, started.request, (resources.displayMetrics.heightPixels * 0.44f).toInt())
        val qr = made.view
        if (qr == null) {
            endPairing()
            return showSetup("The pairing code could not be drawn. Nothing was saved. Details: ${made.words?.heading ?: "QR creation failed"}")
        }
        showWithExtra(
            "Compare the four words",
            "The other device must show the same four words before you continue. This pairing request is for the test counter.",
            started.words.joinToString("   "),
            qr,
            listOf(
                "They match" to {
                    try {
                        Pairing.match(nowSecs())
                        scanBundle()
                    } catch (e: Exception) {
                        endPairing(); showSetup("The word check did not finish. Details: ${safeDetail(e)}")
                    }
                },
                "They differ" to {
                    try { Pairing.differ() } catch (_: Exception) {}
                    showSetup("The words differed. Nothing was saved.")
                },
                "Cancel" to { endPairing(); showSetup() },
            ),
        )
    }

    private fun scanBundle() {
        scan.begin(
            show = { showView("Scan owner bundle", it) },
            onBytes = { bundle -> finishPairing(bundle) },
            onFailed = { failure -> failedScan(failure) },
            onCancel = { endPairing(); showSetup() },
            ur = UrScan(setOf(TYPE_BYTES), Who.MANAGER) { _, payload -> finishPairing(payload) },
        )
    }

    private fun finishPairing(bundle: ByteArray) {
        try {
            val record = Pairing.finish(nowSecs(), bundle)
            endPairing()
            queuePairingImport(record)
        } catch (e: Exception) {
            endPairing()
            showSetup("The owner bundle was not saved. Details: ${safeDetail(e)}")
        }
    }

    private fun failedScan(failure: ScanFailure) {
        endPairing()
        val reason = failure.words?.heading ?: if (failure.off) "Camera permission is needed to pair." else "The camera could not read the code."
        showSetup(reason)
    }

    private fun endPairing() {
        scan.stop()
        try { Pairing.cancel() } catch (_: Exception) {}
    }

    private fun startCounter() {
        if (!::authorityFlow.isInitialized || !authorityFlow.isSignedIn() || !businessStoresInitialized || businessStartupFault != null) return
        if (!visible || node != null) return
        val record = pairing ?: return showSetup("No pairing record is saved.")
        val info = try { Shop.pairing(record, nodeDir.path) } catch (e: Exception) {
            return showMessage("The pairing could not be read", "No network connection was started. Go back to setup or report this test-build fault.", safeDetail(e)) { showSetup() }
        }
        if (info.network != CounterNetwork.Regtest) {
            config = null
            return showMessage("This network is not allowed", "This checkout build accepts Regtest only. No counter node was started.", "Network: ${info.network}") { showSetup() }
        }
        if (info.settings.currency !in setOf("USD", "SAT")) {
            config = null
            return showMessage("This currency is not supported", "This test checkout accepts USD with a fixed developer quote, or Bitcoin Standard in sats.", "Currency: ${info.settings.currency}") { showSetup() }
        }
        config = info
        val run = ++generation
        showStatus("Connecting to Regtest", "The watch-only counter is starting. No wallet signing key is loaded.")
        val startupNavigationRevision = navigationRevision
        Thread({
            var lease: TorLease? = null
            try {
                val proxy = if (info.settings.torOn) {
                    lease = TorRunner.acquire(noBackupFilesDir)
                    val port = lease.awaitPort(180_000)
                    "127.0.0.1:$port"
                } else null
                nodeDir.mkdirs()
                val started = CounterNode.start(StartConfig(nodeDir.path, nowSecs(), record, proxy))
                main.post {
                    if (!visible || run != generation) {
                        started.close()
                        lease?.close()
                        return@post
                    }
                    torLease = lease
                    node = started
                    latestCounterLine = "Counter connected to Regtest."
                    resumePendingSale()
                    val receiptId = receiptToResume
                    val pendingTenderId = pendingTenderSaleId()
                    when {
                        receiptId != null -> showReceipt(receiptId)
                        navigationRevision == startupNavigationRevision -> when {
                            pendingTenderId != null -> showTenderChoice(pendingTenderId)
                            activeSaleId != null -> renderActiveSale()
                            else -> showHome()
                        }
                        else -> Unit
                    }
                    startPump(run, started)
                }
            } catch (e: Exception) {
                lease?.close()
                main.post {
                    if (run == generation && navigationRevision == startupNavigationRevision && receiptToResume == null) {
                        showStatus("Counter did not start", "No payment code is available. Keep the test fixture unchanged and report this fault.", safeDetail(e))
                    }
                }
            } catch (e: LinkageError) {
                lease?.close()
                main.post {
                    if (run == generation && navigationRevision == startupNavigationRevision && receiptToResume == null) {
                        showStatus("The native counter is unavailable", "No payment code is available. Rebuild the test APK with its native libraries.", safeDetail(e))
                    }
                }
            }
        }, "checkout-node-start").apply { isDaemon = true; start() }
    }

    private fun startPump(run: Int, runningNode: CounterNode) {
        val thread = Thread({
            while (visible && run == generation) {
                try {
                    val pump = nativeLock.withLock { runningNode.pump(nowSecs(), 250) }
                    main.post {
                        if (run != generation || !visible) return@post
                        if (pump.stopped) latestCounterLine = "Counter node stopped."
                        val second = nowSecs()
                        if (second > lastNativeRefreshAt) {
                            lastNativeRefreshAt = second
                            reconcileBitcoinAttempts()
                            if (activeSaleShowing && activeSaleId != null) renderActiveSale() else refreshHomeReadiness()
                        }
                    }
                    try { Thread.sleep(50L) } catch (_: InterruptedException) { break }
                } catch (e: Exception) {
                    main.post {
                        if (run == generation) {
                            latestCounterLine = "The counter check stopped."
                            if (activeSaleId != null) showStatus("Payment check stopped", "Do not hand over goods. Restart the app to try the Regtest connection again.", safeDetail(e))
                        }
                    }
                    break
                }
            }
        }, "checkout-counter-pump").apply { isDaemon = true }
        pumpThread = thread
        thread.start()
    }

    private fun resumePendingSale() {
        val sale = ledger.listSales().asReversed().firstOrNull { s ->
            s.attempts.any { it.tender == Tender.BITCOIN && it.state !in setOf(PaymentState.CANCELED, PaymentState.CONFIRMED, PaymentState.PAID) }
        } ?: return
        val attempt = sale.attempts.last { it.tender == Tender.BITCOIN && it.state !in setOf(PaymentState.CANCELED, PaymentState.CONFIRMED, PaymentState.PAID) }
        activeSaleId = sale.saleId
        activeAttemptId = attempt.attemptId
        attempt.counterIndex?.let { index ->
            try {
                val screen = nativeLock.withLock { node?.judge(nowSecs(), index) } ?: return
                activeNativeScreen = screen
                activePayment = screen.payment
                activeUri = screen.payment?.copyUri
            } catch (_: Exception) { }
        }
    }

    private fun pendingTenderSaleId(): String? = ledger.listSales().asReversed().firstOrNull { sale ->
        sale.attempts.isEmpty() && sale.remainingMinor > 0 && !sale.requiresReview
    }?.saleId

    private fun reconcileBitcoinAttempts() {
        val currentNode = node ?: return
        val runSales = ledger.listSales()
        for (sale in runSales) {
            for (attempt in sale.attempts) {
                if (attempt.tender != Tender.BITCOIN) continue
                val index = attempt.counterIndex ?: continue
                try {
                    val screen = nativeLock.withLock { currentNode.judge(nowSecs(), index) }
                    val received = screen.receivedSats.coerceAtLeast(0)
                    val previous = attempt.bitcoinEvidence.firstOrNull()
                    if (attempt.cancellationRequested && !attempt.cancellationAcknowledged && screen.state == SaleState.Cancelled && received == 0L) {
                        try { ledger.acknowledgeBitcoinCancellation(sale.saleId, attempt.attemptId) } catch (_: Exception) {}
                    }
                    val enoughDepth = screen.state == SaleState.Confirmed || screen.state == SaleState.Final
                    // Counter FFI exposes its configured check state, not raw block depth. This 1/1 witness means
                    // only that the native Regtest counter has satisfied its confirmation policy; 0/1 is waiting.
                    val confirmations = if (enoughDepth) 1 else 0
                    val chainState = if (screen.reopened && !enoughDepth) BitcoinChainState.REORGED else BitcoinChainState.ACTIVE
                    if (previous != null && previous.receivedSats == received && previous.confirmations == confirmations && previous.requiredConfirmations == 1 && previous.chainState == chainState) {
                        if (activeAttemptId == attempt.attemptId) {
                            activeNativeScreen = screen
                            activePayment = screen.payment
                            activeUri = screen.payment?.copyUri
                        }
                        continue
                    }
                    val observed = maxOf(nowSecs(), (previous?.observedAt ?: -1L) + 1L)
                    ledger.recordBitcoinEvidence(
                        sale.saleId,
                        attempt.attemptId,
                        "counter:${attempt.attemptId}",
                        "openbmt-counter-ffi:${index}",
                        received,
                        confirmations,
                        1,
                        observed,
                        chainState,
                    )
                    if (activeAttemptId == attempt.attemptId) {
                        activeNativeScreen = screen
                        activePayment = screen.payment
                        activeUri = screen.payment?.copyUri
                    }
                } catch (_: Exception) {
                    // Keep the last durable evidence. A next pump or app restart retries the native read.
                }
            }
        }
    }

    private fun showHome() {
        receiptToResume = null
        activeSaleShowing = false
        if (pairing == null) return showSetup()
        val info = config ?: try { pairing?.let { Shop.pairing(it, nodeDir.path) } } catch (_: Exception) { null }
        if (info == null) return showStatus("Regtest checkout is not ready", "The pairing could not be read. No payment code is available.")
        config = info
        val healthy = try { node?.let { nativeLock.withLock { it.status().health?.healthy == true } } ?: false } catch (_: Exception) { false }
        latestCounterLine = when {
            hasUnresolvedSale() -> "An earlier sale needs follow-up. New orders will be saved separately."
            node == null -> "Counter is starting."
            healthy -> "Regtest counter checks are healthy."
            else -> "Waiting for the Regtest counter's native network checks."
        }
        if (info.settings.currency == "USD") showCatalog(healthy) else showManualCheckout(healthy)
    }

    private fun showCatalog(healthy: Boolean, query: String = catalogSearchDraft) {
        if (!catalogReady || catalogWriteBlocked) return showManualCheckout(healthy, catalogUnavailableMessage())
        catalogSearchDraft = query.take(128)
        val body = ArrayList<View>()
        body.add(ui.receipt(listOf(
            ReceiptLine("Saved local catalog · USD", LineStyle.Strong),
            ReceiptLine("TEST ONLY · Regtest · no real funds or handover", LineStyle.Small),
        )))
        body.add(ui.text("Local saved items", 13f, colour = ui.pal.secondary))
        if (hasUnresolvedSale()) body.add(ui.receipt(listOf(
            ReceiptLine("Earlier payment needs review. Open Sales.", LineStyle.Strong),
        )))
        val searchField = ui.field("Search items or identifiers")
        searchField.setSingleLine(true)
        searchField.setText(catalogSearchDraft)
        val searchRow = LinearLayout(this).apply { orientation = LinearLayout.HORIZONTAL; gravity = Gravity.CENTER_VERTICAL }
        searchRow.addView(searchField, LinearLayout.LayoutParams(0, -2, 1f).also { it.rightMargin = ui.px(6) })
        searchRow.addView(ui.button("Search", ButtonKind.Secondary) { showCatalog(healthy, searchField.text.toString()) }, LinearLayout.LayoutParams(-2, -2))
        searchRow.addView(ui.button("Add code", ButtonKind.Secondary) { lookupCatalogCode(searchField.text.toString(), healthy) }, LinearLayout.LayoutParams(-2, -2))
        body.add(searchRow)
        val products = try {
            if (catalogSearchDraft.isBlank()) catalog.listProducts().take(MAX_MANAGE_PRODUCTS) else catalog.search(catalogSearchDraft)
        } catch (e: Exception) { return showCatalogBlocked(safeDetail(e)) }
        if (products.isEmpty()) body.add(ui.text("No matching saved items.", 14f, colour = ui.pal.secondary))
        for (product in products) {
            body.add(ui.receipt(listOf(ReceiptLine(product.name, LineStyle.Strong))))
            for (variant in product.variants) {
                val quantity = cartSelections.values.filter { it.productId == product.productId && it.variantId == variant.variantId }.sumOf { it.quantity }
                body.add(ui.receipt(listOf(
                    ReceiptLine("${variant.name} · ${formatAmount(variant.priceMinor, "USD")} each", LineStyle.Strong),
                    ReceiptLine(listOfNotNull(variant.sku?.let { "SKU $it" }, variant.barcode?.let { "Barcode $it" }, "In basket $quantity", if (product.modifiers.isNotEmpty()) "Options" else null).joinToString(" · ")),
                )))
            val controls = LinearLayout(this).apply {
                orientation = LinearLayout.HORIZONTAL
                gravity = Gravity.CENTER_VERTICAL
            }
            val removeButton = ui.button("Remove", ButtonKind.Secondary) {
                    removeCatalogSelection(product.productId, variant.variantId)
                    showCatalog(healthy)
            }
            removeButton.isEnabled = quantity > 0
            controls.addView(removeButton, LinearLayout.LayoutParams(0, -2, 1f).also { it.rightMargin = ui.px(6) })
            controls.addView(ui.button("Add", ButtonKind.Primary) {
                    addCatalogSelection(product, variant)
                    showCatalog(healthy)
            }, LinearLayout.LayoutParams(0, -2, 1f).also { it.leftMargin = ui.px(6) })
            body.add(controls)
            }
        }
        val total = try { cartTotalMinor() } catch (_: ArithmeticException) {
            return showHomeWithError("The saved basket total is outside the supported range.")
        }
        val cartCount = cartSelections.values.sumOf { it.quantity }
        body.add(ui.facts(listOf(
            "Items" to cartCount.toString(),
            "Total" to formatAmount(total, "USD"),
        )))
        val statusView = ui.text(latestCounterLine, 13f, colour = ui.pal.secondary)
        body.add(statusView)
        val review = ui.primary("Review basket · ${formatAmount(total, "USD")}") { showCartReview(healthy) }
        review.isEnabled = cartSelections.isNotEmpty()
        val actions = mutableListOf(
            review,
            ui.quiet("Sales and receipts") { showSales() },
            ui.secondary("Shop tools") { showShopTools(healthy) },
        )
        showUiPage(ui.bar("Checkout", sub = "Regtest · local items", logo = true, gear = { showAbout() }), body, actions)
        homeStatusView = statusView
        homePrimaryButton = review
        homePrimaryAllowed = cartSelections.isNotEmpty()
    }

    private fun showShopTools(healthy: Boolean) {
        val body = arrayListOf<View>(
            ui.text("TEST ONLY · local app data · no real funds", 14f, bold = true),
            ui.text("Local test-only staff authority is active. Provider catalog sync, camera or physical scanner, and production payment processing are not connected. Inventory counts are configured separately from item prices.", 13f, colour = ui.pal.secondary),
            ui.button("Manual amount") { showManualCheckout(healthy) },
            ui.button("Manage items") { showCatalogManagement(healthy) },
            ui.button("Local inventory · test only") { showInventoryStock() },
            ui.button("Cash sessions · test only") { showCashSessions() },
            ui.button("Reports · local ledger") { reportController.show() },
        )
        showUiPage(ui.bar("Shop tools", back = { showCatalog(healthy) }), body,
            listOf(ui.secondary("Back to checkout") { showCatalog(healthy) }))
    }

    private fun showCartReview(healthy: Boolean) {
        if (cartSelections.isEmpty()) return showCatalog(healthy)
        if (cartSelections.size > MAX_CART_LINES || cartSelections.values.any { it.quantity !in 1..MAX_CART_QUANTITY })
            return showHomeWithError("The saved basket exceeds supported line or quantity limits.")
        val items = cartItemsSnapshot()
        checkedItemsTotal(items) // Validate the current cart before presenting editable adjustments.
        val body = ArrayList<View>()
        body.add(ui.text("Review the basket before creating its saved sale.", 15f))
        body.add(ui.receipt(cartReviewReceiptLines()))
        body.add(ui.text("Explicit order adjustments · test merchant settings only. Tax is one exclusive flat rate entered by the merchant; no jurisdiction lookup or automatic tax calculation.", 13f, colour = ui.pal.secondary))
        val discount = reviewAmountField("Order discount (USD)", discountDraft)
        val tax = reviewPercentField("Merchant tax rate (%)", taxRateDraft)
        val tip = reviewAmountField("Optional tip (USD)", tipDraft)
        body.add(discount); body.add(tax); body.add(tip)
        val pricingView = ui.text("", 14f)
        body.add(pricingView)
        val updateDrafts = {
            discountDraft = discount.text.toString().take(12)
            taxRateDraft = tax.text.toString().take(12)
            tipDraft = tip.text.toString().take(12)
            updatePricingPreview(discountDraft, taxRateDraft, tipDraft, pricingView)
        }
        listOf(discount, tax, tip).forEach { field -> field.addTextChangedListener(object : TextWatcher {
            override fun beforeTextChanged(s: CharSequence?, start: Int, count: Int, after: Int) = Unit
            override fun onTextChanged(s: CharSequence?, start: Int, before: Int, count: Int) = updateDrafts()
            override fun afterTextChanged(s: Editable?) = Unit
        }) }
        updatePricingPreview(discountDraft, taxRateDraft, tipDraft, pricingView)
        val charge = ui.primary("Save priced sale · choose payment") {
            try {
                val pricing = freezeCartPricing(discountDraft, taxRateDraft, tipDraft)
                if (pricing.totalMinor <= 0) throw IllegalArgumentException("The final sale total must be positive.")
                confirmSaleCreation(pricing.totalMinor, "USD", items, pricing)
            } catch (e: Exception) {
                Toast.makeText(this, e.message ?: "Check the price adjustments.", Toast.LENGTH_LONG).show()
            }
        }
        showUiPage(
            ui.bar("Review basket", back = { showCatalog(healthy) }), body,
            listOf(charge, ui.secondary("Keep editing") { showCatalog(healthy) }),
        )
    }

    private fun showManualCheckout(healthy: Boolean, notice: String? = null) {
        val info = config ?: return showSetup()
        val currency = info.settings.currency
        val quoteLine = if (currency == "SAT") "Bitcoin Standard uses the entered whole-satoshi amount." else "Developer Regtest quote: $100,000/BTC. This is fixed test data, not a live price."
        val field = ui.field(if (currency == "SAT") "Amount in sats" else "Amount in USD")
        field.inputType = if (currency == "SAT") android.text.InputType.TYPE_CLASS_NUMBER else android.text.InputType.TYPE_CLASS_NUMBER or android.text.InputType.TYPE_NUMBER_FLAG_DECIMAL
        field.setSingleLine(true)
        val statusView = ui.text(latestCounterLine, 13f, colour = ui.pal.secondary)
        val body = listOf(
            ui.receipt(listOf(ReceiptLine("Manual amount", LineStyle.Strong), ReceiptLine("TEST ONLY · Regtest", LineStyle.Small))),
            notice?.let { ui.text(it, 13f, colour = ui.pal.secondary) } ?: if (hasUnresolvedSale()) ui.receipt(listOf(
                ReceiptLine("Earlier sale needs follow-up", LineStyle.Strong),
                ReceiptLine("This manual amount creates a separate sale; earlier attempts remain unchanged.", LineStyle.Small),
            )) else ui.text("Manual test amount", 13f, colour = ui.pal.secondary),
            ui.text(quoteLine, 14f, colour = ui.pal.secondary),
            field,
            statusView,
        )
        val review = ui.primary("Review amount · choose payment") { createManualSale(field.text.toString()) }
        review.isEnabled = true
        showUiPage(
            ui.bar(if (currency == "SAT") "Amount in sats" else "Manual amount", back = { if (currency == "USD") showCatalog(healthy) else showHome() }, logo = true),
            body,
            listOf(review, ui.secondary("Cash sessions · test only") { showCashSessions() },
                ui.secondary("Reports · local ledger") { reportController.show() },
                ui.quiet("Sales and receipts") { showSales() }),
        )
        homeStatusView = statusView
        homePrimaryButton = review
        homePrimaryAllowed = true
    }

    private fun createManualSale(raw: String) {
        val info = config ?: return showSetup()
        val amount = try { parseAmount(raw, info.settings.currency) } catch (_: Exception) {
            return Toast.makeText(this, "Enter a positive ${if (info.settings.currency == "SAT") "whole-satoshi" else "USD"} amount.", Toast.LENGTH_LONG).show()
        }
        val quote = quote(info.settings.currency, amount)
        if (quote.expectedSats <= 0 || quote.expectedSats > MAX_SUPPORTED_SATS) return showHomeWithError("This test amount is outside the supported range.")
        if (quote.expectedSats < info.settings.minSaleSats) return showHomeWithError("This test amount is below the paired counter's minimum sale of ${formatSats(info.settings.minSaleSats)}.")
        confirmSaleCreation(amount, info.settings.currency, emptyList())
    }

    private fun confirmSaleCreation(amountMinor: Long, currency: String, items: List<SaleItem>, pricing: CheckoutPricingSnapshot? = null) {
        val followup = latestFollowupSale()
        val create = {
            val saleId = UUID.randomUUID().toString()
            executeAuthorized(
                CheckoutAuthorityCapability.SALE_CREATE,
                "The sale could not be saved. No payment attempt was started.",
                {
                    if (pricing != null) ledger.createPricedSale(saleId, currency, nowSecs(), pricing)
                    else ledger.createSale(saleId, amountMinor, currency, nowSecs(), items)
                },
            ) {
                if (items.isNotEmpty()) {
                    cartSelections.clear()
                    cartQuantities.clear()
                    discountDraft = ""
                    taxRateDraft = ""
                    tipDraft = ""
                }
                showTenderChoice(saleId)
            }
        }
        if (followup == null) return create()
        AlertDialog.Builder(this)
            .setTitle("Earlier sale needs follow-up")
            .setMessage("${formatAmount(followup.amountMinor, followup.currency)} · ${saleStateLabel(followup)}\n\nIts attempts and evidence will stay unchanged. Start a separate sale only if this is a different customer order.")
            .setNegativeButton("Resume earlier sale") { _, _ ->
                if (followup.attempts.isEmpty()) showTenderChoice(followup.saleId) else showReceipt(followup.saleId)
            }
            .setNeutralButton("Cancel", null)
            .setPositiveButton("Start separate sale") { _, _ -> create() }
            .show()
    }

    private fun showTenderChoice(saleId: String) {
        receiptToResume = null
        val sale = try { ledger.getSale(saleId) } catch (_: Exception) { return showSalesIfPossible() }
        val body = ArrayList<View>()
        body.add(ui.receipt(pricedItemReceiptLines(sale)))
        sale.pricing?.let { pricing ->
            val priceLines = mutableListOf(ReceiptLine("Subtotal · ${formatAmount(pricing.subtotalMinor, sale.currency)}"))
            priceLines.add(ReceiptLine("Order discount · ${if (pricing.discountMinor > 0) "−" else ""}${formatAmount(pricing.discountMinor, sale.currency)}"))
            pricing.taxes.forEach { tax -> priceLines.add(ReceiptLine(taxReceiptLabel(tax, sale.currency))) }
            if (pricing.taxes.isEmpty()) priceLines.add(ReceiptLine("Tax · ${formatAmount(pricing.taxMinor, sale.currency)} · not configured"))
            priceLines.add(ReceiptLine("Tip · +${formatAmount(pricing.tipMinor, sale.currency)}"))
            body.add(ui.receipt(priceLines))
        }
        body.add(ui.facts(listOf("Sale total" to formatAmount(sale.amountMinor, sale.currency))))
        body.add(ui.text("Choose a payment method for this balance. The payment attempt is recorded before the counter or card SDK is called.", 14f, colour = ui.pal.secondary))
        if (sale.currency == "USD" && cardBridge == null) body.add(ui.text("Square Sandbox is not included in this base build.", 13f, colour = ui.pal.secondary))
        val healthy = try { node?.let { nativeLock.withLock { it.status().health?.healthy == true } } ?: false } catch (_: Exception) { false }
        val statusView = ui.text(
            if (healthy) "Native Bitcoin checks are ready." else "Waiting for native Regtest network checks.",
            13f,
            colour = ui.pal.secondary,
        )
        body.add(statusView)
        val bitcoin = ui.primary("Pay with Bitcoin") { openBitcoinSale(saleId) }
        bitcoin.isEnabled = healthy
        val actions = mutableListOf(bitcoin)
        if (sale.currency == "USD" && cardBridge != null) {
            actions.add(ui.secondary("Pay with Square Sandbox") { beginSquareAttempt(saleId, sale.remainingMinor) })
        }
        if (canOfferCash(saleId)) {
            actions.add(ui.secondary("Cash") { showCashEntry(saleId) })
        }
        actions.add(ui.quiet("Sales and receipts") { showSales() })
        showUiPage(ui.bar("Choose payment", back = { showReceipt(saleId) }), body, actions)
        tenderStatusView = statusView
        tenderBitcoinButton = bitcoin
    }

    private fun cartItemsSnapshot(): List<SaleItem> = cartSelections.values.map { selected ->
        val effectiveUnit = selected.modifiers.fold(selected.unitMinor) { total, modifier -> Math.addExact(total, modifier.amountMinor) }
        SaleItem(selected.productId, selected.name, effectiveUnit, selected.quantity)
    }

    private fun cartReviewReceiptLines(): List<ReceiptLine> = cartSelections.values.flatMap { line ->
        buildList {
            add(ReceiptLine("${line.name} · ${line.quantity} × ${formatAmount(line.unitMinor, "USD")} base each"))
            line.modifiers.forEach { modifier ->
                add(ReceiptLine("  ${modifier.name} modifier · +${formatAmount(modifier.amountMinor, "USD")} each × ${line.quantity} = +${formatAmount(Math.multiplyExact(modifier.amountMinor, line.quantity.toLong()), "USD")}"))
            }
            val effectiveUnit = line.modifiers.fold(line.unitMinor) { total, modifier -> Math.addExact(total, modifier.amountMinor) }
            add(ReceiptLine("  Line subtotal · ${formatAmount(Math.multiplyExact(effectiveUnit, line.quantity.toLong()), "USD")}"))
        }
    }

    private fun addCatalogSelection(product: CatalogProduct, variant: CatalogVariant,
                                    selectedModifierIds: List<String> = emptyList(), amount: Int = 1) {
        if (!catalogReady || catalogWriteBlocked) return showManualCheckout(false, catalogUnavailableMessage())
        val selectedIds = selectedModifierIds.sorted()
        val currentName = "${product.name} — ${variant.name}"
        val currentModifiers = selectedIds.mapNotNull { id -> product.modifiers.firstOrNull { it.modifierId == id } }
        val existing = cartSelections.values.firstOrNull { line ->
            line.productId == product.productId && line.variantId == variant.variantId &&
                line.productVersion == product.version && line.name == currentName && line.unitMinor == variant.priceMinor &&
                line.modifiers.map { it.modifierId }.sorted() == selectedIds &&
                line.modifiers.zip(currentModifiers).all { (saved, current) -> saved.name == current.name && saved.amountMinor == current.amountMinor }
        }
        if (existing != null) {
            if (existing.quantity + amount > MAX_CART_QUANTITY) {
                Toast.makeText(this, "Maximum quantity is $MAX_CART_QUANTITY per saved item", Toast.LENGTH_LONG).show()
                return
            }
            cartSelections[existing.lineId] = existing.copy(quantity = existing.quantity + amount)
            return
        }
        if (cartSelections.size >= MAX_CART_LINES) {
            Toast.makeText(this, "A basket can contain at most $MAX_CART_LINES distinct item lines", Toast.LENGTH_LONG).show()
            return
        }
        try {
            val lineId = frozenCatalogLineId(product, variant, selectedIds)
            val frozen = catalog.snapshotLine(lineId, product.productId, variant.variantId, amount, selectedIds)
            cartSelections[frozen.lineId] = CartSelection(frozen.lineId, product.productId, variant.variantId,
                product.version, frozen.name, frozen.unitMinor, amount, frozen.modifiers.toList())
        } catch (e: Exception) {
            if (!handleCatalogFailure(e)) Toast.makeText(this, safeDetail(e), Toast.LENGTH_LONG).show()
        }
    }

    private fun frozenCatalogLineId(product: CatalogProduct, variant: CatalogVariant, modifierIds: List<String>): String {
        val identity = listOf(product.productId, variant.variantId, product.version.toString()) + modifierIds.sorted()
        val selected = identity.joinToString("") { "${it.toByteArray(Charsets.UTF_8).size}:$it" }
        val digest = MessageDigest.getInstance("SHA-256").digest(selected.toByteArray(Charsets.UTF_8))
        val identityToken = digest.joinToString("") { "%02x".format(it.toInt() and 0xff) }
        return "v${product.version}-$identityToken"
    }

    private fun removeCatalogSelection(productId: String, variantId: String) {
        val existing = cartSelections.values.lastOrNull { it.productId == productId && it.variantId == variantId } ?: return
        if (existing.quantity <= 1) cartSelections.remove(existing.lineId)
        else cartSelections[existing.lineId] = existing.copy(quantity = existing.quantity - 1)
    }

    private fun chooseVariant(product: CatalogProduct, healthy: Boolean) {
        if (product.variants.size == 1) {
            chooseModifiers(product, product.variants.single(), healthy)
            return
        }
        val variants = product.variants
        AlertDialog.Builder(this)
            .setTitle("Choose ${product.name} variant")
            .setItems(variants.map { "${it.name} · ${formatAmount(it.priceMinor, "USD")}" }.toTypedArray()) { _, which ->
                chooseModifiers(product, variants[which], healthy)
            }
            .setNegativeButton("Cancel", null)
            .show()
    }

    private fun chooseModifiers(product: CatalogProduct, variant: CatalogVariant, healthy: Boolean) {
        if (product.modifiers.isEmpty()) {
            addCatalogSelection(product, variant)
            showCatalog(healthy)
            return
        }
        val modifiers = product.modifiers
        val selected = BooleanArray(modifiers.size)
        AlertDialog.Builder(this)
            .setTitle("Choose ${product.name} options")
            .setMultiChoiceItems(modifiers.map { "${it.name} · +${formatAmount(it.amountMinor, "USD")} each" }.toTypedArray(), selected) { _, which, checked -> selected[which] = checked }
            .setPositiveButton("Add to basket") { _, _ ->
                val ids = modifiers.indices.filter { selected[it] }.map { modifiers[it].modifierId }
                addCatalogSelection(product, variant, ids)
                showCatalog(healthy)
            }
            .setNegativeButton("Cancel", null)
            .show()
    }

    private fun lookupCatalogCode(raw: String, healthy: Boolean) {
        if (!catalogReady || catalogWriteBlocked) return showManualCheckout(healthy, catalogUnavailableMessage())
        val code = raw.trim()
        if (code.isEmpty() || code.length > 160) return Toast.makeText(this, "Enter a barcode or SKU up to 160 characters.", Toast.LENGTH_LONG).show()
        try {
            val barcodeMatch = catalog.findByBarcode(code)
            val skuMatch = catalog.findBySku(code)
            if (barcodeMatch == null && skuMatch == null) {
                return Toast.makeText(this, "No saved item matches that barcode or SKU.", Toast.LENGTH_LONG).show()
            }
            val sameVariant = barcodeMatch != null && skuMatch != null &&
                barcodeMatch.first.productId == skuMatch.first.productId && barcodeMatch.second.variantId == skuMatch.second.variantId
            if (barcodeMatch != null && skuMatch != null && !sameVariant) {
                val matches = listOf("Barcode" to barcodeMatch, "SKU" to skuMatch)
                AlertDialog.Builder(this)
                    .setTitle("Choose exact code match")
                    .setItems(matches.map { (kind, pair) ->
                        "$kind · ${pair.first.name} — ${pair.second.name} · ${formatAmount(pair.second.priceMinor, "USD")}"
                    }.toTypedArray()) { _, which ->
                        val selected = matches[which].second
                        chooseModifiers(selected.first, selected.second, healthy)
                    }
                    .setNegativeButton("Cancel", null)
                    .show()
            } else {
                val match = barcodeMatch ?: skuMatch!!
                chooseModifiers(match.first, match.second, healthy)
            }
        } catch (e: Exception) {
            if (!handleCatalogFailure(e)) Toast.makeText(this, safeDetail(e), Toast.LENGTH_LONG).show()
        }
    }

    private fun showCatalogManagement(healthy: Boolean) {
        if (!catalogReady || catalogWriteBlocked) return showManualCheckout(healthy, catalogUnavailableMessage())
        val body = arrayListOf<View>(
            ui.text("Manage the saved local test catalog. Changes apply to future basket additions; items already in the basket keep their saved name and price.", 14f),
            ui.text("Catalog updates are not provider-synced and do not configure real tax or inventory.", 13f, colour = ui.pal.secondary),
        )
        val allProducts = try { catalog.listProducts(includeArchived = true) } catch (e: Exception) { return showCatalogBlocked(safeDetail(e)) }
        val products = allProducts.take(MAX_MANAGE_PRODUCTS)
        if (allProducts.size > products.size) body.add(ui.text("Showing the first $MAX_MANAGE_PRODUCTS catalog products.", 13f, colour = ui.pal.secondary))
        if (products.isEmpty()) body.add(ui.text("No saved products.", 14f, colour = ui.pal.secondary))
        products.forEach { product ->
            body.add(ui.receipt(listOf(
                ReceiptLine(product.name + if (product.archived) " · ARCHIVED" else "", LineStyle.Strong),
                ReceiptLine("Revision ${product.version} · ${product.variants.joinToString { "${it.name} ${formatAmount(it.priceMinor, "USD")}" }}"),
            )))
            if (!product.archived) {
                body.add(ui.button("Edit ${product.name}", ButtonKind.Secondary) { chooseVariantForEdit(product, healthy) })
                body.add(ui.quiet("Archive ${product.name}") { confirmArchiveProduct(product, healthy) })
            }
        }
        showUiPage(ui.bar("Manage items", back = { showCatalog(healthy) }), body,
            listOf(ui.primary("Add item") { showProductEditor(null, null, healthy) },
                ui.secondary("Local inventory · test only") { showInventoryStock() },
                ui.secondary("Back to checkout") { showCatalog(healthy) }))
    }

    private fun chooseVariantForEdit(product: CatalogProduct, healthy: Boolean) {
        if (product.variants.size == 1) return showProductEditor(product.productId, product.variants.single().variantId, healthy)
        val variants = product.variants
        AlertDialog.Builder(this).setTitle("Choose variant to edit")
            .setItems(variants.map { "${it.name} · ${formatAmount(it.priceMinor, "USD")}" }.toTypedArray()) { _, which ->
                showProductEditor(product.productId, variants[which].variantId, healthy)
            }.setNegativeButton("Cancel", null).show()
    }

    private fun showProductEditor(productId: String?, variantId: String?, healthy: Boolean) {
        val existing = productId?.let { catalog.getProduct(it) }
        val variant = existing?.variants?.firstOrNull { it.variantId == variantId }
        val name = ui.field("Item name")
        name.setSingleLine(true); name.filters = arrayOf(InputFilter.LengthFilter(120)); name.setText(existing?.name.orEmpty())
        val variantName = ui.field("Variant name")
        variantName.setSingleLine(true); variantName.filters = arrayOf(InputFilter.LengthFilter(120)); variantName.setText(variant?.name ?: "Regular")
        val price = ui.field("Price in USD")
        price.inputType = InputType.TYPE_CLASS_NUMBER or InputType.TYPE_NUMBER_FLAG_DECIMAL
        price.setSingleLine(true); price.filters = arrayOf(InputFilter.LengthFilter(12)); price.setText(variant?.let { moneyInput(it.priceMinor) }.orEmpty())
        val sku = ui.field("SKU · optional")
        sku.setSingleLine(true); sku.filters = arrayOf(InputFilter.LengthFilter(160)); sku.setText(variant?.sku.orEmpty())
        val barcode = ui.field("Barcode · optional")
        barcode.setSingleLine(true); barcode.filters = arrayOf(InputFilter.LengthFilter(160)); barcode.setText(variant?.barcode.orEmpty())
        val body = arrayListOf<View>(
            ui.text(if (existing == null) "Create one item with one saved variant." else "Edit the selected variant. Other variants and saved modifiers are preserved.", 14f),
            name, variantName, price, sku, barcode,
            ui.text("This test screen supports one-variant item creation and editing one existing variant at a time. Modifier definitions are preserved but not configured here.", 13f, colour = ui.pal.secondary),
        )
        val save = ui.primary("Save item") {
            try {
                val itemName = name.text.toString().trim()
                val variantLabel = variantName.text.toString().trim()
                val priceMinor = parseExactUsd(price.text.toString(), allowZero = false)
                require(priceMinor <= MAX_CATALOG_PRICE_MINOR) { "Item price exceeds the ${formatAmount(MAX_CATALOG_PRICE_MINOR, "USD")} test-app limit." }
                val skuValue = sku.text.toString().trim().takeIf { it.isNotEmpty() }
                val barcodeValue = barcode.text.toString().trim().takeIf { it.isNotEmpty() }
                val versions = existing?.variants?.toMutableList() ?: mutableListOf()
                val commandId = UUID.randomUUID().toString()
                var savedProduct: CatalogProduct? = null
                authorityFlow.executeCapability(CheckoutAuthorityCapability.CATALOG_MANAGE, {
                    savedProduct = if (existing == null) {
                        val id = UUID.randomUUID().toString()
                        catalog.createProduct(commandId, id, itemName,
                            listOf(CatalogVariant("default", variantLabel, priceMinor, skuValue, barcodeValue)))
                    } else {
                        val idx = versions.indexOfFirst { it.variantId == variantId }
                        require(idx >= 0) { "The selected variant is no longer available." }
                        versions[idx] = CatalogVariant(versions[idx].variantId, variantLabel, priceMinor, skuValue, barcodeValue)
                        catalog.editProduct(commandId, existing.productId, existing.version, itemName, versions, existing.modifiers)
                    }
                }) { error ->
                    if (error == null) {
                        val saved = checkNotNull(savedProduct)
                        Toast.makeText(this, "Saved ${saved.name} · revision ${saved.version}", Toast.LENGTH_SHORT).show()
                        showCatalogManagement(healthy)
                    } else if (error !is CheckoutAuthorityDeniedException && error !is CheckoutAuthorityUnavailableException) {
                        if (!handleCatalogFailure(error)) Toast.makeText(this, error.message ?: "Check the item details.", Toast.LENGTH_LONG).show()
                    }
                }
            } catch (e: Exception) {
                if (!handleCatalogFailure(e)) Toast.makeText(this, e.message ?: "Check the item details.", Toast.LENGTH_LONG).show()
            }
        }
        showUiPage(ui.bar(if (existing == null) "Add item" else "Edit item", back = { showCatalogManagement(healthy) }), body,
            listOf(save, ui.secondary("Cancel") { showCatalogManagement(healthy) }))
    }

    private fun confirmArchiveProduct(product: CatalogProduct, healthy: Boolean) {
        AlertDialog.Builder(this).setTitle("Archive ${product.name}?")
            .setMessage("Archived items disappear from checkout. Saved sales and frozen basket lines keep their original details.")
            .setNegativeButton("Cancel", null)
            .setPositiveButton("Archive") { _, _ ->
                authorityFlow.executeCapability(CheckoutAuthorityCapability.CATALOG_MANAGE, {
                    catalog.archiveProduct(UUID.randomUUID().toString(), product.productId, product.version)
                }) { error ->
                    if (error == null) showCatalogManagement(healthy)
                    else if (error !is CheckoutAuthorityDeniedException && error !is CheckoutAuthorityUnavailableException &&
                        !handleCatalogFailure(error)) Toast.makeText(this, safeDetail(error), Toast.LENGTH_LONG).show()
                }
            }.show()
    }

    private fun handleCatalogFailure(error: Exception): Boolean {
        val detail = safeDetail(error)
        if (detail.contains("uncertain", ignoreCase = true) || detail.contains("poisoned", ignoreCase = true)) {
            catalogWriteBlocked = true
            catalogReady = false
            catalogFault = detail
            showManualCheckout(false, catalogUnavailableMessage())
            return true
        }
        return false
    }

    private fun showCatalogBlocked(detail: String? = null) {
        catalogReady = false
        catalogFault = detail ?: catalogFault
        showManualCheckout(false, catalogUnavailableMessage())
    }

    private fun catalogUnavailableMessage(): String = buildString {
        append("Local catalog unavailable. No empty catalog was substituted. Close and reopen the test app and inspect Manage items before editing again.")
        catalogFault?.let { append(" Details: ").append(it) }
    }

    private fun showCashSessions() {
        if (cashSessionsReady && ::cashController.isInitialized) cashController.show()
        else show("Cash sessions unavailable", "Local drawer session data could not be read. No empty session history was substituted. Close and reopen the test app to inspect it. Canonical cash tender on a saved sale remains available; drawer reconciliation is unavailable until this local data is repaired.",
            listOf("Sales and receipts" to { showSales() }, "Close test app" to { finish() }), cashSessionsFault)
    }

    private fun showInventoryStock() {
        if (inventoryReady && catalogReady && ::inventoryController.isInitialized) inventoryController.showStock()
        else showInventoryBlocked(null)
    }

    private fun showInventoryBlocked(saleId: String?) {
        val detail = inventoryFault ?: catalogFault
        show("Inventory unavailable", "Local inventory data could not be read. No empty stock counts were substituted. Close and reopen the test app to inspect it. Checkout and payment remain available; tracked handoff recording is unavailable until local inventory is repaired.",
            buildList {
                if (saleId != null) add("Back to saved receipt" to { showReceipt(saleId) })
                add("Sales and receipts" to { showSales() })
                add("Close test app" to { finish() })
            }, detail)
    }

    private fun moneyInput(value: Long): String = "${value / 100}.${(value % 100).toString().padStart(2, '0')}"

    private fun pricedItemReceiptLines(sale: Sale): List<ReceiptLine> {
        val pricing = sale.pricing
        if (pricing == null) return sale.items.map { item ->
            ReceiptLine("${item.name} · ${item.quantity} × ${formatAmount(item.unitMinor, sale.currency)}")
        }.ifEmpty { listOf(ReceiptLine("Manual amount sale", LineStyle.Small)) }
        return pricing.lines.flatMap { line ->
            buildList {
                add(ReceiptLine("${line.name} · ${line.quantity} × ${formatAmount(line.unitMinor, sale.currency)} base = ${formatAmount(Math.multiplyExact(line.unitMinor, line.quantity.toLong()), sale.currency)}"))
                line.modifiers.forEach { modifier ->
                    add(ReceiptLine("  ${modifier.name} modifier · +${formatAmount(modifier.amountMinor, sale.currency)} each × ${line.quantity} = +${formatAmount(Math.multiplyExact(modifier.amountMinor, line.quantity.toLong()), sale.currency)}"))
                }
            }
        }
    }

    private fun taxReceiptLabel(tax: TaxSnapshot, currency: String): String {
        val rate = "${tax.rateBasisPoints / 100}.${(tax.rateBasisPoints % 100).toString().padStart(2, '0')}%"
        return if (tax.treatment == TaxTreatment.EXCLUSIVE)
            "${tax.name} · $rate exclusive · +${formatAmount(tax.amountMinor, currency)}"
        else "${tax.name} · $rate included in price · extracted ${formatAmount(tax.amountMinor, currency)}"
    }

    private fun reviewAmountField(label: String, initial: String): EditText = ui.field(label).apply {
        inputType = InputType.TYPE_CLASS_NUMBER or InputType.TYPE_NUMBER_FLAG_DECIMAL
        filters = arrayOf(InputFilter.LengthFilter(12))
        setSingleLine(true)
        setText(initial)
    }

    private fun reviewPercentField(label: String, initial: String): EditText = reviewAmountField(label, initial)

    private fun freezeCartPricing(discountText: String, taxText: String, tipText: String): CheckoutPricingSnapshot {
        val discount = parseOptionalUsd(discountText)
        val tip = parseOptionalUsd(tipText)
        val rate = parseOptionalRate(taxText)
        val tax = if (rate == 0) null else CheckoutTaxRule("merchant-flat-exclusive", "Merchant configured tax", rate, TaxTreatment.EXCLUSIVE)
        val lines = cartSelections.values.map { CheckoutLineInput(it.lineId, it.productId, it.name, it.unitMinor, it.quantity, it.modifiers) }
        return CheckoutPricing.freeze(lines, discount, tax, tip)
    }

    private fun parseOptionalUsd(raw: String): Long {
        if (raw.isBlank()) return 0L
        val value = parseExactUsd(raw.trim(), allowZero = true)
        require(value >= 0) { "Amounts must be zero or greater." }
        require(value <= 100_000_000L) { "Adjustment exceeds the supported USD limit." }
        return value
    }

    private fun parseExactUsd(raw: String, allowZero: Boolean): Long {
        require(raw.matches(Regex("(?:0|[1-9][0-9]*)(?:\\.[0-9]{1,2})?"))) {
            "Enter USD using digits and up to two decimal places."
        }
        val value = BigDecimal(raw).setScale(2, RoundingMode.UNNECESSARY).movePointRight(2).longValueExact()
        require(if (allowZero) value >= 0 else value > 0) { if (allowZero) "Amount cannot be negative." else "Price must be positive." }
        return value
    }

    private fun parseOptionalRate(raw: String): Int {
        if (raw.isBlank()) return 0
        require(raw.matches(Regex("(?:0|[1-9][0-9]*)(?:\\.[0-9]{1,2})?"))) {
            "Enter a rate using digits and up to two decimal places."
        }
        val value = BigDecimal(raw).setScale(2, RoundingMode.UNNECESSARY).movePointRight(2).intValueExact()
        require(value in 0..100_000) { "Tax rate must be between 0% and 1000%." }
        return value
    }

    private fun updatePricingPreview(discount: String, rate: String, tip: String, target: View) {
        try {
            val pricing = freezeCartPricing(discount, rate, tip)
            val lines = mutableListOf("Subtotal · ${formatAmount(pricing.subtotalMinor, "USD")}",
                "Order discount · ${if (pricing.discountMinor > 0) "−" else ""}${formatAmount(pricing.discountMinor, "USD")}")
            pricing.taxes.forEach { lines.add("${it.name} · ${it.rateBasisPoints / 100}.${(it.rateBasisPoints % 100).toString().padStart(2, '0')}% exclusive · ${formatAmount(it.amountMinor, "USD")}") }
            if (pricing.taxes.isEmpty()) lines.add("Tax · ${formatAmount(pricing.taxMinor, "USD")} · not configured")
            lines.add("Tip · ${formatAmount(pricing.tipMinor, "USD")}")
            lines.add("Total · ${formatAmount(pricing.totalMinor, "USD")}")
            (target as? TextView)?.text = lines.joinToString("\n")
        } catch (e: Exception) {
            (target as? TextView)?.text = "Check adjustment amounts: ${e.message ?: "invalid input"}"
        }
    }

    private fun canOfferCash(saleId: String): Boolean = try {
        ledger.canRecordCashPayment(saleId)
    } catch (_: Exception) {
        false
    }

    /** Cash entry records only after the cashier explicitly confirms physical receipt. */
    private fun showCashEntry(saleId: String, stablePaymentId: String = UUID.randomUUID().toString(), stableRecordedAt: Long = nowSecs()) {
        val sale = try { ledger.getSale(saleId) } catch (_: Exception) { return showSalesIfPossible() }
        if (!canOfferCash(saleId)) {
            return showReceipt(saleId)
        }
        val paymentId = stablePaymentId
        val recordedAt = stableRecordedAt
        val remaining = sale.remainingMinor
        val input = EditText(this).apply {
            inputType = InputType.TYPE_CLASS_NUMBER or InputType.TYPE_NUMBER_FLAG_DECIMAL
            setSingleLine(true)
            hint = "0.00"
            contentDescription = "Cash received in USD"
        }
        val preview = TextView(this).apply {
            textSize = 14f
            setTextColor(Color.rgb(24, 34, 43))
            text = "Amount applied · ${formatAmount(remaining, "USD")}\nEnter cash received to preview change."
        }
        val content = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(dp(8), dp(4), dp(8), dp(2))
            addView(input, LinearLayout.LayoutParams(-1, -2))
            addView(preview, LinearLayout.LayoutParams(-1, -2).also { it.topMargin = dp(12) })
        }
        val dialog = AlertDialog.Builder(this)
            .setTitle("Cash received · USD")
            .setMessage("Remaining balance: ${formatAmount(remaining, "USD")}\nEnter the physical cash received.")
            .setView(content)
            .setNegativeButton("Cancel", null)
            .setPositiveButton("Record cash received", null)
            .create()
        fun parsedTender(): Long? = try { parseCashTendered(input.text.toString()) } catch (_: Exception) { null }
        fun refreshPreview() {
            val tendered = parsedTender()
            preview.text = when {
                tendered == null -> "Amount applied · ${formatAmount(remaining, "USD")}\nEnter a USD amount with up to two decimal places."
                tendered < remaining -> "Amount applied · ${formatAmount(remaining, "USD")}\nCash received · ${formatAmount(tendered, "USD")}\nStill due · ${formatAmount(remaining - tendered, "USD")}"
                else -> "Amount applied · ${formatAmount(remaining, "USD")}\nCash received · ${formatAmount(tendered, "USD")}\nChange due · ${formatAmount(Math.subtractExact(tendered, remaining), "USD")}"
            }
            dialog.getButton(AlertDialog.BUTTON_POSITIVE)?.isEnabled = tendered != null && tendered >= remaining
        }
        input.addTextChangedListener(object : TextWatcher {
            override fun beforeTextChanged(s: CharSequence?, start: Int, count: Int, after: Int) = Unit
            override fun onTextChanged(s: CharSequence?, start: Int, before: Int, count: Int) = refreshPreview()
            override fun afterTextChanged(s: Editable?) = Unit
        })
        dialog.setOnShowListener {
            val record = dialog.getButton(AlertDialog.BUTTON_POSITIVE)
            record.isEnabled = false
            record.setOnClickListener {
                val tendered = parsedTender() ?: return@setOnClickListener
                if (tendered < remaining) return@setOnClickListener
                // Preserve the destination before the durable write so recreation can reopen its receipt.
                receiptToResume = saleId
                dialog.dismiss()
                authorityFlow.executeCapability(CheckoutAuthorityCapability.TENDER_BEGIN, {
                    ledger.recordCashPayment(saleId, paymentId, tendered, recordedAt)
                }) { error ->
                    if (error == null) return@executeCapability showReceipt(saleId)
                    if (error is CheckoutAuthorityDeniedException || error is CheckoutAuthorityUnavailableException) return@executeCapability
                    val uncertain = (error.message ?: "").contains("uncertain", ignoreCase = true)
                    if (!uncertain) receiptToResume = null
                    showMessage(
                        if (uncertain) "Cash record needs checking" else "Cash payment was not recorded",
                        "The cash may have been received even though this screen did not confirm the ledger result. Do not collect or record it again until you have inspected the sale in Sales.",
                        safeDetail(error),
                    ) { if (uncertain) finish() else showReceipt(saleId) }
                }
            }
            refreshPreview()
        }
        dialog.show()
    }

    private fun parseCashTendered(raw: String): Long {
        require(raw.matches(Regex("(?:0|[1-9][0-9]*)(?:\\.[0-9]{1,2})?"))) { "Enter USD using digits and up to two decimal places." }
        val minor = BigDecimal(raw).setScale(2, RoundingMode.UNNECESSARY).movePointRight(2).longValueExact()
        require(minor > 0) { "Cash received must be positive." }
        return minor
    }

    private fun checkedItemsTotal(items: List<SaleItem>): Long = items.fold(0L) { total, item ->
        Math.addExact(total, Math.multiplyExact(item.unitMinor, item.quantity.toLong()))
    }

    private fun cartTotalMinor(): Long = checkedItemsTotal(cartItemsSnapshot())

    private fun openBitcoinSale(saleId: String) {
        val currentNode = node ?: return showStatus("Counter is starting", "Wait until the Regtest counter is connected. No code has been made.")
        val healthy = try { nativeLock.withLock { currentNode.status().health?.healthy == true } } catch (_: Exception) { false }
        if (!healthy) return showMessage("Bitcoin is not ready", "The native Regtest counter has not passed its network checks. No Bitcoin attempt was created; you can choose Square Sandbox if enabled.") { showTenderChoice(saleId) }
        val info = config ?: return showHome()
        if (info.network != CounterNetwork.Regtest) return showHome()
        val sale = try { ledger.getSale(saleId) } catch (_: Exception) { return showSalesIfPossible() }
        val amount = sale.remainingMinor
        if (amount <= 0) return showReceipt(saleId)
        val quote = quote(info.settings.currency, amount)
        if (quote.expectedSats <= 0 || quote.expectedSats > MAX_SUPPORTED_SATS) return showReceipt(saleId)
        if (quote.expectedSats < info.settings.minSaleSats) return showMessage("Amount is below the counter minimum", "No Bitcoin attempt was created. Choose another payment method or review the saved sale.") { showTenderChoice(saleId) }
        val attemptId = UUID.randomUUID().toString()
        val createdAt = nowSecs()
        var expectedIndex = -1L
        var openedResult: Opened? = null
        var unexpectedIndex = false
        authorityFlow.executeCapability(CheckoutAuthorityCapability.TENDER_BEGIN, {
            // The native index is predictable before open because this prototype permits one live
            // node and no concurrent cashier. Persist the attempt before exposing its payment code.
            val existing = nativeLock.withLock { currentNode.status().sales }
            expectedIndex = (existing.maxOrNull() ?: -1L) + 1L
            ledger.beginBitcoinAttempt(saleId, attemptId, expectedIndex, amount, quote.expectedSats, quote.id, quote.at)
            openedResult = nativeLock.withLock {
                currentNode.open(createdAt, amount, info.settings.currency, quote.rate, quote.source, quote.at)
            }
            if (openedResult!!.index != expectedIndex) {
                unexpectedIndex = true
                ledger.requestCancellation(saleId, attemptId)
            }
        }) { error ->
            if (error is CheckoutAuthorityDeniedException || error is CheckoutAuthorityUnavailableException) return@executeCapability
            if (error != null) {
                showMessage("Bitcoin attempt needs review", "The sale and attempt remain saved. No payment code is available; inspect the receipt before choosing another tender.", safeDetail(error)) { showReceipt(saleId) }
                return@executeCapability
            }
            val opened = openedResult ?: return@executeCapability showMessage(
                "Bitcoin attempt needs review",
                "The payment request was not returned. Inspect the saved sale before choosing another tender.",
            ) { showReceipt(saleId) }
            if (unexpectedIndex) {
                showMessage("No payment code was shown", "The native counter returned an unexpected sale index. Stop here and report this test-build fault.", "Expected $expectedIndex · received ${opened.index}") { showReceipt(saleId) }
                return@executeCapability
            }
            activeSaleId = saleId
            activeAttemptId = attemptId
            activePayment = opened.payment
            activeUri = opened.payment.copyUri
            activeNativeScreen = nativeLock.withLock { currentNode.judge(nowSecs(), opened.index) }
            renderActiveSale()
        }
    }

    private data class Quote(val rate: Long, val source: String, val at: Long, val id: String, val expectedSats: Long)

    private fun quote(currency: String, amountMinor: Long): Quote {
        if (currency == "SAT") return Quote(0L, "", 0L, "bitcoin-standard", amountMinor)
        val rate = REGTEST_USD_MINOR_PER_BTC
        val numerator = BigInteger.valueOf(amountMinor).multiply(BigInteger.valueOf(SATS_PER_BTC))
        val sats = numerator.add(BigInteger.valueOf(rate - 1L)).divide(BigInteger.valueOf(rate)).longValueExact()
        val at = nowSecs()
        return Quote(rate, "dev-regtest-usd-100000-v1", at, "dev-regtest-usd-100000-v1", sats)
    }

    private fun parseAmount(raw: String, currency: String): Long {
        if (currency == "SAT") return raw.trim().toLong().also { require(it > 0) }
        val value = BigDecimal(raw.trim()).setScale(2, RoundingMode.UNNECESSARY).movePointRight(2).longValueExact()
        require(value > 0)
        return value
    }

    private fun renderActiveSale() {
        activeSaleShowing = true
        val saleId = activeSaleId ?: return showHome()
        val attemptId = activeAttemptId ?: return showHome()
        val sale = try { ledger.getSale(saleId) } catch (_: Exception) { return showHome() }
        val attempt = sale.attempts.firstOrNull { it.attemptId == attemptId } ?: return showHome()
        if (sale.paid && !sale.requiresReview) return showReceipt(saleId)
        val native = activeNativeScreen
        val amountText = formatAmount(sale.amountMinor, sale.currency)
        val received = formatAmount(sale.receivedMinor, sale.currency)
        val state = when {
            sale.requiresReview -> "OWNER REVIEW REQUIRED · review payment and refund evidence before fulfillment"
            native?.state == SaleState.Final || attempt.state == PaymentState.CONFIRMED || attempt.state == PaymentState.PAID -> "Confirmed by the Regtest counter"
            native?.state == SaleState.Confirmed -> "Confirmed by the Regtest counter"
            attempt.state == PaymentState.REORGED || native?.reopened == true -> "Payment check reopened · do not hand over goods"
            native?.state == SaleState.PaidInstant -> "Payment seen · waiting for the counter's required confirmation"
            native?.state == SaleState.PartPaid -> "Part of the requested Bitcoin amount has arrived"
            native?.state == SaleState.Waiting -> "Payment found · the counter is checking it"
            native?.state == SaleState.DoNotHandOver -> "Stop · the counter says not to hand over goods"
            attempt.cancellationAcknowledged -> "Checkout cancelled · late payment evidence is still monitored"
            native?.state == SaleState.Cancelled || attempt.cancellationRequested -> "Cancellation requested · late payment evidence is still checked"
            else -> "Waiting for Bitcoin on Regtest"
        }
        val payment = if (attempt.cancellationAcknowledged) null else activePayment ?: native?.payment
        val uri = if (attempt.cancellationAcknowledged) "" else payment?.copyUri ?: activeUri.orEmpty()
        val actions = ArrayList<Pair<String, () -> Unit>>()
        if (!attempt.cancellationAcknowledged && uri.isNotEmpty()) actions.add("Copy Bitcoin payment code" to { copyText(uri); Toast.makeText(this, "Copied the Bitcoin payment code", Toast.LENGTH_SHORT).show() })
        if (!attempt.cancellationAcknowledged && attempt.state !in setOf(PaymentState.CONFIRMED, PaymentState.PAID)) {
            actions.add((if (attempt.cancellationRequested) "Try cancellation again" else "Cancel checkout") to {
                val index = attempt.counterIndex
                if (index != null) cancelBitcoin(saleId, attemptId, index)
            })
        }
        if (!sale.requiresReview && attempt.state == PaymentState.CANCELED && attempt.receivedSats == 0L && sale.remainingMinor > 0 && sale.currency == "USD" && cardBridge != null) {
            actions.add("Pay remaining by Square Sandbox" to { beginSquareAttempt(saleId, sale.remainingMinor) })
        }
        actions.add("Sales and receipts" to { showSales() })
        val renderKey = listOf(
            sale.saleId, attempt.attemptId, amountText, state, received, sale.remainingMinor,
            attempt.state, attempt.cancellationAcknowledged, native?.words?.what.orEmpty(), native?.words?.next.orEmpty(),
            native?.reopened, uri, actions.joinToString("|") { it.first },
        ).joinToString("\u0000")
        if (activeSaleRootShown && renderKey == lastActiveSaleRenderKey) return
        val body = ArrayList<View>()
        body.add(ui.text(amountText, 32f, bold = true))
        body.add(ui.text(state, 15f, bold = sale.requiresReview || attempt.state in setOf(PaymentState.REORGED, PaymentState.UNCONFIRMED)))
        body.add(ui.text("TEST ONLY · Regtest · do not hand over goods", 12f, colour = ui.pal.secondary))
        if (attempt.cancellationAcknowledged) {
            body.add(ui.receipt(listOf(
                ReceiptLine("Cancellation acknowledged", LineStyle.Strong),
                ReceiptLine("The old payment code is hidden. Late evidence continues to be watched.", LineStyle.Small),
            )))
        }
        payment?.let { payment ->
            body.add(FrameLayout(this).apply {
                minimumHeight = dp(240)
                addView(PaymentQrView(this@CheckoutActivity, payment), FrameLayout.LayoutParams(-1, dp(240)))
            })
        }
        val progress = if (sale.currency == "SAT") {
            "Received $received · Remaining ${formatAmount(sale.remainingMinor, sale.currency)}"
        } else {
            "Received $received · ${formatSats(attempt.receivedSats)} observed · Remaining ${formatAmount(sale.remainingMinor, sale.currency)}"
        }
        body.add(ui.text(progress, 13f, colour = ui.pal.secondary))
        if (uri.isNotEmpty()) body.add(ui.text(uri, 12f, mono = true, colour = ui.pal.secondary))
        val actionButtons = actions.map { (label, action) ->
            when {
                label == "Copy Bitcoin payment code" -> ui.primary(label, onClick = action)
                label == "Cancel checkout" || label == "Try cancellation again" -> ui.button(label, ButtonKind.Danger, action)
                label == "Sales and receipts" -> ui.quiet(label, action)
                else -> ui.secondary(label, action)
            }
        }
        showUiPage(ui.bar("Bitcoin payment", sub = "TEST ONLY · Regtest", back = { showSales() }, logo = true), body, actionButtons)
        lastActiveSaleRenderKey = renderKey
        activeSaleRootShown = true
    }

    private fun cancelBitcoin(saleId: String, attemptId: String, counterIndex: Long) {
        authorityFlow.executeCapability(CheckoutAuthorityCapability.TENDER_BEGIN, {
            ledger.requestCancellation(saleId, attemptId)
        }) { error ->
            if (error is CheckoutAuthorityDeniedException || error is CheckoutAuthorityUnavailableException) return@executeCapability
            if (error != null) {
                showStatus("Cancellation was not saved", "The attempt remains on file. Do not start another payment until Sales shows its current state.", safeDetail(error))
                return@executeCapability
            }
            val currentNode = node
            if (currentNode != null) {
                try {
                    nativeLock.withLock { currentNode.cancel(nowSecs(), counterIndex) }
                    ledger.acknowledgeBitcoinCancellation(saleId, attemptId)
                } catch (_: Exception) {
                    val screen = try { nativeLock.withLock { currentNode.judge(nowSecs(), counterIndex) } } catch (_: Exception) { null }
                    if (screen?.state == SaleState.Cancelled && screen.receivedSats == 0L) {
                        try { ledger.acknowledgeBitcoinCancellation(saleId, attemptId) } catch (_: Exception) {}
                    }
                }
            }
            renderActiveSale()
        }
    }

    private fun showSales() {
        receiptToResume = null
        activeSaleShowing = false
        val allSales = ledger.listSales()
        val sales = searchSales(allSales, SaleHistoryQuery(historyQueryText, historyTenderFilter, historyStatusFilter))
        val body = ArrayList<View>()
        if (hasUnresolvedSale()) body.add(ui.receipt(listOf(
            ReceiptLine("Follow-up needed", LineStyle.Strong),
            ReceiptLine("Each sale keeps its own attempts and evidence. Start another order only through the separate-sale acknowledgement.", LineStyle.Small),
        )))
        val search = ui.field("Search receipt ID or item name").apply {
            inputType = InputType.TYPE_CLASS_TEXT
            imeOptions = EditorInfo.IME_ACTION_SEARCH
            filters = arrayOf(InputFilter.LengthFilter(128))
            setSingleLine(true)
            setText(historyQueryText)
            setOnEditorActionListener { _, actionId, _ ->
                if (actionId == EditorInfo.IME_ACTION_SEARCH) {
                    historyQueryText = text.toString()
                    showSales()
                    true
                } else false
            }
        }
        body.add(search)
        body.add(ui.secondary("Search") {
            historyQueryText = search.text.toString()
            showSales()
        })
        val filterControls = LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
        }
        val tenderLabel = when (historyTenderFilter) {
            null -> "All"
            HistoryTender.BITCOIN -> "Bitcoin"
            HistoryTender.CARD -> "Card"
            HistoryTender.CASH -> "Cash"
        }
        val statusLabel = when (historyStatusFilter) {
            null -> "All"
            HistoryStatus.PAID -> "Paid"
            HistoryStatus.UNPAID -> "Unpaid"
            HistoryStatus.REVIEW -> "Needs review"
        }
        filterControls.addView(ui.secondary("Tender · $tenderLabel") {
            historyQueryText = search.text.toString()
            historyTenderFilter = when (historyTenderFilter) {
                null -> HistoryTender.BITCOIN
                HistoryTender.BITCOIN -> HistoryTender.CARD
                HistoryTender.CARD -> HistoryTender.CASH
                HistoryTender.CASH -> null
            }
            showSales()
        }, LinearLayout.LayoutParams(0, -2, 1f).also { it.rightMargin = ui.px(4) })
        filterControls.addView(ui.secondary("Status · $statusLabel") {
            historyQueryText = search.text.toString()
            historyStatusFilter = when (historyStatusFilter) {
                null -> HistoryStatus.PAID
                HistoryStatus.PAID -> HistoryStatus.UNPAID
                HistoryStatus.UNPAID -> HistoryStatus.REVIEW
                HistoryStatus.REVIEW -> null
            }
            showSales()
        }, LinearLayout.LayoutParams(0, -2, 1f).also { it.leftMargin = ui.px(4) })
        body.add(filterControls)
        body.add(ui.quiet("Clear search and filters") {
            historyQueryText = ""
            historyTenderFilter = null
            historyStatusFilter = null
            showSales()
        })
        body.add(ui.text("${sales.size} of ${allSales.size} sales", 13f, colour = ui.pal.secondary))
        if (sales.isEmpty()) {
            body.add(ui.receipt(listOf(
                ReceiptLine("No matching sales", LineStyle.Strong),
                ReceiptLine("Clear the search or reset filters to see saved sales.", LineStyle.Small),
            )))
        }
        for (sale in sales) {
            val lines = ArrayList<ReceiptLine>()
            lines.add(ReceiptLine("${formatAmount(sale.amountMinor, sale.currency)} · ${saleStateLabel(sale)}", LineStyle.Strong))
            val itemSummary = sale.items.joinToString(", ") { "${it.quantity} × ${it.name}" }
            lines.add(ReceiptLine(if (itemSummary.isEmpty()) "Manual amount" else itemSummary, LineStyle.Small))
            body.add(ui.receipt(lines))
            body.add(ui.secondary("Open receipt") { showReceipt(sale.saleId) })
        }
        showUiPage(
            ui.bar("Sales and receipts", back = { showHome() }, logo = true),
            body,
            listOf(ui.secondary("Back to checkout") { showHome() }),
        )
    }

    private fun showReceipt(saleId: String) {
        receiptToResume = saleId
        activeSaleShowing = false
        val sale = ledger.getSale(saleId)
        val body = ArrayList<View>()
        if (sale.requiresReview) {
            val review = ArrayList<ReceiptLine>()
            review.add(ReceiptLine("OWNER REVIEW REQUIRED", LineStyle.Strong))
            review.add(ReceiptLine("Review late or excess payment evidence before fulfilling or closing this sale.", LineStyle.Small))
            if (sale.lateBitcoinReceipt) review.add(ReceiptLine("Bitcoin arrived after cancellation was acknowledged.", LineStyle.Small))
            if (sale.overpaidMinor > 0) review.add(ReceiptLine("Excess received: ${formatAmount(sale.overpaidMinor, sale.currency)}", LineStyle.Small))
            if (sale.overpaidSats > 0) review.add(ReceiptLine("Excess Bitcoin: ${formatSats(sale.overpaidSats)}", LineStyle.Small))
            body.add(ui.receipt(review))
        }
        val receiptLines = ArrayList<ReceiptLine>()
        receiptLines.addAll(pricedItemReceiptLines(sale))
        sale.pricing?.let { pricing ->
            receiptLines.add(ReceiptLine("Subtotal · ${formatAmount(pricing.subtotalMinor, sale.currency)}"))
            receiptLines.add(ReceiptLine("Order discount · ${if (pricing.discountMinor > 0) "−" else ""}${formatAmount(pricing.discountMinor, sale.currency)}"))
            pricing.taxes.forEach { tax -> receiptLines.add(ReceiptLine(taxReceiptLabel(tax, sale.currency))) }
            if (pricing.taxes.isEmpty()) receiptLines.add(ReceiptLine("Tax · ${formatAmount(pricing.taxMinor, sale.currency)} · not configured"))
            receiptLines.add(ReceiptLine("Tip · +${formatAmount(pricing.tipMinor, sale.currency)}"))
            receiptLines.add(ReceiptLine("Frozen total · ${formatAmount(pricing.totalMinor, sale.currency)}", LineStyle.Strong))
        }
        body.add(ui.receipt(receiptLines))
        val paymentLines = ArrayList<ReceiptLine>()
        paymentLines.add(ReceiptLine("${saleStateLabel(sale)} · ${formatAmount(sale.amountMinor, sale.currency)}", LineStyle.Strong))
        paymentLines.add(ReceiptLine("Received · ${formatAmount(sale.receivedMinor, sale.currency)}"))
        paymentLines.add(ReceiptLine("Remaining · ${formatAmount(sale.remainingMinor, sale.currency)}"))
        for (cash in sale.cashPayments) {
            paymentLines.add(ReceiptLine("Cash applied · ${formatAmount(cash.amountMinor, "USD")}"))
            paymentLines.add(ReceiptLine("Cash received · ${formatAmount(cash.tenderedMinor, "USD")}"))
            paymentLines.add(ReceiptLine("Change · ${formatAmount(cash.changeMinor, "USD")}"))
        }
        paymentLines.add(ReceiptLine("TEST ONLY · Regtest", LineStyle.Small))
        body.add(ui.receipt(paymentLines))
        body.add(ui.secondary("Copy receipt") {
            val receiptText = "TEST ONLY · REGTEST\n\n${CheckoutReceipt.receiptText(sale)}"
            copyText(receiptText, "OpenBMT test receipt")
            Toast.makeText(this, "Copied the test receipt", Toast.LENGTH_SHORT).show()
        })
        for (attempt in sale.attempts) {
            val shortTender = if (attempt.tender == Tender.BITCOIN) "Bitcoin" else "Card"
            val stateLine = when {
                attempt.tender == Tender.CARD && attempt.state == PaymentState.UNKNOWN -> "Card outcome needs reconciliation; do not retry this sale."
                attempt.state == PaymentState.UNCONFIRMED -> "Bitcoin seen; waiting for the counter's configured confirmation check."
                attempt.state == PaymentState.REORGED -> "Bitcoin confirmation reopened; do not hand over goods."
                attempt.cancellationAcknowledged -> "Cancellation acknowledged; late Bitcoin is still monitored."
                attempt.cancellationRequested -> "Cancellation requested; late Bitcoin is still monitored."
                else -> null
            }
            body.add(ui.receipt(listOfNotNull(
                ReceiptLine("$shortTender · ${attempt.state}", LineStyle.Strong),
                stateLine?.let { ReceiptLine(it, LineStyle.Small) },
            )))
        }
        for (refund in sale.refunds) {
            val original = sale.attempts.firstOrNull { it.attemptId == refund.paymentAttemptId }
            val amount = if (refund.amountSats > 0) formatSats(refund.amountSats) else formatAmount(refund.amountMinor, sale.currency)
            val returned = if (refund.amountSats > 0) formatSats(refund.actuallyPaidSats) else formatAmount(refund.actuallyPaidMinor, sale.currency)
            body.add(ui.receipt(listOf(
                ReceiptLine("Refund requested · $amount · ${refund.state}", LineStyle.Strong),
                ReceiptLine("Original payment · ${if (original?.tender == Tender.BITCOIN) "Bitcoin" else "card"}"),
                ReceiptLine("Funds returned with evidence · $returned", LineStyle.Small),
            )))
        }
        val actions = ArrayList<Pair<String, () -> Unit>>()
        if (sale.attempts.isEmpty() && sale.remainingMinor > 0) {
            actions.add("Choose payment" to { showTenderChoice(sale.saleId) })
        }
        if (canOfferCash(sale.saleId)) {
            actions.add("Pay remaining by Cash" to { showCashEntry(sale.saleId) })
        }
        val resumableBitcoin = sale.attempts.lastOrNull { it.tender == Tender.BITCOIN && it.state !in setOf(PaymentState.CONFIRMED, PaymentState.PAID, PaymentState.CANCELED) }
        if (resumableBitcoin != null) actions.add("Resume Bitcoin request" to {
            activeSaleId = sale.saleId
            activeAttemptId = resumableBitcoin.attemptId
            val index = resumableBitcoin.counterIndex
            if (index != null && node != null) {
                try {
                    activeNativeScreen = nativeLock.withLock { node?.judge(nowSecs(), index) }
                    activePayment = activeNativeScreen?.payment
                    activeUri = activeNativeScreen?.payment?.copyUri
                } catch (_: Exception) { }
                renderActiveSale()
            } else {
                showStatus("Bitcoin request is not available", "The saved attempt remains on file. Restart the Regtest connection to reconcile it.")
            }
        })
        for (attempt in sale.attempts) {
            val refundable = refundRequestCapacity(sale, attempt) ?: continue
            val label = if (refundable.second > 0) "Request refund of ${formatSats(refundable.second)}" else "Request refund of ${formatAmount(refundable.first, sale.currency)}"
            actions.add(label to { confirmRefundRequest(sale.saleId, attempt.attemptId) })
        }
        if (attemptsNeedCardReconcile(sale)) actions.add("Reconcile Square outcome" to { reconcileCard(sale.saleId) })
        actions.add("Payment details" to { showReceiptDetails(sale.saleId) })
        if (sale.paid && !sale.requiresReview && sale.items.isNotEmpty()) {
            actions.add("Record local handoff · test only" to {
                if (inventoryReady && catalogReady && ::inventoryController.isInitialized)
                    inventoryController.showFulfillment(sale.saleId) { showReceipt(sale.saleId) }
                else showInventoryBlocked(sale.saleId)
            })
        }
        actions.add("New sale" to { showHome() })
        actions.add("Back to sales" to { showSales() })
        val bitcoinBlocksCard = sale.attempts.any { it.tender == Tender.BITCOIN && it.state !in setOf(PaymentState.CANCELED, PaymentState.CONFIRMED, PaymentState.PAID) }
        val cardOutcomeUnknown = sale.attempts.any { it.tender == Tender.CARD && it.state == PaymentState.UNKNOWN }
        if (!sale.requiresReview && sale.currency == "USD" && sale.remainingMinor > 0 && cardBridge != null && !bitcoinBlocksCard && !cardOutcomeUnknown) {
            actions.add(0, "Pay remaining by Square Sandbox" to { beginSquareAttempt(sale.saleId, sale.remainingMinor) })
        }
        val fixedActions = ArrayList<Pair<String, () -> Unit>>()
        for ((label, action) in actions) {
            when {
                label.startsWith("Request refund") -> body.add(ui.secondary(label, action))
                label == "Payment details" -> body.add(ui.quiet(label, action))
                else -> fixedActions.add(label to action)
            }
        }
        var primaryUsed = false
        val finalButtons = fixedActions.map { (label, action) ->
            val mainAction = label == "Choose payment" || label == "Resume Bitcoin request" || label == "Reconcile Square outcome" ||
                label == "Pay remaining by Square Sandbox" || (label == "New sale" && sale.paid && !sale.requiresReview)
            when {
                label.startsWith("Request refund") -> ui.secondary(label, action)
                label == "Payment details" || label == "Back to sales" -> ui.quiet(label, action)
                label == "Back to checkout" -> ui.secondary(label, action)
                mainAction && !primaryUsed -> {
                    primaryUsed = true
                    ui.primary(label, onClick = action)
                }
                label == "New sale" && !primaryUsed -> {
                    primaryUsed = true
                    ui.primary(label, onClick = action)
                }
                label == "Cancel checkout" || label == "Try cancellation again" -> ui.button(label, ButtonKind.Danger, action)
                else -> ui.secondary(label, action)
            }
        }
        showUiPage(ui.bar(if (sale.requiresReview) "Owner review" else "Sale receipt", sub = "TEST ONLY · Regtest", back = { showSales() }, logo = true), body, finalButtons)
    }

    private fun showReceiptDetails(saleId: String) {
        val sale = ledger.getSale(saleId)
        val lines = ArrayList<ReceiptLine>()
        lines.add(ReceiptLine("Sale ID · ${sale.saleId}", LineStyle.Small))
        lines.add(ReceiptLine("Created · ${java.time.Instant.ofEpochSecond(sale.createdAt)}", LineStyle.Small))
        for (cash in sale.cashPayments) {
            lines.add(ReceiptLine("Cash payment ID · ${cash.paymentId}", LineStyle.Small))
            lines.add(ReceiptLine("Cash recorded · ${java.time.Instant.ofEpochSecond(cash.recordedAt)}", LineStyle.Small))
        }
        for (attempt in sale.attempts) {
            lines.add(ReceiptLine("${attempt.tender} attempt ID · ${attempt.attemptId}", LineStyle.Small))
            attempt.expectedSats?.let { lines.add(ReceiptLine("Bitcoin quote · ${formatSats(it)} · ${attempt.quoteId}", LineStyle.Small)) }
            lines.add(ReceiptLine("Bitcoin observed · ${formatSats(attempt.receivedSats)}", LineStyle.Small))
            lines.add(ReceiptLine("Cancellation requested · ${attempt.cancellationRequested} · acknowledged · ${attempt.cancellationAcknowledged}", LineStyle.Small))
            for (e in attempt.cardEvidence) lines.add(ReceiptLine("Card evidence · ${e.result} · ${e.amountMinor} minor units · ${e.evidenceId}", LineStyle.Small))
            for (e in attempt.bitcoinEvidence) {
                val depth = if (e.confirmations >= e.requiredConfirmations) "counter configured confirmation check satisfied" else "counter configured confirmation check waiting"
                lines.add(ReceiptLine("Counter evidence · ${e.receivedSats} sats · $depth · ${e.chainState} · ${e.evidenceId}", LineStyle.Small))
            }
        }
        for (refund in sale.refunds) {
            lines.add(ReceiptLine("Refund request ID · ${refund.refundId}", LineStyle.Small))
            for (e in refund.evidence) lines.add(ReceiptLine("Refund evidence · ${e.evidenceId} · pending ${e.pending} · ${e.paidMinor} minor units · ${e.paidSats} sats", LineStyle.Small))
        }
        showUiPage(
            ui.bar("Payment details", back = { showReceipt(saleId) }, logo = true),
            listOf(ui.receipt(lines)),
            listOf(ui.secondary("Back to receipt") { showReceipt(saleId) }),
        )
    }

    private fun saleStateLabel(sale: Sale): String = when {
        sale.requiresReview -> "Owner review required"
        sale.paid && sale.cashPayments.isNotEmpty() && sale.cashPayments.sumOf { it.amountMinor } == sale.amountMinor -> "Paid in cash"
        sale.paid && sale.cashPayments.isNotEmpty() -> "Paid · mixed tenders"
        sale.paid -> "Paid in test ledger"
        sale.attempts.isEmpty() -> "Tender not selected"
        sale.attempts.any { it.state == PaymentState.REORGED } -> "Payment check reopened"
        sale.attempts.any { it.state == PaymentState.UNCONFIRMED } -> "Payment seen, unconfirmed"
        sale.attempts.any { it.state == PaymentState.PARTIAL } -> "Partial payment"
        sale.attempts.any { it.state == PaymentState.UNKNOWN } -> "Payment outcome unknown"
        sale.attempts.any { it.state == PaymentState.CANCELED } -> "Canceled; late evidence retained"
        sale.attempts.any { it.cancellationRequested } -> "Cancellation requested"
        else -> "Waiting"
    }

    private fun refundRequestCapacity(sale: Sale, attempt: PaymentAttempt): Pair<Long, Long>? {
        val settled = attempt.state in setOf(PaymentState.PAID, PaymentState.CONFIRMED, PaymentState.PARTIAL)
        if (!settled) return null
        val prior = sale.refunds.filter { it.paymentAttemptId == attempt.attemptId }
        return if (attempt.tender == Tender.BITCOIN) {
            val requested = prior.fold(java.math.BigInteger.ZERO) { total, refund -> total.add(java.math.BigInteger.valueOf(refund.amountSats)) }
            val remaining = java.math.BigInteger.valueOf(attempt.receivedSats).subtract(requested).coerceAtLeast(java.math.BigInteger.ZERO).toLong()
            if (remaining == 0L) null else 0L to remaining
        } else {
            val requested = prior.fold(java.math.BigInteger.ZERO) { total, refund -> total.add(java.math.BigInteger.valueOf(refund.amountMinor)) }
            val remaining = java.math.BigInteger.valueOf(attempt.receivedMinor).subtract(requested).coerceAtLeast(java.math.BigInteger.ZERO).toLong()
            if (remaining == 0L) null else remaining to 0L
        }
    }

    private fun confirmRefundRequest(saleId: String, attemptId: String) {
        val sale = ledger.getSale(saleId)
        val attempt = sale.attempts.firstOrNull { it.attemptId == attemptId } ?: return showReceipt(saleId)
        val amount = refundRequestCapacity(sale, attempt) ?: return showMessage(
            "No refundable amount remains",
            "The original payment is unsettled or its received amount has already been allocated to refund requests.",
        ) { showReceipt(saleId) }
        val amountText = if (amount.second > 0) formatSats(amount.second) else formatAmount(amount.first, sale.currency)
        AlertDialog.Builder(this)
            .setTitle("Record refund request?")
            .setMessage("Original attempt: ${attempt.attemptId}\nRequested amount: $amountText\n\nThis records a refund request only. It does not send Bitcoin, issue a Square refund, or move funds. Keep this sale open until an owner records external refund evidence.")
            .setNegativeButton("Keep sale open", null)
            .setPositiveButton("Record request") { _, _ ->
                authorityFlow.executeCapability(CheckoutAuthorityCapability.REFUND_REQUEST, {
                    ledger.requestRefund(saleId, UUID.randomUUID().toString(), attemptId, amount.first, amount.second, nowSecs())
                }) { error ->
                    if (error == null) showReceipt(saleId)
                    else if (error !is CheckoutAuthorityDeniedException && error !is CheckoutAuthorityUnavailableException) {
                        showMessage("Refund request was not saved", "No refund was sent. Keep the sale open and review its current payment evidence.", safeDetail(error)) { showReceipt(saleId) }
                    }
                }
            }
            .show()
    }

    private fun beginSquareAttempt(saleId: String, amountMinor: Long) {
        val bridge = cardBridge ?: return showMessage("Square is unavailable in this build", "No card attempt was created. Use Bitcoin or install the Sandbox-only build.") { showReceipt(saleId) }
        val sale = ledger.getSale(saleId)
        if (sale.requiresReview || sale.currency != "USD" || amountMinor <= 0 || amountMinor > sale.remainingMinor) {
            return showMessage("This card amount cannot be started", "The saved sale was not changed. Check its amount and currency.") { showReceipt(saleId) }
        }
        val attemptId = UUID.randomUUID().toString()
        val referenceId = attemptId
        authorityFlow.executeCapability(CheckoutAuthorityCapability.TENDER_BEGIN, {
            // The stable attempt ID is also the persisted Square reference ID used after restart.
            ledger.beginCardAttempt(saleId, attemptId, "square", attemptId, amountMinor)
        }) { error ->
            if (error is CheckoutAuthorityDeniedException || error is CheckoutAuthorityUnavailableException) return@executeCapability
            if (error != null) {
                showMessage("Square outcome needs checking", "The card flow was not opened. Inspect the saved sale before retrying or switching tender.", safeDetail(error)) { showReceipt(saleId) }
                return@executeCapability
            }
            receiptToResume = saleId
            showMessage("Square Sandbox is opening", "A card attempt was saved before the Square SDK was called. Approval alone is not payment. This is a test environment; do not hand over goods.", "Reference: $referenceId") { showReceipt(saleId) }
            try {
                bridge.begin(attemptId, referenceId, amountMinor, "USD", object : CardCheckoutBridge.Listener {
                    override fun onEvidence(evidenceId: String, result: CardResult, amountMinor: Long) {
                        val latest = ledger.getSale(saleId).attempts.first { it.attemptId == attemptId }.cardEvidence.maxByOrNull { it.observedAt }
                        val observedAt = maxOf(nowSecs(), (latest?.observedAt ?: -1L) + 1L)
                        ledger.recordCardEvidence(saleId, attemptId, evidenceId, attemptId, result, amountMinor, observedAt)
                        main.post { showReceipt(saleId) }
                    }

                    override fun onUncertain() {
                        ledger.markCancellationUnknown(saleId, attemptId)
                        main.post { showMessage("Card payment outcome is unknown", "The saved Square attempt stays locked. Do not retry the card or switch tender until it is reconciled.", "Attempt: $attemptId · reference: $referenceId") { showReceipt(saleId) } }
                    }
                })
            } catch (e: Exception) {
                try { ledger.markCancellationUnknown(saleId, attemptId) } catch (_: Exception) {}
                showMessage("Square outcome needs checking", "The saved attempt remains locked. Do not retry or switch tender.", safeDetail(e)) { showReceipt(saleId) }
            }
        }
    }

    private fun showAbout() {
        receiptToResume = null
        show("About this test app", "OpenBMT native checkout prototype. Regtest only. The USD quote is fixed developer test data. The app holds no wallet spending key. The Square SDK path, when built, is Sandbox only. No real funds or goods handover.", listOf("Back" to { showHome() }))
    }

    private fun showHomeWithError(message: String) {
        if (config == null) return showSetup(message)
        showHome()
        Toast.makeText(this, message, Toast.LENGTH_LONG).show()
    }

    private fun executeAuthorized(
        capability: CheckoutAuthorityCapability,
        failureMessage: String,
        operation: () -> Unit,
        success: () -> Unit,
    ) {
        authorityFlow.executeCapability(capability, operation) { error ->
            when (error) {
                null -> success()
                is CheckoutAuthorityDeniedException, is CheckoutAuthorityUnavailableException -> Unit
                else -> showHomeWithError("$failureMessage Details: ${safeDetail(error)}")
            }
        }
    }

    private fun showStatus(title: String, body: String, details: String? = null) {
        activeSaleShowing = false
        show(title, body, listOf("Retry" to { if (pairing != null) startCounter() else showSetup() }, "Sales" to { showSalesIfPossible() }), details)
    }

    private fun showSalesIfPossible() {
        if (::ledger.isInitialized) showSales() else showSetup()
    }

    private fun refreshHomeReadiness() {
        if ((homeStatusView == null && tenderStatusView == null) || activeSaleShowing) return
        val current = node ?: return
        val healthy = try { nativeLock.withLock { current.status().health?.healthy == true } } catch (_: Exception) { false }
        latestCounterLine = if (healthy) "Regtest counter checks are healthy." else "Waiting for the Regtest counter's native network checks."
        val unresolved = hasUnresolvedSale()
        if (unresolved) latestCounterLine = "An earlier sale needs follow-up. New orders remain separate."
        homeStatusView?.text = latestCounterLine
        tenderStatusView?.text = if (healthy) "Native Bitcoin checks are ready." else "Waiting for native Regtest network checks."
        tenderBitcoinButton?.isEnabled = healthy
        homePrimaryButton?.isEnabled = homePrimaryAllowed
    }

    private fun showMessage(title: String, body: String, details: String? = null, next: (() -> Unit)? = null) {
        activeSaleShowing = false
        val actions = listOf((if (next == null) "Back" else "Continue") to { (next ?: { showSetup() }).invoke() })
        show(title, body, actions, details)
    }

    private fun show(title: String, body: String, actions: List<Pair<String, () -> Unit>>, details: String? = null) {
        val column = baseColumn(title)
        column.addView(text(body, 16f), margins(top = 16))
        if (!details.isNullOrBlank()) column.addView(text(details, 12f), margins(top = 12))
        for ((label, action) in actions) column.addView(button(label, action), margins(top = 10))
        showRoot(column)
    }

    private fun showWithExtra(title: String, body: String, code: String, extra: View?, actions: List<Pair<String, () -> Unit>>) {
        val column = baseColumn(title)
        column.addView(text(body, 15f), margins(top = 12))
        if (extra != null) column.addView(extra, LinearLayout.LayoutParams(-1, dp(300)).also { it.topMargin = dp(14) })
        if (code.isNotEmpty()) column.addView(text(code, 12f, mono = true), margins(top = 10))
        for ((label, action) in actions) column.addView(button(label, action), margins(top = 10))
        showRoot(column)
    }

    private fun showView(title: String, content: View) {
        activeSaleShowing = false
        val column = baseColumn(title)
        (content.parent as? ViewGroup)?.removeView(content)
        column.addView(content, LinearLayout.LayoutParams(-1, 0, 1f))
        showRoot(column, scroll = false)
    }

    private fun baseColumn(title: String): LinearLayout = LinearLayout(this).apply {
        orientation = LinearLayout.VERTICAL
        setPadding(dp(22), dp(24), dp(22), dp(24))
        addView(text("OpenBMT · TEST ONLY", 12f, true), margins(bottom = 6))
        addView(text(title, 24f, true), margins(bottom = 8))
        filterTouchesWhenObscured = true
    }

    @Suppress("DEPRECATION")
    private fun showRoot(column: LinearLayout, scroll: Boolean = true) {
        activeSaleRootShown = false
        val root: View = if (scroll) ScrollView(this).apply { isFillViewport = true; addView(column) } else column
        root.setOnApplyWindowInsetsListener { view, insets ->
            view.setPadding(view.paddingLeft, dp(16) + insets.systemWindowInsetTop, view.paddingRight, view.paddingBottom)
            insets
        }
        installRoot(root)
    }

    private fun showUiPage(bar: View, body: List<View>, actions: List<Button>) {
        installRoot(ui.page(bar, body, actions))
    }

    private fun installRoot(root: View) {
        if (::authorityFlow.isInitialized && !authorityFlow.isSignedIn()) {
            renderAuthorityGate()
            return
        }
        navigationRevision++
        activeSaleRootShown = false
        homeStatusView = null
        homePrimaryButton = null
        homePrimaryAllowed = false
        tenderStatusView = null
        tenderBitcoinButton = null
        setContentView(root)
    }

    private fun text(value: String, size: Float, bold: Boolean = false, mono: Boolean = false): TextView = TextView(this).apply {
        text = value
        textSize = size
        if (bold) setTypeface(typeface, android.graphics.Typeface.BOLD)
        if (mono) typeface = android.graphics.Typeface.MONOSPACE
        setTextColor(Color.rgb(24, 34, 43))
        gravity = Gravity.START
        filterTouchesWhenObscured = true
    }

    private fun button(label: String, action: () -> Unit): Button = Button(this).apply {
        text = label
        isAllCaps = false
        minHeight = dp(52)
        setOnClickListener { action() }
        filterTouchesWhenObscured = true
    }

    private fun margins(top: Int = 0, bottom: Int = 0) = LinearLayout.LayoutParams(-1, -2).also {
        it.topMargin = dp(top); it.bottomMargin = dp(bottom)
    }

    private fun dp(n: Int) = (n * resources.displayMetrics.density).toInt()

    private fun copyText(value: String, label: String = "Bitcoin payment code") {
        val clipboard = getSystemService(CLIPBOARD_SERVICE) as android.content.ClipboardManager
        clipboard.setPrimaryClip(android.content.ClipData.newPlainText(label, value))
    }

    private fun formatAmount(minor: Long, currency: String): String = if (currency == "SAT") "${minor} sats" else "${BigDecimal.valueOf(minor, 2).setScale(2)} USD"

    private fun formatSats(sats: Long): String = "${"%,d".format(sats)} sats"

    private fun groups(text: String): String = text.chunked(4).joinToString(" ")

    private fun nowSecs(): Long = System.currentTimeMillis() / 1000L

    private fun safeDetail(e: Throwable): String = (e.message ?: e.javaClass.simpleName).filter { it >= ' ' && it != '\u007f' }.take(160)

    private fun latestFollowupSale(): Sale? = ledger.listSales().asReversed().firstOrNull(::needsPaymentFollowup)

    private fun hasUnresolvedSale(): Boolean = ledger.listSales().any(::needsPaymentFollowup)

    private fun readLimited(input: java.io.InputStream, limit: Int): ByteArray {
        val output = ByteArrayOutputStream()
        val buffer = ByteArray(8192)
        while (true) {
            val read = input.read(buffer)
            if (read < 0) break
            if (output.size() + read > limit) throw IOException("The pairing record exceeds the file size limit.")
            output.write(buffer, 0, read)
        }
        return output.toByteArray()
    }

    private fun attemptsNeedCardReconcile(sale: Sale): Boolean = sale.attempts.any {
        it.tender == Tender.CARD && it.state == PaymentState.UNKNOWN
    }

    private fun reconcileCard(saleId: String) {
        val bridge = cardBridge ?: return showMessage("Square is unavailable in this build", "The attempt remains unresolved. Use the same Sandbox app and seller to reconcile it.") { showReceipt(saleId) }
        val sale = ledger.getSale(saleId)
        val attempt = sale.attempts.lastOrNull { it.tender == Tender.CARD && it.state == PaymentState.UNKNOWN }
            ?: return showReceipt(saleId)
        val referenceId = attempt.attemptId
        authorityFlow.executeCapability(CheckoutAuthorityCapability.TENDER_BEGIN, { Unit }) { error ->
            if (error is CheckoutAuthorityDeniedException || error is CheckoutAuthorityUnavailableException) return@executeCapability
            if (error != null) {
                showMessage("Square could not be opened", "The unresolved attempt remains locked. Sign in again before reconciling it.", safeDetail(error)) { showReceipt(saleId) }
                return@executeCapability
            }
            try {
                bridge.reconcile(attempt.attemptId, referenceId, attempt.amountMinor, sale.currency, object : CardCheckoutBridge.Listener {
                    override fun onEvidence(evidenceId: String, result: CardResult, amountMinor: Long) {
                        val latest = ledger.getSale(saleId).attempts.first { it.attemptId == attempt.attemptId }.cardEvidence.maxByOrNull { it.observedAt }
                        val observedAt = maxOf(nowSecs(), (latest?.observedAt ?: -1L) + 1L)
                        ledger.recordCardEvidence(saleId, attempt.attemptId, evidenceId, attempt.stableId!!, result, amountMinor, observedAt)
                        main.post { showReceipt(saleId) }
                    }

                    override fun onUncertain() {
                        ledger.markCancellationUnknown(saleId, attempt.attemptId)
                        main.post { showMessage("Square still has no final answer", "This attempt remains locked. Do not retry the card or switch tender.", referenceId) { showReceipt(saleId) } }
                    }
                })
            } catch (e: Exception) {
                ledger.markCancellationUnknown(saleId, attempt.attemptId)
                showMessage("Square could not confirm the outcome", "The attempt remains locked. Do not retry the card or switch tender.", safeDetail(e)) { showReceipt(saleId) }
            }
        }
    }

    companion object {
        private const val PICK_REGTEST_RECORD = 7610
        private const val TYPE_BYTES = "bytes"
        private const val PAIRING_KEY_ALIAS = "openbmt-checkout-pairing-regtest-v1"
        private const val PAIRING_FILE = "pairing.obks"
        private const val LEDGER_FILE = "checkout-ledger.bin"
        private const val CATALOG_FILE = "checkout-catalog.bin"
        private const val CASH_SESSIONS_FILE = "checkout-cash-sessions.bin"
        private const val INVENTORY_FILE = "checkout-inventory.bin"
        private const val FIXTURE_FILE = "fixture.pending"
        private const val REGTEST_USD_MINOR_PER_BTC = 10_000_000L
        private const val SATS_PER_BTC = 100_000_000L
        private const val MAX_CART_QUANTITY = 999
        private const val MAX_CART_LINES = 100
        private const val MAX_MANAGE_PRODUCTS = 100
        private const val MAX_SAVED_MODIFIERS = 32
        private const val MAX_CATALOG_PRICE_MINOR = 100_000_000L
        private const val MAX_SUPPORTED_SATS = 2_100_000_000_000_000L
        private const val CART_IDS_KEY = "checkout.cart.ids"
        private const val CART_QUANTITIES_KEY = "checkout.cart.quantities"
        private const val CART_LINE_IDS_KEY = "checkout.cart.line.ids"
        private const val CART_PRODUCT_IDS_KEY = "checkout.cart.product.ids"
        private const val CART_VARIANT_IDS_KEY = "checkout.cart.variant.ids"
        private const val CART_PRODUCT_VERSIONS_KEY = "checkout.cart.product.versions"
        private const val CART_LINE_NAMES_KEY = "checkout.cart.line.names"
        private const val CART_LINE_PRICES_KEY = "checkout.cart.line.prices"
        private const val CART_LINE_QUANTITIES_KEY = "checkout.cart.line.quantities"
        private const val CART_MODIFIER_LINE_IDS_KEY = "checkout.cart.modifier.line.ids"
        private const val CART_MODIFIER_IDS_KEY = "checkout.cart.modifier.ids"
        private const val CART_MODIFIER_NAMES_KEY = "checkout.cart.modifier.names"
        private const val CART_MODIFIER_PRICES_KEY = "checkout.cart.modifier.prices"
        private const val CATALOG_SEARCH_KEY = "checkout.catalog.search"
        private const val DISCOUNT_DRAFT_KEY = "checkout.cart.discount"
        private const val TAX_DRAFT_KEY = "checkout.cart.tax"
        private const val TIP_DRAFT_KEY = "checkout.cart.tip"
        private const val RECEIPT_KEY = "checkout.receipt.resume"
        private const val HISTORY_TEXT_KEY = "checkout.history.text"
        private const val HISTORY_TENDER_KEY = "checkout.history.tender"
        private const val HISTORY_STATUS_KEY = "checkout.history.status"
        private const val MAX_PAIRING_BYTES = 1024 * 1024
        private const val MAIN_CASH_DRAWER_ID = "main-drawer"
        private val BUSINESS_FILE_NAMES = listOf(LEDGER_FILE, CATALOG_FILE, CASH_SESSIONS_FILE, INVENTORY_FILE)
    }

    private class AndroidLedgerStorage(private val dir: File, private val name: String) : DurableLedgerStorage {
        private val file = File(dir, name)
        override fun read(): ByteArray? = if (file.exists()) file.readBytes() else null
        override fun writeAtomically(bytes: ByteArray) = DurableFile.write(dir, file, bytes)
    }
}

private class PaymentQrView(context: android.content.Context, private val payment: Payment) : View(context) {
    private val ink = Paint(Paint.ANTI_ALIAS_FLAG).apply { color = Color.BLACK; style = Paint.Style.FILL }
    private val paper = Paint().apply { color = Color.WHITE; style = Paint.Style.FILL }

    init { contentDescription = "Bitcoin payment QR code"; filterTouchesWhenObscured = true }

    override fun onDraw(canvas: Canvas) {
        super.onDraw(canvas)
        canvas.drawRect(0f, 0f, width.toFloat(), height.toFloat(), paper)
        val side = payment.qrSize.toInt()
        if (side <= 0) return
        val cell = minOf(width, height).toFloat() / (side + 8)
        val left = (width - side * cell) / 2f
        val top = (height - side * cell) / 2f
        for (y in 0 until side) for (x in 0 until side) if (payment.module(x, y)) {
            canvas.drawRect(left + x * cell, top + y * cell, left + (x + 1) * cell, top + (y + 1) * cell, ink)
        }
    }
}
