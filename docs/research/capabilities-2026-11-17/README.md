# OpenBMT capability research

Research snapshot: November 17, 2026. Product evidence is pinned to commit `ff831bd4dc1e7082877e067b5a893e880ebfa920`. This phase changes research records only.

The proposed rule is to **match or improve the merchant job behind every retained capability**, including its important failure and recovery cases. Any exception must name the mechanism or scope being changed, explain why, preserve the useful merchant outcome, and state when to revisit it. A deferred capability remains an open requirement.

This is a bounded first-party documentation inventory. It does not establish every capability of these companies, hands-on compatibility, feature parity, or a production-ready OpenBMT. The [coverage ledger](source-coverage.md) and [independent corpus audit](analysis/corpus-audit.json) expose the inspection boundary and unfinished branches. We should keep expanding that boundary before making a whole-product parity claim.

## Review the inventory

- [Capability index](capability-index.md): the complete comparison-record list, grouped by domain, with stable IDs and current OpenBMT evidence references.
- [Capability register](capability-register.json): the authoritative consolidated data, including every source claim, vendor condition, improvement hypothesis, Bitcoin assessment, acceptance scenario, and proposed decision reference.
- [Source coverage](source-coverage.md): inspected topics, exclusions, unresolved questions, and the reasons for each.
- [Current OpenBMT evidence](analysis/openbmt-evidence.json) and [claim mapping](analysis/openbmt-mapping.json): distinguish engine primitives, prototype integration, scoped workflow checks, and unassessed behavior.
- [Bitcoin opportunities and proposed decisions](bitcoin-review.md): readable design candidates, costs, validation, exceptions, and the merchant jobs each preserves. The underlying [opportunity](analysis/bitcoin-opportunities.json) and [decision](analysis/proposed-decisions.json) records remain editable data.
- [Prior benchmark crosswalk](analysis/prior-benchmark-crosswalk.json): keeps the previous B01–B32 backlog visible, including capabilities not covered by these four product corpora.
- [Delivery sequence and proof](roadmap.md): how to turn the inventory into measurable merchant workflows after this research phase.

The review workbook presents the same snapshot as filterable tables. The JSON files remain the repository record; the workbook is a review export.

<!-- SNAPSHOT_COUNTS_START -->
| Population | Source claims | Source records | Coverage topics | Reviewed / excluded / unresolved |
| --- | ---: | ---: | ---: | --- |
| Four product corpora | 445 | 363 | 178 | 151 / 13 / 14 |
| Card components | 37 | 20 | 17 | 14 / 1 / 2 |
| Bitcoin-native requirements | 32 | 16 | 21 | 17 / 2 / 2 |

The consolidated register contains **502 comparison records**, retaining **514 source claims**, plus **10 Bitcoin opportunity proposals** and **12 decision/gate proposals**. Source records cover 368 distinct URLs; repeated page sections count separately. The prior crosswalk preserves all 32 benchmark families.

A source claim is not a unique feature count. A comparison record may preserve multiple qualified vendor variants. These totals describe the research artifact, not a percentage of product coverage or OpenBMT completion.
<!-- SNAPSHOT_COUNTS_END -->

## Why these benchmarks

| Corpus | What it contributes | Inspection boundary |
| --- | --- | --- |
| Square | General checkout, retail operations, selected restaurant workflows, cash closeout, supported hardware, and its own Bitcoin acceptance model | Public US Support sections listed in the source ledger. Standard, Retail, and Restaurant qualifications remain distinct. Appointments, payroll, lending, and every adjacent Square product are not silently included. |
| Shopify POS | Catalog and variants, inventory and purchase orders, returns, omnichannel fulfillment, customer records, permissions, cash tracking, and device conditions | Public POS Help and the linked admin guides actually inspected. POS Pro, app extensions, hardware/country limits, and historical Stocky material retain their labels. |
| Toast | Restaurant checks, seats, splitting, transfer, fulfillment, menu/modifier configuration, kitchen routing, drawer activity, closeout, enterprise configuration, and outage behavior | Public Platform Guide plus the specific linked Support leaves inspected. The Platform Guide is not the complete Toast commercial suite. |
| BTCPay Server | Bitcoin invoice exceptions, receiving and custody modes, refunds and payouts, wallet/PSBT operations, store roles, API/webhook behavior, node operation, deployment, and recovery | Public merchant and operator documentation listed in the ledger. Plugin, hosting, wallet mode, and Lightning backend differences remain visible. |
| Card components | Optional card acceptance inside OpenBMT | Square Mobile Payments SDK and Stripe Terminal documentation. These are component candidates, not two additional full POS benchmarks. Sandbox evidence does not establish production eligibility. |
| Bitcoin-native requirements | Requirements incumbents may not solve for a self-custodial terminal | Protocol and pinned implementation evidence, with product hypotheses kept separate. Protocol documentation does not prove OpenBMT's device, liquidity, backup, or merchant experience. |

The older [merchant feature benchmark](../merchant-feature-benchmark-2026-11-16.md) also considered Bitcart, LNbits, wallet projects, and processors. Their selected ideas remain in the crosswalk. Their entire current documentation has **not** been inventoried in this snapshot, so they are not counted as complete product comparators here.

## What each record asks

Every source claim records the merchant outcome, actor, trigger, documented behavior, preconditions, device/plan/country/backend qualifications, failure or recovery boundary, and exact inspected source section. It then asks:

1. What concrete experience or outcome could OpenBMT improve?
2. Does Bitcoin improve this job? A neutral answer is valid.
3. Could a different Bitcoin design improve it, and what does that design cost in authority, privacy, fees, availability, capital, or recovery work?
4. What observable scenario would demonstrate the behavior and its failure path?

Hypotheses and acceptance scenarios are proposals. They are not claims that a competitor or OpenBMT passed those scenarios. A source marked partially documented or unknown must be resolved before relying on its details.

The [normalization review](analysis/normalization-decisions.json) combines only narrow equivalent actions. Broader similarities remain navigation relationships. An item split, a seat split, and split tender retain different obligations. A full stock count and a cycle count do too. Every vendor variant survives normalization, even when several variants share a row.

## Assessment

OpenBMT needs the operational completeness of a serious POS alongside its Bitcoin architecture. Catalog maintenance, modifiers, tax configuration, pricing corrections, receipts, returns, staff controls, cash counts, peripherals, and daily reconciliation cannot be excused by direct Bitcoin settlement. Most of these jobs are payment-rail neutral. The sources and hypotheses for each are retained in the register.

Bitcoin offers stronger candidates around merchant control over funds, public-only receiving roles, independent verification of receipt, portable payment requests, and explicit owner authority over outgoing transactions. It also introduces requirements a card-oriented POS does not solve for us: confirmation and reorganization policy, address discovery after recovery, UTXO spending economics, channel state, inbound liquidity, and independent exit. These remain engineering and merchant-experience problems to prove.

The best innovation targets join those two bodies of work. A cashier should see one sale and one amount due while OpenBMT preserves the different evidence behind cash received, a card attempt, an observed on-chain payment, and a settled Lightning payment. The implementation must retain those distinctions through a crash, an unknown result, a refund, and the end of the shift. See the [opportunity register](analysis/bitcoin-opportunities.json) for the candidate architecture, costs, and validation behind each proposal.

The comparison does not support declaring the existing prototype a complete POS. Its sample basket, cash journal, history and receipt projection, authorization metadata, wallet engines, and Sandbox adapter provide useful foundations. Each has a narrower boundary than the corresponding full merchant workflow. No capability in this snapshot is certified as matched or surpassed.

## Decisions that remain open

The established product direction is standalone OpenBMT first, Bitcoin-first receiving under merchant authority, independent recovery, cash, and optional third-party card processing. Existing POS integration follows that work. Optional routing, liquidity, synchronization, and other services still require an explicit account of keys, blocking powers, disclosure, fees, recovery, and replaceability.

Current code limits do not become permanent product rules. In particular, the existing UTXO advisory is not a veto on researching bounded, owner-approved protection or automation. Shop Lightning remains a core goal. A proposed retail-shift validation sequence does not remove restaurant, online, unattended, or multi-location jobs from the backlog.

The exception register changes mechanisms where needed: custodial Bitcoin settlement, forced conversion, mandatory company permission over funds, or a universal auto-debit promise cannot be copied merely to match a checkbox. Direct receiving, approved refunds, and recurring billing remain merchant jobs to solve. Outage handling must preserve useful cashier work while accurately showing which payments await observation or reconciliation.

## What this phase verifies

The [validation record](analysis/validation.json) checks the research schema, unique identifiers, source and claim references, required improvement/Bitcoin questions, and the consolidated counts. The independent corpus audit checks documentation boundaries and selected substantive claims. The workbook is recalculated, inspected, rendered, and checked for export errors separately.

These checks validate the research artifact. They do not simulate a merchant shift, validate real funds, establish tax compliance, test physical hardware, or prove a wallet recovery design. Those are explicit future release gates in the roadmap.

To rebuild the repository index after editing the source bundles, run `python3 docs/research/capabilities-2026-11-17/build_register.py` from the repository root. Update this narrative and audit when the declared corpus changes; a passing structural validator alone does not establish source completeness.
