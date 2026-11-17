# Standalone sale contract: first acceptance batch

Planning contract dated 17 November 2026. This is a bounded acceptance target for the existing native checkout/demo path. It does not assert a shared POS engine, completed end-to-end workflow, production card approval, or release readiness. Native behavior below is based on `CheckoutLedger.kt` and the standalone POS delivery plan; target requirements are deliberately separated from what is present.

## Implemented baseline

The native durable ledger currently stores a sale total and currency, optional copied `SaleItem` snapshots (`productId`, name, unit minor amount, quantity), payment attempts, cash payments, refunds, and local event history that the checkout now initializes at startup. It uses integer `Long` amounts for minor currency units and satoshis, checked multiplication/sums in key paths, and a `BigInteger` conversion for Bitcoin received value. Bitcoin attempts retain a quote, expected sats, counter index, and stable attempt ID. Card attempts persist provider and stable idempotency ID before an external SDK call; pending, approved, or unresolved cancellation states block a retry. Evidence is append/replace-by-evidence-ID and payment state is derived. Cash records tendered, applied amount, and change for USD. Refunds are associated with the original card or Bitcoin attempt and distinguish requested/pending from evidenced paid amounts. Sale view derives remaining balance, overpayment, late Bitcoin receipt, and review-required status.

This baseline does not include a full price/tax/discount/modifier/tip model, a canonical fulfillment record, inventory reservation or stock movements, cash refunds, role-based staff permissions, or a complete recovery workflow across every supported sale/tender. The sale's `paid` flag is a payment-ledger derivation, not durable proof that goods were handed over. Existing payment/event persistence and scoped harness checks do not establish these missing workflows.

## Target sale record

Before tender starts, freeze one durable sale snapshot containing:

- Sale ID, creation time, currency, and immutable line snapshots: product identity/description, quantity, unit amount, selected modifiers, and line extensions.
- Explicit integer minor-unit subtotal, discounts, tax components, tip, and final total. Specify each rounding rule at the calculation boundary; reject overflow and invalid negative/zero quantities. No binary floating-point amount may enter persisted sale or tender state.
- Stable IDs for sale, tender attempt, evidence observation, refund request, inventory reservation/movement, and fulfillment action. Repeating a command with the same key and same payload returns its prior result; key reuse with different payload is an error.
- A versioned immutable snapshot. A post-freeze change creates an auditable amendment or a new sale; it never rewrites the amount behind an in-flight tender. The current `SaleItem` snapshot is useful groundwork but does not yet meet this expanded contract.

For a first batch connected to the existing demo, retain its supported amount/currency path and prove exact integer basket totals and snapshot recovery. Tax, discounts, modifiers, and tips remain explicit target fields until implemented; do not manufacture them as current functionality.

## Tender and sale state rules

A sale owns one authoritative total and a derived remaining balance. For each tender, retain the requested amount, stable attempt ID, tender-specific external reference, immutable observations, and derived state. Distinguish at least awaiting, unknown, unconfirmed, partial, settled, canceled, reorged, late, excess, and review-required conditions. State transitions must be deterministic under duplicate, stale, and out-of-order observations.

- Each new attempt is bounded by the current unpaid balance and committed durably before invoking a receiver or payment SDK. The stable ID survives restart and is the reconciliation key.
- Unknown card outcome is not a decline. Reconcile against the same provider ID before retrying, switching tender, or creating a second collection. Pending/approved/cancellation-unknown remain blocking. A confirmed failure/cancel may clear the block; a late completion reopens reconciliation and must not cause another charge.
- Bitcoin observations keep source/output identity, amount in sats, confirmation threshold, chain/reorg state, and observation time. Unconfirmed receipt is not settlement. A late receipt after cancellation and an excess receipt require review and reconciliation; neither silently completes fulfillment nor starts a new spend.
- Partial tender updates the exact remaining minor-unit balance. Do not round remaining balances or treat displayed fiat conversion as received Bitcoin. Multiple tenders must sum without overflow, and tender allocations cannot exceed the amount requested for that attempt.
- Cash acknowledgment means an operator recorded cash, not that a drawer or cash movement was independently verified. Record amount applied and change due/returned distinctly. The currently supported cash path is USD and exact-balance-or-over-tender; cash refunds and drawer accounting are outside this batch.
- `paid` or settled payment state alone must not imply handoff. Payment evidence and fulfillment authorization are separate facts.

## Fulfillment, inventory, refunds, and authority

Fulfillment must be represented separately from tender: not-ready, ready, fulfilled, canceled, and review-required are sufficient initial outcomes. A readiness decision identifies its sale version and policy result. A durable idempotency key on the fulfillment action prevents duplicate handoff/inventory application after retries or restart. Duplicate evidence or repeated action cannot fulfill twice. Partial, unconfirmed, unknown, late, excess, reorged, or otherwise review-required payment cannot move a sale to ready without an explicit supported policy and recorded owner-authorized resolution.

Inventory is a future target, not a present native capability. Where connected, reserve/decrement and release/return movements must reference the frozen sale line/version and use durable movement IDs so retry cannot double-decrement or double-restore stock. Do not block basic payment acceptance on inventory availability unless a configured inventory policy says so; surface unavailable/stale stock for operator review.

Refunds are obligations/actions associated with the original sale and original settled tender. Persist request amount, currency/asset units, requester/approver authority, idempotency key, pending status, and independently observed amount actually returned. Cumulative requests and payments cannot exceed evidenced settled value, with Bitcoin amounts retained in sats. A request or pending response is not a paid refund. Refund recovery retries/reconciles the same refund identity; it never becomes a merchant payout, a new wallet-spend permission, or a general-purpose withdrawal. Current ledger refund support is card/Bitcoin only; this batch does not add cash refunds.

Authority remains separated: cashier checkout can create a sale and initiate an allowed tender; status/reporting can read; refund request cannot itself approve or execute an owner-restricted action; inventory action cannot sign; owner settings govern wallet/destination/signing changes; actual spending remains under existing wallet authority. No checkout role or recovery path receives broader wallet authority. Local recovery reloads the same durable sale, stable attempt IDs, evidence, and unresolved status. If persistence reports an uncertain write, stop use of stale in-memory state, reopen/reload, and reconcile by stable IDs before any external retry. Backup/restart recovery is not permission to repeat collection or create a new spend.

## First-batch acceptance scenarios

These are proposed bounded checks against the connected native demo/ledger harness. They specify acceptance targets, not evidence that each behavior is already implemented. They do not require a new provider, inventory backend, printer, or shared engine.

1. **Frozen exact basket and restart:** Create a two-line sale with integer minor-unit prices and quantities. Reject overflow or a mismatched total. Attempt tender, restart/reopen storage, and confirm the same line names, quantities, unit values, total, and remaining balance are restored unchanged.
2. **Partial Bitcoin then completion:** Record an underpaid output, confirm the UI/state shows exact remaining balance and does not fulfill; record a second valid output and threshold evidence. Duplicate the same evidence and reopen the ledger. Total received and readiness remain unchanged by the duplicate and match the evidence.
3. **Unknown card and late completion:** Persist one card attempt before the demo SDK call, simulate a timeout/unknown result, and show retry/new tender is blocked. Reconcile using the same provider stable ID; only a definitive failed/canceled outcome permits a new attempt. If completion arrives late, record it against that attempt and reconcile balance without a second charge.
4. **Late Bitcoin after cancel:** Acknowledge cancellation, then record a receipt for that existing attempt. Show late/review-required state and preserve the exact receipt; do not mark the sale ready, create an automatic refund/payout, or grant a new wallet spend. Replaying the observation does not duplicate funds or actions.
5. **Over-tender cash:** On a USD sale, record cash tender greater than the remaining amount. Show applied amount and change separately; restart and verify neither amount is duplicated. Attempting cash against a non-USD sale remains rejected.
6. **Refund recovery boundary:** For an evidenced settled card or Bitcoin attempt, create a bounded partial refund request. A pending observation must show zero paid; a duplicate paid observation cannot inflate cumulative returned value. Restart preserves the same refund and evidence. Cash refund remains unavailable, and no refund path exposes a payout or new wallet-spend capability.
7. **Fulfillment/inventory idempotency seam:** Against the demo's available action seam or a deterministic fixture, submit the same fulfillment action twice and after restart. At most one fulfillment/inventory movement is recorded. Unknown, partial, unconfirmed, late, or review-required sale state cannot fulfill; if the existing demo has no fulfillment/inventory seam, record this scenario as an unmet target rather than claiming a pass.
8. **Uncertain durable write:** Use the existing storage fault seam, if available, to fail an atomic write with uncertain outcome. The caller must reload/reopen and reconcile the same stable IDs before retry; it must not continue from stale state or invoke a duplicate collection.

## Exit and scope boundary

First-batch completion means deterministic sale/tender/refund state and restart behavior are demonstrated for the scenarios that the current connected native path can support, with unsupported fulfillment/inventory/authority seams called out as unmet backlog. It does not mean complete inventory operations, staffed permissions, POS parity, cash closeout, card production eligibility, mainnet readiness, real-funds approval, or merchant-shift readiness. Report each result as implemented-and-observed, target-only, or unsupported in the acceptance evidence.
