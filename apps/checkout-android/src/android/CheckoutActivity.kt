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
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.TextView
import android.widget.Toast
import java.io.ByteArrayOutputStream
import java.io.File
import java.io.IOException
import java.math.BigDecimal
import java.math.BigInteger
import java.math.RoundingMode
import java.util.UUID
import java.util.concurrent.locks.ReentrantLock
import kotlin.concurrent.withLock
import org.openbmt.app.AnimatedQrView
import org.openbmt.app.DurableFile
import org.openbmt.app.KeystoreSealer
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
    private lateinit var pairingFile: SealedFile
    private lateinit var ui: Ui
    private val nodeDir by lazy { File(noBackupFilesDir, "counter-node") }
    private var pairing: ByteArray? = null
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
    private var amountField: EditText? = null
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
    private var openBitcoinButton: Button? = null
    private var cardSandboxButton: Button? = null

    /** The plain APK has no Square class on its class path. The Sandbox project overrides this. */
    protected open fun createCardCheckoutBridge(): CardCheckoutBridge? = null

    private val cardBridge: CardCheckoutBridge? get() = createCardCheckoutBridge()

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        ui = Ui(this)
        scan = ScanFlow(this, ui)
        try {
            val noBackup = noBackupFilesDir
            pairingFile = SealedFile(noBackup, KeystoreSealer(PAIRING_KEY_ALIAS), PAIRING_FILE)
            // A damaged ledger is a blocking fault. Never replace it with an empty ledger on startup.
            ledger = CheckoutLedger(AndroidLedgerStorage(noBackup, LEDGER_FILE))
            pairing = pairingFile.load()
        } catch (e: Exception) {
            show("Saved test data cannot be opened", "No saved checkout data was reset or changed. Close this test app and report the storage fault.", listOf("Close test app" to { finish() }), "Details: ${safeDetail(e)}")
            return
        }
        if (pairing == null) consumePrivateFixture()
        if (pairing == null) showSetup() else showHome()
    }

    override fun onStart() {
        super.onStart()
        visible = true
        if (pairing != null && node == null) startCounter()
    }

    override fun onStop() {
        visible = false
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
        super.onStop()
    }

    override fun onDestroy() {
        scan.stop()
        super.onDestroy()
    }

    override fun onRequestPermissionsResult(requestCode: Int, permissions: Array<out String>, grantResults: IntArray) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults)
        scan.permissionResult(requestCode, grantResults)
    }

    @Deprecated("The Android document picker returns its URI here on the minimum supported API.")
    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        super.onActivityResult(requestCode, resultCode, data)
        if (requestCode != PICK_REGTEST_RECORD || resultCode != RESULT_OK) return
        val uri = data?.data ?: return
        try {
            val stream = contentResolver.openInputStream(uri) ?: throw IOException("The selected file could not be opened.")
            stream.use { input ->
                val bytes = readLimited(input, MAX_PAIRING_BYTES)
                importPairingRecord(bytes)
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
            importPairingRecord(pending.readBytes())
            pending.delete()
        } catch (e: Exception) {
            showSetup("The private fixture was not accepted. It must be a Regtest pairing record. Details: ${safeDetail(e)}")
        }
    }

    private fun importPairingRecord(bytes: ByteArray) {
        val info = Shop.pairing(bytes, nodeDir.path)
        require(info.network == CounterNetwork.Regtest) { "Only Regtest records are accepted by this test app." }
        require(info.settings.currency == "USD" || info.settings.currency == "SAT") { "This test app accepts USD or SAT pricing." }
        pairingFile.save(bytes)
        pairing = bytes.copyOf()
        config = info
        showMessage("Regtest setup saved", "The app sealed the watch-only pairing in private app storage. This is a test environment; do not accept real funds or hand over goods.", "Network: Regtest · Pricing: ${info.settings.currency}") {
            showHome()
            if (visible) startCounter()
        }
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
            importPairingRecord(record)
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
                    when {
                        receiptId != null -> showReceipt(receiptId)
                        navigationRevision == startupNavigationRevision -> if (activeSaleId != null) renderActiveSale() else showHome()
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
        val currency = info.settings.currency
        val healthy = try { node?.let { nativeLock.withLock { it.status().health?.healthy == true } } ?: false } catch (_: Exception) { false }
        latestCounterLine = when {
            node == null -> "Counter is starting."
            hasUnresolvedSale() -> "Resolve the saved sale before starting another checkout."
            healthy -> "Regtest counter checks are healthy."
            else -> "Waiting for the Regtest counter's native network checks."
        }
        val title = if (currency == "SAT") "Amount in sats" else "Amount in USD"
        val field = EditText(this).apply {
            hint = if (currency == "SAT") "e.g. 8000" else "e.g. 8.00"
            inputType = if (currency == "SAT") android.text.InputType.TYPE_CLASS_NUMBER else android.text.InputType.TYPE_CLASS_NUMBER or android.text.InputType.TYPE_NUMBER_FLAG_DECIMAL
            setSingleLine(true)
        }
        amountField = field
        val quoteLine = if (currency == "SAT") "Bitcoin Standard: the entered amount is satoshis; no price is fetched." else "DEVELOPER REGTEST QUOTE: USD 100,000 per BTC. This is fixed test data, not a live price."
        showWithEdit(
            "Regtest Bitcoin checkout",
            "TEST ONLY · REGTEST · NO REAL FUNDS\n\n$quoteLine\n\nEnter a test amount. The app creates the code and uses the counter's watcher for payment evidence. There is no manual paid button.",
            title,
            field,
            listOf(
                "Create Bitcoin payment code" to { openBitcoinSale(field.text.toString()) },
                "Sales and receipts" to { showSales() },
                "Card Sandbox" to { startCardFlow() },
                "About this test app" to { showAbout() },
            ),
            status = text(latestCounterLine, 14f, true).also { homeStatusView = it },
        )
        openBitcoinButton = rootButtonByText("Create Bitcoin payment code")
        openBitcoinButton?.isEnabled = healthy && !hasUnresolvedSale()
        cardSandboxButton = rootButtonByText("Card Sandbox")
        cardSandboxButton?.isEnabled = cardBridge != null
    }

    private fun openBitcoinSale(raw: String) {
        val currentNode = node ?: return showStatus("Counter is starting", "Wait until the Regtest counter is connected. No code has been made.")
        val healthy = try { nativeLock.withLock { currentNode.status().health?.healthy == true } } catch (_: Exception) { false }
        if (hasUnresolvedSale()) return showHomeWithError("Resolve the saved sale before opening another checkout. No sale or attempt was created.")
        if (!healthy) return showHomeWithError("The counter's native network checks are not ready. No sale or attempt was created.")
        val info = config ?: return showHome()
        if (info.network != CounterNetwork.Regtest) return showHome()
        val amount = try { parseAmount(raw, info.settings.currency) } catch (e: Exception) {
            return showHomeWithError("Enter a positive ${if (info.settings.currency == "SAT") "whole satoshi amount" else "USD amount with at most two decimal places"}.")
        }
        val quote = quote(info.settings.currency, amount)
        if (quote.expectedSats <= 0 || quote.expectedSats > 2_100_000_000_000_000L) return showHomeWithError("This test amount is outside the supported range.")
        if (quote.expectedSats < info.settings.minSaleSats) return showHomeWithError("This test amount is below the paired counter's minimum sale of ${formatSats(info.settings.minSaleSats)}.")
        val saleId = UUID.randomUUID().toString()
        val attemptId = UUID.randomUUID().toString()
        val createdAt = nowSecs()
        try {
            // The native index is predictable before open because this prototype permits one live node and no concurrent cashier.
            val existing = nativeLock.withLock { currentNode.status().sales }
            val counterIndex = (existing.maxOrNull() ?: -1L) + 1L
            ledger.createSale(saleId, amount, info.settings.currency, createdAt)
            ledger.beginBitcoinAttempt(saleId, attemptId, counterIndex, amount, quote.expectedSats, quote.id, quote.at)
            val opened: Opened = nativeLock.withLock {
                currentNode.open(createdAt, amount, info.settings.currency, quote.rate, quote.source, quote.at)
            }
            if (opened.index != counterIndex) {
                ledger.requestCancellation(saleId, attemptId)
                showMessage("No payment code was shown", "The native counter returned an unexpected sale index. Stop here and report this test-build fault.", "Expected $counterIndex · received ${opened.index}") { showHome() }
                return
            }
            activeSaleId = saleId
            activeAttemptId = attemptId
            activePayment = opened.payment
            activeUri = opened.payment.copyUri
            activeNativeScreen = nativeLock.withLock { currentNode.judge(nowSecs(), opened.index) }
            renderActiveSale()
        } catch (e: Exception) {
            showHomeWithError("The attempt was saved, but no usable payment code was opened. Keep it in Sales for reconciliation. Details: ${safeDetail(e)}")
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
        val native = activeNativeScreen
        val amountText = formatAmount(sale.amountMinor, sale.currency)
        val received = if (sale.currency == "SAT") formatSats(attempt.receivedSats) else "${formatAmount(sale.receivedMinor, sale.currency)} received in the ledger · ${formatSats(attempt.receivedSats)} observed"
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
        if (!attempt.cancellationAcknowledged) actions.add("Copy Bitcoin payment code" to { copyText(uri); Toast.makeText(this, "Copied the Bitcoin payment code", Toast.LENGTH_SHORT).show() })
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
        val qrView = payment?.let { PaymentQrView(this, it) }
        showWithExtra(
            "Bitcoin payment · TEST ONLY",
            "Amount: $amountText\n$state\n$received\n${if (attempt.cancellationAcknowledged) "\nThe counter acknowledged cancellation. The old payment code is hidden, and late evidence continues to be watched." else ""}\n\nTEST ONLY · REGTEST · Do not hand over goods. No real funds.\n\n${native?.words?.what.orEmpty()}\n${native?.words?.next.orEmpty()}",
            uri,
            qrView,
            actions,
        )
        lastActiveSaleRenderKey = renderKey
        activeSaleRootShown = true
    }

    private fun cancelBitcoin(saleId: String, attemptId: String, counterIndex: Long) {
        try {
            ledger.requestCancellation(saleId, attemptId)
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
        } catch (e: Exception) {
            showStatus("Cancellation was not saved", "The attempt remains on file. Do not start another payment until Sales shows its current state.", safeDetail(e))
        }
    }

    private fun showSales() {
        receiptToResume = null
        activeSaleShowing = false
        val sales = ledger.listSales().asReversed()
        if (sales.isEmpty()) return showHomeWithError("There are no test sales yet.")
        val choices = sales.map { sale ->
            val label = "${formatAmount(sale.amountMinor, sale.currency)} · ${saleStateLabel(sale)}"
            label to { showReceipt(sale.saleId) }
        }.toMutableList()
        choices.add("Back to checkout" to { showHome() })
        show("Sales and receipts", "Durable local test ledger · newest first\n\nEvery row comes from a saved sale and payment attempt. A card approval is not shown as paid.", choices)
    }

    private fun showReceipt(saleId: String) {
        receiptToResume = saleId
        activeSaleShowing = false
        val sale = ledger.getSale(saleId)
        val lines = ArrayList<String>()
        lines.add("Sale: ${sale.saleId}")
        lines.add("Amount: ${formatAmount(sale.amountMinor, sale.currency)}")
        lines.add("Received in ledger: ${formatAmount(sale.receivedMinor, sale.currency)}")
        lines.add("Remaining: ${formatAmount(sale.remainingMinor, sale.currency)}")
        lines.add("Created: ${java.time.Instant.ofEpochSecond(sale.createdAt)}")
        if (sale.requiresReview) {
            lines.add("")
            lines.add("OWNER REVIEW REQUIRED · review payment and refund evidence before fulfilling or closing this sale.")
            if (sale.lateBitcoinReceipt) lines.add("Bitcoin arrived after the counter acknowledged cancellation.")
            if (sale.overpaidMinor > 0) lines.add("Excess received: ${formatAmount(sale.overpaidMinor, sale.currency)}")
            if (sale.overpaidSats > 0) lines.add("Excess Bitcoin observed: ${formatSats(sale.overpaidSats)}")
        }
        for (attempt in sale.attempts) {
            lines.add("")
            lines.add("${attempt.tender} · ${attempt.state}")
            lines.add("Attempt: ${attempt.attemptId}")
            attempt.expectedSats?.let { lines.add("Quoted Bitcoin amount: ${formatSats(it)} · ${attempt.quoteId}") }
            lines.add("Bitcoin observed: ${formatSats(attempt.receivedSats)}")
            lines.add("Cancellation requested: ${attempt.cancellationRequested}")
            lines.add("Cancellation acknowledged by counter: ${attempt.cancellationAcknowledged}")
            if (attempt.tender == Tender.CARD && attempt.state == PaymentState.UNKNOWN) {
                lines.add("Outcome is unknown. Reconcile this attempt before any new card payment.")
            }
            for (e in attempt.cardEvidence) lines.add("Card evidence: ${e.result} · ${e.amountMinor} minor units · ${e.evidenceId}")
            for (e in attempt.bitcoinEvidence) {
                val depth = if (e.confirmations >= e.requiredConfirmations) "counter's configured confirmation check satisfied" else "counter's configured confirmation check still waiting"
                lines.add("Counter evidence: ${e.receivedSats} sats · $depth · ${e.chainState}")
            }
        }
        for (refund in sale.refunds) {
            val original = sale.attempts.firstOrNull { it.attemptId == refund.paymentAttemptId }
            val amount = if (refund.amountSats > 0) formatSats(refund.amountSats) else formatAmount(refund.amountMinor, sale.currency)
            val returned = if (refund.amountSats > 0) formatSats(refund.actuallyPaidSats) else formatAmount(refund.actuallyPaidMinor, sale.currency)
            lines.add("")
            lines.add("Refund request ${refund.refundId} · ${refund.state} · $amount")
            lines.add("Original attempt: ${original?.attemptId ?: refund.paymentAttemptId}")
            lines.add("Evidence of funds returned: $returned")
        }
        val actions = ArrayList<Pair<String, () -> Unit>>()
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
        actions.add("Back to sales" to { showSales() })
        actions.add("Back to checkout" to { showHome() })
        val bitcoinBlocksCard = sale.attempts.any { it.tender == Tender.BITCOIN && it.state !in setOf(PaymentState.CANCELED, PaymentState.CONFIRMED, PaymentState.PAID) }
        val cardOutcomeUnknown = sale.attempts.any { it.tender == Tender.CARD && it.state == PaymentState.UNKNOWN }
        if (!sale.requiresReview && sale.currency == "USD" && sale.remainingMinor > 0 && cardBridge != null && !bitcoinBlocksCard && !cardOutcomeUnknown) {
            actions.add(0, "Pay remaining by Square Sandbox" to { beginSquareAttempt(sale.saleId, sale.remainingMinor) })
        }
        show(if (sale.requiresReview) "OWNER REVIEW REQUIRED · test receipt" else "Sale receipt · test record", lines.joinToString("\n"), actions)
    }

    private fun saleStateLabel(sale: Sale): String = when {
        sale.requiresReview -> "Owner review required"
        sale.paid -> "Paid in test ledger"
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
                try {
                    ledger.requestRefund(saleId, UUID.randomUUID().toString(), attemptId, amount.first, amount.second, nowSecs())
                    showReceipt(saleId)
                } catch (e: Exception) {
                    showMessage("Refund request was not saved", "No refund was sent. Keep the sale open and review its current payment evidence.", safeDetail(e)) { showReceipt(saleId) }
                }
            }
            .show()
    }

    private fun startCardFlow() {
        if (cardBridge == null) return showMessage("Card is not available in this build", "No card attempt was created. Use the Regtest Bitcoin path or install the optional Sandbox build.", "Square SDK is not present in the base APK.") { showHome() }
        val info = config ?: return showHome()
        if (info.network != CounterNetwork.Regtest) return showHome()
        if (info.settings.currency != "USD") return showMessage("Square Sandbox uses USD only", "No card attempt was created. Use the Bitcoin path for this SAT-priced test shop.") { showHome() }
        val amount = try { parseAmount(amountField?.text?.toString().orEmpty(), "USD") } catch (_: Exception) {
            return showHomeWithError("Enter a positive USD amount with at most two decimal places first.")
        }
        if (hasUnresolvedSale()) return showHomeWithError("Resolve the saved sale before opening another checkout. No card attempt was created.")
        val saleId = UUID.randomUUID().toString()
        try {
            ledger.createSale(saleId, amount, "USD", nowSecs())
            beginSquareAttempt(saleId, amount)
        } catch (e: Exception) {
            showHomeWithError("The card sale could not be saved. No Square payment was started. Details: ${safeDetail(e)}")
        }
    }

    private fun beginSquareAttempt(saleId: String, amountMinor: Long) {
        val bridge = cardBridge ?: return showMessage("Square is unavailable in this build", "No card attempt was created. Use Bitcoin or install the Sandbox-only build.") { showReceipt(saleId) }
        val sale = ledger.getSale(saleId)
        if (sale.currency != "USD" || amountMinor <= 0 || amountMinor > sale.remainingMinor) {
            return showMessage("This card amount cannot be started", "The saved sale was not changed. Check its amount and currency.") { showReceipt(saleId) }
        }
        val attemptId = UUID.randomUUID().toString()
        val referenceId = attemptId
        try {
            // The stable attempt ID is also the persisted Square reference ID used after restart.
            ledger.beginCardAttempt(saleId, attemptId, "square", attemptId, amountMinor)
            receiptToResume = saleId
            showMessage("Square Sandbox is opening", "A card attempt was saved before the Square SDK was called. Approval alone is not payment. This is a test environment; do not hand over goods.", "Reference: $referenceId") { showReceipt(saleId) }
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

    private fun showAbout() {
        receiptToResume = null
        show("About this test app", "OpenBMT native checkout prototype. Regtest only. The USD quote is fixed developer test data. The app holds no wallet spending key. The Square SDK path, when built, is Sandbox only. No real funds or goods handover.", listOf("Back" to { showHome() }))
    }

    private fun showHomeWithError(message: String) {
        if (config == null) return showSetup(message)
        showHome()
        Toast.makeText(this, message, Toast.LENGTH_LONG).show()
    }

    private fun showStatus(title: String, body: String, details: String? = null) {
        activeSaleShowing = false
        show(title, body, listOf("Retry" to { if (pairing != null) startCounter() else showSetup() }, "Sales" to { showSalesIfPossible() }), details)
    }

    private fun showSalesIfPossible() {
        if (::ledger.isInitialized) showSales() else showSetup()
    }

    private fun refreshHomeReadiness() {
        if (homeStatusView == null || activeSaleShowing) return
        val current = node ?: return
        val healthy = try { nativeLock.withLock { current.status().health?.healthy == true } } catch (_: Exception) { false }
        latestCounterLine = if (healthy) "Regtest counter checks are healthy." else "Waiting for the Regtest counter's native network checks."
        val unresolved = hasUnresolvedSale()
        if (unresolved) latestCounterLine = "Resolve the saved sale before starting another checkout."
        homeStatusView?.text = latestCounterLine
        openBitcoinButton?.isEnabled = healthy && !unresolved
        cardSandboxButton?.isEnabled = cardBridge != null
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

    private fun showWithEdit(title: String, body: String, fieldLabel: String, field: EditText, actions: List<Pair<String, () -> Unit>>, status: TextView? = null) {
        activeSaleShowing = false
        val column = baseColumn(title)
        column.addView(text(body, 15f), margins(top = 12))
        if (status != null) column.addView(status, margins(top = 12))
        column.addView(text(fieldLabel, 14f, true), margins(top = 20))
        column.addView(field, margins(top = 6))
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
        navigationRevision++
        activeSaleRootShown = false
        val root: View = if (scroll) ScrollView(this).apply { isFillViewport = true; addView(column) } else column
        root.setOnApplyWindowInsetsListener { view, insets ->
            view.setPadding(view.paddingLeft, dp(16) + insets.systemWindowInsetTop, view.paddingRight, view.paddingBottom)
            insets
        }
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

    private fun rootButtonByText(label: String): Button? {
        fun find(v: View): Button? {
            if (v is Button && v.text.toString() == label) return v
            if (v is ViewGroup) for (i in 0 until v.childCount) find(v.getChildAt(i))?.let { return it }
            return null
        }
        return find(window.decorView)
    }

    private fun copyText(value: String) {
        val clipboard = getSystemService(CLIPBOARD_SERVICE) as android.content.ClipboardManager
        clipboard.setPrimaryClip(android.content.ClipData.newPlainText("Bitcoin payment code", value))
    }

    private fun formatAmount(minor: Long, currency: String): String = if (currency == "SAT") "${minor} sats" else "${BigDecimal.valueOf(minor, 2).setScale(2)} USD"

    private fun formatSats(sats: Long): String = "${"%,d".format(sats)} sats"

    private fun groups(text: String): String = text.chunked(4).joinToString(" ")

    private fun nowSecs(): Long = System.currentTimeMillis() / 1000L

    private fun safeDetail(e: Throwable): String = (e.message ?: e.javaClass.simpleName).filter { it >= ' ' && it != '\u007f' }.take(160)

    private fun hasUnresolvedSale(): Boolean = ledger.listSales().any { sale ->
        sale.attempts.any { attempt ->
            (attempt.tender == Tender.BITCOIN && attempt.state !in setOf(PaymentState.CONFIRMED, PaymentState.PAID, PaymentState.CANCELED)) ||
                (attempt.tender == Tender.CARD && attempt.state in setOf(PaymentState.UNKNOWN, PaymentState.PARTIAL))
        }
    }

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

    companion object {
        private const val PICK_REGTEST_RECORD = 7610
        private const val TYPE_BYTES = "bytes"
        private const val PAIRING_KEY_ALIAS = "openbmt-checkout-pairing-regtest-v1"
        private const val PAIRING_FILE = "pairing.obks"
        private const val LEDGER_FILE = "checkout-ledger.bin"
        private const val FIXTURE_FILE = "fixture.pending"
        private const val REGTEST_USD_MINOR_PER_BTC = 10_000_000L
        private const val SATS_PER_BTC = 100_000_000L
        private const val MAX_PAIRING_BYTES = 1024 * 1024
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
