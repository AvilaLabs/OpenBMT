from pathlib import Path
import re
import unittest


ROOT = Path(__file__).resolve().parents[1]


class CheckoutAuthorityActivitySourceTest(unittest.TestCase):
    @staticmethod
    def _function_section(source, name):
        """Return one Kotlin member and its body, independent of formatting."""
        member_pattern = r"(?m)^ {4}(?:(?:private|internal|public)\s+)?fun\s+"
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
            (activity, "confirmSaleCreation", "SALE_CREATE", "executeAuthorized"),
            (activity, "showProductEditor", "CATALOG_MANAGE", "executeCapability"),
            (activity, "confirmArchiveProduct", "CATALOG_MANAGE", "executeCapability"),
            (inventory, "showConfirmation", "INVENTORY_MANAGE", "executeCapability"),
            (inventory, "configureProductWithMode", "INVENTORY_MANAGE", "executeCapability"),
            (inventory, "adjustProduct", "INVENTORY_MANAGE", "executeCapability"),
            (inventory, "changeMode", "INVENTORY_MANAGE", "executeCapability"),
            (activity, "showCashEntry", "TENDER_BEGIN", "executeCapability"),
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

        # Sale creation uses the shared wrapper; assert that it actually enters the
        # authority worker rather than allowing the wrapper to become a bypass.
        wrapper = self._function_section(activity, "executeAuthorized")
        self.assertRegex(wrapper, r"authorityFlow\.executeCapability\s*\(\s*capability\s*,")

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
