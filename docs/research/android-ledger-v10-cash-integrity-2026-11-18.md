# Android Ledger v10 cash integrity

**Date:** 18 November 2026

**Base commit:** `dc01307356fe21e720b92e2026d7113d7ce4b029`

**Product boundary:** experimental standalone Android checkout, Regtest/test-only; no real funds, production card processing, or physical cash verification

**Status:** source, complete JVM, APK, and disposable API 35 emulator validation passed for the exact artifact below

## Outcome

Ledger v10 extends authenticated same-file command history from manual-sale
creation to a two-stage manual-cash handover. OpenBMT durably prepares a stable
cash intent before telling the cashier to collect. A prepared intent survives
process death, blocks every other tender, and exposes only two explicit
resolutions: confirm after inspecting/receiving the cash, or cancel only after
confirming that no cash was received. Completion and cancellation use stable
operation IDs, return `NEW_COMMIT` or `REPLAY`, and preserve the original actor
attribution while exact replay reauthorizes the actor's current capability and
scope.

Completion binds a fresh sale version and remaining balance. Later Bitcoin or
card evidence may lower or raise that balance without stranding the handover:
the cash actually received is applied up to the fresh balance, excess is shown
as change, and an under-tendered handover remains a partial payment with the
rest due. A fully paid sale can record zero applied and full handback only when
the record is tied to the matching completed intent. Strict decode binds the
persisted completion snapshot, payment, command digest, status, time, actor,
scope, and commit version; later legitimate evidence does not rewrite that
historical snapshot.

Ledger writes now compare-and-swap against the exact bytes opened, including
raw v2–v9 files normalized only in memory. NIO and Android adapters serialize
cooperating writers with a canonical-path process mutex and sibling OS file
lock. A stale instance fails definitively without overwriting the winner; a
storage exception remains outcome-uncertain. Both cases poison that instance,
block authority/business commands, and force close/reopen. Android propagates
the same reload latch through typed commands and the remaining compatibility
Bitcoin/card/refund/evidence paths.

Ledger v10 records a per-sale coverage baseline. Every semantic mutation after
that baseline must have exactly one same-file command at the matching sale
version. Legacy history remains explicitly unattributed instead of being
backfilled with the currently signed-in actor. SYSTEM observation commands are
not invented in this tranche; Bitcoin/card/refund/provider observations remain
compatibility writes and advance the legacy baseline honestly.

## Validation

The coordinator ran:

- `python3 -m unittest discover -s apps/checkout-android/tests -p 'test_*.py'`: 21 source and harness tests passed;
- the complete checkout Kotlin/JVM suite under JDK 21, Kotlin 2.2.20, JVM 1.8, and `-Werror`;
- `python3 -m py_compile apps/checkout-android/tests/android/authority_acceptance.py` and `git diff --check`;
- the two-ABI checkout APK build with coordinator-provided feature-verified native libraries, Kotlin compilation, dexing, alignment, test signing, and signature verification; and
- the full non-expiry Android authority harness on disposable `emulator-5554`, API 35.

Model coverage includes exact cash prepare/complete/cancel replay, current
authorization after role/policy changes, actor/request/scope conflicts,
pre-write and post-commit uncertainty, two stale ledger instances and absent-
file races, initialization CAS, raw legacy-byte migration, strict command and
coverage corruption, maximum sale-ID round trip, process-death recovery, late
partial/full Bitcoin evidence, rising card balance, zero-applied/full-change
handoff, partial cash after a balance rise, and reopening after still-later
provider evidence.

The exact API 35 run passed:

- first-manager enrollment and authorized Regtest wallet pairing;
- one attributed $1.23 manual sale exactly once across restart;
- durable cash preparation followed by force-stop before receipt confirmation;
- recovery of that prepared intent with all other tenders blocked and an unchanged ledger hash;
- one explicit cash completion, correct paid/received/change receipt, and byte-identical ledger across another restart;
- staff policy/role reauthentication, cashier route restrictions, process and lifecycle locks, report-picker process death and reviewed CSV authorization, manager recovery, authority/descriptor fail-closed cases, prior-data enrollment blocking, and pairing-picker process death.

The resulting test-only APK is
`/tmp/openbmt-checkout-payment-v10/openbmt-checkout-test.apk`, 62,773,039 bytes,
SHA-256 `7e3125eb4ca3ac6b40efecf769c1dd2f8e2a97e28d8d7e190e1b18c9187b39bb`.

## Claim boundaries and next gates

1. Cash receipt is a signed-in human acknowledgment. OpenBMT does not detect physical notes, prove change was returned, or control a cash drawer.
2. Actor-bearing sale coverage is limited to new manual-sale creation and manual-cash prepare/complete/cancel. Bitcoin/card, refund, provider observation, catalog, inventory, fulfillment, and cash-session linking remain separate compatibility or module paths.
3. Locks and CAS protect cooperating OpenBMT writers from lost updates. The ledger is not tamper-evident, rollback/ABA-resistant, or anchored to external monotonic state. Simultaneous multiprocess Android contention was not device fault-injected.
4. Terminal cash intents are retained. The 1,000-intent, 4 MiB file, or 5,000-command bound eventually fails closed; production needs an authenticated checkpoint/archive design before any bound is approached.
5. v2–v9 files are supported read-only and migrate on the next successful write. Unreleased intermediate v10 drafts are not a compatibility promise.
6. The emulator validates normal prepare/restart/complete behavior. CAS/uncertain-write faults and late/rising provider evidence are JVM fault-injection/model coverage, not emulator fault injection or physical-device validation.
7. Production card approval/hardware, mainnet/real funds, integrated Lightning tender, merchant shifts, backup/restore freshness, and a typed SYSTEM observation writer remain open.
8. Canonical statuses in the 502-capability register remain unchanged. This tranche closes integrity prerequisites; it does not establish competitor parity or merchant readiness.

Machine-readable evidence is recorded in
`tools/merchant-acceptance-investigation/evidence/android-ledger-v10-cash-integrity-2026-11-18.json`.
