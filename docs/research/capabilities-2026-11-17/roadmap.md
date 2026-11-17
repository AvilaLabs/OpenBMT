# From capability research to a merchant product

This is a proposed sequence, not an instruction to implement this research batch. The [register](capability-register.json) remains the complete set of retained comparison records. A phase selects work and validation; it does not delete requirements assigned later.

## 1. Resolve the product and architecture gates

Specify the canonical sale, immutable item/price/tax snapshot, tender attempts, remaining balance, refund obligation, inventory effects, and fulfillment permission. Define idempotency and recovery before designing the happy-path screens. Mixed tender must retain each leg's actual state and refund bounds. Unknown card outcomes and late Bitcoin payments must not be treated as a new unpaid sale that is safe to charge again.

Define the owner, manager, cashier, integration, and device authorities. Receiving and reporting permission must not imply spending, destination changes, arbitrary signing, or backup access. Research bounded owner-approved automation with explicit transaction, destination, fee, value, concurrency, revocation, and recovery rules. Keep current signing behavior intact until the proposed change is reviewed and proved.

Choose and document the initially supported country, tax configuration responsibilities, devices, wallet modes, and card hardware. The proposed first scenario is a complete counter-service retail shift. That is a validation target; the user's broader standalone POS goal still includes restaurant, online, unattended, and multi-location workflows.

Exit evidence: reviewable state transitions, authority matrix, data/contact inventory, immutable-money rules, interruption/recovery cases, and a declared supported workflow. Each selected requirement links to its CAP and source-claim IDs. Unresolved source facts have an owner and a resolution path, not an assumed answer.

## 2. Complete one useful standalone shift

Build the ordinary merchant jobs together: configurable catalog and variants, applicable modifiers, item lookup and scanning, exact totals, supported tax configuration, discounts, tips, basket corrections, cash received/change, optional card, direct Bitcoin receiving, receipts, order history, returns/refunds, inventory changes, staff permissions, opening float, cash movements, count, close, and tender-aware reporting.

Use the existing engines where their evidence fits. Complete the merchant behavior around them. A second receipt UI or a new refund engine is not justified merely because a competitor has a differently named screen.

Measure these scenarios on the named physical setup:

| Scenario | Required outcome | Evidence to record |
| --- | --- | --- |
| First setup | A merchant can identify where funds go, configure the selected shop, and complete the first sale without exposing secrets | Setup time, interventions, misunderstood choices, wallet/reader pairing failures |
| Busy checkout | Staff can find/customize items, correct a mistake, take the enabled tender, and give the right receipt | Completion time, taps, input errors, correction time, accessibility observations |
| Cash over/under tender | Correct remaining balance/change; inventory and receipt update once | Exact-money cases, accidental repeat actions, staff correction and audit history |
| Card result lost | Sale remains pending/unknown until reconciled; no duplicate charge is invited | Provider reference, timeout/restart/reconnect sequence, result repair |
| Partial/split payment | Each successful leg is retained; only the remaining amount is collected | Tender allocations, cancel/refund/retry, receipt and report reconciliation |
| On-chain payment exception | Observed, confirmed, late, partial, excess, conflicted, and reorganized states produce the correct permitted next action | Controlled transaction evidence, freshness/depth, no false finality or duplicate fulfillment |
| Refund/return | Returned quantities, stock, liability, fees/tips/tax policy, approval, and actual payout remain coherent | Permission denial, fresh destination validation, partial/mixed tender, receipt and closeout effects |
| Outage/restart | Supported local work survives; unobserved or uncertain payments stay explicit | Network/power/process interruption, persisted state, later reconciliation |
| Shift close | Cash count, discrepancy, tender totals, refunds, fees where known, and outstanding obligations reconcile | Independently computed totals, business-day/time-zone boundaries, exports |
| Device loss/recovery | Merchant can recover funds and necessary records without OpenBMT company permission | Clean-device restoration, receiving-address discovery, local ledger and key recovery tested separately |

Exit evidence: a complete observed shift with representative staff, correct reconciliation, repeatable interruption recovery, and a measured usability baseline. “Best in class” remains an objective until comparative observation supports it. No invented usability score or parity percentage substitutes for that evidence.

## 3. Prove the Bitcoin-specific modes

**On-chain:** support hot-wallet and public-only cold-wallet receiving with independent address verification and recovery discovery. Define confirmation/fulfillment and reorganization policies. Show UTXO future-spend economics and research protection against uneconomic receipts and outgoing fee mistakes. Safe reserves, fee budgets, coin selection, consolidation, fee bumps, and signing automation require explicit authority and race/recovery design. A minimum invoice value alone does not prevent an arbitrary sender from creating a small output; a fee ceiling does not guarantee timely confirmation.

**Shop Lightning:** retain merchant-operated Lightning as a core goal with an easy default and advanced control. Prototype the actual onboarding/capital quote and receiving-readiness experience before promising automatic operation. Prove inbound capacity, usable routes, connectivity/availability, persistent channel state, backup freshness, recovery, peer loss, unilateral close and sweep. Evaluate optional replaceable services individually. A merchant-owned key and a seed backup do not by themselves prove live channel recovery; a capacity indicator is not a route guarantee. The exact device/Box/availability arrangement remains a design and testing question.

**Payment handoff:** validate QR, copy, deep links, and supported NFC paths with a named wallet/device matrix, amount/network binding, malformed/required parameters, duplicate taps, expiry, stale state, accessibility, and safe return to the sale. Publish only the combinations actually tested.

Exit evidence: mode-specific merchant setup and recovery on named versions/devices, costs and capital requirements explained before commitment, and failure cases that fail safely. General POS completion and Bitcoin-mode work can proceed in parallel after the shared contracts are settled.

## 4. Finish reusable merchant interfaces and additional workflows

Finish the versioned local invoice/query/event contract, scoped credentials, reconciliation cursors, durable delivery, and optional webhook bridge. An app need not accept public inbound connections to send an outbound notification, but a sleeping or unreachable app cannot promise continuous observation or delivery. Signed payloads, duplicate/out-of-order handling, revocation, replay, state repair, minimal disclosure, and merchant-selected destinations remain obligations.

Retain payment requests, recurring cycle invoices, digital fulfillment, cart/import/export, staff and accounting workflows, customer privacy, and additional card adapters. Carry NWC, BOLT12/LNURL, advanced wallet management, and related prior B01–B32 work through the [crosswalk](analysis/prior-benchmark-crosswalk.json); items outside this corpus need targeted source research before a support claim.

Expand restaurant checks/seats/tables, kitchen routing, held/fired orders, scheduling, pickup/delivery, stock purchasing/transfers, multi-location authority and synchronization according to merchant demand. Validate each as a complete operational workflow with its own offline and recovery cases. An API record or generic order screen is not evidence of restaurant or online completeness.

Exit evidence: capability-specific workflow demonstrations, contract tests that exercise meaningful failures, hardware/provider/version qualifications, and explicit data and authority boundaries. Deferrals remain in the register.

## 5. Consider existing POS integrations

Only after the standalone product meets its declared release gates, revisit host-specific adapters and distribution approval. Technical capability, commercial approval, device support, and a smooth merchant workflow are separate questions. Do not make Square, Clover, Lightspeed, or another host a hidden prerequisite for core Bitcoin receiving.

The standalone sale/payment engine should make reuse possible, but this research phase does not implement those adapters or promise access to any commercial POS.

## How to close a capability

For a selected capability, preserve the exact source variants and define OpenBMT's supported interpretation. Record the implementation and observed workflow, evidence on target hardware, permission and recovery behavior, and the comparison showing match or improvement. If a different mechanism better serves the merchant while respecting their funds authority, document the exception and its proof. An unexplored feature remains unassessed; a scoped test remains scoped; no completion credit follows from a screen, function name, or documentation claim alone.
