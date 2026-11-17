# Local authority foundation implementation record

**Date:** 17 November 2026

**Product boundary:** experimental standalone Android checkout model

**Status:** model implemented and JVM-tested; Android/product enforcement not implemented

## Outcome

The checkout model now has a durable, fail-closed local staff-authority foundation. It establishes reusable identity and authorization primitives without claiming that current Android checkout actions are protected.

Implemented:

- exact merchant, store, location, register and device scope;
- stable actor IDs independent of mutable display names;
- fixed cashier/manager capability allowlists with unknown actions denied;
- per-actor salted PBKDF2-HMAC-SHA256 PIN verifiers, constant-time comparisons and duplicate enabled-PIN refusal;
- durable attempt counters and monotonic, boot-aware lockout holds;
- opaque identity-tracked sessions with expiry, exact-scope checks and policy-generation invalidation;
- last-active-manager and designated-recovery-manager invariants;
- prepare-before-commit bootstrap/recovery codes so an uncertain committed rotation does not create an unknown credential;
- compare-and-swap authority storage that rejects stale instances rather than overwriting lockout, revocation or policy changes;
- strict versioned decoding, corruption refusal, atomic desktop/JVM storage and poisoning after conflicts or uncertain writes;
- immutable actor-attribution output for audit metadata. Attribution is not accepted as authorization proof.

The NIO implementation is for JVM/desktop validation. Its contract explicitly requires Android production wiring to use Keystore-backed authenticated sealing.

## Validation

The coordinator ran the complete checkout JVM harness with JDK 21 and Kotlin 2.2.20 after adding `CheckoutAuthorityTest` to the standard runner. The final run completed successfully.

The coordinator also built the experimental checkout APK from a clean temporary Cargo/output target for both `arm64-v8a` and `x86_64`. Kotlin compiled with `-Werror`; the APK was dexed, aligned, test-signed and independently verified by `apksigner`. This proves platform compilation/package integrity only, not an Android authority workflow. The exact commands, artifact hash and claim boundary are recorded in [the validation evidence](../../tools/merchant-acceptance-investigation/evidence/local-authority-foundation-2026-11-17.json).

Authority-specific coverage includes:

- correct/wrong PIN and durable lockout across reopen;
- exact-scope and role-matrix enforcement;
- session expiry and invalidation after role, PIN, disable and policy changes;
- stable actor identity across rename;
- manager creation/disable invariants;
- rotating designated-manager recovery without touching a separate business-data sentinel;
- corrupt and unsupported record refusal;
- pre-commit and uncertain post-commit failures;
- retained prepared bootstrap/recovery codes after commit-then-throw;
- stale multi-instance CAS conflicts for authentication and policy changes;
- duplicate PIN, change-PIN and recovery-PIN impersonation refusal;
- redacted session/draft/result/error rendering; and
- non-secret enabled-staff login-directory projection.

The dependency graph assembler also passed its exact validation: 502 canonical rows from 12 manifest-bound lanes, canonical status totals, 711 resolved non-cyclic edges, repository-relative evidence paths, and a 40-capability/9-primitive first focus set.

## Capability effect

This work is infrastructure toward, but does not close:

- `CAP-6A5843392F` — store identity and creation;
- `CAP-7506E394AB` — location identity;
- `CAP-D43AD7DE36` — device/register/location assignment;
- `CAP-7490EEDBCF` — staff PIN action attribution;
- `CAP-FF921BD61F` — staff permission configuration;
- `CAP-B554F6817A` — role permission scope;
- `CAP-A1F539309D` — high-risk staff activity log; and
- `CAP-CE8A35BA24` — checkout delegation without spend authority.

Their canonical statuses remain unchanged. Model code and tests alone do not demonstrate the merchant workflow or Android enforcement.

## Required next slice

1. Add a Keystore-AES-GCM, compare-and-swap Android authority store with scope-bound authenticated data.
2. Add explicit first-manager enrollment, recovery-code acknowledgement, staff selection/sign-in and lifecycle lock screens. Run PBKDF2 off the main thread.
3. Make each business command accept an opaque authority session and call `authorize` inside its trusted mutation boundary. Treat returned attribution only as persisted event metadata.
4. Start with the cash journal: bind it to one configured drawer and exact scope, remove free-text write authority, retain v1–v3 labels as legacy-only data, and write authenticated v4 attribution.
5. Persist actor-attributed authority administration events. Do not conflate the local recovery code with Owner-app or Bitcoin wallet authority.
6. Keep any unmigrated mutation path unavailable rather than relying on a hidden button.

Until those gates pass, existing Android sale, catalog, inventory, cash, refund and report paths remain outside this authority boundary.
