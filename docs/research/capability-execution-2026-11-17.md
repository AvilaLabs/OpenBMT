# Capability execution ledger

This is the execution companion to the [502-record capability register](capabilities-2026-11-17/capability-register.json). It sequences implementation and evidence work; it adds no competitor research and makes no parity claim. The original register and historical evidence corpus are unchanged.

See [capability-execution-2026-11-17.json](capability-execution-2026-11-17.json) for one row per retained capability ID, including linked benchmark variants, workstream, phase, substream, deliverable kind, dependency gates, capability-specific acceptance target, evidence boundary, candidate evidence, disposition and decision references.

## Sequence and parallel tracks

- **Shared gates:** review the canonical durable sale/pricing/tender/recovery contract and the authority/recovery boundaries. These are prerequisites, not completed capabilities.
- **P1 — Core standalone sale:** begin with the native durable priced-sale snapshot, then develop catalog, basket, tax/discount/tip, tender and stock work.
- **P2 — POS operations:** staff, cash, customer operations, reporting and closeout can build once the minimal sale/tender contract exists; they do not wait for the Bitcoin workstream.
- **P3 — Bitcoin safety, readiness and recovery:** run alongside P1/P2 after both shared gates. The roadmap permits general POS and Bitcoin-mode work to proceed in parallel after shared contracts. Bitcoin safety gates still apply to every release stage.
- **P4 — Optional provider and hardware:** qualify replaceable integrations independently; this cannot become a prerequisite for cash or merchant-authorized Bitcoin operation.
- **P5 — Later verticals, online and multi-location:** begin after a validated standalone shift. Their jobs remain in the ledger for workflow-specific work.

Rows include concrete substreams such as catalog CRUD, barcode scanning, modifiers, tax/discount/tip calculation, stock count/procurement/transfer, cash closeout, backup/restore, Lightning capacity/exit, and on-chain fee/UTXO safety. Dependencies reference shared gates and, where a prerequisite is clear, exact source capability keys and capability IDs. Cross-cutting safety gates apply to every phase. Decision references are an overlay on execution work, not a reason to defer the entire Bitcoin track.

Capability completion and exact benchmark parity are separate axes. A row is `verified_supported` only when OpenBMT exposes the canonical merchant job and meaningful source, test or observed-workflow evidence validates it. Vendor-specific mechanisms and qualifiers remain separate benchmark obligations until demonstrated. A demo, simulated result, UI, documentation statement or adjacent primitive cannot close a row by itself. No phase assignment means implementation is complete. See the [first closure audit](capability-closure-audit-2026-11-17.md) and the [full 502-capability audit](full-capability-audit-2026-11-17.md).

Mechanism exceptions are narrow and linked to cited decisions: reject mandatory processor custody/permission dependencies where the source-backed decision identifies them, while preserving the merchant job. Other POS jobs remain in scope for evaluation; Bitcoin-specific principles do not justify wholesale feature rejection.

## Current implementation standing

The JSON ledger maps seven bounded feature clusters to exact capability and benchmark variant IDs: priced sale/pricing, local catalog and manual lookup, inventory, manual cash session, report/CSV, receipt projection, and advisory Shop Lightning readiness snapshot. Each entry names source paths, supported subset and unmet boundary. Fifteen individual canonical capabilities are now `verified_supported`; 77 are partially supported, 334 are not implemented and 76 need deeper research. Their exact Square, Toast, BTCPay and other benchmark variants remain open and do not imply parity.

Staff authority remains absent: operator labels are unauthenticated and role permissions are not enforced. Physical devices, card provider approval, multi-location, online/restaurant workflows, and hardware qualification are untouched. Model tests or partial UI are not a complete POS acceptance result.

## Coverage audit

- Source and ledger rows: 502 / 502.
- Unique ledger IDs: 502; exact ID set match: yes.
- Linked benchmark variant references: 514; all references resolve to source claim IDs: yes.
- Canonical OpenBMT capability status: 15 `verified_supported`; 77 `partially_supported`; 334 `not_implemented`; 76 `needs_research`.
- Full-register audit: all 495 previously open rows covered once across 12 parallel lanes; 8 newly verified.
- Remaining not verified: 487.
- All 514 linked benchmark variants remain retained and open/unverified.
