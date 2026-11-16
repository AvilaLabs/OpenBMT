# OpenBMT merchant product plan

Planning update dated 16 November 2026. The wallet authority choices below are accepted for design and validation. They do not change current signing behavior or approve real-funds deployment. The [feature benchmark](merchant-feature-benchmark-2026-11-16.md) is a recommended backlog, with existing components separated from missing integrations.

OpenBMT should provide one merchant checkout across ordinary on-chain payments, Lightning, and optional business-system connections. Cashiers should not manage channels, outputs, or wallet migration during a sale. Merchants retain control of keys, recovery, destinations, and the authority granted to integrations. Shop Lightning remains part of that goal.

## Merchant adoption direction

Treat a merchant's willingness to make OpenBMT the daily checkout as conditional on a better end-to-end experience and low switching burden. A merchant may keep Square or another provider for supported card processing, catalog, hardware, or operating functions; connections preserve only capabilities that are actually supported and tested, not blanket compatibility. Keep OpenBMT integrations inside Clover and other existing POS software as optional routes wherever supported. No provider account is required to accept Bitcoin. In the OpenBMT checkout mode, OpenBMT supplies its own interface and sale record; connecting a provider does not host its full POS application. Keep the app-first, serverless default and existing custody, recovery, and key-authority boundaries unchanged.

Adoption evidence must cover setup and import effort; one sale entry; checkout speed and steps against the merchant's current workflow; clearly disclosed support for essential features; combined records, refunds, and reconciliation; and staff recovery from routine errors or interruptions without interpreting logs or accidentally repeating a charge. Test real merchant tasks across relevant workflows, not simply whether they can use “one app.”

## Accepted wallet authority choices

### Start with a local wallet and support an independently created cold wallet

Merchants may start with a local hot wallet or connect a supported wallet they already control. Explain the difference between the merchant's local wallet software and OpenBMT company custody: company servers receive no spending keys and are not required to approve withdrawals. Local software with an accessible ordinary private key nevertheless has meaningful spending authority and can be abused if compromised.

When the merchant adds cold storage, its device or offline environment creates fresh recovery material. OpenBMT imports only the supported public wallet description. The merchant verifies a receive address on the independent wallet's own display before moving funds. Moving a previously exposed hot seed onto a hardware device does not establish that the seed was always offline.

The owner approves the initial transfer from the starter wallet. Keep tracking addresses already issued and preserve a recovery route for late payments. Key retirement must account for pending transactions, confirmations, and migration state; it cannot assume old addresses will never receive again. Existing wallet import and migration components are starting points, with their actual script and hardware compatibility tested explicitly.

### Offer two ongoing receiving modes

| Mode | Authority and everyday behavior | Owner involvement |
| --- | --- | --- |
| Direct to cold storage | Checkout derives and watches public receive addresses. It has no cold spending key. | Owner signs prepared refunds, spending, or consolidation batches when required. |
| Separate on-chain working wallet | A distinct merchant-controlled online key receives proceeds and can consolidate them into verified savings destinations. | Owner explicitly enables the mode and authorizes its signing policy. Unattended operation needs its own validated authority model. |

The general receiving working wallet is a new mode. It must not silently repurpose the refund float, remove the current starter wallet's per-signature owner authentication, or import the cold recovery words. The existing refund float remains purpose-limited.

Before the merchant connects cold storage, combining receipts within the hot wallet can improve output economics but does not put those funds offline. After pairing, transfers into savings require the working key's signature; receiving them requires no cold signature. Spending or combining savings later still requires the cold wallet's authority, except for a specifically authorized limited construction.

### Separate receiving capability from spending capability

The cashier interface and ordinary integrations receive public payment capabilities, not a generic wallet-signing interface. Keep invoice creation, status reading, reporting, refund requests, and actual spending as separate permissions. Owner authorization is required to change savings destinations, expand authority, or enable a new signing mode.

Destination checks and fee budgets in a process holding an ordinary private key are software protections. They are not cryptographic restrictions against compromise of that process. Independent policy enforcement, process isolation, platform behavior, and key provisioning need tests before stronger claims. A merchant-owned policy signer can be evaluated without requiring extra hardware from every merchant.

Online-balance limits are operating targets, not absolute loss guarantees. Late and unsolicited payments can exceed a target, and a compromised key can steal ongoing receipts it controls. Remove contrary absolute-limit wording from product copy before a pilot. Cold savings remain outside the working key's spend authority, while destination verification and checkout integrity protect the path of new receipts.

## One controller for receiving capacity and savings

Plan on-chain fees, output sizes, Lightning capacity, pending settlements, online exposure, and signing availability together. Preserve gross sale amounts, actual paid fees, estimated future spending costs, and unsettled funds as distinct records. A scheduled withdrawal does not immediately free capacity or remove exposure.

The completed [transaction and economic experiments](merchant-money-system-2026-11-16.md) support further implementation of batching and bounded overlapping settlements. They do not establish a production controller, service independence, hardware isolation, or sustained merchant economics.

Shop Lightning should automate operation of a merchant-controlled receiver, with advanced controls available. Prepare verified capacity before checkout where feasible. A separate merchant Bitcoin deposit should be avoided as an onboarding target, while disclosing who supplies capital and pays fees. No fixed onboarding price, free liquidity, or particular liquidity service is accepted by this plan. Funding verification, backup freshness, independent exit, device availability, and the current Box requirement remain unresolved gates.

## Local interfaces and optional webhooks

OpenBMT remains app-first and serverless in its default merchant deployment: no required merchant web server, public device listener, or OpenBMT company backend. The foundation is a versioned local invoice/status interface, durable payment events, and reconciliation. Existing SDK/FFI or suitable local interfaces should expose that contract. A public REST service is a separate optional deployment, not the definition of the API.

Webhooks are optional outbound notifications to a merchant-selected store or automation endpoint. The sender can be the app; it needs no public listener. The destination needs a reachable receiver. Native checkout must work without webhooks. A callback or customer redirect is not sufficient independent payment or fulfillment evidence. Scope any external credentials per merchant, register, and capability; invoice-only access does not confer spending authority.

Default event payloads contain opaque resource IDs, event type, version, and necessary state. Keep seeds, private keys, public wallet descriptions, customer identity, addresses, and transaction identifiers out of default outbound payloads. The merchant must explicitly configure the destination and permitted metadata. Merchant-operated receivers and local/pull interfaces are the baseline; no OpenBMT cloud relay or account is required by design.

This adds a network capability absent from the existing app-contact policy. Before implementation, update the contact inventory and privacy disclosure with the precise endpoint, data, permissions, and availability requirements. This is not blanket approval to send wallet information to arbitrary services. Third-party destinations must pass the existing privacy standard; extra address or transaction disclosure remains a separate decision.

Local durable events and a query that repairs missed events belong in the core contract. An enabled webhook bridge adds idempotent delivery, retries, delivery history, and manual replay. A sleeping or offline app cannot promise prompt webhook delivery or even observe every new payment immediately; it must catch up and replay after reconnecting. A remote store cannot query a sleeping or unreachable app merely because a query API exists. Remote reconciliation needs a defined outbound synchronization or reachable merchant-operated arrangement. A merchant needing continuous ecommerce automation must select and validate an availability arrangement separately. Do not make an always-on server a default requirement for ordinary checkout, and do not treat this interface decision as having removed the current Shop Lightning backup/Box gate.

## Current checkout proof

The [live proposed checkout](https://openbmt.avilalabs.org/?path=checkout) shows a single cart, tender selection, interruptions, receipt history, and the next order. Its external payment outcomes are explicitly simulated.

The separate [native Android checkout](../../apps/checkout-android/README.md#validation-on-16-november-2026) has observed actual Regtest receipts through the counter FFI, including a partial payment, restart, completion, and restored receipt history. The [optional Square Sandbox harness](../../apps/checkout-android/square-sandbox/README.md) uses the same amount-entry screen and the real Mobile Payments SDK. These experiments leave current wallet signing authority unchanged. Their validation notes distinguish the browser simulation, the native watcher, and Square's test backend.

This is an integration foundation. Merchant onboarding, catalog and operating-system compatibility, physical readers, production SDK permissions and privacy, wallet automation, and Shop Lightning's funding and backup gates still require their own validation. A successful Android Sandbox payment does not establish iPad compatibility or replace Square's complete POS interface.

## Implementation order

1. Specify authority boundaries, wallet modes, canonical sale/payment states, and the event/API contract. Retain current behavior until each replacement is tested.
2. Connect the existing receiving and record primitives to a durable sale ledger. Exercise partial, late, excess, expired, conflicting, duplicate, and reorganized payments, refunds, and fulfillment acknowledgments.
3. Build the optional working-wallet path and verified cold-wallet transition. Test owner authorization, destination changes, delayed settlement, fee budgets, old-address receipts, restart, and independent recovery. Keep direct-to-cold receiving usable.
4. Expose the versioned local query/event interface and connect it to native checkout. Add an optional merchant-configured outbound webhook bridge where an existing store or automation endpoint needs it. Complete one real commerce integration before multiplying adapters. The WooCommerce prototype and native checkout candidate are distinct validation targets; a required BTCPay-style server is not the baseline.
5. Complete merchant-facing receipts, accounting exports, staff controls, and owner exception handling using existing components. Add catalog, inventory, peripheral, and multi-location capabilities in measured stages.
6. Evaluate reusable offers, Payjoin, NWC, subscriptions, and advanced automation against the feature benchmark and purity rules. Do not adopt the funding or settlement backend of a competitor merely to obtain one interface feature.

The [BTCPay practical friction review](btcpay-practical-friction-2026-11-16.md#assessment-for-openbmt) adds concrete acceptance criteria for order modifiers, staff permissions, refund accounting, readiness, and recovery; use those alongside the feature backlog.

The initial product scope must state which workflows are supported. A retail-only pilot does not establish unattended, online, or multi-location readiness. Existing test-network successes, emulators, fixtures, and documentation are not substitutes for physical-device tests and the project's independent review and real-funds gates.

## Purity and privacy boundaries

Keep Bitcoin on-chain and Lightning as the baseline. Reject mandatory third-party custody, issuer IOUs presented as Bitcoin balances, a company approval requirement to recover funds, forced fiat or stablecoin settlement, and required publisher telemetry or sales-history upload. Optional card processing remains separate from Bitcoin custody and acceptance.

Display and accounting currencies do not imply conversion. Merchant-configured accounting exports and notifications are distinct from mandatory platform data collection. Any extra settlement network, intermediary-held credit, custodial wallet, third-party recovery dependency, or spending delegation needs a specific assessment; this plan does not accept those merely because a provider describes its product as noncustodial.

Preserve manual owner-chosen updates, independent wallet recovery, and open interfaces. Keep optional services replaceable and assess actual failure/exit behavior. A product may provide a useful merchant feature while using an unacceptable funds model; adapt the feature without adopting that model.
