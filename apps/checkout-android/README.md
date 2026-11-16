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

The coordinator built both Android ABIs and ran the x86_64 APK on the existing
API 35 emulator against an isolated Bitcoin Core 32.1 Regtest node. A signed
Key app fixture supplied 1,000 public receive scripts; the checkout held no
wallet spending key. The feature build relaxed network-group quorum for this
single-node lab, required a connected filter-serving peer, and retained chain
freshness, clock, header-lag, and block-progress checks.

An $8.00 request at the explicit developer quote requested 8,000 sats. The
counter observed a confirmed 4,000-sat partial payment and updated the same
address's request to the remaining 4,000 sats. A forced process restart
restored that receipt and address. After a second payment and block, the
counter reported 8,000 sats confirmed. A second process restart restored the
paid sale in receipt history. These were actual Core transactions and native
FFI observations, not a browser simulation or a manually marked paid state.
A refund request then reserved 8,000 sats, continued to show zero returned,
and removed the duplicate full-refund action. It did not send a transaction.

The pure JVM ledger recovery tests and all 227 default counter-FFI tests
passed. Five focused tests cover the optional artifact's network refusal,
local-peer configuration, and retained health checks. The feature build also
passed clippy. Ledger cases include uncertain writes, duplicate evidence,
unknown card outcomes, late canceled Bitcoin receipts, overpayment, and a
refund request followed by reorganization and restart.

This establishes a narrow native on-chain integration proof. It does not
establish physical-device, iOS, Square-reader, Lightning, production wallet,
catalog, or merchant adoption readiness. The [Square harness](square-sandbox/README.md)
records its own SDK validation separately. The browser's proposed checkout
is an explicitly labeled simulation and does not use these native payment
observations.
