# Local payment events and reconciliation contract

**Status: proposed contract; not implemented.** This document defines a design target for an app-first, serverless local payment interface. It does not add a production wire format, promise compatibility, or authorize network delivery. Scope is the core payment/request lifecycle and local checkout/integration callers; POS product behavior is out of scope.

The native checkout now has a narrower local journal in
[`CheckoutLedger`](../../apps/checkout-android/src/model/CheckoutLedger.kt):
explicit initialization, atomic sale/event persistence, a consistent snapshot,
and bounded cursor replay. Its events contain opaque identifiers and change
types. Its local snapshot returns full sale records and needs an appropriate
authorization boundary before any external interface is added. The owner
request-book transaction and transport described below remain proposed.

## Goals and boundaries

The local interface should let an authorized local caller create or inspect a payment resource, observe durable state changes, and recover after a missed notification or restart. It must distinguish a current-state query from an event replay. A query repairs gaps; a notification only prompts the caller to query or read events.

This is a local capability, not a public listener. It grants only explicitly scoped invoice/request creation and read/reconciliation operations. It does not grant key access, address derivation authority, transaction signing, spending, return approval, or fulfillment authority. Any future local transport must authenticate a caller and bind each capability to a book/store and permitted operations. The local API should expose no generic signer.

No HTTP service, webhook, or other network transmission is part of this proposal. Before any outbound endpoint is implemented, the network contact inventory and privacy disclosure need review for the destination, fields, permissions, and failure behavior. Default event/query data must use opaque identifiers and omit addresses, transaction IDs, xpubs, wallet descriptions, customer identity, and customer data.

## Identity and version model

Use three distinct values:

1. **Book identity** scopes a local resource collection to one merchant store/book. It is opaque and stable across ordinary process restarts and file migration. It is not a wallet fingerprint or derivation identifier. A supported restore rotates a separate history epoch/incarnation even when the book identity remains the same.
2. **Resource identity** identifies one request/invoice within a book. It is opaque and stable for that resource. Existing `RequestBook` numbers (1, 2, …) are useful internal references, but must not be assumed globally unique or treated as sufficient external identity. A public identity can be derived or mapped only after its persistence and migration behavior is specified.
3. **State version** is a monotonically increasing version of one resource's externally visible state. A successful no-op or refused mutation does not advance it. Multiple changed resources in one atomic operation each receive a version for their resulting state.

An **event journal cursor** is a separate book-scoped position in the ordered local journal. It advances once for each committed event record (or committed event batch with explicitly defined positions). It is not a timestamp and is not a resource version. Consumers persist the last fully processed cursor. Event identity is stable across rereads and replay, and distinct from both resource identity and cursor.

**Lifecycle rules:** replacing/importing a book creates a new book identity unless the operation is explicitly a verified continuation preserving the same history. A supported restore to an earlier snapshot must rotate a persisted history epoch/incarnation. Cursors and cached resource versions from another epoch are invalid and require a full resync; resource identities may be preserved only where the restored resource is demonstrably the same logical resource. Cursor values may restart only within the new epoch, and event identities must include that epoch or otherwise be guaranteed distinct across it. Copying a book to create a second independent merchant/store requires a new identity. A future migration must preserve identity and event history or explicitly declare a new epoch and require full resync; silently reusing IDs or cursor positions is prohibited. These rules need a stable store identity source and a restore operation in the owner layer; the current requests file contains neither. Arbitrary raw filesystem rollback cannot be detected from the rolled-back file alone. Therefore a supported restore must pass through an operation that rotates the epoch, or use a separately persisted monotonic anchor outside the restored snapshot. Without such a boundary/anchor, no cursor-continuity claim is safe.

## Atomicity and event meaning

For every covered successful state transition, the resulting resource state, its new state version, and the event record(s) describing that transition must be stored in one atomic envelope. This guarantees state and event records cannot be split across separate file replacements; it does not guarantee that the caller always learns whether replacement committed. A failure before replacement is a **pre-commit failure**: the old envelope remains authoritative and no new state/event is published. If replacement succeeds but the containing-directory sync or later acknowledgment fails, the result is **commit-uncertain**: the new envelope may be present and must be treated as potentially committed. Do not report definitive success, emit a definitive success notification, or retry the business action as new until readback/recovery establishes which envelope is authoritative. The transaction layer must reconcile by inspecting the persisted envelope and its transaction/operation identity. In-memory state is published only after a successful commit acknowledgment or recovery establishes the committed envelope. The event record describes what committed; it is not a second authority for reconstructing funds or signing actions.

Event types should describe evidence and policy distinctly. At minimum model separate facts for payment observed, payment reaching the configured confirmation policy, partial balance, late payment, overpayment, chain reorganization/reversal, return requested/queued, return transaction sent/observed, and return completed. A return request or queued return is not an actual payout. Reorg rollback is a new event/version, not erasure of history. Late arrival remains late even when it later confirms. Do not collapse observed funds into confirmed funds or confirmation into fulfillment.

Events are durable local facts, not delivery guarantees. The contract promises neither exactly-once consumer processing nor event arrival/notification order. A consumer deduplicates using event identity, applies only newer resource versions, and may always query current state. Replaying an event does not authorize a second charge, refund, or fulfillment action.

## Query, snapshot, and replay

The local contract needs these conceptual operations (names and encoding remain for implementation design):

- **Get resource:** return current state, resource identity, state version, and book identity.
- **List/query resources:** return a bounded page and a continuation marker for a consistent query snapshot, or state explicitly that pages may reflect concurrent changes and require reconciliation.
- **Read events after cursor:** return an ordered bounded page, its last cursor, and `has_more`. Consumers advance only after successfully processing the page.
- **Full snapshot:** return all authorized current resource state for a book plus a high-water journal cursor captured consistently. Read events after that cursor to catch up.

Cursor cases must be explicit:

- Empty cursor means a full initial sync, not “start at now.”
- A cursor below the retained journal floor is stale; return a distinct `resync_required` result and a fresh consistent snapshot/high-water cursor. Never silently skip missing events.
- A cursor beyond the current high-water mark is invalid/future and is refused without advancing it.
- A valid cursor at the current high-water mark returns an empty page. A page limit is bounded; the returned cursor is the last event actually included.
- Journal pruning may advance the floor only if stale cursors are detectable. Retention must not create a cursor gap that looks like an empty successful read.

Snapshot plus event catch-up closes the race between reading current state and subscribing: capture snapshot and high-water atomically, then replay strictly after that high-water. A client may still receive a wake-up late or more than once; the journal/query path is the repair mechanism.

## Current mutation coverage and persistence seams

The request domain already has useful facts and safeguards, but no event/version/cursor contract. `crates/openbmt-requests/src/codec.rs` encodes the entire request book as `OBM=` version 2; `fsio.rs::write_file` writes a temporary file, syncs it, renames it, and syncs the directory. `RequestBook::save` persists the whole snapshot. Its successful file replacement is atomic for that file, but there is no journal to commit with it.

| State change to cover | Current domain method(s) and owner entry points | Event distinction needed |
| --- | --- | --- |
| Request creation and repeat creation | `book.rs::RequestBook::make`; `repeat.rs::make_repeat`; owner `requests.rs::make_request`, `make_repeats` | Created resource; each repeated request gets its own stable identity/version. |
| Quote/share | `book.rs::share`; owner `share_request` | Quote terms changed; keep private note and address out of the event payload. |
| Payment credit and duplicate suppression | `book.rs::credit`; owner chain-arrival processing in `requests.rs` | Observed/credited facts, partial/late/overpayment; current credit idempotency uses `(txid, vout)` internally, but that key is not an outbound identity. |
| Request end and repeat policy | `book.rs::cancel`, `close`, `count_as_paid`; `repeat.rs::stop_repeating` | Cancelled, closed, counted paid, and series stopped are distinct state changes. |
| Return lifecycle | `returns.rs::queue_return`, `cancel_return`, `mark_returns_sent`, `mark_returns_done`, `requeue_returns`; owner `queue_return`/return processing | Return requested/queued, cancelled, submitted, completed, or requeued; never report queued as paid out. |
| Reorganization | `reorg.rs::take_back_above`; owner chain reconciliation | Reversal/reorg and resulting balances/status; append a new event, retain prior event history. |
| Pending arrivals and owner state | owner `requests.rs::edit`, `store_requests`; `Loaded` plus `REQUEST_ARRIVALS_FILE` | Inspect transaction boundary: currently the book is saved before changed pending arrivals are written, so those two files are not one atomic transaction. An event about both must not be emitted as committed until a durable shared commit exists. |

Read-only helpers (`request`, `requests`, `returns`, `repeats_due`) do not themselves produce events. Operations that change only in-memory state or metadata must be reviewed too; the table is the initial coverage set, not proof that every owner mutation has been inventoried. Cross-book settings, address allocation, and chain-observation durability belong to their owning layers and must be included when their changes affect externally visible payment state.

## Implementation seam decision

Two approaches are viable, but should not be mixed casually:

**Transaction facade (preferred first implementation seam).** Route each owner-facing command through one owner-node transaction that clones/derives the next domain state, computes resource versions and event records, serializes a single durable envelope (state plus journal and required pending-observation state), commits that envelope atomically, then swaps in-memory state. Existing `Requests::edit` is a natural interception point for request-book edits, but its current `store_requests` writes the request book and pending arrivals as separate files. It cannot be called an atomic state+event transaction until storage layout/commit recovery is redesigned. Chain watcher state and desk index allocation may require a broader owner transaction; document the exact boundary before claiming coverage.

**Mutation instrumentation.** Emitting events inside each domain mutator would touch `make`, `share`, `credit`, end operations, repeat operations, returns, and reorg logic; it risks missed paths and makes event persistence inseparable from callers unless every mutation returns a complete event set and every caller commits it atomically. Direct calls to the library can currently mutate a book without persistence. Instrumentation alone therefore does not meet the durability contract.

Do not create an unused generic event module or add a journal field only to `openbmt-requests`. First establish the owning transaction boundary and state/event envelope. Then expose the same semantics through the existing suitable local SDK/FFI surface; do not invent a new external transport as a substitute for the local contract.

## Restart and missed-notification example

1. A local checkout reads a consistent snapshot at epoch `E`, cursor 40. Request `R` has resource version 3 and is partially paid.
2. The owner observes another payment, reaches its confirmation policy, and commits the updated request (version 4) plus `PaymentConfirmed` event at cursor 41 in the same durable transaction. The process stops before the checkout's notification is delivered.
3. After restart, checkout asks for events after `(E, 40)`. It receives event `(E, 41)`, records its event identity, and updates the visible state. If it crashes after reading but before saving the cursor, it may read event 41 again; deduplication prevents a repeated business action.
4. If cursor 40 has since fallen below the retained floor, the API says `resync_required`; checkout fetches a consistent snapshot and high-water cursor, then resumes after it. It never assumes the missing event means no change.
5. If the owner restores an earlier backup, the supported restore rotates epoch to `E2`. A client presenting `(E, 41)` or cached resource version 4 receives `resync_required`, discards the prior version cache, and reads the new snapshot. If someone rolls files back outside the supported restore operation and there is no external monotonic anchor, the service cannot detect that rollback and must not claim this protection.
6. If directory sync fails after the new envelope replacement, the writer reports an uncertain outcome. On restart/readback, the transaction is resolved by the envelope's operation identity. The caller does not issue the create/return/other business action again until this resolution completes.

The example says nothing about prompt notification, network reachability, exactly-once processing, transaction identifiers in payloads, or authorization to fulfill an order.

## Finite implementation sequence and gates

1. **Inventory and transaction design — owner-node maintainers.** Enumerate every owner-visible mutation and files changed in the same operation, starting with the table and all `requests.rs::edit` callers. Specify book identity creation, supported restore boundary/epoch rotation, fork behavior, schema evolution, atomic envelope and commit-uncertainty recovery, snapshot consistency, and bounded journal retention. Resolve the current split write of book and pending-arrival files. If arbitrary rollback protection is required, identify and validate an external monotonic anchor; the envelope alone cannot provide it.
2. **Envelope and migration — request/owner persistence maintainers.** Add a versioned persisted envelope for state, identity, per-resource versions, journal floor/high-water, and events only after the transaction boundary is accepted. Preserve current request-book reads; explicitly migrate old books into a new identity/history baseline and require resync for any consumer without that history. No new production magic or compatibility claim before this is reviewed.
3. **Instrument covered transactions — owner-node maintainers.** Compute state deltas/events for all listed operations and chain/reorg paths, commit state plus events atomically, and publish memory only after commit. Refusals and failed writes must not advance versions or cursors.
4. **Local query surface — SDK/FFI maintainers.** Implement scoped capability checks, get/list, consistent snapshot, and paged replay with stale/future cursor behavior. Keep this transport local and payload-minimized. No signing/spending operation enters the capability.
5. **Failure and migration validation — coordinator.** Test restart at each commit boundary, including pre-replacement failure and replacement followed by directory-sync/acknowledgment failure; resolve uncertain outcomes through readback without duplicating the business action. Test duplicate, missing, delayed, and replayed notifications; reorg and late/partial changes; stale/future cursors; bounded pagination; supported backup restore with epoch rotation and invalidation of old cursors/resource-version caches; independent book copy; corruption and interrupted migration; and privacy field allowlists. Verify an event cannot be present without its corresponding state or vice versa within the same envelope. Separately document that unmediated raw rollback is undetectable absent an external monotonic anchor.
6. **Outbound delivery review — privacy/contact owner, only if later proposed.** Update contact inventory and privacy disclosure before any webhook/network implementation. Define merchant-selected destination, fields, auth, retry/attempt history, revocation, and offline behavior separately. Local event durability does not imply outbound delivery approval.

Until these gates land, existing request snapshots and signed/internal records remain useful internal mechanisms, but must not be described as the proposed durable merchant event API or as cursor-based reconciliation.
