# Standalone POS delivery plan

Planning update dated 16 November 2026. This plan sets a finite first merchant product: standalone OpenBMT checkout/POS, followed by integrations into existing POS systems. It is an implementation plan, not a claim of release readiness or real-funds approval. No mainnet validation is claimed.

## Product boundary

OpenBMT owns the canonical basket, immutable sale snapshot, tender attempts, receipts, refunds, inventory actions, and day-close records. A merchant enters a sale once and chooses among:

- **Bitcoin:** full on-chain acceptance to merchant-controlled keys. Lightning remains in scope as a separate receiving mode only after its capacity, funding, availability, and recovery gates pass.
- **Cash:** record cash received and change due. The MVP accounting currency is USD. Cash requires no processor.
- **Card:** an optional third-party processor adapter, isolated from Bitcoin authority. The existing tested Square Mobile Payments SDK Sandbox flow is the implementation starting point. It proves a virtual-card Sandbox flow only; provider production eligibility, supported countries/accounts/devices, readers, and privacy approval remain unproven. Additional card adapters require separate validation; do not claim provider/country eligibility without fresh evidence.

The first release is a focused standalone retail product, not universal POS parity. Existing POS integrations follow after the standalone sale and closeout flow works through a measured merchant shift. Do not promise migration convenience or vendor feature parity. Keep the earlier POS-preserving investigation as deferred research. Preserve all accepted wallet authority choices: local hot start or independently created cold wallet; public-only pairing; verification on the wallet's own display; owner-approved transfer; tracking of issued addresses and late payments; independent recovery; and explicit authority boundaries. Do not change current signing behavior or imply a hard maximum-loss guarantee.

## Foundation work and evidence status

Scoped checks passed: 1,181 Rust tests, strict Clippy, the complete checkout JVM harness, both Android ABI builds, the Square Sandbox module build, APK signatures/alignment, and Android emulator cash/restart/history checks. See [validation evidence](../../tools/merchant-acceptance-investigation/evidence/standalone-foundations-2026-11-16.json). These checks do not establish a complete POS or production readiness.

These narrow slices are implemented in the current tree, with scoped checks passed; they do not by themselves complete end-user workflows:

| Work | Current boundary | Merchant value when integrated |
| --- | --- | --- |
| UTXO cost-threshold advisory | Implemented as the SP-88 advisory; scoped checks passed. Advisory only, with no autonomous coin selection, consolidation, or spending authority. | Explain future spend costs and avoid uneconomic outputs. |
| Confirmed-receiving assessment | Implemented as point-in-time `Node::receiving_assessment` for confirmed funding and strict/lazy-queue status; scoped checks passed. Not a route or capacity guarantee. | Show current readiness signals and limits. |
| Backup fault fixture | Implemented fixture; scoped checks passed. Does not establish Box recovery or device-loss recovery. | Exercise interrupted/stale backup handling. |
| Guarded Books reconciliation CSV | Implemented with exact satoshi amounts and timestamps; scoped checks passed. Requires the payment-disclosure warning acknowledgment; the narrower accountant export is unchanged. Not a complete accounting system. | Reconcile sale records with on-chain facts. |
| Local events contract | Narrow v6 checkout journal implemented: event initialization plus snapshot/replay bounded to 1,000 events and atomic state updates; scoped checks passed. The merchant API/contract, owner Requests API, and webhook bridge remain proposed/backlog. | Enable local integrations without an OpenBMT cloud backend. |
| Durable cash ledger/UI | Native checkout v6 USD cash journal and amount/change flow implemented; passed the checkout JVM harness and Android cash/restart checks. No noncash drawer, shift accounting, or cash refund support. | Record tender received and change due. |

### Current checkout additions (implemented; scoped checks passed)

The native ledger also has searchable payment history (ID/item search, tender/status filters, and API date-range filters); a privacy-limited receipt copy/test projection (no actual printing or email); a two-item sample cart (not configurable catalog CRUD or tax/modifier engine); and `openbmt-core::capabilities` for verified authorization/revocation/expiry metadata scoped to exact store/register (no authenticated transport or staff screens). Scoped checks passed. These additions do not complete catalog, staff controls, physical receipt output, or the merchant workflow.

The later [local authority foundation](local-authority-foundation-2026-11-17.md) adds a JVM-tested checkout model for typed merchant/device scope, staff PINs, fixed grants, lockout, expiring sessions, CAS persistence and prepared recovery. It is not wired to Android screens or business command paths, so it does not complete staff controls or change canonical capability status.

## Delivery slices

Each slice has a reviewable artifact and exit evidence. Keep scope vertical and finite; defer additional providers, catalogs, and deployment types until the relevant gate passes.

### 1. Canonical sale and authority contract — remaining backlog

Extend the current native ledger into a validated canonical contract: exact-money basket lines, configurable modifiers, tax, discounts and tips; freeze an immutable sale snapshot before tender; record tender attempts and external references; and define sale, payment, unknown, partial, late, excess, split, canceled, refunded, and fulfillment states. Separate receiving evidence from permission to fulfill. Assign distinct capabilities for checkout, payment status, reporting, refund requests, spending, inventory, and owner settings.

**Exit:** deterministic contract and migration behavior; duplicate/out-of-order observations cannot fulfill twice; unresolved provider responses cannot trigger a second charge; restart reconstructs the same sale and attempt state. No card provider is assumed to be authoritative for the Bitcoin sale.

### 2. On-chain Bitcoin sale and recovery

Complete a sale from basket through quote/request, partial and complete payment observations, honest confirmations, receipt, restart, late/expired payment handling, and reconciliation. Keep the merchant's own receiving keys and recovery path. Surface fee/UTXO advisories and readiness without claiming automation. Cold-wallet pairing must use a supported public description, independently verify a receive address, and preserve owner-approved transfer and late-address tracking.

**Exit:** regtest interruption/restart and reconciliation fixtures cover partial, late, excess, duplicate, conflicting and reorganized observations; restore and migration exercises demonstrate recovery from documented artifacts. The current scripts and test networks do not establish mainnet readiness or real-funds approval.

### 3. Cash tender — implemented; scoped checks passed; operations remain backlog

The current native ledger records amount due, amount tendered, and change due/returned in USD, with a local event journal and restart snapshot/replay path. Scoped checks passed. Complete receipt validation and make retries safe. MVP scope is cash only. Exclude noncash drawer tenders, drawer-opening automation, shift accounting, and cash refunds until their workflows and reconciliation exist.

**Exit:** root-integrated cash ledger/UI survives restart and interruption; exact-money and change scenarios reconcile to the same canonical sale; receipt wording distinguishes cash received from change due. Until then, cash work remains in progress.

### 4. Optional card adapter

Use the already tested Square Mobile Payments SDK Sandbox implementation as the starting adapter. Persist attempt identity before invocation; distinguish declined, completed, canceled, and unknown outcomes; reconcile an unknown response before retry or changing tender. Keep card secrets and authorization within provider-approved boundaries, and ensure provider unavailability does not disable Bitcoin or cash. Do not infer production eligibility, country coverage, supported account/device status, physical-reader support, or privacy acceptability from Sandbox.

**Exit:** supported target account/country/device and physical reader are independently verified for the intended release; lost responses, duplicate submissions, cancellation, refund, revocation, force-stop and relaunch reconcile without duplicate charge; privacy and key-isolation review passes. A Sandbox-only build is not a production card release.

### 5. Inventory, refunds, and receipt correctness

Apply inventory reservation and stock adjustment at most once using durable idempotency keys. Record refunds against the original sale and tender, cumulative amount, approval, status, and evidence; distinguish refund requested/owed from paid. Preserve the existing refund rules and owner authorization rather than rebuilding policy in checkout. Ensure receipts show the basket, tax/discount/tip, tender split, partial/late/excess status, and refund history accurately.

**Exit:** crash/retry and concurrent-change cases cannot double-decrement stock or double-pay a refund; tender-level reports reconcile Bitcoin, cash, and card outcomes. Third-party card refunds remain subject to the provider and must be independently reconciled.

### 6. Staff, owner, cash control, and closeout — remaining backlog

Provide role-based staff and owner permissions, safe owner exception handling, opening cash float, cash-in/out records, count, close, and discrepancy review. Keep owner-only actions for changing wallet authority, destination, signing mode, and other sensitive settings. Provide useful recovery steps for normal interruptions without exposing secrets or requiring log interpretation.

**Exit:** permission matrix tests and observed staff tasks show routine cashier actions cannot change authority; opening/closing counts and discrepancy trail survive restart and reconcile with tender records. Unsupported roles and overrides fail closed with actionable messages.

### 7. Peripheral capability and reporting

Publish a tested capability matrix for each supported device/adapter: receipt printer, cash drawer, barcode scanner, display, connectivity, and card reader. Do not imply that an SDK or kiosk's card support controls attached peripherals. Deliver itemized receipts and day reports for sales, tender totals, refunds, tax, discounts/tips, inventory actions, cash movements, and discrepancies. USD reporting is the MVP; any exchange conversion or additional accounting currency requires an explicit separate decision.

**Exit:** physical-device tests validate each claimed capability; day-close totals reconcile to immutable sale/tender/refund records. Unavailable peripherals have honest manual fallbacks.

## Cross-cutting release gates

- **Offline and failure behavior:** demonstrate honest Bitcoin observation states; recover after app kill, reboot, power loss, network loss, card timeout, and storage interruption. A missing response is unknown until reconciled. Cash and card availability must not change Bitcoin payment truth.
- **Local recovery and wallet transition:** verify cold-wallet public pairing and receive-address display, owner-authorized migration, outstanding transactions, old addresses, late receipts, independent recovery, and documented fallback. Box/backup fault fixtures alone do not pass this gate.
- **Fees and receiving readiness:** explain on-chain fee/output economics and UTXO advisories. Lightning readiness must account for confirmed capacity, funding, unsettled payments, backup freshness, availability, and independent exit. No liquidity or route guarantee is claimed.
- **Purity and privacy:** no mandatory custody, conversion, company approval for recovery, publisher telemetry, or sales-history upload. Card processor access is optional and scoped; it receives no Bitcoin spending authority. Webhooks remain opt-in, with the accepted privacy contract. No public listener or hosted publisher ledger is required for standalone cash/on-chain checkout.
- **Merchant validation:** complete a real merchant shift with the target physical devices and staff. Measure setup time, checkout steps and latency, exception recovery, receipt correctness, inventory/refund accuracy, day close, and support burden. Record device, software, provider, country/account, workflow, and date for each claim.
- **Release scope:** name the exact supported workflow and devices. A retail shift does not establish online, unattended, multi-location, iOS, or general POS compatibility. Separate security review and real-funds gates remain mandatory.

## Later: integrations into existing POS systems

After standalone exit gates pass, select POS adapters based on merchant evidence and workflow fit. Preserve the POS order where desired; validate exact amount, tax/modifiers, inventory, kitchen routing, staff attribution, tips, refunds, and day-close once per adapter. The old POS-preserving investigation is a research source, not proof or current priority. A public API, documentation, or API-created sandbox order does not prove an end-to-end cashier workflow.
