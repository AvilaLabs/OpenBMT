# OpenBMT Checkout for Android (experimental)

This is a separate experimental Android checkout shell built from plain
`android.widget` views. It is distinct from the Counter app. Checkout payments
remain **Regtest only** and use the existing counter FFI to create a BIP21
payment request and observe the counter's peer and chain checks. The checkout
does not sign or hold wallet spending keys. The durable checkout ledger is the
source for sale and receipt history. Square remains an optional Sandbox-only
path supplied by `square-sandbox`; the base build has no Square SDK and does
not claim production card acceptance.

**TEST ONLY. No mainnet, real funds, production payments or goods handover.**
The USD flow uses the fixed developer rate of 10,000,000 minor units per BTC
($100,000/BTC), identified as `dev-regtest-usd-100000-v1`; it is not a live
price. The app accepts only a Regtest pairing record with USD or SAT pricing.
The Regtest pairing is watch-only for checkout and cannot sign spends.

## Current unpublished native batch

The current Android batch adds a durable local catalog with product revisions,
create/edit/archive, a bounded one-variant item editor, optional SKU/barcode
lookup by manual entry, and frozen basket selections. It adds a separate local
stock store with tracked or manual mode, reasoned count adjustments, and an
explicit sale-linked handoff that records stock changes once. It does not
connect to a physical scanner or prove that goods were physically handed over.

USD catalog checkout now freezes an integer-minor-unit pricing snapshot before
tender: item and modifier amounts, quantities, fixed order discount, one flat
tax component, optional fixed tip, and exact total. Tax arithmetic supports
inclusive extraction or exclusive addition, but the current Android form
exposes a merchant-entered exclusive flat rate. A final zero-total sale is not
supported. The snapshot is stored in checkout ledger format version 7. Ledger
versions 2–6 remain readable; they retain historical totals without an invented
pricing breakdown.

Cash tender remains an explicit cashier acknowledgment: the cashier records
cash received, amount applied, change, stable payment ID, and timestamp in the
canonical sale ledger. A separate local cash-session store records opening
float, reasoned cash in/out movements, links to canonical cash payments,
operator-entered count, close and discrepancy. A payment link records an
existing payment; it does not collect cash again. The sale ledger and session
store are separate, so an interrupted link must be reconciled before retrying.
Neither the cash tender nor session count detects or verifies physical cash or
a drawer.

A local report provides a device-timezone sale-created date range, tender-aware
observed/applied totals, refund totals, pricing breakdown where known, and
reviewed CSV copy/save. It is not a full shift close, cashbook or accounting
report. Receipt projections include saved pricing and tender details. No
printer or email delivery is qualified.

Catalog, inventory and cash-session storage are independent local modules.
An optional module startup fault does not erase its records or disable the
separate payment ledger/manual checkout; the affected module is unavailable
until its saved state is reopened and inspected. An uncertain write requires
reopen and inspection before repeating the action.

The model layer now also contains a local authority foundation: typed
merchant/store/location/register/device scope; stable staff IDs; fixed cashier
and manager grants; salted PIN verification; durable throttling; process-local
expiring sessions; compare-and-swap persistence; designated-manager recovery;
and event-time actor attribution. The authority record format is strict and
versioned, and prepared recovery codes remain known to the caller across an
uncertain commit. JVM contract tests cover scope, role, expiry, revocation,
lockout, recovery, stale concurrent instances, duplicate PIN refusal,
corruption and uncertain writes.

The Android test host now connects that model to a Keystore-sealed authority
store, authenticated installation descriptor, first-manager enrollment,
staff PIN sign-in, manager recovery, five-minute expiry and lifecycle lock.
Current merchant-started sale, tender, refund, catalog, inventory and report
commands pass exact capability checks. Cash-session v4 records persist actor
and scope attribution; v1-v3 labels remain explicitly legacy/unverified.
Non-cash business records do not yet persist the returned actor attribution,
and the model audit journal is not Android-wired or transactionally atomic
with authority changes. No staff-permission capability is claimed complete
from this integration alone.

The coordinator's current implementation and validation record is
[native POS foundations (17 November 2026)](../../docs/research/native-pos-foundations-2026-11-17.md).
The later [Android authority and cash-attribution record](../../docs/research/android-authority-cash-foundation-2026-11-17.md)
lists the exact local proof and remaining boundaries. No emulator or physical
device authority workflow has yet been claimed. The Shop Lightning work in
this batch is an owner-app readiness report only. **The native checkout still
has no integrated Lightning tender path.**

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
libraries are the repository's `openbmt-counter-ffi` and QR JNI plus the Tor
library from the SHA-256-pinned `tor-android` AAR described in
`docs/DEPENDENCIES.md`. Square is not part of this build. Building the APK does
not establish that it has been run on a device or that the end-to-end Regtest
payment path works.

## Cash and tender limits

Cash is offered for eligible USD balances when earlier tender outcomes are
resolved and the sale does not require owner review. SAT-priced sales do not
offer cash. Cash refunds, drawer automation, denomination tracking, and
physical cash verification are not provided. The Square path is Sandbox-only;
a virtual reader or Sandbox approval does not establish production processing
or physical-hardware support. Unknown provider results remain pending until
reconciled.

## Sales history and local journal

Sales history searches saved receipt IDs and immutable basket item names
without remote queries. Tender filters cover Bitcoin, card and cash; status
filters cover paid, unpaid and needs review. Search filters are stored in the
Activity saved-state bundle. The local append-only checkout event journal is
initialized before checkout UI is served. Journal data remains in app-private
storage and is not exported by the cashier UI. There are no webhooks, remote
delivery, or complete team workflows in this prototype.

## Limits

This remains an experimental checkout integration, not a merchant-ready
release or security review. Regtest and the fixed developer quote are mandatory.
The on-screen Bitcoin status comes from the counter node's watcher and durable
ledger. Bitcoin/card payment states require recorded evidence; cash is an
explicit cashier acknowledgment. Cancellation records a request and retains
the attempt so late payment evidence can still be reconciled. A Square Sandbox
approval is not completion, and uncertain card results remain pending until
reconciled.

Legacy staff/operator labels are not authentication. New cash-session writes
require a signed-in actor, and current Android merchant commands enforce local
capabilities, but non-cash actor persistence, Android admin-audit wiring,
tamper-evident business journals, and device workflow evidence remain open.
Physical devices, production card provider approval, merchant onboarding,
independent Lightning recovery, actual merchant shifts, and mainnet/real-funds
gates remain open. Partial native UI and model tests do not establish POS
parity or merchant readiness. The browser checkout is a separate explicitly
labeled simulation and does not use these native payment observations.

## Historical validation milestone: 16 November 2026

The following is the prior November 16 milestone, not validation of the current
unpublished native batch. At that milestone, the coordinator built both Android
ABIs, the APK and Square SDK module, and verified matching native method
signatures. The checkout saved a native sample basket as an immutable sale
snapshot in ledger version 4; force-stop recovery restored the sale, and the
preceding version-3 ledger remained preserved. JVM migration tests passed for
version 2 and 3 records, including conservative handling of historical refund
state.

On the API 35 Android emulator, that milestone's app created a 10,000-sat
Bitcoin request on an isolated Bitcoin Core 32.1 Regtest node. A 4,000-sat
confirmed payment was observed as partial. After force-stop and restart, the
same address showed the 6,000-sat remainder. Paying the remainder completed
the sale, and the itemized receipt restored unchanged after another force-stop.
These were actual Core transactions and native FFI observations, not simulated
or manually marked payment states.

The Square SDK path used Mobile Payments SDK 2.7.0 in the same cart. A virtual
card payment for $10.00 completed; Square's independent Payments API result
matched the native payment ID/reference. This used Square Sandbox and a virtual
reader, not production processing or physical hardware. See the separate
[Square validation record](square-sandbox/README.md#validation-on-16-november-2026).

The historical standalone foundations validation also exercised the ledger,
history, receipt and event JVM harness, and the prior cash flow on an Android
emulator: a $5 cash entry against a $10 sample basket was refused, $20 tender
showed $10 change, and the saved cash ledger/receipt survived force-stop and
relaunch. History filters, no-match search, reset and receipt copy were
exercised. That evidence used synthetic cash acknowledgment and no additional
card or Bitcoin payment. See the
[captured November 16 evidence](../../tools/merchant-acceptance-investigation/evidence/standalone-foundations-2026-11-16.json).

Those historical observations do not validate the current catalog, pricing
snapshot, inventory, cash-session, reporting or Shop Lightning changes. Current
coordinator validation remains in progress; see the linked native foundations
record for exact scope and outstanding checks.
