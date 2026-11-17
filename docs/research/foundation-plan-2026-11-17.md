# OpenBMT foundation plan

**Date:** 17 November 2026

**Scope:** all 502 canonical benchmark capabilities

**Machine-readable graph:** [foundation-dependency-graph-2026-11-17.json](foundation-dependency-graph-2026-11-17.json)

**Selected IDs:** [foundation-tranche-selection-2026-11-17.json](foundation-tranche-selection-2026-11-17.json)

This plan corrects the execution order: build and integrate shared foundations before assembling a complete merchant shift. It is not a claim that the selected capabilities are complete or that every benchmark capability deserves its own subsystem.

## Validated graph

Twelve non-overlapping lanes classified all 502 capability IDs. The assembler overrides lane-local status wording with the canonical execution/full-audit status, rejects duplicate or missing IDs, resolves dependency IDs, and includes the independently reviewed tranche.

| Layer | Not implemented | Partial | Research | Verified | Total |
|---|---:|---:|---:|---:|---:|
| L0 safety/correctness invariant | 12 | 7 | 3 | 1 | 23 |
| L1 shared foundation | 59 | 23 | 10 | 6 | 98 |
| L2 core workflow | 95 | 27 | 29 | 7 | 158 |
| L3 enhancement | 82 | 13 | 14 | 0 | 109 |
| L4 vertical/provider adapter | 86 | 7 | 20 | 1 | 114 |
| **Total** | **334** | **77** | **76** | **15** | **502** |

The graph records 711 explicit capability-dependency edges after removing a cyclic backup cluster and semantically false browser-cart, Docker-recovery, local-hub, and location-switch prerequisites. These edges guide sequencing; they are architectural judgments, not proof of implementation or benchmark parity.

## First foundation tranche

The first tranche is a dependency-prioritized focus set of 40 exact capability obligations: 18 not implemented, 15 partially supported, 6 needing research, and 1 verified. It relies on a few existing or separately researched anchors outside the set rather than claiming to be a closed subgraph. The 40 obligations collapse into nine shared primitives:

| Rank | Shared primitive | Capability IDs | Immediate treatment |
|---:|---|---:|---|
| 1 | Merchant/store/location/register/device scope | 3 | Build typed identity and device assignment. |
| 2 | Authenticated actor, grants, and audit | 5 | Build one fail-closed command boundary. |
| 3 | Canonical sale and exact pricing snapshot | 6 | Preserve and integrate the existing catalog/pricing/sale kernel. |
| 4 | Provider-neutral tender allocation and completion | 5 | Unify the contract without erasing tender-specific truth. |
| 5 | Refund case, balance, and permission boundary | 3 | Separate request, approval, execution, and refundable balance. |
| 6 | Merchant-controlled on-chain receiving and observation | 5 | Bind receive authority and chain evidence to the sale contract. |
| 7 | UTXO economics, spend safety, and recovery | 5 | Advise first; automate only after explicit signing and restore gates. |
| 8 | Shop Lightning ownership, readiness, liquidity, and recovery | 5 | Keep Shop Lightning behind a replaceable, fail-closed engine contract. |
| 9 | Offline truth and local event contract | 3 | Define readiness, replay, scoped credentials, and optional outbound webhooks. |

The selection JSON lists every exact capability ID, key, status, and layer. Inclusion means the capability contributes to a shared foundation closure target; it does not mean OpenBMT supports it today.

## Preserve rather than rebuild

- `CheckoutLedger`: atomic persistence, immutable sale versions, stable payment-attempt IDs, idempotent observations, restart reconciliation, event cursors/epochs, and partial, late, excess, and reorg handling.
- `CheckoutPricing`: integer minor-unit arithmetic, deterministic rounding, and frozen line/modifier/tax/discount/tip snapshots.
- `CheckoutCatalog`: versioned product revisions and stale-revision rejection.
- `CheckoutInventory`: idempotent sale-linked stock journal, while acknowledging its current separate/unscoped boundary.
- `CheckoutCashSessionStore`: opening float, movements, expected cash, counted close, discrepancy, and restart replay; its current staff strings are labels, not authentication.
- `openbmt-core`: cryptographic device identity, signed settings/device roles, and fail-closed policy checks over already verified credential metadata.

The first engineering task is to join and enforce these primitives, not introduce a second transaction ledger.

## Immediate code slice

Implement a single-device local authority foundation before widening merchant workflows:

1. Persist a typed `merchant / store / location / register / device` scope. Keep human actor identity, cryptographic device identity, and wallet keys separate.
2. Add stable actor IDs, explicit manager bootstrap and recovery, fixed `CASHIER` and `MANAGER` grants, durable PIN throttling, and short-lived sessions bound to exact scope and policy generation.
3. Require an immutable authorized actor attribution at each enabled user mutation. A display name, typed staff label, role enum, hidden button, or client-supplied `SYSTEM` value is never authority.
4. Keep wallet signing, wallet spending, destination changes, refund execution, and verified payment evidence outside local POS staff grants. Payment/node reconciliation uses a private observer path tied to a pre-existing stable attempt.
5. Migrate cash and sale records explicitly. Preserve old labels only as unverified legacy text; never infer an actor from a matching name. Corrupt, unsupported, failed, or uncertain writes fail closed without clearing business data.

### Exit gates

- Correct and wrong PINs, durable lockout, restart, session expiry, disable/role/PIN changes, exact-scope mismatch, unknown actions, and stale policy generations are tested.
- First-manager enrollment produces an independent one-time recovery credential; recovery changes authentication state without altering sales, tenders, cash, refunds, or wallet authority.
- Direct model/controller calls cannot bypass the command boundary for migrated mutations. Unmigrated mutation paths remain unavailable rather than merely hidden.
- New financial events retain immutable actor and scope attribution. Legacy records remain explicitly unauthenticated.
- No PIN, verifier, recovery secret, or session token appears in logs, receipts, exports, exceptions, or object rendering.

This slice establishes infrastructure; it does not by itself close the five staff/scope capability obligations.

### Implementation checkpoint

The local authority model and JVM contract tests are now implemented. They include compare-and-swap persistence, prepared recovery credentials, duplicate-PIN refusal, strict scope/role/session enforcement, and non-secret login-directory projection. The Android Keystore adapter, enrollment/sign-in screens, lifecycle lock, business command wiring, actor-attributed journals, and admin event log remain open; therefore the related capability statuses are unchanged. See [the implementation record](local-authority-foundation-2026-11-17.md).

## Sequence after authority

1. Thread scope and actor context through the existing catalog, sale, tender, cash, inventory, refund, report, and event records. Close authoritative remaining balance and provider-neutral tender completion without replacing exact-money logic.
2. Complete the refund case and reservation boundary. Bind on-chain receiving authority and chain observation to the same sale/tender contract. Add UTXO cost warnings, reviewed fee authority, input reservation, and recovery evidence before automated consolidation.
3. Integrate Shop Lightning through the common tender interface, with distinct ownership, availability, amount-specific capacity, and channel-state recovery gates. An unavailable Lightning engine must not disable cash or on-chain checkout.
4. Expose the durable local event feed through scoped pull/replay and optional outbound webhooks after privacy, idempotency, retry, and offline/reconnection semantics are fixed.

## Corrections and deferrals

- `checkout.order.check.structure` is a restaurant multi-check capability, not the canonical sale kernel, despite its high raw dependency count.
- `checkout.pos.catalog_cart` concerns a browser/static/keypad POS surface, not the native transaction kernel.
- BTCPay Docker archive/restore mechanics are not foundations for the Android register. Retain restore-safety principles while prioritizing mechanism-neutral on-chain recovery and Lightning channel-state independence.
- Restaurant tables/seats/checks, enterprise configuration rollout, purchase orders, peripheral adapters, card-provider SDK/OAuth/reader details, loyalty, gift cards, stored value, subscriptions, and marketing systems remain later verticals, adapters, or separate liability/privacy systems.
- Cash and Bitcoin must not depend on an optional card provider. Staff authorization must never substitute for wallet signatures.

All 502 capabilities remain in the graph. Closing any row still requires its canonical merchant workflow, failure/recovery and authority boundaries, and the applicable benchmark-variant evidence.
