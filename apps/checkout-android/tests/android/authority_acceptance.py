#!/usr/bin/env python3
"""Black-box Android acceptance flow for the TEST ONLY checkout APK.

Drives Android's accessibility tree through adb/uiautomator. It stores no screen dumps,
screenshots, recovery codes, PINs, or logcat output. Run only against a disposable emulator.
"""
from __future__ import annotations

import argparse
import hashlib
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import time
import xml.etree.ElementTree as ET


PACKAGE = "org.openbmt.checkout"
ACTIVITY = "org.openbmt.checkout.CheckoutActivity"
REMOTE_XML = "/sdcard/authority-acceptance-window.xml"
AUTHORITY_FILE = "no_backup/checkout-authority.obis"
INSTALLATION_FILE = "no_backup/checkout-installation-scope.obis"
LEDGER_FILE = "no_backup/checkout-ledger.bin"
BUSINESS_FILES = (
    "no_backup/pairing.obks",
    LEDGER_FILE,
    "no_backup/checkout-catalog.bin",
    "no_backup/checkout-cash-sessions.bin",
    "no_backup/checkout-inventory.bin",
)


class AcceptanceFailure(RuntimeError):
    pass


def say(message: str) -> None:
    print(message, flush=True)


def find_adb() -> str:
    sdk = Path(os.environ.get("ANDROID_SDK_ROOT", os.environ.get("ANDROID_HOME", Path.home() / "Android/Sdk")))
    candidate = sdk / "platform-tools/adb"
    if candidate.is_file():
        return str(candidate)
    found = shutil.which("adb")
    if found:
        return found
    raise AcceptanceFailure("adb was not found; set ANDROID_SDK_ROOT or add platform-tools to PATH")


class Device:
    def __init__(self, adb: str, serial: str | None):
        self.adb_path = adb
        try:
            listing = subprocess.run(
                [self.adb_path, "devices", "-l"], stdout=subprocess.PIPE,
                stderr=subprocess.PIPE, check=True, timeout=45,
            ).stdout
        except (subprocess.TimeoutExpired, subprocess.CalledProcessError) as error:
            raise AcceptanceFailure("adb could not enumerate connected devices") from error
        devices = listing.decode("utf-8", "replace").splitlines()[1:]
        connected = [line.split()[0] for line in devices if line.split() and line.split()[1] == "device"]
        if serial is None:
            if len(connected) != 1:
                raise AcceptanceFailure(f"expected one connected emulator, found {len(connected)}")
            serial = connected[0]
        if serial not in connected:
            raise AcceptanceFailure("the requested serial is not an authorized connected adb device")
        self.serial = serial
        qemu = self.shell("getprop", "ro.kernel.qemu").strip() or self.shell("getprop", "ro.boot.qemu").strip()
        if qemu != "1":
            raise AcceptanceFailure("refusing to install/uninstall on a physical or unidentified device; use an Android emulator")
        sdk = self.shell("getprop", "ro.build.version.sdk").strip()
        if not sdk.isdigit() or int(sdk) < 29:
            raise AcceptanceFailure("checkout acceptance requires an API 29+ emulator")

    def command(self, *args: str, input_bytes: bytes | None = None, output: bool = False) -> bytes:
        try:
            result = subprocess.run(
                [self.adb_path, "-s", self.serial, *args],
                input=input_bytes,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                check=True,
                timeout=45,
            )
            return result.stdout if output else b""
        except subprocess.TimeoutExpired as error:
            raise AcceptanceFailure(f"adb operation timed out: {args[0] if args else 'command'}") from error
        except subprocess.CalledProcessError as error:
            # Never echo adb arguments: some tests pass credentials to `input text`.
            raise AcceptanceFailure(f"adb operation failed: {args[0] if args else 'command'}") from error

    def shell(self, *args: str) -> str:
        return self.command("shell", *args, output=True).decode("utf-8", "replace")

    def app_file(self, path: str) -> bytes:
        if not re.fullmatch(r"(?:files|no_backup)/[a-zA-Z0-9._-]+", path):
            raise AcceptanceFailure("invalid app-private test path")
        self.command("shell", "run-as", PACKAGE, "test", "-f", path)
        return self.command("exec-out", "run-as", PACKAGE, "cat", path, output=True)

    def write_app_file(self, path: str, data: bytes) -> None:
        if not re.fullmatch(r"(?:files|no_backup)/[a-zA-Z0-9._-]+", path):
            raise AcceptanceFailure("invalid app-private test path")
        self.shell("run-as", PACKAGE, "mkdir", "-p", path.split("/", 1)[0])
        self.command("shell", "run-as", PACKAGE, "dd", f"of={path}", input_bytes=data)

    def remove_app_file(self, path: str) -> None:
        if not re.fullmatch(r"(?:files|no_backup)/[a-zA-Z0-9._-]+", path):
            raise AcceptanceFailure("invalid app-private test path")
        self.shell("run-as", PACKAGE, "rm", "-f", path)

    def stop_app(self) -> None:
        self.shell("am", "force-stop", PACKAGE)
        time.sleep(1)

    def launch(self) -> None:
        self.command("shell", "input", "keyevent", "KEYCODE_WAKEUP")
        self.shell("wm", "dismiss-keyguard")
        self.shell("am", "start", "-W", "-n", f"{PACKAGE}/{ACTIVITY}")

    def dump_nodes(self) -> list[ET.Element]:
        parse_error: ET.ParseError | None = None
        for attempt in range(3):
            try:
                self.shell("uiautomator", "dump", "--compressed", REMOTE_XML)
                raw = self.command("exec-out", "cat", REMOTE_XML, output=True)
            except AcceptanceFailure as error:
                raise AcceptanceFailure("uiautomator dump failed; confirm the emulator includes Android UIAutomator") from error
            finally:
                try:
                    self.shell("rm", "-f", REMOTE_XML)
                except AcceptanceFailure:
                    pass
            try:
                return list(ET.fromstring(raw).iter("node"))
            except ET.ParseError as error:
                parse_error = error
                if attempt < 2:
                    time.sleep(0.5)
        raise AcceptanceFailure("Android accessibility tree was unreadable") from parse_error

    @staticmethod
    def node_text(node: ET.Element) -> str:
        return node.attrib.get("text", "") or node.attrib.get("content-desc", "")

    def has_text(self, pattern: str) -> bool:
        expression = re.compile(pattern, re.IGNORECASE)
        return any(expression.search(self.node_text(node)) for node in self.dump_nodes())

    def wait_text(self, pattern: str, timeout: float = 30) -> None:
        end = time.monotonic() + timeout
        while time.monotonic() < end:
            if self.has_text(pattern):
                return
            time.sleep(0.7)
        raise AcceptanceFailure(f"timed out waiting for UI text matching {pattern!r}")

    def tap_text(self, pattern: str, timeout: float = 15, scroll: bool = True) -> None:
        expression = re.compile(pattern, re.IGNORECASE)
        end = time.monotonic() + timeout
        while time.monotonic() < end:
            for node in self.dump_nodes():
                if expression.search(self.node_text(node)) and node.attrib.get("clickable") == "true":
                    bounds = self._bounds(node)
                    self.shell("input", "tap", str((bounds[0] + bounds[2]) // 2), str((bounds[1] + bounds[3]) // 2))
                    time.sleep(0.3)
                    return
            if scroll:
                self.shell("input", "swipe", "540", "1500", "540", "520", "350")
            else:
                time.sleep(0.4)
        raise AcceptanceFailure(f"timed out finding clickable UI text matching {pattern!r}")

    def ensure_text_visible(self, pattern: str, timeout: float = 25) -> list[ET.Element]:
        expression = re.compile(pattern, re.IGNORECASE)
        end = time.monotonic() + timeout
        while time.monotonic() < end:
            nodes = self.dump_nodes()
            if any(expression.search(self.node_text(node)) for node in nodes):
                return nodes
            self.shell("input", "swipe", "540", "1500", "540", "520", "350")
        raise AcceptanceFailure(f"timed out scrolling to UI text matching {pattern!r}")

    def type_into_field(self, value: str, index: int = 0) -> None:
        fields = [node for node in self.dump_nodes() if node.attrib.get("class", "").endswith("EditText")]
        if index >= len(fields):
            raise AcceptanceFailure(f"expected editable field {index + 1}, found {len(fields)}")
        bounds = self._bounds(fields[index])
        self.shell("input", "tap", str((bounds[0] + bounds[2]) // 2), str((bounds[1] + bounds[3]) // 2))
        # The host passes only ASCII test strings. Spaces use Android input's %s marker.
        encoded = value.replace("%", "%25").replace(" ", "%s")
        self.shell("input", "text", encoded)

    def replace_field(self, value: str, index: int = 0) -> None:
        fields = [node for node in self.dump_nodes() if node.attrib.get("class", "").endswith("EditText")]
        if index >= len(fields):
            raise AcceptanceFailure(f"expected editable field {index + 1}, found {len(fields)}")
        bounds = self._bounds(fields[index])
        self.shell("input", "tap", str((bounds[0] + bounds[2]) // 2), str((bounds[1] + bounds[3]) // 2))
        self.shell("input", "keyevent", "KEYCODE_MOVE_END")
        self.shell("input", "keyevent", *(["KEYCODE_DEL"] * 180))
        encoded = value.replace("%", "%25").replace(" ", "%s")
        self.shell("input", "text", encoded)

    def download_file_size(self, name: str) -> int | None:
        if not re.fullmatch(r"[a-zA-Z0-9._-]+", name):
            raise AcceptanceFailure("invalid test download name")
        # A successful directory listing distinguishes an absent name from an adb,
        # permission, or stat failure. Only the former is a valid "no file" result.
        names = self.shell("ls", "-1", "/sdcard/Download").splitlines()
        if name not in names:
            return None
        raw = self.shell("stat", "-c", "%s", f"/sdcard/Download/{name}").strip()
        if not raw.isdigit():
            raise AcceptanceFailure("test download size was unreadable")
        return int(raw)

    def type_into_field_after_label(self, anchor_pattern: str, field_after: int, value: str) -> None:
        expression = re.compile(anchor_pattern, re.IGNORECASE)
        end = time.monotonic() + 25
        while time.monotonic() < end:
            nodes = self.dump_nodes()
            anchor = next((i for i, node in enumerate(nodes) if expression.search(self.node_text(node))), None)
            if anchor is None:
                self.shell("input", "swipe", "540", "1500", "540", "520", "350")
                continue
            fields = [node for node in nodes[anchor + 1:] if node.attrib.get("class", "").endswith("EditText")]
            if field_after < len(fields):
                bounds = self._bounds(fields[field_after])
                self.shell("input", "tap", str((bounds[0] + bounds[2]) // 2), str((bounds[1] + bounds[3]) // 2))
                encoded = value.replace("%", "%25").replace(" ", "%s")
                self.shell("input", "text", encoded)
                return
            self.shell("input", "swipe", "540", "1050", "540", "800", "300")
        raise AcceptanceFailure("timed out finding staff editor field")

    def recovery_code_from_accessibility(self) -> str:
        codes = [
            self.node_text(node)
            for node in self.dump_nodes()
            if re.fullmatch(r"[0-9a-f]{64}", self.node_text(node))
        ]
        if len(codes) != 1:
            raise AcceptanceFailure("expected exactly one visible one-time recovery code")
        return codes[0]

    @staticmethod
    def _bounds(node: ET.Element) -> tuple[int, int, int, int]:
        match = re.fullmatch(r"\[(\d+),(\d+)\]\[(\d+),(\d+)\]", node.attrib.get("bounds", ""))
        if not match:
            raise AcceptanceFailure("UI node has no usable screen bounds")
        return tuple(map(int, match.groups()))


class Suite:
    def __init__(self, device: Device, apk: Path, fixture: Path, include_expiry: bool):
        self.device = device
        self.apk = apk
        self.fixture = fixture
        self.include_expiry = include_expiry
        self.pin = "314159"
        self.new_pin = "271828"
        self.recovery_code = ""

    def install_fresh(self, inject_fixture: bool = True) -> None:
        try:
            installed = "package:" in self.device.shell("pm", "path", PACKAGE)
        except AcceptanceFailure:
            installed = False
        if installed:
            self.device.shell("pm", "uninstall", PACKAGE)
        self.device.command("install", "-t", str(self.apk))
        if inject_fixture:
            # This internal fixture path is debug-only and consumed after manager sign-in.
            self.device.write_app_file("files/fixture.pending", self.fixture.read_bytes())

    def enroll_first_manager(self, inject_fixture: bool) -> None:
        self.device.launch()
        self.device.wait_text(r"Set up manager access")
        self.device.wait_text(r"TEST ONLY.*REGTEST")
        self.device.tap_text(r"Begin manager setup")
        self.device.wait_text(r"Save your recovery code")
        self.recovery_code = self.device.recovery_code_from_accessibility()
        self.device.type_into_field(self.recovery_code)
        self.device.tap_text(r"Confirm recovery code")
        self.device.wait_text(r"Create manager PIN")
        self.device.type_into_field("Acceptance Manager", 0)
        self.device.type_into_field(self.pin, 1)
        self.device.tap_text(r"Create manager")
        self.device.wait_text(r"Sign in")
        self.sign_in(self.pin, expect_checkout=inject_fixture)
        if not inject_fixture:
            self.device.wait_text(r"Set up test checkout")
            return
        # Pairing is committed only inside the WALLET_CONFIGURE capability operation;
        # sign_in() acknowledges the resulting confirmation and reaches checkout.
        self.expect_app_file("no_backup/pairing.obks", exists=True)
        self.expect_app_file("files/fixture.pending", exists=False)
        say("PASS fresh manager enrollment and authorized wallet pairing")

    def launch_fresh_enrollment(self) -> None:
        self.install_fresh()
        self.enroll_first_manager(inject_fixture=True)

    def wait_checkout_ready(self, timeout: float = 45) -> None:
        self.device.wait_text(r"^(?:Shop tools|Regtest setup saved)$", timeout=timeout)
        if self.device.has_text(r"^Regtest setup saved$"):
            self.device.tap_text(r"^Continue$", timeout=15)
        self.device.wait_text(r"^Shop tools$", timeout=timeout)
        self.device.wait_text(r"^Sales and receipts$", timeout=timeout)

    def open_shop_tools(self) -> None:
        self.device.tap_text(r"^Shop tools$", timeout=30)
        self.device.wait_text(r"^Shop tools$", timeout=15)

    def open_reports(self) -> None:
        self.open_shop_tools()
        self.device.tap_text(r"^Reports · local ledger$", timeout=20)
        self.device.wait_text(r"^Sales report$", timeout=45)

    def open_sales(self) -> None:
        self.device.tap_text(r"^Sales and receipts$", timeout=25)
        self.device.wait_text(r"^Sales and receipts$", timeout=30)

    def assert_single_unpaid_manual_sale(self) -> None:
        self.device.wait_text(r"^1 of 1 sales$", timeout=30)
        self.device.wait_text(r"^1\.23 USD · Tender not selected$", timeout=15)
        self.device.wait_text(r"^Manual amount$", timeout=15)

    def attributed_sale_persistence(self) -> None:
        self.open_sales()
        if not self.device.has_text(r"^0 of 0 sales$"):
            raise AcceptanceFailure("fresh paired checkout did not start with an empty sales list")
        self.device.tap_text(r"^Back to checkout$")
        self.wait_checkout_ready()
        ledger_before = hashlib.sha256(self.device.app_file(LEDGER_FILE)).hexdigest()

        self.open_shop_tools()
        self.device.tap_text(r"^Manual amount$")
        self.device.wait_text(r"^Manual amount$")
        self.device.type_into_field("1.23")
        self.device.shell("input", "keyevent", "KEYCODE_BACK")
        self.device.tap_text(r"^Review amount · choose payment$")
        self.device.wait_text(r"^Choose payment$", timeout=45)
        # Viewing Sales from this screen leaves the sale's tender list empty.
        self.open_sales()
        self.assert_single_unpaid_manual_sale()
        ledger_after_create = hashlib.sha256(self.device.app_file(LEDGER_FILE)).hexdigest()
        if ledger_after_create == ledger_before:
            raise AcceptanceFailure("manual sale did not change the durable ledger")

        self.force_restart_to_signin()
        self.sign_in(self.pin, expect_checkout=None)
        self.device.wait_text(r"^Choose payment$", timeout=45)
        self.open_sales()
        self.assert_single_unpaid_manual_sale()
        ledger_after_restart = hashlib.sha256(self.device.app_file(LEDGER_FILE)).hexdigest()
        if ledger_after_restart != ledger_after_create:
            raise AcceptanceFailure("durable ledger changed while reopening the unpaid sale")
        say("PASS one attributed-path manual sale remains unpaid and durable after restart")

        # Resolve the fixture sale before unrelated staff/report scenarios. The normal
        # checkout preserves the sale and requires reopening its receipt explicitly.
        self.device.tap_text(r"^Back to checkout$")
        self.wait_checkout_ready()
        self.open_sales()
        self.device.tap_text(r"^Open receipt$")
        self.device.wait_text(r"^Sale receipt$", timeout=30)
        self.device.tap_text(r"^Choose payment$")
        self.device.wait_text(r"^Choose payment$", timeout=30)
        self.device.tap_text(r"^Cash$")
        self.device.wait_text(r"^Cash received · USD$", timeout=20)
        self.device.type_into_field("1.23")
        self.device.tap_text(r"^Record cash received$")
        self.device.wait_text(r"^Sale receipt$", timeout=30)
        self.device.tap_text(r"^New sale$")
        self.wait_checkout_ready()

    def sign_in(self, pin: str, expect_checkout: bool | None = True) -> None:
        self.device.wait_text(r"Sign in")
        self.device.tap_text(r"Acceptance Manager.*Manager")
        self.device.wait_text(r"Enter PIN")
        self.device.type_into_field(pin)
        self.device.tap_text(r"Sign in")
        if expect_checkout is True:
            self.wait_checkout_ready(timeout=45)
        elif expect_checkout is False:
            self.device.wait_text(r"Set up test checkout", timeout=45)

    def force_restart_to_signin(self) -> None:
        self.device.stop_app()
        self.device.launch()
        self.device.wait_text(r"Sign in")
        # Process-local access must not be serialized or restored across process recreation.
        if self.device.has_text(r"^(?:Shop tools|Sales and receipts|Reports · local ledger)$|^Review basket(?:\s|$)"):
            raise AcceptanceFailure("normal checkout content appeared before reauthentication")

    def background_lock(self) -> None:
        self.device.shell("input", "keyevent", "KEYCODE_HOME")
        time.sleep(1)
        self.device.launch()
        self.device.wait_text(r"Sign in")
        self.sign_in(self.pin)
        say("PASS background/foreground lifecycle lock")

    def staff_policy_and_role_access(self) -> None:
        self.open_shop_tools()
        self.device.tap_text(r"^Manage staff · local authority$")
        self.device.wait_text(r"^Manage staff access$")
        self.device.type_into_field("Acceptance Cashier", 0)
        self.device.type_into_field("135790", 1)
        self.device.tap_text(r"^Create staff member$")
        self.device.wait_text(r"^Sign in$", timeout=30)
        self.sign_in(self.pin)

        self.open_shop_tools()
        self.device.tap_text(r"^Manage staff · local authority$")
        self.device.wait_text(r"^Manage staff access$")
        self.device.ensure_text_visible(r"Acceptance Manager · Manager · enabled")
        self.device.type_into_field_after_label(
            r"Acceptance Manager · Manager · enabled", 1, "493827"
        )
        self.device.shell("input", "keyevent", "KEYCODE_BACK")
        self.device.tap_text(r"^Change PIN$", timeout=20)
        self.device.wait_text(r"^Sign in$", timeout=30)
        self.pin = "493827"
        self.sign_in(self.pin)
        say("PASS staff creation and manager PIN policy changes force reauthentication")

        self.force_restart_to_signin()
        self.device.tap_text(r"Acceptance Cashier.*Cashier")
        self.device.wait_text(r"Enter PIN")
        self.device.type_into_field("135790")
        self.device.tap_text(r"^Sign in$")
        self.wait_checkout_ready()
        self.open_shop_tools()
        for manager_only in (
            r"^Manage staff · local authority$",
            r"^Reports · local ledger$",
            r"^Manage items$",
            r"^Local inventory · test only$",
        ):
            if self.device.has_text(manager_only):
                raise AcceptanceFailure("cashier can see a manager-only Shop Tools path")
        say("PASS cashier does not see manager-only Staff, Reports, Catalog, or Inventory paths")
        self.force_restart_to_signin()
        self.sign_in(self.pin)

    def process_recreation(self) -> None:
        self.force_restart_to_signin()
        self.sign_in(self.pin)
        say("PASS process recreation requires sign-in")

    def manager_recovery(self) -> None:
        self.force_restart_to_signin()
        self.device.tap_text(r"Recover manager access")
        self.device.wait_text(r"Recover manager access")
        self.device.type_into_field(self.recovery_code, 0)
        self.device.type_into_field(self.new_pin, 1)
        self.device.tap_text(r"Prepare recovery")
        self.device.wait_text(r"Save replacement recovery code")
        next_code = self.device.recovery_code_from_accessibility()
        self.device.type_into_field(next_code)
        self.device.tap_text(r"Confirm code and recover")
        self.device.wait_text(r"Sign in")
        self.recovery_code = next_code
        self.pin = self.new_pin
        self.sign_in(self.pin)
        say("PASS manager recovery requires exact replacement-code acknowledgement")

    def report_picker_continuation(self) -> None:
        self.open_reports()
        self.device.wait_text(r"Save CSV", timeout=45)
        self.device.tap_text(r"Save CSV")
        self.device.wait_text(r"Save report CSV")
        self.device.tap_text(r"Choose file")
        self.device.wait_text(r"Save", timeout=40)
        interrupted_name = f"openbmt-authority-interrupted-{int(time.time())}.csv"
        self.device.replace_field(interrupted_name)
        self.device.stop_app()
        self.device.shell("input", "keyevent", "KEYCODE_BACK")
        time.sleep(0.3)
        self.device.shell("input", "keyevent", "KEYCODE_BACK")
        time.sleep(0.5)
        self.device.launch()
        self.device.wait_text(r"^Sign in$", timeout=40)
        interrupted_size = self.device.download_file_size(interrupted_name)
        if interrupted_size not in (None, 0):
            raise AcceptanceFailure("report picker process-death case wrote report content without reauthorization")
        self.sign_in(self.pin)
        self.open_reports()
        self.device.wait_text(r"Save CSV", timeout=45)
        self.device.tap_text(r"Save CSV")
        self.device.wait_text(r"Save report CSV")
        self.device.tap_text(r"Choose file")
        self.device.wait_text(r"Save", timeout=40)
        export_name = f"openbmt-authority-export-{int(time.time())}.csv"
        self.device.replace_field(export_name)
        # The standard DocumentsUI picker may first show its location list.
        if self.device.has_text(r"Downloads"):
            try:
                self.device.tap_text(r"Downloads", timeout=3, scroll=False)
            except AcceptanceFailure:
                pass
        self.device.wait_text(r"Save", timeout=20)
        self.device.tap_text(r"^Save$", timeout=20)
        self.device.wait_text(r"Sign in", timeout=30)
        self.sign_in(self.pin, expect_checkout=None)
        self.device.wait_text(r"CSV saved", timeout=60)
        exported_size = self.device.download_file_size(export_name)
        if exported_size is None or exported_size <= 0:
            raise AcceptanceFailure("report picker returned, but the named CSV has no content in Downloads")
        say("PASS report picker process death and continuation reauthorize before writing reviewed CSV")

    def session_expiry(self) -> None:
        say("WAIT optional five-minute session expiry probe")
        self.device.wait_text(r"Sign in", timeout=330)
        say("PASS timed session expiry returns to sign-in")

    def authority_failure_probes(self) -> None:
        self.force_restart_to_signin()
        self.sign_in(self.pin)
        original = self.device.app_file(AUTHORITY_FILE)
        if not original:
            raise AcceptanceFailure("completed authority record was empty")
        business_before = self.business_file_hashes()
        self.device.stop_app()
        self.device.write_app_file(AUTHORITY_FILE, b"broken-test-record")
        self.device.launch()
        self.device.wait_text(r"Checkout access unavailable")
        self.assert_business_file_hashes(business_before, "corrupt authority")
        self.device.stop_app()
        self.device.write_app_file(AUTHORITY_FILE, original)
        self.device.remove_app_file(AUTHORITY_FILE)
        self.device.launch()
        self.device.wait_text(r"Checkout access unavailable")
        self.assert_business_file_hashes(business_before, "missing authority")
        if self.device.has_text(r"Begin manager setup"):
            raise AcceptanceFailure("completed installation offered manager re-enrollment after authority loss")
        self.device.stop_app()
        self.device.write_app_file(AUTHORITY_FILE, original)
        self.device.remove_app_file(INSTALLATION_FILE)
        self.device.launch()
        self.device.wait_text(r"Checkout access unavailable")
        self.assert_business_file_hashes(business_before, "missing installation descriptor")
        if self.device.has_text(r"Begin manager setup"):
            raise AcceptanceFailure("completed installation regenerated a lost authenticated descriptor")
        say("PASS corrupt record, missing authority record, and completed-install descriptor loss fail closed")

    def missing_authority_with_business_data(self) -> None:
        self.install_fresh(inject_fixture=False)
        sentinel = b"preserve-existing-ledger-sentinel"
        self.device.write_app_file(LEDGER_FILE, sentinel)
        business_before = self.business_file_hashes()
        self.device.launch()
        self.device.wait_text(r"Checkout access unavailable")
        self.assert_business_file_hashes(business_before, "existing business files without authority")
        if self.device.app_file(LEDGER_FILE) != sentinel:
            raise AcceptanceFailure("existing ledger sentinel changed while authority was unavailable")
        try:
            self.device.app_file(INSTALLATION_FILE)
        except AcceptanceFailure:
            pass
        else:
            raise AcceptanceFailure("installation descriptor was created over existing business data")
        say("PASS missing authority with prior business data preserves data and blocks enrollment")

    def business_file_hashes(self) -> dict[str, str | None]:
        hashes: dict[str, str | None] = {}
        for path in BUSINESS_FILES:
            try:
                content = self.device.app_file(path)
            except AcceptanceFailure:
                hashes[path] = None
            else:
                hashes[path] = hashlib.sha256(content).hexdigest()
        return hashes

    def assert_business_file_hashes(self, expected: dict[str, str | None], scenario: str) -> None:
        actual = self.business_file_hashes()
        if actual != expected:
            raise AcceptanceFailure(f"business-file hashes changed during {scenario}")

    def pairing_picker_process_death(self) -> None:
        self.install_fresh(inject_fixture=False)
        self.enroll_first_manager(inject_fixture=False)
        self.device.tap_text(r"^Import developer Regtest pairing file$")
        self.device.wait_text(r"Open from|Recent files|Browse files|Downloads|Documents", timeout=40)
        self.device.stop_app()
        self.device.shell("input", "keyevent", "KEYCODE_BACK")
        time.sleep(0.3)
        self.device.shell("input", "keyevent", "KEYCODE_BACK")
        time.sleep(0.5)
        self.device.launch()
        self.device.wait_text(r"^Sign in$", timeout=40)
        self.sign_in(self.pin, expect_checkout=False)
        self.device.wait_text(r"Set up test checkout", timeout=30)
        self.expect_app_file("no_backup/pairing.obks", exists=False)
        say("PASS pairing picker process death discards uncommitted import")

    def expect_app_file(self, path: str, exists: bool) -> None:
        try:
            self.device.app_file(path)
            present = True
        except AcceptanceFailure:
            present = False
        if present != exists:
            raise AcceptanceFailure(f"unexpected presence of app-private test file {path}")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--apk", required=True, type=Path, help="coordinator-built TEST ONLY checkout APK")
    parser.add_argument("--fixture", required=True, type=Path, help="public TEST ONLY Regtest pairing fixture")
    parser.add_argument("--serial", help="adb emulator serial; default requires exactly one connected device")
    parser.add_argument("--include-expiry", action="store_true", help="wait for the production five-minute session timeout")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    if not args.apk.is_file() or not args.fixture.is_file():
        raise AcceptanceFailure("APK and Regtest fixture paths must both exist")
    device = Device(find_adb(), args.serial)
    suite = Suite(device, args.apk.resolve(), args.fixture.resolve(), args.include_expiry)
    suite.launch_fresh_enrollment()
    suite.attributed_sale_persistence()
    suite.staff_policy_and_role_access()
    suite.process_recreation()
    suite.background_lock()
    suite.report_picker_continuation()
    suite.manager_recovery()
    if args.include_expiry:
        suite.session_expiry()
        suite.sign_in(suite.pin)
    suite.authority_failure_probes()
    suite.missing_authority_with_business_data()
    suite.pairing_picker_process_death()
    say("ALL ANDROID AUTHORITY ACCEPTANCE SCENARIOS PASSED")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except AcceptanceFailure as error:
        # Failure output contains no UI dumps, recovery codes, PINs, or adb stderr.
        print(f"FAIL {error}", file=sys.stderr)
        raise SystemExit(1)
