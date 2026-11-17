# OpenBMT Checkout for Android (experimental)

This is a separate, experimental Android checkout shell built from plain
`android.widget` views. It is distinct from the Counter app. Its first payment
path is Bitcoin on **Regtest only**; it uses the existing counter FFI to make
the BIP21 payment request and observe the counter's real peer and chain checks.
It does not sign or hold wallet spending keys. The durable checkout ledger is
the source for sale and receipt history. Square is an optional Sandbox-only
path supplied by `square-sandbox`; the base build has no Square SDK and does
not claim that card payment is enabled.

**TEST ONLY. No mainnet, real funds, production payments or goods handover.**
The USD flow uses the fixed developer rate of 10,000,000 minor units per BTC
($100,000/BTC), identified as `dev-regtest-usd-100000-v1`; it is not a live
price. The app accepts only a Regtest pairing record with USD or SAT pricing.
The Regtest pairing is watch-only for checkout and cannot sign spends.

The USD cashier screen includes a small sample basket (latte $5.50 and bagel
$4.50), exact integer totals, quantity controls, and a review step. The saved
sale includes an immutable copy of item names, prices, and quantities. This is
not a merchant product catalog or inventory integration. Manual amount entry
remains available, including for SAT-priced shops. After saving the sale, the
cashier chooses Bitcoin or the optional Square Sandbox path for that same sale.

## Pairing and fixture setup

Use the camera to scan the owner's public-wallet offer and bundle through the
existing counter `Pairing` API, or import a developer Regtest pairing file from
the Android document picker. The raw bytes returned by `Pairing.finish` are
accepted as `openbmt-regtest-pairing.obmt`; the app validates that the record
is for Regtest before sealing it with Android Keystore in `noBackupFilesDir`.
The pairing record contains the counter's device identity and public wallet
metadata, not a wallet spending key.

For emulator automation only, an explicitly test-only internal file named
`filesDir/fixture.pending` is consumed on next launch. It receives the same
Regtest validation and Keystore sealing, then is deleted. It is not an
exported content provider or runtime endpoint. The APK is marked debuggable
only because this artifact is an experimental test build.

## Build

Run `build.sh`. It uses the Android SDK, Kotlin and NDK already installed for
the repository. It refuses to fetch missing Rust targets and uses Cargo's
offline mode. The counter FFI is built with the artifact-only
`checkout-regtest` feature, which is forbidden for non-Regtest records; the
normal Counter and Owner build paths do not enable it. The Android native
libraries are the repository's `openbmt-counter-ffi` and QR JNI plus the Tor library from
the SHA-256-pinned `tor-android` AAR described in `docs/DEPENDENCIES.md`.
Square is not part of this build. Building the APK does not establish that it
has been run on a device or that the end-to-end Regtest payment path works.

## Limits

This is a checkout integration prototype, not a release app or a security
review. It has not been independently audited. Regtest and the developer quote
are mandatory. The on-screen status comes from the counter node's watcher and
the durable ledger; there is no manual “paid” control. Cancellation records a
request and retains the attempt so late payment evidence can still be
reconciled. A Square Sandbox approval is not completion, and uncertain card
results remain pending until reconciled.

## Validation on 16 November 2026

The coordinator built both Android ABIs, the APK and Square SDK module, and
verified matching native method signatures. The milestone date is 16 November
in America/New_York (17 November UTC). The checkout now saves a native
basket as an immutable sale snapshot: one latte at $5.50 and one bagel at $4.50,
for $10.00 total, in ledger version 4. A force-stop before tender creation
restored the same sale ledger and allowed tender creation to resume. The
preceding version-3 ledger remains preserved.
JVM migration tests passed for version 2 and 3 records, including
conservative handling of historical refund state.

On the API 35 Android emulator, the app created a 10,000-sat Bitcoin request on
an isolated Bitcoin Core 32.1 Regtest node. A 4,000-sat confirmed payment was
observed as partial. After force-stopping and restarting the app, the identical
ledger was restored and the same address presented the 6,000-sat remainder.
After the remainder was paid, the sale became paid and the itemized receipt was
restored unchanged after another force-stop. These are actual Core transactions
and native FFI observations, not simulated or manually marked payment states.

The Square SDK path used Mobile Payments SDK 2.7.0 in the same cart. A virtual
card payment for $10.00 completed; Square's independent Payments API result
matched the native payment ID/reference. Square Sandbox and a virtual reader
were used, not production processing or physical hardware. See the separate
[Square validation record](square-sandbox/README.md#validation-on-16-november-2026).

The native checkout's limits remain: Regtest only, fixed developer exchange
rate, no wallet spending key, no Lightning payment path, and no physical-device,
iOS, production-reader, catalog/inventory, merchant onboarding, or adoption
validation. The cart is a test fixture, not a merchant catalog. This prototype
does not reproduce Square's full POS interface or feature set. The browser
checkout remains an explicitly labeled simulation and does not use these native
payment observations.
