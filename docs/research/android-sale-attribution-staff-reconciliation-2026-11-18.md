# Android sale attribution and staff reconciliation

**Date:** 18 November 2026

**Base commit:** `36782bbf74c47861b2326bde3166690134da507a`

**Product boundary:** experimental standalone Android checkout, Regtest/test-only, single app process

**Status:** model, source, APK, and disposable API 35 emulator validation passed; the boundaries below remain open

## Outcome

Android manual-sale creation now enters Ledger v9 through the authenticated authority worker. One stable sale ID also determines one stable operation ID, and the sale plus its `sale.create` actor event are encoded in the same ledger write. A single-flight guard prevents repeat taps from becoming separate sales while the first command is running. Cancellation before ledger access reports a definitive unavailable result. Once ledger access begins, its result is delivered even if a lifecycle lock changes the authority generation. An uncertain ledger write latches authority and business commands unavailable and forces the Activity to close; recreation reopens the ledger before checkout can resume.

Ledger v9 domain-separates the sale command digest. Existing attributed Ledger v8 files are verified with the original v8 digest, left byte-for-byte unchanged on read, normalized in memory, and written as v9 on the next mutation. Strict decode requires supported human `sale.create` events, one installation scope, one distinct create event for each sale whenever history claims complete attribution, an existing target sale, a valid commit identity, and a digest recomputed from the saved sale.

New staff mutations use audit protocol v2. A prepared audit row contains a process-epoch owner marker; the authority mutation stores a bounded non-secret receipt and versioned request digest in the same authority compare-and-swap. Reconciliation reads audit, authority, then audit again and accepts only a stable audit revision. A matching receipt is sufficient historical proof of commit. Without a receipt, a no-change terminal requires an inactive same-process owner, the original authority generation, and the exact saved pre-state. Live operations, malformed bindings, cross-file disagreements with a surviving anchor, legacy ambiguous prepares, and every other unproved outcome remain blocked.

Audit protocol v2 reserves and strictly binds `p-`, `c-`, `r-`, and `n-` operation rows, requires the prepare to precede its one permitted terminal, and rejects legacy/current same-base collisions before the authority mutation. Protocol-v1 rows remain readable and opaque during migration. PINs, PIN verifiers, salts, and PIN-derived hashes are excluded from audit events and request digests.

## Validation

The coordinator ran:

- `python3 -m unittest discover -s apps/checkout-android/tests -p 'test_*.py'`: 18 source and harness tests passed;
- the complete checkout Kotlin/JVM suite under JDK 21, Kotlin 2.2.20, JVM 1.8, and `-Werror`;
- `python3 -m py_compile apps/checkout-android/tests/android/authority_acceptance.py` and `git diff --check`;
- the two-ABI checkout APK build with coordinator-provided feature-verified native libraries, Kotlin compilation, dexing, alignment, test signing, and signature verification; and
- the full non-expiry Android authority harness on disposable `emulator-5554`, API 35.

The emulator passed:

- first-manager enrollment and exact recovery-code acknowledgement;
- authorized Regtest wallet pairing;
- one $1.23 manual sale with no tender, exactly once before and after process restart, with an unchanged ledger hash across restart;
- deliberate cash settlement of that fixture sale before unrelated scenarios;
- staff creation and manager-PIN changes with mandatory reauthentication;
- cashier absence of manager-only Staff, Reports, Catalog, and Inventory routes;
- process recreation and background/foreground lifecycle lock;
- report-picker process death and fresh authorization before a nonempty CSV export;
- manager recovery with exact replacement-code acknowledgement;
- corrupt authority, missing authority, and missing completed descriptor fail-closed behavior without changing business-file hashes;
- preexisting business data blocking fresh enrollment without mutation; and
- pairing-picker process death discarding the uncommitted import.

The resulting test-only APK is `/tmp/openbmt-checkout-reconcile-final/openbmt-checkout-test.apk`, 62,740,271 bytes, SHA-256 `36a71bb50be7e588369a1f76bc0af13d40e4d7cc3687282320b2a7388aff002a`.

## Claim boundaries and next gates

1. Actor-bearing Android coverage is limited to new manual-sale creation. Tender, refund, provider-observation, catalog, and inventory writes remain unattributed compatibility paths. Cash-session attribution is a separate earlier format.
2. Ledger sale and actor event atomicity is local to one file. Ledger storage is not multi-instance compare-and-swap, tamper-evident, or rollback-resistant.
3. Staff no-change ownership is process-local. OpenBMT's Android app is single-process; multiprocess authority/audit access is unsupported until an interprocess lock or lease exists.
4. Receipt anchors detect disagreements only while an anchor survives. Coordinated rollback of both sealed files, or rollback that erases the only receipt before a terminal audit row exists, cannot be detected without external monotonic state.
5. The emulator proves the normal attributed sale and restart path. The uncertain-write latch and staff cross-file crash windows are source/JVM fault-injection covered, not emulator fault-injected, and no physical-device run was performed.
6. Pre-v2 ambiguous prepared staff rows remain blocked rather than guessed. The bounded authority receipt and audit histories need an archival/rotation design before their limits are approached.
7. The optional five-minute expiry wait was not run in this invocation. Production card approval/hardware, mainnet/real-funds operation, integrated Lightning tender, tamper-evident backup/restore, and merchant-shift trials remain open.
8. Canonical statuses in the 502-capability register remain unchanged. This tranche strengthens foundations; it is not a competitor-parity completion claim.

Machine-readable evidence is recorded in `tools/merchant-acceptance-investigation/evidence/android-sale-attribution-staff-reconciliation-2026-11-18.json`.
