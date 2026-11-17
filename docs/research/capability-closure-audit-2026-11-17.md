# Capability closure audit: native POS foundations

This audit separates two questions that the initial ledger incorrectly collapsed:

1. **OpenBMT capability:** does the current product expose and meaningfully validate the canonical merchant job?
2. **Benchmark parity:** has OpenBMT also demonstrated every linked vendor workflow, qualifier and recovery case?

The first question controls the canonical capability checkbox. The second remains visible as an independent obligation. OpenBMT does not need to copy irrelevant vendor mechanics to support a merchant job, but a model-only primitive, inaccessible screen, demo or adjacent feature is insufficient.

## Result

Of the 23 capabilities linked to the native foundations batch:

- 7 are `verified_supported` as canonical OpenBMT merchant capabilities.
- 11 are partial: useful product behavior exists, but the central canonical job is incomplete.
- 5 are currently unsupported: the central workflow is absent.
- Exact benchmark parity remains open for all 23.

| Capability | OpenBMT status | Evidence boundary | Benchmark parity |
|---|---|---|---|
| `inventory.stock.adjust` | **Verified supported** | Product UI records a signed stock correction with required reason; durable model tests cover replay, changed payload and stale versions. | Partial: Square location, variation and channel-specific workflow remains unverified. |
| `inventory.sales.decrement` | **Verified supported** | Eligible local sale handoff decrements tracked stock exactly once; JVM and emulator evidence cover 5→3, duplicate replay and restart. | Partial: automatic Square POS, invoice and shipped-online channel behavior remains unverified. |
| `cash.drawer.cash_in` | **Verified supported** | Product action records amount, reason and operator label in an open cash session and updates expected cash; JVM tests validate arithmetic and durable replay. | Partial: physical Toast drawer opening and its Cash Activity Audit are not implemented. |
| `cash.drawer.cash_out` | **Verified supported** | Product action records a reasoned removal and updates expected cash; model rejects an impossible negative expected balance. | Partial: Toast physical movement, payout/drop distinctions and undo workflow remain open. |
| `cash.drawer.close_count_actual` | **Verified supported** | Native workflow stores actual count, closes the session, freezes expected/actual/discrepancy history and rejects later movements; emulator observed 2,900 vs 2,916. | Partial: Toast pause/reopen/correction and physical drawer qualification remain open. |
| `cash.drawer.review.expected_actual_variance` | **Verified supported** | Native workflow displays expected cash, accepts actual count and persists signed over/short; emulator verified a −16 discrepancy. | Partial: Toast blind/full permission split and authorized correction remain open. |
| `reporting.export` | **Verified supported** | Native report supports reviewed CSV copy/save; emulator verified analyzable rows, successful system-file save and exact reviewed-export bytes. | Partial: Square report-type output rules, email/print paths and permission/geography qualifiers remain open. |
| `checkout.pos.catalog_cart` | Unsupported | Canonical capability requires a published browser POS; current candidate is the native Android checkout. | Open. |
| `checkout.cart.item.variation_modifier` | Partial | Selection and frozen pricing primitives exist, but merchant UI cannot define required modifiers or multi-variant products. | Open. |
| `checkout.discount.manual_apply` | Partial | Flat order-dollar reduction exists; configured percentage/fixed rules, eligibility and duplicate prevention do not. | Open. |
| `checkout.tax.inclusive_additive` | Partial | Model and receipt support both treatments; merchant UI exposes exclusive tax only. | Open. |
| `checkout.tip.collect` | Partial | Operator-entered fixed tip exists; customer card tip choices, eligibility and caps do not. | Open. |
| `catalog.product.create.and.option` | Partial | Product creation works, but UI cannot add selectable size/color variants under one product. | Open. |
| `catalog.item.variant_options` | Partial | Existing variants can be selected/edited; adding variants and generating option combinations are absent. | Open. |
| `catalog.pos.barcode.scan.resolve` | Unsupported | Typed SKU/barcode lookup exists; scanner and device-camera input do not. | Open. |
| `inventory.count.adjust` | Partial | Opening count and signed deltas exist; no observed-count reconciliation against current stock. | Open. |
| `cash.day_close.steps_and_z_report` | Unsupported | Cash-session close is not end-of-day orchestration or a dynamic Z report. | Open. |
| `reporting.pos.daily.and.staff` | Partial | Daily/date-range sales report exists; staff reporting and attribution do not. | Open. |
| `checkout.pos.receipt_view_print` | Partial | Receipt view/copy exists; selected-printer printing and failure/reprint behavior do not. | Open. |
| `checkout.receipt.template` | Unsupported | No merchant receipt-layout or location-content editor exists. | Open. |
| `lightning.receive.capacity` | Partial | Advisory owner snapshot and Regtest drill distinguish pending from confirmed capacity; native checkout gating and amount-specific route/fee readiness remain absent. | Open; not production-qualified. |
| `lightning.backup.state_independence` | Unsupported | Backup-state visibility exists, but no restore/device-loss and stale-manager/monitor recovery drill exists. | Open; not production-qualified. |
| `lightning.node.availability` | Partial | Regtest Box outage falls back to on-chain and recovers, but node/network loss during payment and standalone-checkout reconciliation remain incomplete. | Open; not production-qualified. |

The local validation record is [native-pos-foundations-2026-11-17.md](native-pos-foundations-2026-11-17.md), with machine-readable evidence in `tools/merchant-acceptance-investigation/evidence/native-pos-foundations-2026-11-17.json`. All 514 linked benchmark variants remain retained in the execution ledger.
