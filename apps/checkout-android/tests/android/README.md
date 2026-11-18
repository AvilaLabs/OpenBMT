# Android authority acceptance harness

`authority_acceptance.py` is a black-box UI harness for the **TEST ONLY**
checkout APK. It drives an Android emulator through `adb` and the UIAutomator
accessibility tree. It installs/uninstalls the package, so use a disposable API
29+ emulator only. The script checks `ro.kernel.qemu` and refuses unidentified
or physical devices.

## Coordinator run

The coordinator should perform one build and emulator validation run, in this
order, from the repository root:

```sh
CHECKOUTAPK_OUT=/tmp/checkout-android-build apps/checkout-android/build.sh
cargo run --manifest-path apps/checkout-android/tools/regtest-pairing/Cargo.toml -- /tmp/checkout-regtest-pairing.obmt
python3 apps/checkout-android/tests/android/authority_acceptance.py \
  --apk /tmp/checkout-android-build/openbmt-checkout-test.apk \
  --fixture /tmp/checkout-regtest-pairing.obmt
```

If the APK output path differs, pass the actual APK path. The harness requires
exactly one connected emulator unless `--serial` is provided. To include the
full production five-minute session-expiry wait, add `--include-expiry`.
Android SDK platform-tools must be on `PATH`, or `ANDROID_SDK_ROOT` must point
to the SDK. The emulator must be booted and unlocked before starting.

The host-side file-inspection helper has isolated Python unit coverage; run it
without an emulator with:

```sh
python3 -m unittest discover -s apps/checkout-android/tests/android -p 'test_*.py'
```

## Covered flows

- First-manager enrollment with exact recovery-code acknowledgement, manager
  sign-in, and debug-fixture wallet pairing through the authorized import path.
- One manual $1.23 sale created through the normal worker path, with no tender
  started; the sales list must show exactly one unpaid sale before and after
  process restart. After restart, the app must resume at `Choose payment` before
  the harness opens Sales; the ledger hash must remain stable across that restart.
  It prepares one $1.23 cash handover, force-stops before acknowledging receipt,
  and verifies the pending recovery screen blocks Bitcoin, Square, and other
  cash tenders. It then explicitly confirms receipt, verifies one paid cash row
  and zero change, and checks that the paid sale and post-completion ledger hash
  remain stable after another restart.
  The runner does not decode or print actor-bearing ledger contents; exact
  attribution and replay behavior are checked by source and Kotlin ledger tests.
- Manager creates a cashier, verifies the policy change requires manager
  reauthentication, changes the manager PIN and verifies another reauthentication,
  then signs in as cashier and checks that Staff, Reports, Catalog management,
  and Inventory management are absent from Shop Tools.
- Process recreation and background/foreground lock, followed by sign-in.
- Process death while report and pairing document pickers are open.
- Report CSV picker continuation, reauthentication, and file creation.
- Manager recovery with exact acknowledgement of the replacement recovery code.
- Corrupt and missing authority records, plus loss of the installation descriptor
  after completion; all must fail closed without offering reenrollment.
- A fresh install seeded with a business-ledger sentinel before first launch;
  missing authority must block enrollment and leave the sentinel unchanged.
- SHA-256 before/after comparisons of pairing, ledger, catalog, cash-session, and
  inventory files around corrupt or missing authority checks.
- Optional five-minute session expiry.

The pairing fixture is public Regtest test material. It must not be used outside
local Regtest proof runs.

Real in-place upgrade from a prior APK and Android Keystore invalidation are
optional coordinator checks only when a compatible prior APK or a dedicated
Keystore fault hook is available. They are not covered by this harness and
should be reported as not run when those inputs are unavailable.

## Secret handling

PINs and one-time recovery codes are held only in the runner's process memory
while entering the UI. The runner does not save screenshots, accessibility
dumps, logcat, PINs, or recovery codes and prints only scenario status. It
removes the temporary remote UIAutomator XML immediately after reading it.
The app's private files are touched only on the emulator through `run-as` for
fixture injection and authority-failure probes. Keep the emulator disposable:
the suite removes the package to establish clean-install scenarios.

This script is acceptance automation, not a production security certification.
It does not validate real-funds payments, hardware-backed Keystore behavior on
physical devices, or Android versions outside the configured emulator.

The UI path assumes the current English labels `Shop tools`, `Manage staff ·
local authority`, `Manage staff access`, `Create staff member`, and
`Reports · local ledger`. System DocumentsUI labels and picker task behavior
vary by emulator image. Both process-death picker cases passed on the
coordinator's API 35 image; other DocumentsUI versions remain untested.
