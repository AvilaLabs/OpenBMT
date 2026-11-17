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

Every row remains `pending_unverified`. Existing OpenBMT evidence IDs are candidate references only. Acceptance boundaries preserve the linked claim tests and unmet scope; a demo, simulated result, UI, documentation statement, or adjacent primitive cannot close a row by itself. All linked variants remain obligations even when grouped under one capability key. No phase assignment means implementation is complete.

Mechanism exceptions are narrow and linked to cited decisions: reject mandatory processor custody/permission dependencies where the source-backed decision identifies them, while preserving the merchant job. Other POS jobs remain in scope for evaluation; Bitcoin-specific principles do not justify wholesale feature rejection.

## Current native implementation candidates

The JSON ledger maps seven bounded current-diff feature clusters to exact capability and benchmark variant IDs: priced sale/pricing, local catalog and manual lookup, inventory, manual cash session, report/CSV, receipt projection, and advisory Shop Lightning readiness snapshot. Each entry names source paths, supported subset and unmet boundary. Every candidate is `in_progress_unverified`; scoped local tests and observed native workflows are linked in the [validation record](native-pos-foundations-2026-11-17.md), with full capability acceptance still open. These links do not change row status or imply completion/parity.

Staff authority remains absent: operator labels are unauthenticated and role permissions are not enforced. Physical devices, card provider approval, multi-location, online/restaurant workflows, and hardware qualification are untouched. Model tests or partial UI are not a complete POS acceptance result.

## Coverage audit

- Source and ledger rows: 502 / 502.
- Unique ledger IDs: 502; exact ID set match: yes.
- Linked benchmark variant references: 514; all references resolve to source claim IDs: yes.
- Statuses assigned pending/unverified: 502; completed rows: 0.
