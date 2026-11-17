# Full capability audit

This audit classifies every capability in the 502-record register against the current OpenBMT product. It answers two different questions independently:

1. Does OpenBMT currently perform the bounded canonical merchant job?
2. Has OpenBMT demonstrated the exact linked vendor workflow, qualifiers and recovery cases?

The first question controls the canonical status. The second remains an open benchmark obligation unless separately demonstrated. The machine-readable record is [full-capability-audit-2026-11-17.json](full-capability-audit-2026-11-17.json).

## Result

- 15 `verified_supported`
- 77 `partially_supported`
- 334 `not_implemented`
- 76 `needs_research`
- 487 capabilities remain not verified after this audit.
- All 514 linked benchmark variants remain retained and open; this audit makes no exact vendor-parity claim.

The 495 previously open records were split across 12 parallel category lanes with exact one-time coverage. Each lane inspected product-accessible workflows, source, tests and recorded observations. Every machine-readable row records its lane assignment, and the JSON includes a 12-lane manifest whose counts total 495. The eight proposed positive closures received a separate review. That review rejected a ninth proposal, `wallet.custody.create_watch_only`, because imported public-wallet receiving does not satisfy the create-then-erase and recovered-signer transaction boundary.

`Not implemented` means the central current product workflow was absent from inspected implementation and evidence. It does not mean the capability has been rejected from the roadmap. `Needs research` is used where the evidence was insufficient to make a responsible support-or-absence conclusion, especially around provider and hardware behavior.

## Newly verified canonical capabilities

| Capability | Supported OpenBMT boundary | Benchmark boundary still open |
|---|---|---|
| `cash.register.session.reconcile` | Durable opening float, reasoned movements, cash receipts, expected-versus-counted close, discrepancy and restored history. | Shopify admin/location reporting, physical drawer qualification, roles, variance adjustment and reopening. |
| `cash_drawer.pay_in_out` | Reasoned pay-in/pay-out entries update expected cash and replay durably. | Square Cash Management eligibility, physical movement, roles and Dashboard behavior. |
| `cash_drawer.session.end` | Counted close freezes expected, actual and discrepancy; later movement is refused and history remains available. | No-count close, report email, remote/automatic end, permissions and hardware qualification. |
| `cash_drawer.session.start` | Durable drawer start with explicit opening float and restart recovery. | Square eligibility, POS/Dashboard restrictions, roles, remote-end consequences and inactivity behavior. |
| `checkout.pos.static_catalog` | Configured local items and prices can be browsed, searched, added and frozen into a sale. | BTCPay browser Static view, on-chain invoice details and production qualification. |
| `payments.cash.accept_and_calculate_change` | Remaining/change preview, underpayment refusal and durable tender/applied/change amounts; emulator observed $10.00 tendered on $9.16 with $0.84 change. | Partial cash plus Bitcoin, physical drawer verification, non-USD policy and cash refunds. |
| `pricing.menu_item.base_price` | A fixed USD item price is saved, displayed and frozen into a durable sale. | Toast publication, menu/group/location/time pricing, other currencies and production rate pricing. |
| `wallet.custody.import_public_data` | Owner scan/paste import rejects private-key material, derives a receive address and requires match confirmation before saving public derivation data. | Operator-mediated comparison and broader BTCPay format, network, selector and history behavior. |

These eight join the seven capabilities closed in the [native POS foundations audit](capability-closure-audit-2026-11-17.md).

## Status by category

| Category | Verified | Partial | Not implemented | Needs research | Total |
|---|---:|---:|---:|---:|---:|
| Backup and recovery | 0 | 1 | 9 | 0 | 10 |
| Cash closeout | 8 | 9 | 7 | 0 | 24 |
| Catalog | 1 | 6 | 13 | 0 | 20 |
| Checkout | 0 | 25 | 78 | 1 | 104 |
| Customers and loyalty | 0 | 0 | 26 | 0 | 26 |
| Hardware | 0 | 1 | 0 | 23 | 24 |
| Integrations and API | 0 | 0 | 0 | 12 | 12 |
| Inventory | 2 | 4 | 15 | 0 | 21 |
| Lightning operations | 0 | 1 | 14 | 0 | 15 |
| Multi-location | 0 | 0 | 14 | 0 | 14 |
| Offline availability | 0 | 1 | 26 | 0 | 27 |
| Onboarding | 0 | 2 | 8 | 0 | 10 |
| On-chain operations | 0 | 9 | 8 | 0 | 17 |
| Online invoices | 0 | 0 | 0 | 2 | 2 |
| Payments | 1 | 0 | 0 | 38 | 39 |
| Pricing and tax | 1 | 5 | 42 | 0 | 48 |
| Refunds and returns | 0 | 4 | 30 | 0 | 34 |
| Reporting | 1 | 5 | 8 | 0 | 14 |
| Security and privacy | 1 | 1 | 25 | 0 | 27 |
| Staff and permissions | 0 | 3 | 11 | 0 | 14 |
| **Total** | **15** | **77** | **334** | **76** | **502** |

## Evidence and interpretation limits

- A screen, source primitive, planning document or adjacent workflow alone did not close a capability.
- `Verified supported` is bounded to the product and validation environment actually observed; it does not imply production readiness.
- `Partially supported` records useful existing behavior without hiding the missing central boundary.
- Vendor-specific hardware, geography, permissions, subscription tiers, processor approval and recovery behavior remain separate obligations.
- The full JSON retains evidence paths, rationale and missing scope for each of the 495 audited records. The prior seven verified rows retain their closure evidence in the execution ledger.
