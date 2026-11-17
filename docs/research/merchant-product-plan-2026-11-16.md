# OpenBMT merchant product plan

Planning update dated 16 November 2026. The wallet authority choices below are accepted for design and validation. They do not change current signing behavior or approve real-funds deployment. The [feature benchmark](merchant-feature-benchmark-2026-11-16.md) is a recommended backlog, with existing components separated from missing integrations.

The first merchant product is a standalone OpenBMT checkout for full Bitcoin acceptance: on-chain Bitcoin received to merchant-controlled keys, cash, and an optional third-party card processor. Existing POS integrations follow after the standalone sale, tender, receipt, refund, inventory, and closeout workflows are complete and validated. POS replacement is a bounded merchant choice, not a claim that OpenBMT already matches a general-purpose POS. Cashiers should not manage channels, outputs, or wallet migration during a sale. Merchants retain control of keys, recovery, destinations, and the authority granted to integrations. Shop Lightning remains part of the Bitcoin roadmap, subject to its distinct funding, availability, and recovery gates.

## Merchant adoption direction

Build and validate a standalone OpenBMT point-of-sale workflow first. Its canonical sale ledger handles exact baskets, Bitcoin, cash, and an optional card adapter without requiring a provider account for Bitcoin or cash. This means a merchant choosing it replaces their POS order-entry and closeout workflow; do not imply existing POS parity. After standalone workflows pass measured merchant gates, integrate with existing POS systems to reduce migration burden for merchants who prefer their current system.

The first card-adapter implementation should start from the existing tested Square Mobile Payments SDK Sandbox flow and its durable payment-attempt reference. That is a concrete engineering starting point, not evidence of production eligibility, supported-country coverage, approved devices, reader support, or a final processor choice. Validate each additional processor, target account and device separately. Cash requires no processor. Bitcoin acceptance remains independent of card authorization, account status, and provider availability.

The standalone delivery scope and finite release sequence are in [the standalone POS delivery plan](standalone-pos-delivery-plan-2026-11-16.md). The [POS-preserving investigation](pos-preserving-adoption-2026-11-16.md) remains useful as deferred research for the later integration phase; its former priority is superseded by this sequencing decision. Keep vendor claims classified by demonstrated workflow, documentation, manual fallback, or unavailable capability. USD display and accounting do not convert the Bitcoin received.

The accepted wallet authority choices below remain in force. Local hot start and independently created cold-wallet pairing, address verification, owner-approved transfer, late-payment tracking, and independent recovery are required parts of the standalone product path; they do not change current signing behavior or authorize unattended signing. Do not claim mainnet readiness or real-funds deployment from test-network evidence.

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

The [live proposed checkout](https://openbmt.avilalabs.org/?path=checkout) remains a browser illustration: its payment outcomes are simulated, the sample QR cannot pay an invoice, and a page refresh resets the session. It is not evidence of a native payment.

The separate [native Android checkout](../../apps/checkout-android/README.md#validation-on-16-november-2026) now saves a $10.00 sample cart (latte $5.50, bagel $4.50) as an immutable version-4 item snapshot. Force-stop before tender creation restored the same ledger and allowed the cashier to resume. The prior version-3 ledger remains preserved. Against Bitcoin Core Regtest, it requested 10,000 sats, observed a 4,000-sat confirmed partial payment, and after force-stop restored the identical ledger and same-address 6,000-sat remainder. After payment of the remainder, the paid sale and itemized receipt survived another force-stop unchanged. Version-2 and version-3 JVM ledger migration tests passed, including conservative handling of historical refund state.

The [Square Sandbox path](../../apps/checkout-android/square-sandbox/README.md#integrated-checkout-milestone-on-16-november-2026) completed a $10 virtual-card payment from that same cart using Mobile Payments SDK 2.7.0. Square's independent Payments API result matched the native payment ID and attempt reference. After force-stop and relaunch, the native ledger remained byte-identical and the itemized paid card receipt remained visible. Separately, an actual Sandbox API test created a $10 catalog item plus $1 tax, completed an order using an EXTERNAL/CRYPTO payment, and observed stock decrease from 10 to 9. These are distinct proofs: the SDK virtual-card flow and the API-created order/payment flow. Neither demonstrates Square's native saved-cart handoff, a cashier button in Square POS, or a single-entry merchant workflow.

The checkout Android ABIs, APK and SDK module builds, method-signature alignment, and shared native library hashes were verified. The project’s node-claim gate also passed its 205 node tests, four key tests, Clippy and TCP all-target checks; Bitcoin Core's 107 checks and compact-filter suite's 37 checks passed with their full suites. Those engineering checks do not establish merchant convenience, production deployment, or real-funds readiness.

These are integration proofs with explicit test fixtures. Merchant onboarding, physical readers, iOS, production SDK permissions and privacy, catalog/inventory synchronization, first-sale convenience and capacity provisioning, UTXO automation, phone-only backup, and Shop Lightning's funding and recovery gates remain open. No full provider catalog or inventory integration is claimed. The API-created Square Sandbox order test did not establish saved-cart handoff or native Square POS UI; the Android SDK Sandbox success does not establish complete Square POS integration.

## Implementation order

Follow the finite vertical slices and exit gates in [the standalone POS delivery plan](standalone-pos-delivery-plan-2026-11-16.md). The foundation work proceeds in parallel where it does not claim a completed merchant workflow: UTXO cost-threshold advisory (not automated spending), confirmed-receiving assessment (not a guaranteed route), backup fault fixture (not Box recovery), guarded Books reconciliation CSV, and a proposal for the local events contract. The durable cash ledger/UI work is in progress; cash receipts and change are not complete until integrated and validated. MVP accounting is USD-only, with no noncash drawer, shift accounting, or refund claim until those paths exist.

Preserve the accepted wallet authority choices and all existing custody and recovery boundaries. Bitcoin remains merchant-controlled; card processing stays an optional third-party service; cash does not require a processor. The existing tested Square MPS Sandbox flow is the card-adapter starting point, while production access, country/account/device eligibility, privacy review, and physical-reader support remain unproven. POS-preserving adapters are a later phase, not the current adoption gate.

The [BTCPay practical friction review](btcpay-practical-friction-2026-11-16.md#assessment-for-openbmt) remains a comparison source for order modifiers, staff permissions, refund accounting, readiness, and recovery. The [feature benchmark](merchant-feature-benchmark-2026-11-16.md) separates existing components from missing integrations.

No retail-only pilot establishes unattended, online, or multi-location readiness. Test-network successes, emulators, fixtures, and documentation are not substitutes for physical-device tests, applicable merchant shifts, independent review, and separate real-funds release gates. No mainnet proof is claimed here.

## Purity and privacy boundaries

Keep Bitcoin on-chain and Lightning as the baseline. Reject mandatory third-party custody, issuer IOUs presented as Bitcoin balances, a company approval requirement to recover funds, forced fiat or stablecoin settlement, and required publisher telemetry or sales-history upload. Optional card processing remains separate from Bitcoin custody and acceptance.

Display and accounting currencies do not imply conversion. Merchant-configured accounting exports and notifications are distinct from mandatory platform data collection. Any extra settlement network, intermediary-held credit, custodial wallet, third-party recovery dependency, or spending delegation needs a specific assessment; this plan does not accept those merely because a provider describes its product as noncustodial.

Preserve manual owner-chosen updates, independent wallet recovery, and open interfaces. Keep optional services replaceable and assess actual failure/exit behavior. A product may provide a useful merchant feature while using an unacceptable funds model; adapt the feature without adopting that model.
