# Synthetic merchant acceptance money model

This is a deterministic, standard-library Python feasibility model. It is not
connected to a wallet implementation, node, provider, build, network, or funds.
It compares these paths using the same synthetic sale stream and, for Lightning
policies, the same explicitly external confirmed inbound capacity:

The cost of acquiring that capacity, tied capital, startup, routing and channel
closures are outside this model. Results compare operating flows after setup;
they cannot establish an all-in price or a commercially preferred payment path.

| Policy | Receipt and export behavior |
| --- | --- |
| `onchain` | Every sale creates one cold-wallet output equal to the full sale. A payer-side receive fee is excluded; merchant receive fee is zero by assumption. |
| `hold` | Lightning receipts accumulate in the hot balance. No cold export is attempted. |
| `splice` | When unreserved hot value reaches the threshold, a spliceout exports that aggregate. It reduces local hot value and total channel capacity; inbound capacity is not replenished. |
| `swapbatch` | When unreserved hot value reaches the threshold, a swapout exports that aggregate. At confirmation it restores inbound capacity by the gross exported amount. |

Run in this directory:

```sh
python3 model.py --scenario small_tickets --policy swapbatch
python3 model.py --all
python3 -m unittest -v
```

JSON output includes all replayable inputs (scenario parameters and exact
sales), summary metrics, and chronological event traces. Only paid sales count
in `recognized_revenue`; refused sales are unpaid. A pending action is left
pending if its due tick is beyond the scenario event horizon. It does not
silently finish. The `signer_offline` scenario intentionally does not block
hot-key swapouts or receipt to cold outputs: cold-wallet consolidation is
optional behavior and is excluded from this model.
Receiving to a cold address needs no spend signature. Signer availability
constrains spending or merging the existing outputs later.

## State, value, and constraints

All values and costs are integer satoshis; time is discrete integer ticks.
For Lightning policies, `I` is remote confirmed inbound capacity, `L` is local
hot value, and `C = I + L` is channel capacity. `I` is explicitly external
capacity, not merchant-owned capital. Receiving `x` moves `x` from `I` to `L`.
An ordinary outgoing Lightning payment moves `x` from `L` to `I`. A spliceout
of `x` decreases `L` and `C` by `x` and leaves `I` unchanged. A swapout moves
`x` from `L` to `I` while leaving `C` unchanged. Capacity identity, hot cap,
nonnegative balances, pending fee reservation, and capital conservation are
asserted throughout the replay.

An export's gross `x` stays in hot balance and is reserved while the action is
pending. The default allows one pending export; `max_pending_exports` enables
an explicitly bounded number of concurrent exports. A new export threshold
uses unreserved hot balance only, and cumulative fee budget includes every
pending reservation. Reserved amounts cannot fund ordinary outgoing payments
and do not restore inbound capacity before their own confirmations. Fees are budget-reserved
at action start, then deducted exactly once from the output on confirmation;
no cold-signature or cold-fee debit is assumed. A fee above the configured
per-action limit, cumulative fee budget, minimum cold output, or future-spend stress
condition blocks the export. The model records fees paid, cold output values
and count, per-output estimated future input fee liability, and uneconomic
cold output count. These future input costs are explicit stress assumptions,
not market quotes or invoices.

Cold and hot balances are merchant-owned assets. Every paid sale adds its
principal; outgoing Lightning payments and completed export fees reduce owned
assets. The asserted accounting identity is:

```text
ending cold + ending hot - initial cold
  = recognized sale principal - paid Lightning withdrawals - paid export fees
```

Cold output count is tracked as a simple output count, not a full UTXO set:
there is no coin selection, input consolidation, change output, or script-size
model. On-chain sales create one modeled output apiece. The output stress metric
is `output_count * future_input_fee_stress`; it is not charged as an actual fee.

## Scenarios and limitations

Built-in scenarios cover small tickets, high hypothetical fees, offline cold
signer, delayed export, swap service outage, a small hot cap with an uneconomic
export, batched outgoing Lightning payments, and 1,000 low-ticket sales with
slow serial settlement versus a four-export bounded pipeline. Both long streams
use the same initial inbound and hot capacities; the pipeline uses a 50,000-sat
threshold and explicitly permits four concurrent pending exports. This is a
synthetic concurrency assumption, not a claim about a specific wallet or
provider. Fixed-plus-ppm action fee is
`fixed_sats + ceil(amount * ppm / 1,000,000)`. All such values are invented
scenario inputs, not fee estimates or market/provider quotes. Runtime
validation rejects negative configuration values, invalid sale ticks/amounts,
zero hot caps, invalid batching intervals, and unknown policies.

This is not an actual-node or regtest result, production forecast, or claim
about a provider. It excludes JIT lease selection/terms, ambiguous or failed
payment attempts, HTLC routing constraints, channel and transaction behavior,
confirmation distributions, UTXO selection/consolidation, retries, invoice
lifecycle details, BTC price movement, taxes/accounting treatment, and cold
wallet operations. Synthetic availability and action delay are deterministic
flags/ticks, not reliability distributions.
