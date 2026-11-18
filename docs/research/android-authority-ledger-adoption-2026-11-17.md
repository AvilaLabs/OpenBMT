# Android authority, ledger attribution, and adoption-core record

**Date:** 17 November 2026

**Base commit:** `4393ea12`

**Product boundary:** experimental standalone Android checkout, Regtest/test-only

**Status:** model, APK, and disposable API 35 emulator validation passed; boundaries below remain open

## Outcome

This batch closes the first usable local staff-administration loop and establishes two separate foundations for attributable business activity and safe adoption.

The Android checkout now gives an authenticated manager a local staff screen for creating, renaming, changing the role or PIN of, and disabling staff. Manager-only tools are absent from a cashier's Shop Tools screen. Staff mutations use the exact `STAFF_MANAGE` capability and first append a Keystore-sealed prepared intent. A known rejection gets a terminal rejection row; a successful mutation gets a commit row. If the mutation or commit outcome is uncertain, subsequent staff administration fails closed. There is not yet an automatic reconciliation procedure for that unresolved intent.

Ledger format v8 adds an append-only command-event envelope in the same atomic file transaction as its owning sale-creation mutation. The new opaque-session sale overloads reauthorize exact scope and capability, persist operation ID, canonical payload digest, event-time actor attribution, and commit identity, and replay the same operation idempotently. A safe typed writer for provider/native observations is deliberately deferred: the schema reserves that distinction, but no current callback guesses causal links or actor identity. Existing Android callers still use compatibility methods, and those writes set an explicit legacy/unattributed marker rather than inventing an actor. Tender, refund, catalog, and inventory attribution remain future migrations.

A separate business prepare/commit/reconcile journal now accepts only an opaque live session and a closed exact event vocabulary. It reauthorizes before every prepare CAS or existing-operation return and rejects expired, disabled, stale, wrong-scope, near-prefix, and unknown requests. It is a coordination primitive only: it is not Android-wired and cannot make a second domain file atomic.

The existing-install adoption decision is now Owner approval. `openbmt-core::adoption` provides strict `OBAR` counter-signed requests and `OBAG` Owner-signed grants binding the exact five-part scope, fixed five-file manifest, required pairing record, one-use nonce, and trusted Owner/counter identities. It contains no wallet or spend operation. The Kotlin descriptor gate remains unreachable scaffolding; it deliberately cannot ship until native verification, manifest rechecks, nonce consumption, and persisted grant evidence are wired.

## Android findings

The emulator run found and fixed a real platform issue: `Mac.getInstance("HmacSHA256", "AndroidKeyStore")` is not portable as a Mac provider. The installation marker still stores a non-exportable AndroidKeyStore HMAC key, while ordinary JCA provider selection supplies the compatible HMAC implementation. Fresh descriptor creation then completed on API 35.

The black-box harness also now treats Android's document picker accurately: an interrupted create-document flow may leave a zero-byte destination, but no reviewed report content may be written without fresh manager authentication. A successful picker result writes only after reauthentication. Process death during pairing import discards the uncommitted record.

## Validation

The coordinator ran:

- 13 Kotlin/JVM checkout model groups under JDK 21, Kotlin 2.2.20, JVM 1.8, and `-Werror`;
- 13 Python Android source-contract tests;
- seven focused `openbmt-core` adoption tests, including strict decoding, every single-bit mutation, changed bindings, wrong signers, zero nonce, required pairing, and cross-domain signature rejection;
- `cargo fmt --all -- --check` and `git diff --check`;
- the two-ABI checkout APK build with feature-verified native libraries, including Kotlin, dex, 16 KiB native alignment, package alignment, signing, and signature verification; and
- the full non-expiry Android authority harness on disposable `emulator-5554`, API 35.

The emulator passed:

- first-manager enrollment and exact recovery-code acknowledgement;
- explicit manager sign-in and authorized Regtest pairing persistence;
- cashier creation and manager-PIN change, each followed by required reauthentication;
- cashier absence of Staff, Reports, Catalog-management, and Inventory-management routes;
- process recreation and foreground/background locking;
- report-picker process death, fresh export reauthorization, and nonempty CSV output;
- manager recovery with exact replacement-code acknowledgement;
- corrupt authority, missing authority, and missing completed descriptor fail-closed behavior while business-file hashes remained unchanged;
- preexisting business data blocking fresh enrollment without mutation; and
- pairing-picker process death discarding the uncommitted import.

The resulting APK was `/tmp/openbmt-checkout-authority-next/openbmt-checkout-test.apk`, 62,723,887 bytes, SHA-256 `95036991df92b6df63cf24843d6035ec40f4aaf99f70bb97cc107e1a4439cea6`.

## Claim boundaries and next gates

1. Android sale/tender/refund call sites are not yet using Ledger v8's attributed sale-creation overloads. Typed tender/refund/provider-observation writers remain deferred, and no general noncash Android actor-attribution claim is made.
2. Catalog and inventory still need domain-local atomic actor envelopes; the generic journal is not a substitute.
3. A prepared staff intent with an uncertain outcome requires a supported reconciler before production release.
4. Existing-install adoption is not an Android workflow. Counter/Owner FFI, Owner review UI, pending-nonce storage, pairing verification, manifest locking/rechecks, and durable grant evidence remain required.
5. The optional five-minute device wait was not run in this invocation; expiry remains model/source covered. A real prior-version APK upgrade, Keystore invalidation fault hook, physical-device run, merchant-shift trial, production card hardware, mainnet, and Lightning tender were not tested.
6. Local business files other than sealed authority/pairing records still need a complete authenticated backup/rollback strategy.
7. Canonical statuses in the 502-capability register remain unchanged. These foundations are evidence toward capabilities, not complete competitor-parity claims.

Machine-readable evidence is recorded in `tools/merchant-acceptance-investigation/evidence/android-authority-ledger-adoption-2026-11-17.json`.
