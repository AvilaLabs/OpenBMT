from pathlib import Path
import re
import unittest


ROOT = Path(__file__).resolve().parents[1]


class CheckoutAuthorityActivitySourceTest(unittest.TestCase):
    @staticmethod
    def _function_section(source, name):
        """Return one Kotlin member and its body, independent of formatting."""
        member_pattern = r"(?m)^ {4}(?:(?:private|internal|public)\s+)?fun(?:\s+<[^>\n]+>)?\s+"
        match = re.search(rf"{member_pattern}{re.escape(name)}\s*\(", source)
        if match is None:
            raise AssertionError(f"Kotlin function {name} was not found")
        next_member = re.search(member_pattern, source[match.end():])
        end = match.end() + next_member.start() if next_member else len(source)
        return source[match.start():end]

    def test_signed_in_session_expiry_is_scheduled_and_locks_authority(self):
        flow = (ROOT / "src/android/AndroidCheckoutAuthorityFlow.kt").read_text()
        self.assertRegex(flow, r"expireSession\s*=\s*Runnable\s*\{[^}]*publishedState\s+is\s+CheckoutAccessState\.SignedIn[^}]*\block\(\)")
        self.assertRegex(flow, r"main\.postDelayed\(\s*expireSession\s*,\s*SESSION_LIFETIME_MILLIS\s*\)")
        self.assertRegex(flow, r"SESSION_LIFETIME_MILLIS\s*=\s*5\s*\*\s*60\s*\*\s*1000L")
        self.assertRegex(flow, r"main\.removeCallbacks\(\s*expireSession\s*\)")

    def test_business_file_snapshot_precedes_store_initialization(self):
        activity = (ROOT / "src/android/CheckoutActivity.kt").read_text()
        snapshot = activity.index("val initialBusinessFiles = BUSINESS_FILE_NAMES")
        store_open = activity.index("ledger = CheckoutLedger(")
        self.assertLess(snapshot, store_open)
        self.assertIn("initialBusinessFiles.none { it }", activity)

    def test_normal_render_and_counter_start_require_signed_in_state(self):
        activity = (ROOT / "src/android/CheckoutActivity.kt").read_text()
        render = activity.split("private fun installRoot(root: View)", 1)[1].split("private fun text(", 1)[0]
        start = activity.split("private fun startCounter()", 1)[1].split("private fun startPump(", 1)[0]
        self.assertIn("!authorityFlow.isSignedIn()", render)
        self.assertIn("!authorityFlow.isSignedIn()", start)

    def test_authority_lock_stops_counter_and_resume_masks_previous_screen(self):
        activity = (ROOT / "src/android/CheckoutActivity.kt").read_text()
        state = self._function_section(activity, "onAuthorityState")
        stop = self._function_section(activity, "stopCounterForAuthorityLock")
        on_start = activity.split("override fun onStart()", 1)[1].split("override fun onStop()", 1)[0]
        self.assertIn("stopCounterForAuthorityLock()", state)
        self.assertIn("pumpThread?.interrupt()", stop)
        self.assertIn("node?.close()", stop)
        self.assertIn("torLease?.close()", stop)
        self.assertRegex(on_start, r"authorityFlow\.refresh\(\)[\s\S]*renderAuthorityGate\(CheckoutAccessState\.Loading\)")

    def test_authority_secrets_are_not_saved_or_copied_by_the_gate_ui(self):
        ui = (ROOT / "src/android/CheckoutAuthorityUi.kt").read_text()
        flow = (ROOT / "src/android/AndroidCheckoutAuthorityFlow.kt").read_text()
        activity = (ROOT / "src/android/CheckoutActivity.kt").read_text()
        self.assertIn("setSaveEnabled(false)", ui)
        self.assertIn("setTextIsSelectable(false)", ui)
        self.assertIn("PasswordTransformationMethod.getInstance()", ui)
        self.assertIn('DigitsKeyListener.getInstance("0123456789")', ui)
        self.assertIn("copyAndClear", ui)
        gate = self._function_section(activity, "renderAuthorityGate")
        self.assertIn("WindowManager.LayoutParams.FLAG_SECURE", gate)
        for forbidden in ("Bundle", "ClipData", "ClipboardManager", "Log."):
            self.assertNotIn(forbidden, ui)
            self.assertNotIn(forbidden, flow)

    def test_authority_work_is_serialized_and_stale_results_are_dropped(self):
        flow = (ROOT / "src/android/AndroidCheckoutAuthorityFlow.kt").read_text()
        self.assertIn("Executors.newSingleThreadExecutor", flow)
        self.assertIn("generation.get() != ticket", flow)
        self.assertIn("AndroidCheckoutAuthorityStorage(appContext, scope)", flow)
        self.assertIn("authority().signIn(actorId, pin)", flow)

    def test_staff_administration_is_manager_routed_audited_and_lifecycle_gated(self):
        activity = (ROOT / "src/android/CheckoutActivity.kt").read_text()
        flow = (ROOT / "src/android/AndroidCheckoutAuthorityFlow.kt").read_text()
        controller = (ROOT / "src/model/CheckoutAuthorityController.kt").read_text()
        tools = activity.split("private fun showShopTools", 1)[1].split("private fun showCartReview", 1)[0]
        request = activity.split("private fun requestStaffAdministration", 1)[1].split("private fun renderStaffAdministration", 1)[0]
        self.assertIn("authorityFlow.isManager()", tools)
        self.assertIn("staffAdministrationVisible", request)
        self.assertIn("staffAdministrationRevision", request)
        self.assertIn("CheckoutAuthorityAuditJournal", flow)
        self.assertIn("createStaffPrepared", controller)
        self.assertIn("renameStaffPrepared", controller)
        self.assertIn("changeRolePrepared", controller)
        self.assertIn("changePinPrepared", controller)
        self.assertIn("disableStaffPrepared", controller)
        self.assertIn("reconcileStaffIntents(active, scope)", controller)
        self.assertIn("authority().staffAdministration()", flow)

    def test_queued_staff_commands_are_dropped_after_lifecycle_generation_changes(self):
        flow = (ROOT / "src/android/AndroidCheckoutAuthorityFlow.kt").read_text()
        staff_operation = self._function_section(flow, "staffOperation")
        worker = staff_operation.split("executor.execute {", 1)[1]
        guard = worker.index("if (closed || generation.get() != ticket)")
        clear_secrets = worker.index("secrets.forEach { it.fill('\\u0000') }")
        command = worker.index("operation()")
        self.assertLess(guard, clear_secrets)
        self.assertLess(clear_secrets, command)
        self.assertIn("return@execute", worker[guard:command])

    def test_staff_reconciliation_failure_blocks_staff_writes_without_locking_checkout(self):
        controller = (ROOT / "src/model/CheckoutAuthorityController.kt").read_text()
        for name in ("createStaffMember", "renameStaffMember", "changeStaffRole", "changeStaffPin", "disableStaffMember"):
            section = self._function_section(controller, name)
            self.assertLess(
                section.index("catch (_: CheckoutAuthorityAuditReconciliationRequiredException)"),
                section.index("catch (_: CheckoutAuthorityAuditUnavailableException)"),
            )

    def test_cash_writes_use_the_opaque_session_bridge(self):
        activity = (ROOT / "src/android/CheckoutActivity.kt").read_text()
        cash = (ROOT / "src/android/CheckoutCashController.kt").read_text()
        self.assertIn("authorityFlow.cashSessionStore(", activity)
        self.assertIn("authorityFlow::executeCommand", activity)
        self.assertIn("store.openSession(it,", cash)
        self.assertIn("store.recordMovement(it,", cash)
        self.assertIn("store.linkCashSale(it,", cash)
        self.assertIn("store.closeSession(it,", cash)
        self.assertNotIn("Operator label (not authenticated)", cash)

    def test_user_mutations_route_through_the_expected_capability(self):
        activity = (ROOT / "src/android/CheckoutActivity.kt").read_text()
        inventory = (ROOT / "src/android/CheckoutInventoryController.kt").read_text()
        commands = (
            (activity, "showProductEditor", "CATALOG_MANAGE", "executeCapability"),
            (activity, "confirmArchiveProduct", "CATALOG_MANAGE", "executeCapability"),
            (inventory, "showConfirmation", "INVENTORY_MANAGE", "executeCapability"),
            (inventory, "configureProductWithMode", "INVENTORY_MANAGE", "executeCapability"),
            (inventory, "adjustProduct", "INVENTORY_MANAGE", "executeCapability"),
            (inventory, "changeMode", "INVENTORY_MANAGE", "executeCapability"),
            (activity, "beginSquareAttempt", "TENDER_BEGIN", "executeCapability"),
            (activity, "openBitcoinSale", "TENDER_BEGIN", "executeCapability"),
            (activity, "cancelBitcoin", "TENDER_BEGIN", "executeCapability"),
            (activity, "reconcileCard", "TENDER_BEGIN", "executeCapability"),
            (activity, "confirmRefundRequest", "REFUND_REQUEST", "executeCapability"),
            (activity, "commitPendingPairingImport", "WALLET_CONFIGURE", "executeCapability"),
        )
        for source, function_name, capability, boundary in commands:
            with self.subTest(function=function_name, capability=capability):
                section = self._function_section(source, function_name)
                self.assertIn(boundary, section)
                self.assertRegex(
                    section,
                    rf"CheckoutAuthorityCapability\s*\.\s*{re.escape(capability)}\b",
                )

    def test_sale_creation_uses_worker_session_and_atomic_ledger_command(self):
        activity = (ROOT / "src/android/CheckoutActivity.kt").read_text()
        flow = (ROOT / "src/android/AndroidCheckoutAuthorityFlow.kt").read_text()
        ledger = (ROOT / "src/model/CheckoutLedger.kt").read_text()
        create = self._function_section(activity, "confirmSaleCreation")
        worker = self._function_section(flow, "executeSaleCreate")
        self.assertIn("val saleId = UUID.randomUUID().toString()", create)
        self.assertIn('val operationId = "sale-$saleId"', create)
        self.assertIn("var createdAt: Long? = null", create)
        self.assertIn("createdAt ?: nowSecs()", create)
        self.assertIn("authorityFlow.executeSaleCreate(", create)
        self.assertNotIn("ledger.createSale(", create)
        self.assertIn("ledger.bindCommandAuthority(authority, scope)", worker)
        self.assertIn("runCommand { session ->", worker)
        self.assertIn("ledger.createSale(session, operationId, saleId", worker)
        self.assertIn("ledger.createPricedSale(session, operationId, saleId", worker)
        attributed = ledger.split("fun createSale(session:", 1)[1].split("/** Compatibility API", 1)[0]
        self.assertIn("CheckoutAuthorityCapability.SALE_CREATE", attributed)
        self.assertIn("replay = { getSale(saleId) }", attributed)
        self.assertIn("state.commandEvents.add(CheckoutLedgerCommandEvent(", ledger)
        self.assertIn("state.legacyHistoryUnattributed = true", ledger)
        digest = ledger.split("private fun saleCommandDigest", 1)[1].split("private fun commandDigest", 1)[0]
        self.assertIn('typedCommandDigest("sale.create")', digest)
        typed_digest = ledger.split("private fun typedCommandDigest", 1)[1].split("/** Digest used", 1)[0]
        self.assertIn('out.str("openbmt.checkout.ledger-command")', typed_digest)
        self.assertIn("COMMAND_DIGEST_FORMAT_VERSION", typed_digest)
        self.assertIn('"tender.cash.prepare", "tender.cash.complete", "tender.cash.cancel" ->', ledger)
        self.assertIn('else -> error("unsupported human ledger event type")', ledger)
        self.assertIn('error("system ledger commands are not implemented in v10")', ledger)
        self.assertIn("const val LEGACY_COMMAND_DIGEST_VERSION = 8", ledger)
        self.assertIn("const val VERSION = 10", ledger)
        self.assertIn("migratesV8SaleCommandDigestsWithoutChangingOnRead", (ROOT / "tests/CheckoutLedgerTest.kt").read_text())

    def test_uncertain_ledger_commit_blocks_checkout_until_reopen(self):
        activity = (ROOT / "src/android/CheckoutActivity.kt").read_text()
        create = self._function_section(activity, "confirmSaleCreation")
        blocked = self._function_section(activity, "lockAfterLedgerReload")
        flow = (ROOT / "src/android/AndroidCheckoutAuthorityFlow.kt").read_text()
        self.assertIn("is CheckoutLedgerWriteUncertainException, is CheckoutLedgerConcurrentWriteException -> lockAfterLedgerReload()", create)
        self.assertIn("stopCounterForAuthorityLock()", blocked)
        self.assertIn("authorityFlow.lock()", blocked)
        self.assertIn("Another checkout instance updated the ledger first", blocked)
        self.assertIn("The storage write may have committed", blocked)
        self.assertIn("reopen the app to reload the ledger", blocked)
        self.assertNotIn("showHomeWithError", blocked)
        self.assertIn("CheckoutLedgerFailureKind.DEFINITIVE_CONFLICT", flow)
        self.assertIn("CheckoutLedgerFailureKind.UNCERTAIN_COMMIT", flow)
        self.assertIn("fun latchLedgerWriteFailure(error: Exception)", flow)
        ledger = (ROOT / "src/model/CheckoutLedger.kt").read_text()
        self.assertIn("class CheckoutLedgerWriteUncertainException", ledger)

    def test_sale_creation_is_single_flight_through_dialog_and_worker(self):
        activity = (ROOT / "src/android/CheckoutActivity.kt").read_text()
        create = self._function_section(activity, "confirmSaleCreation")
        self.assertIn("if (saleCreationInFlight) return", create)
        self.assertLess(create.index("saleCreationInFlight = true"), create.index("val saleId = UUID.randomUUID()"))
        self.assertIn("if (commandStarted) return@create", create)
        self.assertIn("saleCreationInFlight = false", create)
        uncertain = create.split(
            "is CheckoutLedgerWriteUncertainException, is CheckoutLedgerConcurrentWriteException ->", 1
        )[1].split("else ->", 1)[0]
        self.assertNotIn("saleCreationInFlight = false", uncertain)
        self.assertIn("setOnCancelListener { if (!commandStarted) saleCreationInFlight = false }", create)
        self.assertIn("saleCreationInFlight = false\n                if (followup.attempts.isEmpty())", create)

    def test_sale_worker_distinguishes_pre_access_cancel_from_post_access_outcome(self):
        flow = (ROOT / "src/android/AndroidCheckoutAuthorityFlow.kt").read_text()
        activity = (ROOT / "src/android/CheckoutActivity.kt").read_text()
        worker = self._function_section(flow, "executeSaleCreate")
        pre_access = worker.split("if (generation.get() != ticket)", 1)[1].split("val error = try", 1)[0]
        self.assertIn("worker has not touched the ledger", pre_access)
        self.assertIn("completion(CheckoutAuthorityUnavailableException())", pre_access)
        post_access = worker.split("val error = try", 1)[1].split("catch (_: java.util.concurrent.RejectedExecutionException)", 1)[0]
        self.assertIn("error?.let(::latchLedgerWriteFailure)", post_access)
        self.assertIn("is CheckoutLedgerConcurrentWriteException", self._function_section(flow, "latchLedgerWriteFailure"))
        self.assertIn("is CheckoutLedgerWriteUncertainException", self._function_section(flow, "latchLedgerWriteFailure"))
        self.assertIn("completion(error)", post_access)
        self.assertNotIn("generation.get() != ticket) return@post", post_access)
        self.assertIn("if (authorityFlow.isSignedIn()) showTenderChoice(saleId)",
                      self._function_section(activity, "confirmSaleCreation"))
        self.assertIn("if (authorityFlow.isSignedIn()) {\n                            showHomeWithError(",
                      self._function_section(activity, "confirmSaleCreation"))
        self.assertIn("if (ledgerOutcomeNeedsReopen) CheckoutAccessState.Unavailable()", flow)
        self.assertIn("fun requiresLedgerReopen(): Boolean = ledgerOutcomeNeedsReopen", flow)
        on_state = self._function_section(activity, "onAuthorityState")
        self.assertIn("authorityFlow.requiresLedgerReopen()", on_state)
        self.assertIn("lockAfterLedgerReload()", on_state)

    def test_cash_handover_is_prepared_before_collection_and_recovers_after_restart(self):
        activity = (ROOT / "src/android/CheckoutActivity.kt").read_text()
        flow = (ROOT / "src/android/AndroidCheckoutAuthorityFlow.kt").read_text()
        ledger = (ROOT / "src/model/CheckoutLedger.kt").read_text()
        entry = self._function_section(activity, "showCashEntry")
        recovery = self._function_section(activity, "showCashIntentRecovery")
        complete = self._function_section(flow, "executeCashIntentMutation")
        complete_entry = self._function_section(flow, "executeCashComplete")
        prepare = self._function_section(flow, "executeCashPrepare")
        self.assertIn("snapshot.saleVersions[saleId]", entry)
        self.assertIn('val operationId = "cash-prepare-$paymentId"', entry)
        self.assertIn("cashRecordInFlight", entry)
        self.assertIn("authorityFlow.executeCashPrepare(", entry)
        self.assertIn("Do not accept cash yet", entry)
        self.assertIn('setPositiveButton("Prepare handover"', entry)
        self.assertNotIn("executeCapability", entry)
        self.assertIn("!authorityFlow.isSignedIn() || !visible", entry)
        self.assertIn("cashRecordInFlight = false", entry)
        self.assertIn("CashPaymentIntentStatus.PREPARED", entry)
        self.assertIn("ledger.prepareCashPayment(", prepare)
        self.assertIn("expectedSaleVersion", prepare)
        self.assertIn("expectedRemainingMinor", prepare)
        self.assertIn("intent.completeOperationId", complete)
        self.assertIn("intent.cancelOperationId", complete)
        self.assertIn("expectedSaleVersion: Long", complete_entry)
        self.assertIn("expectedRemainingMinor: Long", complete_entry)
        self.assertIn("expectedRemainingMinor", complete)
        self.assertIn("I received cash · confirm payment", recovery)
        self.assertIn("No cash received · cancel intent", recovery)
        self.assertIn("blocks all other tenders", recovery)
        self.assertIn("Current remaining", recovery)
        self.assertIn('ui.primary("I received cash · confirm payment")', recovery)
        self.assertNotIn("isEnabled = canComplete", recovery)
        self.assertIn("minOf(freshIntent.tenderedMinor, remaining)", recovery)
        self.assertIn("remainingAfterPayment", recovery)
        self.assertIn("This will record a partial cash payment", recovery)
        self.assertIn("The current balance determines the amount applied", recovery)
        self.assertIn("sale.pendingCashPayment", self._function_section(activity, "showReceipt"))
        self.assertIn("if (sale.pendingCashPayment != null) return showReceipt(saleId)", self._function_section(activity, "showTenderChoice"))
        confirm = self._function_section(activity, "confirmPreparedCash")
        self.assertIn("ledger.eventSnapshot()", confirm)
        self.assertIn("snapshot.saleVersions[saleId]", confirm)
        self.assertIn("sale.remainingMinor", confirm)
        self.assertIn("currentVersion != reviewedVersion", confirm)
        self.assertIn("showCashIntentRecovery(saleId, currentIntent)", confirm)
        self.assertIn("authorityFlow.executeCashComplete(ledger, saleId, currentIntent", confirm)
        self.assertIn("ledger.eventSnapshot().saleVersions[saleId]", self._function_section(activity, "cancelPreparedCash"))
        self.assertIn("authorityFlow.executeCashCancel(ledger, saleId, intent", self._function_section(activity, "cancelPreparedCash"))
        self.assertIn("fun prepareCashPayment(", ledger)
        self.assertIn("fun completePreparedCashPayment(", ledger)
        completed = ledger.split("fun completePreparedCashPayment(", 1)[1].split("fun cancelPreparedCashPayment(", 1)[0]
        self.assertIn("expectedRemainingMinor: Long", completed)
        self.assertIn("appliedMinor = if (allowPreparedCompletion) minOf(tenderedMinor, remainingMinor)", ledger)
        self.assertIn("intent.completeExpectedRemainingMinor = expectedRemainingMinor", completed)
        self.assertIn("completedExpectedRemainingMinor: Long?", ledger)
        self.assertIn("fun cancelPreparedCashPayment(", ledger)
        self.assertIn("fun getCashPaymentIntent(paymentId: String)", ledger)

    def test_emulator_cash_path_proves_prepare_recovery_then_single_completion(self):
        harness = (ROOT / "tests/android/authority_acceptance.py").read_text()
        flow = harness.split("def attributed_sale_persistence", 1)[1].split("def sign_in", 1)[0]
        for marker in (
            r"Prepare handover",
            r"Cash handover recovery",
            "self.device.stop_app()",
            r"CASH HANDOVER NEEDS RECOVERY",
            r"prepared cash intent allowed another tender",
            r"I received cash · confirm payment",
            r"Cash received · 1\.23 USD",
            r"durable cash payment ledger changed while reopening the paid sale",
        ):
            self.assertIn(marker, flow)

    def test_all_ledger_write_failures_latch_reopen(self):
        activity = (ROOT / "src/android/CheckoutActivity.kt").read_text()
        flow = (ROOT / "src/android/AndroidCheckoutAuthorityFlow.kt").read_text()
        self.assertIn("error?.let(::latchLedgerWriteFailure)", self._function_section(flow, "executeCapability"))
        for function_name in ("reconcileBitcoinAttempts", "cancelBitcoin", "beginSquareAttempt", "reconcileCard"):
            section = self._function_section(activity, function_name)
            self.assertIn("latchDirectLedgerFailure", section)

    def test_report_reads_are_authorized_and_never_render_stale_data(self):
        controller = (ROOT / "src/android/CheckoutReportController.kt").read_text()
        show = self._function_section(controller, "show")
        self.assertIn("CheckoutAuthorityCapability.REPORT_READ", show)
        self.assertRegex(show, r"report\s*=\s*null[\s\S]*executeCapability")
        self.assertIn("ledger.eventSnapshot()", show)
        self.assertIn("is CheckoutAuthorityDeniedException", show)
        self.assertIn("is CheckoutAuthorityUnavailableException", show)
        self.assertIn("if (revision != readRevision)", show)
        self.assertIn("No cached report is displayed", controller)

    def test_report_copy_and_save_side_effects_require_report_export(self):
        controller = (ROOT / "src/android/CheckoutReportController.kt").read_text()
        activity_result = self._function_section(controller, "onActivityResult")
        resume = self._function_section(controller, "resumePendingExportAfterSignIn")
        save = self._function_section(controller, "launchSave")
        copy = self._function_section(controller, "copy")
        for section in (resume, save, copy):
            self.assertIn("CheckoutAuthorityCapability.REPORT_EXPORT", section)
        self.assertIn("pendingExportUri = uri", activity_result)
        self.assertNotIn("openOutputStream", activity_result)
        self.assertIn("openOutputStream(uri, \"w\")", resume)
        self.assertIn("startActivityForResult(intent, REQUEST_CREATE_DOCUMENT)", save)
        self.assertIn("setPrimaryClip", copy)

    def test_picker_pairing_write_waits_for_fresh_manager_authority(self):
        activity = (ROOT / "src/android/CheckoutActivity.kt").read_text()
        result = activity.split("override fun onActivityResult", 1)[1].split("private fun consumePrivateFixture", 1)[0]
        commit = self._function_section(activity, "commitPendingPairingImport")
        signed_in = self._function_section(activity, "onAuthorityState")
        self.assertIn("queuePairingImport(bytes)", result)
        self.assertNotIn("pairingFile.save", result)
        self.assertIn("CheckoutAuthorityCapability.WALLET_CONFIGURE", commit)
        self.assertRegex(commit, r"executeCapability[\s\S]*pairingFile\.save\(bytes\)")
        self.assertIn("commitPendingPairingImport()", signed_in)
        self.assertIn("reportController.resumePendingExportAfterSignIn()", signed_in)


if __name__ == "__main__":
    unittest.main()
