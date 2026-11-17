# Bitcoin opportunities and proposed decisions

Design hypotheses from the capability research. These are review proposals and validation targets, not implemented features or verified outcomes. Exact benchmark conditions remain in the [register](capability-register.json).

## BTC-OPP-001: Run a complete sale on merchant-owned Bitcoin and other tenders

**Merchant job:** Sell an itemized basket, take Bitcoin/cash/optional card, issue a correct receipt, handle exceptions, and reconcile a shift; later support additional retail, restaurant, and online workflows when validated.

**What Bitcoin could improve:** Adds a payment method whose on-chain receipt can be verified independently of the card processor; merchant can retain the sale ledger and key authority locally.

**What it does not solve:** Bitcoin does not supply a basket, tax/modifier engine, inventory, staff workflow, physical receipts, or day-close accounting. The first measured retail workflow is proposed validation sequencing; restaurant and online needs remain backlog rather than rejected product jobs.

**Candidate:** Local canonical sale journal with immutable basket and independent tender attempts; integrate on-chain and cash first, with optional replaceable card adapter and separately gated Shop Lightning receiving. Extend the same sale contract to validated restaurant and online workflows.

**Costs and tradeoffs:** Local-first operation reduces backend dependence but makes durable storage, backup, reconciliation and recovery the product’s responsibility. A completed retail validation scenario does not establish restaurant or online coverage.

**Proof needed:** Complete observed retail shifts on named devices; measure setup, checkout time, partial/late/excess recovery, receipt correctness, inventory/refund accuracy, day close, and support burden.

Benchmark references: SHC-C001, SHC-C002, SHC-C011, TOS-C006, CARD-C008, BTC-C006, BTM-C005, SQC-C015.

## BTC-OPP-002: Accept on-chain Bitcoin under merchant key authority

**Merchant job:** Receive payment directly to merchant-controlled keys, recover independently, and keep checkout staff from gaining spend authority.

**What Bitcoin could improve:** Supports direct receiving to merchant-controlled keys and independent recovery without required company permission over funds; checkout staff can use public-only watch access while owner-approved bounded spend automation remains a design candidate.

**What it does not solve:** Self-custody does not remove key loss, device security, backup testing, privacy leakage, or staff training risks; a seed alone does not restore the merchant application ledger. Owner-approved automated spending requires explicit authority bounds and architecture proof.

**Candidate:** Merchant hot wallet or independently created cold wallet, public-only pairing, independently verified receive address, durable issued-address mapping and merchant-held recovery artifacts. Evaluate bounded, owner-authorized signing/UTXO protection without changing signing behavior before proof and review.

**Costs and tradeoffs:** Authority and possible provider independence come with direct responsibility for key protection, backups, safe address derivation, and incident recovery.

**Proof needed:** Clean-device restore, watch-only pairing without secret exposure, register role denial of spend/authority changes, and correct address verification on the signer’s own display.

Benchmark references: SQC-C023, BTO-C007, BTO-C008, BTC-C001, BTC-C003, BTC-C012, BTC-C013.

## BTC-OPP-003: Make payment state and sale exceptions honest

**Merchant job:** Know whether a request is unpaid, partial, overpaid, late, unconfirmed, confirmed, conflicted, reorganized, expired, refunded, or unresolved before fulfillment.

**What Bitcoin could improve:** On-chain transaction and confirmation evidence can be independently checked and tied to a specific request; the merchant need not treat processor messages as the source of Bitcoin truth.

**What it does not solve:** A transaction can be late, replaceable, unconfirmed, reorganized, partial, or sent after cancellation. One confirmation is not a universal fulfillment policy and an address does not expire on-chain.

**Candidate:** Durable request-to-sale mapping and observation journal with freshness/depth, immutable transaction references, explicit exception states, idempotent reconciliation, and separate fulfillment permission.

**Costs and tradeoffs:** More accurate distinctions add cashier exception work and may delay fulfillment; simplistic instant-paid indicators increase loss or duplicate-fulfillment risk.

**Proof needed:** Regtest unseen/unconfirmed/confirmed/conflicting/reorged, partial/excess/duplicate, expired/late, crash/restart and restored-ledger cases; merchant selects confirmation and fulfillment policy.

Benchmark references: BTM-C004, BTM-C005, BTM-C029, BTM-C030, BTM-C031, BTC-C004, BTC-C005, BTC-C006, BTC-C029, SQC-C024.

## BTC-OPP-004: Control receiving and spending cost decisions

**Merchant job:** Understand incoming-output future spend economics and approve outbound fee, destination, and input use with protection against avoidable UTXO loss.

**What Bitcoin could improve:** Direct merchant control can make fee timing, UTXO selection and spend authorization visible rather than hiding them inside a processor payout schedule.

**What it does not solve:** Bitcoin cannot guarantee low fees or rapid confirmation. Tiny outputs can cost more to spend than their value; fee ceilings may delay transactions. Any automated spend behavior must be bounded and owner-approved; fresh addresses do not establish recipient identity.

**Candidate:** Read-only satoshi/UTXO cost advisory plus a research candidate for bounded UTXO protection and fee-aware owner-approved signing, with explicit transaction preview, durable input reservation and limits. Preserve current signing behavior until architecture proof and review pass.

**Costs and tradeoffs:** Advisory-only status is the current code boundary, not a permanent product limit. Automation can protect against uneconomic outputs but adds authority, race, recovery and fee-estimation risks; unbounded spending would violate the intended authority model.

**Proof needed:** Define amount/fee/UTXO bounds, failure behavior, rollback and owner controls; test fee changes, dust-sized receipts, reservation races, invalid/wrong-network refund destinations, actual fee versus approved bounds, recovery and signer display. No hard maximum-loss guarantee.

Benchmark references: BTO-C035, BTO-C041, BTC-C007, BTC-C008, BTC-C027, BTC-C028, BTC-C026.

## BTC-OPP-005: Offer Lightning as a separately gated receiving mode

**Merchant job:** Offer Shop Lightning as a core Bitcoin receiving goal when the merchant-controlled node is funded, operational, backed up and has usable inbound liquidity.

**What Bitcoin could improve:** Can support rapid wallet-to-wallet receiving without on-chain confirmation for each retail sale and can be operated under merchant node authority.

**What it does not solve:** It does not provide guaranteed routes, free or prepaid inbound capacity, zero downtime, instant exit, or channel-state independence from current persistent state and counterparties.

**Candidate:** Merchant-operated Shop Lightning receiving as a core goal, with honest opt-in/setup and readiness gates for confirmed funding, usable inbound capacity, node availability, backup freshness, MPP support and independent exit. Select on-chain-first launch ordering only as a proposed release sequence; persist ChannelManager/Monitor state safely and document close/sweep path.

**Costs and tradeoffs:** Faster checkout requires channel capital, inbound liquidity, connectivity, careful backups and recovery operations; force-close can incur on-chain fees and time locks. A separate initial on-chain-first release is a proposed sequencing choice, not an exclusion of Shop Lightning.

**Proof needed:** Named LDK/node version and wallets; funding confirmation, directionality, liquidity acquisition cost, routing fees/failures, MPP, outage, stale backup, peer loss, reorg and unilateral exit trials.

Benchmark references: SQC-C021, BTC-C014, BTC-C015, BTC-C016, BTC-C017, BTC-C018, BTC-C019, BTC-C020, BTC-C022, BTC-C030, BTO-C020, BTO-C025, BTO-C040.

## BTC-OPP-006: Keep payment privacy and checkout service local by default

**Merchant job:** Take payment and retain sale history without mandatory company-server approval, sales-history upload, customer identity collection, or exposure of needless metadata.

**What Bitcoin could improve:** A direct wallet payment can avoid sending full payment credentials and sale records to a payment processor; merchant can use one-time addresses and local mapping.

**What it does not solve:** Public blockchain activity remains observable and can be linked; address rotation is not anonymity. Local-only operation does not itself solve secure backup, device compromise, or optional service disclosure.

**Candidate:** Local journal, minimal opaque payment-request references, fresh addresses, privacy-reviewed exports, opt-in scoped webhooks only, and no required publisher ledger or telemetry.

**Costs and tradeoffs:** Reducing central data collection can limit remote support, consolidated analytics and automatic multi-site views; opting into integrations creates additional disclosed data paths.

**Proof needed:** Inspect URI/QR/NFC payloads, receipts, logs, exports, webhook payload/signature/retry behavior, retention and permission boundaries; test operation with no company endpoint.

Benchmark references: SHC-C024, SHO-C022, SHO-C023, BTC-C011, BTC-C031, BTC-C032, TOO-C038, BTO-C013, BTO-C034.

## BTC-OPP-007: Continue cashier work through outages without inventing settlement

**Merchant job:** Keep a sale and permitted cash/card workflows usable when network, processor, or Lightning is unavailable, then reconcile uncertain tenders safely.

**What Bitcoin could improve:** Cash/on-chain choices may remain available independently of a card provider, and merchant-operated infrastructure can avoid a mandatory company server for local checkout.

**What it does not solve:** Bitcoin cannot be settled by a disconnected cashier or node; an offline QR/address display is only a request, not proof of received payment. Card offline authorization is provider-specific and may carry delayed decline risk.

**Candidate:** Local sale journal and cash fallback; mark card response unknown until reconciliation; disable Lightning when node status is stale; show on-chain request only as unpaid until a trusted observer catches up.

**Costs and tradeoffs:** Availability differs by tender; safe status can be less convenient than an optimistic paid marker and reconciliation may require staff attention after reconnect.

**Proof needed:** App kill, power/network loss, card timeout, node disconnection, remote sync conflicts, repeated attempts and post-outage reconciliation on target devices without duplicate charge or fulfillment.

Benchmark references: TOS-C013, TOS-C014, TOS-C016, TOS-C017, SQC-C017, SQC-C018, SHC-C028, SHC-C029, CARD-C010, BTC-C018.

## BTC-OPP-008: Reconcile refunds, receipts and day close across tender rails

**Merchant job:** Tell the customer what was sold, what was received, what is owed/refunded, and reconcile exact tender totals at shift close.

**What Bitcoin could improve:** On-chain receipts and transactions provide independently reconcilable satoshi amounts and timestamps; direct merchant refunds can avoid asking an original processor to authorize Bitcoin funds.

**What it does not solve:** A Bitcoin refund is a new spend with fee, destination, confirmation and authorization risks; a receipt or invoice does not prove settlement. Bitcoin does not determine tax treatment, card refund eligibility, inventory or accounting.

**Candidate:** Canonical sale/tender/refund ledger; record refund requested/owed/paid separately, require a fresh destination and owner approval for on-chain payout, and export exact amounts with privacy disclosure.

**Costs and tradeoffs:** Explicit tender-level reconciliation is more accurate but entails staff handling for exceptions and careful non-duplicative refund processing.

**Proof needed:** Multi-tender partial refunds, canceled/late/excess payment, crash/retry, destination replacement, actual transaction match, receipts and day-close totals across Bitcoin/cash/card.

Benchmark references: BTM-C016, BTM-C017, SHC-C015, TOS-C010, SHC-C022, BTM-C007, SHO-C013, TOO-C023, BTC-C026, BTC-C006.

## BTC-OPP-009: Keep card processing optional and replaceable

**Merchant job:** Accept cards when a merchant-approved provider/device is available without making that provider authoritative for Bitcoin keys or sale truth.

**What Bitcoin could improve:** Merchant-controlled Bitcoin acceptance can remain available if card integration or account access changes, and card access can be isolated from Bitcoin signing authority.

**What it does not solve:** Bitcoin does not grant card-network acceptance, production eligibility, reader support, country/account eligibility, chargeback handling, or card offline approval.

**Candidate:** Tender adapter boundary with durable card attempt IDs and unknown-response reconciliation; provider tokens and processor permissions isolated from Bitcoin wallet authority; cash/on-chain remain standalone.

**Costs and tradeoffs:** Provider independence requires maintaining adapter behavior and target-device certification; additional adapters increase operations and support scope.

**Proof needed:** Supported production account/country/device/physical reader evidence, lost response/duplicate/cancel/refund recovery, provider outage independence, privacy review and key isolation.

Benchmark references: CARD-C007, CARD-C008, CARD-C014, CARD-C022, CARD-C026, CARD-C036, SHC-C014, SQC-C023.

## BTC-OPP-010: Support recurring merchant billing without assuming universal wallet auto-debit

**Merchant job:** Set up recurring subscriptions, present each cycle’s amount and due status, and follow up on missing payments.

**What Bitcoin could improve:** A merchant can issue a new on-chain or Lightning request for each billing cycle and reconcile direct settlement per cycle.

**What it does not solve:** A payment request is not standing authority to debit a customer’s self-custody wallet. The protocol does not guarantee arbitrary recurring pull payments, customer approval, funds availability, or successful delivery.

**Candidate:** Recurring schedule emits a distinct amount/network-scoped payment request per cycle with lifecycle notices and cancellation; customer initiates each payment. Treat any separately authorized spending arrangement as a distinct mechanism requiring explicit scope and revocation.

**Costs and tradeoffs:** Customer-initiated invoices preserve explicit per-cycle wallet control but may add renewal friction and missed-payment follow-up; standing authorization could reduce friction while adding authority, revocation and dispute risks.

**Proof needed:** Test recurring invoice generation, amount changes, cancel/pause/resume, late/partial/duplicate payments, failed notices, invoice expiry, reconciliation, and customer cancellation; separately review any proposed reusable spending authority.

Benchmark references: BTM-C013, BTM-C014, BTM-C015, BTM-C003, BTM-C004, BTC-C009.

## Proposed decisions

Established principles, current code limits, and proposed design choices are explicitly distinguished below. Changing a mechanism does not reject the merchant job. Deferral does not close a requirement.

### DEC-001: Keep payment processor access optional and isolated

**Proposal:** Keep card access optional and isolated from Bitcoin authority through a replaceable adapter. Do not require processor enrollment for cash or direct Bitcoin receiving.

**Preserved job:** Complete a retail checkout using the tender the merchant has enabled, with a correct sale ledger and receipt.

**Reason:** The standalone product includes optional third-party card processing. Its account, credential and payment lifecycle must remain separate from merchant Bitcoin keys and authority.

**Basis:** Established direction: optional card processing, cash, merchant Bitcoin authority and independent recovery. The adapter set, eligibility, hardware and exact setup experience remain undecided.

**Proof or revisit trigger:** Review provider credential scopes and data flows; test unknown/canceled/refund outcomes; demonstrate that revoking card access leaves cash and direct Bitcoin receiving available.

### DEC-002: Do not require company server permission for local checkout or recovery

**Proposal:** Keep a usable standalone local sale and on-chain receive path without company login/approval, publisher ledger or telemetry. Any server-backed integration stays opt-in and scoped; it cannot become a hidden prerequisite for recovery.

**Preserved job:** Operate a register, receive Bitcoin, and recover keys and sale records after routine service/device failure.

**Reason:** Accepted purity position rejects mandatory company-server approval for recovery, telemetry, or sales-history upload; however remote-sync UX and support model remain design questions.

**Basis:** established constraint: no required company permission over Bitcoin funds and independent recovery. A company endpoint is not required for standalone/local cash and on-chain operation per current plan; optional service UX remains undecided.

**Proof or revisit trigger:** Demonstrate standalone cash/on-chain checkout and recovery with company endpoints unavailable; review each opt-in sync, webhook or backup service before adding.

### DEC-003: Evaluate bounded owner-approved signing and UTXO protection

**Proposal:** Preserve the current signing behavior in code while researching owner-approved bounded signing/automation for UTXO protection, fee budgets, reservations, and routine approved transactions. Define explicit transaction/value/fee/destination bounds, owner control, safe failure and recovery before implementing any change.

**Preserved job:** Protect merchant funds from uneconomic UTXOs or operational mistakes while allowing an owner to authorize routine transfers and refunds safely.

**Reason:** The current code exposes advisory-only UTXO guidance, but that is an implementation boundary, not a permanent rejection of protection or automation. The user said an earlier signing-control proposal was acceptable; architecture evidence and review remain necessary.

**Basis:** research candidate: bounded owner-approved signing control was acceptable in principle. No exact policy, automatic coin selection, consolidation, fee limits, or signing behavior change has been approved for implementation.

**Proof or revisit trigger:** Produce and review a threat model, limits, authority map, state machine, failure recovery, fee/UTXO evidence and transaction-display proof; obtain approval for exact behavior before changing signing.

### DEC-004: Separate recurring payment requests from standing wallet debit authority

**Proposal:** Preserve subscription plans, cycle invoices, lifecycle notices and merchant-authorized refunds. Default recurring Bitcoin billing to a fresh customer-initiated request per cycle; do not claim a universal wallet can be auto-debited. Research any opt-in recurring spending authority separately with explicit limits, consent, revocation, destination scope and failure handling.

**Preserved job:** Bill a customer on a recurring schedule, let them pay each due cycle, manage subscription changes, and resolve missed payments or refunds.

**Reason:** Invoice/subscription workflows exist, but URI or invoice creation does not authorize a merchant to spend from a customer self-custody wallet. Preserving the billing job does not require assuming pull-payment capability.

**Basis:** merchant recurring billing job is in scope for research; customer-authorized invoice flow is a candidate. Universal automatic debit is not an established user constraint or approved mechanism.

**Proof or revisit trigger:** Validate repeat invoice lifecycle and customer initiation first. Any reusable debit authority requires explicit user approval, protocol/wallet compatibility, clear consent/revocation, bounded amount/frequency, and recovery/dispute policy.

### DEC-005: Treat fee and UTXO automation as a bounded protection candidate

**Proposal:** Current UTXO fee guidance is advisory only. Research protection that can block, reserve, or request approval around uneconomic outputs and fee budgets, including bounded owner-approved transaction automation; do not infer autonomous spend authority or promise zero fees/guaranteed confirmation.

**Preserved job:** Explain what a receipt is worth now and what it may cost to spend later.

**Reason:** The plan describes the current code boundary as advisory with no autonomous selection/consolidation. That does not veto future automation. User accepted a signing-control proposal in principle; implementation requires exact bounds and architecture proof.

**Basis:** research candidate: UTXO protection and bounded owner-approved automation are acceptable for architecture study; current code remains advisory-only. Exact policy and implementation approval remain open.

**Proof or revisit trigger:** Specify value and fee thresholds, privacy effects, reversibility, exception path, recovery, actual fee variance and owner controls; reviewed design before implementation.

### DEC-006: Keep Shop Lightning as a core Bitcoin goal with an honest readiness path

**Proposal:** Treat Shop Lightning as a core receiving goal, opt-in in merchant setup, with capital, inbound-liquidity, availability, persistence/recovery and independent-exit gates. On-chain-first release ordering is a proposed sequencing option; it does not defer the long-term Shop Lightning product goal. Avoid free/prepaid-liquidity and guaranteed-route claims.

**Preserved job:** Receive Bitcoin over a fast channel at the merchant’s own receiving node, and retain a safe on-chain recovery path.

**Reason:** The user’s product goal includes Shop Lightning; the standalone plan requires capacity/funding/availability/recovery gates. Those gates preserve honest economics without turning a proposed first-release order into product exclusion.

**Basis:** established goal: Shop Lightning receiving. Setup, liquidity source, exact release sequence and device coverage remain proposals to prove; no specific provider or free-liquidity assumption is approved.

**Proof or revisit trigger:** Demonstrate node setup, actual funding and inbound capacity, recurring operation, backups, restore, peer/provider loss, route failures, channel exit, timelock and on-chain fees on the chosen version/device.

### DEC-007: Do not promise offline Bitcoin settlement

**Proposal:** When offline, continue only locally supported sale recording and cash tender. Show an on-chain request as awaiting observation and Lightning as unavailable when node status is stale. Keep card offline behavior provider-specific and report pending/unknown until reconciled.

**Preserved job:** Continue a cashier interaction during connectivity loss without claiming money arrived or fulfilling twice.

**Reason:** Bitcoin settlement requires network/node observation or later reconciliation; cached invoice/QR is not payment proof. Existing plan requires honest Bitcoin observation states and treats missing responses as unknown.

**Basis:** established requirement: do not misstate Bitcoin settlement when disconnected or stale. Cash fallback and tender-specific offline UX are supported job candidates; exact device behavior remains to validate.

**Proof or revisit trigger:** Network/node interruption, device restart, duplicate attempts and reconnect reconciliation on target hardware; verify no false paid state or duplicate fulfillment.

### DEC-008: Keep privacy local-first and multi-location optional

**Proposal:** Keep customer identity and sales history local by default, use minimal payment-request metadata, and make multi-location synchronization or webhooks explicit opt-in with location-scoped permissions and disclosed data fields.

**Preserved job:** Complete private local checkout and reconcile each merchant location; offer an optional shared view when the owner chooses it.

**Reason:** Accepted purity position rules out required telemetry and sales-history upload, while the standalone plan defers universal multi-location claims. Optional service design remains undecided.

**Basis:** Established privacy direction: no mandatory publisher telemetry or sales-history upload. Multi-location release support has not been established; optional synchronization and customer-profile design remain proposals.

**Proof or revisit trigger:** Threat/data-flow review, export and webhook preview, location access matrix, retention choice, and offline behavior before enabling any shared service.

### DEC-009: Treat card acceptance as replaceable and production-gated

**Proposal:** Use the existing Square Sandbox flow as the adapter starting point, retain a provider-neutral tender interface, and do not claim production card acceptance until account, country, device, reader, outage, refund, reconciliation and privacy gates pass.

**Preserved job:** Accept cards when the merchant’s chosen provider and physical workflow are supported; preserve cash and Bitcoin choices if card service fails.

**Reason:** Accepted plan explicitly bounds current evidence to virtual-card Sandbox and avoids provider/country/device eligibility claims without fresh evidence.

**Basis:** established_constraint: card optional; production readiness unproven pending gates. A final vendor or adapter selection remains undecided.

**Proof or revisit trigger:** Fresh provider eligibility evidence and an end-to-end physical reader shift including lost responses, duplicate submissions, refunds, revocation, force-stop and relaunch.

### DEC-010: Use standalone retail as the first measured validation scope

**Proposal:** Prioritize a complete standalone retail shift and validate sale, tenders, exception handling, receipts, refunds, staff authority and closeout. Treat restaurant, online, unattended and multi-location capabilities as backlog and evidence targets, not rejected jobs or a permanent product boundary. POS integrations follow as proposed sequencing.

**Preserved job:** Serve merchants who sell at a counter today and preserve their path to restaurant, online or multi-location workflows as their needs and validation evidence justify.

**Reason:** The plan positions standalone retail before integrations and requires measured evidence; it does not establish a final ceiling on product scope. Existing POS later is the accepted sequence, while target workflows beyond retail remain future research.

**Basis:** Established sequence: standalone product before existing-POS integrations. A retail shift is the proposed first validation scenario; the exact release scope and later adapter priority remain undecided.

**Proof or revisit trigger:** Complete a target retail shift, then gather merchant evidence for restaurant/online/multi-location jobs and evaluate the next workflow and integration independently.

### DEC-011: Use familiar operational controls without cloning every incumbent feature

**Proposal:** Use incumbent research to identify familiar merchant jobs and recovery cues—refunds, staff controls, split tender, local outage recovery, receipts, and closeout—then design Bitcoin-specific states and permissions around self-custody. Do not reject merchant jobs merely because one incumbent uses a custodial, hosted, or fiat mechanism.

**Preserved job:** Cashiers can finish, void, refund, hand off and close a shift with understandable status and manageable exception steps.

**Reason:** A vendor’s mechanism is not the merchant job. Better UX remains an empirical design question and must preserve established authority/purity goals.

**Basis:** research gate: merchant UX jobs are candidates for observation; exact screens/defaults are undecided. Self-custody and independent recovery remain established constraints.

**Proof or revisit trigger:** Cashier observation and usability tasks with named retail workflow; measure completion time, error rate, exception recovery, receipt accuracy and training burden before design commitment.

### DEC-012: Reject mandatory custodial Square Bitcoin and forced fiat conversion by default

**Proposal:** Treat a Square-style custodial Bitcoin purchase/acceptance flow, fiat conversion, altcoin settlement or required processor account as a mechanism exception requiring separate consent. Default architecture remains direct Bitcoin receipt to merchant keys. Preserve merchant jobs of direct receive, Bitcoin refund to an owner-approved fresh destination, recurring subscription invoices and related lifecycle management; do not silently replace those jobs with gift-card/fiat/processor credit.

**Preserved job:** Take Bitcoin directly, retain authority and independent recovery, refund Bitcoin under merchant-approved rules, and bill subscriptions on a clear recurring schedule.

**Reason:** A hosted mechanism may simplify quote, conversion or reporting, but changes custody, funds permission and settlement asset. User-established boundaries favor self-custody, no required company funds permission, and Bitcoin-first product intent.

**Basis:** established constraint: self-custody, independent recovery, Bitcoin-first and no required company funds permission. Custodial Square Bitcoin, forced fiat conversion and altcoin substitution are not authorized as defaults; no product job is rejected.

**Proof or revisit trigger:** Only consider a specific custodial/conversion exception after explicit authorization, complete terms and authority map, asset and fees disclosure, refund/subscription path, exit plan, and direct self-custody fallback.
