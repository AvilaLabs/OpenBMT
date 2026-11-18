# Android authority and cash-attribution integration record

**Date:** 17 November 2026

**Base commit:** `afcb756313b531847c3f70b1b4e61d61e3cf7159`

**Product boundary:** experimental standalone Android checkout, Regtest/test-only

**Status:** implemented and locally compiled/tested; no emulator, physical-device, merchant-shift, mainnet, or production-card validation

## Outcome

The reusable local-authority model is now connected to the Android checkout. The register does not open its business stores or start its counter until a staff member has authenticated. A fresh installation requires manager enrollment, exact acknowledgement of an independently prepared recovery code, and then manager sign-in.

The integration adds:

- an Android Keystore AES-GCM authority store with scope-bound authenticated data, bounded records, compare-and-swap writes, atomic replacement, and failure poisoning;
- an authenticated installation descriptor binding merchant, store, location, register, and device identity;
- explicit `PENDING_ENROLLMENT` and `COMPLETED` provisioning states, plus separate pending-creation and descriptor-committed Keystore sentinels;
- refusal to recreate authority after a completed descriptor loses its authority record;
- first-manager enrollment, local staff selection/PIN sign-in, five-minute session expiry, and lock on Activity stop or destruction; every non-signed-in transition also stops the Counter pump, native node, and Tor lease;
- manager recovery that prepares and displays the replacement code before commit, requires exact acknowledgement, clears input arrays, and preserves the replacement on an uncertain commit;
- screenshot/recents capture protection while any authority, PIN, enrollment, or recovery screen is visible;
- exact capability checks around merchant-started sale creation, cash/card/Bitcoin tender initiation, Bitcoin cancellation, card reconciliation, refund requests, catalog changes, inventory changes and fulfillment;
- manager-only report reads and CSV exports, including a second export authorization after Android returns from the document picker; and
- manager-only watch-only wallet/pairing configuration. Picker results and exact reviewed report exports remain only in memory while the Activity is locked, then continue only after fresh manager sign-in; and
- authority-bound cash sessions whose new v4 records retain immutable actor ID, display name at event time, role, capability, policy generation, exact scope, and event time. Older v1-v3 staff strings remain explicitly legacy/unverified.

Automatic sample-catalog writes were removed. A cashier must not gain catalog-management power merely by being the first person to open an empty register.

Provider and Bitcoin-counter evidence callbacks remain private observer paths tied to an already persisted payment attempt. They do not create a new cashier command and therefore do not accept a UI-provided actor or authority session.

## Provisioning and recovery behavior

Creating the installation HMAC key is not, by itself, treated as proof that a descriptor previously existed. Initial creation can resume only while the dedicated pending-creation sentinel exists and the committed sentinel does not. Once descriptor commitment is recorded, deletion or corruption fails closed instead of minting a new scope.

The descriptor begins in `PENDING_ENROLLMENT`. It moves to `COMPLETED` only after the authority store can reread an enabled manager in the exact installation scope. If the manager write committed just before a crash, startup repairs the pending descriptor after verifying that durable profile. If a completed descriptor has no readable authority record, checkout remains unavailable and does not reopen enrollment.

Manager recovery changes only local manager authentication. It does not reset sales, cash, tenders, wallet keys, receiving destinations, Lightning channels, or Owner-app authority. The current recovery code and new PIN remain volatile only for the recovery attempt and are cleared on completion, cancellation, lock, or failure. Recovery-code and PIN fields disable Android saved-state persistence, autofill, selection, and long-click behavior.

## Command boundary

Cash-session writes receive an opaque session and reauthorize the exact cash capability inside the store mutation. Other migrated commands are serialized on the private authority worker and check their exact capability immediately before the operation. The bearer session never enters Activity state or Android saved state.

The current non-cash sale, catalog, tender, refund, inventory, and report stores do **not** yet persist the returned actor attribution. Those commands are authorized, but they must not be described as individually staff-attributed or audited. The separate authority audit-journal model is implemented and JVM-tested, but is not wired into Android; it also cannot make authority-state and audit-file writes transactionally atomic.

An operation that has already passed authorization and begun its durable write may finish after Activity backgrounding or timeout masks the UI. A queued operation that has not begun is dropped, and stale completion UI is discarded. This batch treats an already-started authorized command as an in-flight commit whose durable result must be reconciled on the next sign-in; it does not claim cancellable commits.

## Validation

The Android UI can enroll the first manager, select existing staff at sign-in, and recover the designated manager. It does **not** yet provide staff administration to create a cashier, rename staff, change roles/PINs, disable staff, or designate recovery staff. Those tested model/service operations are not product workflows.

There is also no adoption migration for an already-installed pre-authority checkout. Enrollment requires both Android first-install evidence and an initially empty set of business files. An upgraded prior installation fails closed until an explicit preservation/migration procedure exists; clearing data or reinstalling is not presented as a merchant-safe migration.

The coordinator ran:

- all 12 checkout JVM model groups with JDK 21, Kotlin 2.2.20, `-Werror`, and JVM target 1.8;
- eleven lightweight Android source-contract checks covering installation ordering, session expiry, Counter shutdown, resume masking, secret handling, opaque cash sessions, exact mutation capabilities, report-read gating, resumable export authorization, and manager-only pairing persistence;
- `git diff --check`; and
- the two-ABI experimental checkout APK build using the already feature-verified native libraries. Kotlin compilation, dexing, alignment, test signing, and package verification passed.

This compilation covers the base experimental checkout APK only; the optional Square Sandbox variant was not built. After publication, hosted CI run `44203655417` and the full end-to-end run `44203655306` both completed successfully for the implementation commit.

The resulting base test APK was `/tmp/openbmt-authority-apk-final7/openbmt-checkout-test.apk`, 62,682,927 bytes, SHA-256 `74d6b749d9dbd0b8e0a565595a1c3080727552e3475dd90613047cc5f4a390b3`.

Exact commands and claim boundaries are recorded in [the machine-readable evidence](../../tools/merchant-acceptance-investigation/evidence/android-authority-cash-foundation-2026-11-17.json).

## Capability effect

This batch materially advances the scope/staff/cash/report foundations, especially:

- `CAP-6A5843392F` — store identity and creation;
- `CAP-7506E394AB` — location identity;
- `CAP-D43AD7DE36` — device/register/location assignment;
- `CAP-7490EEDBCF` — staff PIN action attribution;
- `CAP-FF921BD61F` — staff permission configuration;
- `CAP-B554F6817A` — role permission scope;
- `CAP-A1F539309D` — high-risk staff activity log; and
- `CAP-CE8A35BA24` — checkout delegation without spend authority.

Canonical capability statuses remain unchanged in this batch. Local compilation and model tests do not demonstrate the complete merchant workflow, failure recovery on real Android hardware, or competitor parity.

## Remaining gates

1. Persist authenticated actor attribution with non-cash business commands, without creating cross-store partial-audit claims.
2. Add a tamper-evident or sealed Android business-journal strategy. The cash v4 attribution is authenticated at command time but its current business file is not cryptographically sealed.
3. Exercise enrollment, sign-in, expiry, background lock, recovery, corrupt storage, completed-record loss, and report export on an emulator and physical Android device.
4. Decide and test explicit in-flight command semantics across backgrounding, timeout, process death, and uncertain storage outcomes.
5. Connect authority administration to an atomic-enough audit design, or clearly keep it unavailable.
6. Continue the first foundation tranche with provider-neutral tender allocation, refund cases, merchant-controlled on-chain receiving/UTXO safety, Shop Lightning ownership/recovery, and the local event/webhook contract.
7. Add merchant-safe adoption for an existing pre-authority installation and an Android staff-administration workflow before calling multi-staff onboarding complete.

