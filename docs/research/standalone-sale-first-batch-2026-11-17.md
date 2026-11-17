# Standalone sale: first implementation batch

17 November 2026. Builds on the [capability research](capabilities-2026-11-17/README.md) and [sale contract](standalone-sale-contract-2026-11-17.md). This is a bounded prototype improvement, not POS parity or a production payment release.

## What changed

The existing browser walkthrough now connects an editable two-product basket to a payment choice, receipt and order history. Quantities and totals use integer minor units. Beginning payment freezes the basket. Bitcoin and card outcomes are explicitly simulated; cash records a simulated amount received and change. A canceled order remains an order detail rather than a paid receipt. An unknown card result blocks another collection route and resumes the same attempt when returning to checkout. Refund requests retain one identity and do not claim money was returned.

The browser uses a small immutable model, `demo/web2/sale-contract.js`. It is an illustrative model, not the Android ledger, a shared payment engine or durable storage. Leaving and returning to the checkout scenario in the same page session retains its interrupted state; reloading the page does not. The original walkthrough and drawing layer remain the presentation framework.

Android checkout now initializes its existing local event journal during startup. Existing sales are seeded at version zero without clearing their records. New changes append to that journal. Initialization failures follow the ledger's uncertain-write rule: stop using the stale instance, reopen and inspect the saved sales. The journal remains private local data; this batch adds neither an export endpoint nor remote webhooks.

## Acceptance results and remaining targets

The machine-readable [evidence record](../../tools/merchant-acceptance-investigation/evidence/standalone-sale-first-batch-2026-11-17.json) records commands, observations and hashes. The contract's eight scenarios have different levels of coverage:

| Contract scenario | Evidence in this batch | Limit |
|---|---|---|
| Exact basket and recovery | Browser immutable quantity/amount/snapshot tests; existing native ledger/migration harness | Browser state is session-only; this does not connect its basket to Android storage |
| Partial Bitcoin and completion | Existing native ledger harness remains passing | No new real Bitcoin payment or full operator journey demonstrated |
| Unknown card and late completion | Browser unknown-status lock, same-attempt reconciliation and interrupted-scenario recovery; native ledger harness | Browser provider outcomes are simulations; production card eligibility remains open |
| Late Bitcoin after cancellation | Existing native ledger harness remains passing; browser canceled detail stays distinct from receipt | No new end-to-end late-payment operator workflow |
| Cash and change | Browser underpayment/change/receipt/history; Android synthetic cash sale and restart | No cash drawer, closeout or cash refunds |
| Refund boundary | Browser one-time linked request and no payout claim; native refund/evidence harness | Browser request is not a provider refund or an owner approval workflow |
| Fulfillment/inventory idempotency | Target only | No durable fulfillment or inventory engine added |
| Uncertain durable write | JVM initialization faults before/after commit, reopen and retry; legacy migration preservation | Browser has no durable write path |

Desktop and 390-pixel phone browser checks cover the connected checkout with no page errors. Android was updated in place on an API 35 emulator; the three pre-existing synthetic sales were preserved, the journal was initialized, and a new synthetic cash sale and its event versions survived a force-stop and reopen. No real funds were collected or sent.

## Dependency maintenance repair

The previously failing advisory job identified Chrono as unmaintained (RUSTSEC-2026-0543). This was a maintenance warning, not evidence of an exploit in OpenBMT. Rather than suppressing the warning, the pinned `lightning-liquidity` helper now has a focused, documented datetime-backend patch. The main LDK release remains pinned. The helper's published source, checksum, upstream revision, licenses and exact patch are retained in [its provenance record](../../vendor/lightning-liquidity/UPSTREAM.md).

Public regression tests check the existing LSPS date wire formats, UTC spellings, fractional widths, offsets, separators, permissive leap-second behavior, epoch boundary and whole-second policy. Existing direct timestamp consumers use an accessor. Vendored third-party code remains visible in dependency inventories and license checks. Both lockfiles pass the advisory gate with warnings denied and no ignored advisories. Maintaining this small fork is an explicit ongoing responsibility until an appropriate upstream replacement can be adopted.

## What follows

The next product work should connect a durable priced sale to the native checkout, then add the missing total components, catalog administration, fulfillment and inventory actions under the contract's recovery rules. Staff authority, shift closeout, receipts and hardware need their own acceptance work. The capability corpus remains the backlog and evidence index; this batch does not change its historical baseline into a parity claim.
