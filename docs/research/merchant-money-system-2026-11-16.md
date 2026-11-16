# OpenBMT merchant money system investigation

Date: 2026-11-16. Status: architecture research and isolated experiments; the merchant application has not adopted this design.

The wallet authority choices are now accepted for planning in the
[merchant product plan](merchant-product-plan-2026-11-16.md): a local hot start,
independently created cold savings, direct-to-cold receiving, and an explicitly
enabled separate working wallet. Automatic signing remains implementation and
validation work. The [feature benchmark](merchant-feature-benchmark-2026-11-16.md)
extends the backlog with merchant APIs, webhooks, and business workflows.

The strongest direction to test is one merchant-controlled system that plans Lightning receiving capacity, online balances, settlement transactions, and on-chain spending costs together. Routine sales should not require the merchant to understand channels or UTXOs. The controller must expose an actual shortage or an unsatisfied security policy rather than conceal it behind a combined balance.

Two constructions deserve further work: acquiring peer-funded channels with the purchase committed in the funding transaction, and combining eligible swap claims into a single payment to the merchant's savings wallet. A limited pre-authorization from an offline signer may also let a settlement consume one existing savings output without obtaining another signature at settlement time. These mechanisms can reduce trust or maintenance; they do not make liquidity, chain space, or recovery free.

This investigation extends [the merchant acceptance investigation](merchant-acceptance-architecture-2026-11-16.md). Current application behavior is unchanged. It does not relax the Shop Lightning backup, offline reserve-key, or real-funds restrictions.

## Merchant experience to evaluate

During setup, the merchant selects or creates their wallet, verifies their savings destination, approves an online-funds limit and operating-fee budget, and establishes a recoverable Lightning node. OpenBMT prepares confirmed receiving capacity before advertising readiness. Merchant-owned infrastructure that already exists should be connectable; installing another box is a deployment option, not a solved universal onboarding requirement.

During a sale, the merchant uses one checkout. OpenBMT offers payment methods that can actually complete under the approved policy, reserves the necessary resources, and recognizes payment from verified evidence. A merchant-facing readiness estimate describes the available service; it cannot guarantee a route from every possible payer.

Between sales, the controller schedules authorized outgoing Lightning payments, liquidity acquisition, swaps, and settlement batches. Advanced users can select peers and services, specify their own infrastructure, or approve actions individually. Ordinary onboarding should not require typing node IDs, channel sizes, or peer addresses.

When cost, availability, signing authority, or recovery requirements cannot be met, the application explains the specific limitation. Automatically falling back to small on-chain receipts would defeat the economic protection. A fallback needs its own eligible payment method and merchant policy.

## One controller with distinct asset states

A single event journal should drive separate, reconcilable records for:

- Confirmed, usable receiving capacity per channel, reservations, and pending changes.
- Local Lightning funds, locked payments, and other online funds exposed during settlement.
- On-chain outputs, their scripts, confirmation state, signing requirements, and estimated spending costs.
- Savings destinations and any authorized, unconsumed settlement signatures.
- Quoted, reserved, and spent fee budgets; service and peer availability.

An action needs an immutable identity, exact inputs and destinations, maximum cost, eligibility conditions, deadline, durable state, and a restart reconciliation path. A callback cannot stand in for chain verification or durable channel state. Pending swap funds remain merchant exposure; scheduling an action does not immediately restore receiving capacity.

Let `I` be usable remote channel balance and `L` the merchant's local channel balance in a simplified channel with no reserves or HTLCs. Then:

| Operation | Receiving capacity I | Local balance L | Channel capacity I plus L |
| --- | --- | --- | --- |
| Receive x | Decreases by x | Increases by x | Unchanged |
| Pay an external Lightning invoice x | Increases by x | Decreases by x | Unchanged |
| Complete a Lightning to on-chain swap x | Increases by x | Decreases by x | Unchanged |
| Splice out the merchant's x | Unchanged | Decreases by x | Decreases by x |
| Merchant funds a splice in x | Unchanged | Increases by x | Increases by x |
| Peer contributes x | Increases by x | Unchanged | Increases by x |

Actual fees, reserves, HTLC limits, route availability, and confirmation requirements narrow the simplified amounts. A circular rebalance between the merchant's channels changes where liquidity is located; it does not create aggregate receiving capacity. The underlying funding and splice contributions are described in [BOLT 2](https://github.com/lightning/bolts/blob/master/02-peer-protocol.md).

## Acquire capacity with stronger delivery guarantees

Current LSPS1 purchases require payment before delivery, so a dishonest seller can fail to deliver the promised channel. The protocol states this explicitly in its [trust model](https://github.com/lightning/blips/blob/master/blip-0051.md#trust-model). Automation does not remove that exposure.

Core Lightning documents a dual-funded lease request using `request_amt` and `compact_lease`; the latter checks the expected terms and refuses opening when they differ. Its funder plugin can advertise lease terms and contribute funds during channel establishment. This makes the joint funding and initial balance agreement a more promising direction than a separate prepaid delivery promise. The documented feature requires experimental dual funding, configured seller funds, and a suitable peer. Contract terms do not guarantee that the peer will remain online or route payments. See [fundchannel](https://docs.corelightning.org/reference/fundchannel) and [funderupdate](https://docs.corelightning.org/reference/funderupdate).

Lightning Pool provides a different construction: batched channel purchases through an auctioneer. Pool account funds can be spent jointly before expiry or unilaterally after expiry. That is a conditional exit with a waiting period, not immediate independent withdrawal. Account-data loss is a separate issue: the documented recovery tool currently uses auctioneer cooperation. See [batch execution](https://docs.lightning.engineering/lightning-network-tools/pool/batch_execution), [account scripts](https://docs.lightning.engineering/lightning-network-tools/pool/accounts), and [account recovery](https://docs.lightning.engineering/lightning-network-tools/pool/account_recovery).

These are backend candidates. Pool uses LND; OpenBMT currently embeds direct LDK 0.2.8. Core Lightning's lease path would also require a different backend or substantial implementation work. Neither source establishes current market depth, reliable matching, or a choice of independent sellers. Those must be measured before a product relies on automatic purchasing.

The first acceptance test is a local purchase with exact terms, verified funding and commitment state, then recovery with the seller or auctioneer absent. Following that, independent sellers must actually offer viable terms. A service whose advertised exit exists only while its operator cooperates does not meet the intended recovery requirement.

## Export earnings in economic settlement batches

Lightning receipts can accumulate in channels without creating a merchant on-chain output per sale. An authorized external Lightning payment can restore receiving capacity directly. When the merchant wants on-chain savings, a larger noncustodial swap can restore capacity while transferring accumulated earnings to their wallet. The swap still incurs service, routing, and chain costs.

The controller should first use genuine outgoing payments the merchant has authorized; it must not manufacture purchases to improve liquidity. It can compare a larger swap, additional peer-funded capacity, or a deferred withdrawal against the hot limit, available headroom, and fee budget. Sending earnings out through a splice reduces online funds but does not replenish receiving capacity.

With a chosen minimum savings output `M`, a gross withdrawal `G` and all-in withdrawal fee `F(G)`, a necessary condition is `G - F(G) >= M`. The online-funds cap must permit that gross amount while leaving headroom for payments that arrive before the action completes. A cap too small to form the permitted withdrawal is an incompatible configuration, not something a scheduler can solve. Neither low future fees nor a quiet sales period can be assumed.

Each new savings output also has a future spending cost. The controller must report actual fees separately from estimated future liabilities, using the actual output type. Combining receipts in a transaction costs fees now and can reveal relationships between inputs. It should not blindly collapse all wallets into one output.

## Combine unilateral swap claims

OpenBMT currently creates a single-input, single-output script-path claim for a reverse swap. Its claim key and preimage belong to the merchant; the provider's private key is unnecessary. See [SW-40 to SW-46](../SWAP-RULES.md#5-the-claim).

A batch can instead fix the complete input set and destination, then sign each eligible claim input against that final transaction. Multiple claims can pay one savings output. Optional merchant inputs require their own valid signatures. Default Taproot signatures commit to the full transaction, so previously signed single claims cannot simply be concatenated. See [BIP 341](https://github.com/bitcoin/bips/blob/master/bip-0341.mediawiki#common-signature-message).

The batcher must reserve inputs, verify funding, respect the earliest claim deadline, persist recovery data before revealing secrets, and support rebroadcast and fee replacement. A batch with one urgent claim may need to settle earlier than an economic scheduler would prefer. Batching claims does not eliminate their original funding transactions. One larger node-funded swap can therefore be preferable to a reverse swap for each customer sale.

The isolated [batch claim probe](../../tools/merchant-acceptance-investigation/batch-claim-probe/README.md) tests transaction validity for fixed synthetic swap trees. It does not establish provider integration, production eligibility, crash recovery, or payment finality policy.

Bitcoin Core 32.1 accepted and confirmed the two-claim transaction on a fresh local regtest chain: 200,000 input sats, one 199,000-sat savings output, and a 1,000-sat test fee at 249 virtual bytes. Core rejected an altered claim signature. The probe also refused malformed funding data and the wrong funding script. These synthetic fee amounts were selected for the test, not quoted by a service. [Execution evidence](../../tools/merchant-acceptance-investigation/evidence/settlement-batching-2026-11-16.json)

## Limited offline authorization for a future savings increase

An offline signer can potentially authorize a specific known savings input together with a fixed output back to a verified merchant savings address. With Taproot `SIGHASH_SINGLE | SIGHASH_ANYONECANPAY`, that signature commits to the signed input and its paired output while allowing other inputs to be added later. The paired output must exceed the signed input's value by a chosen increment. The online system must supply that increment and the fee from the added funds; it cannot decrease or redirect the paired output using that signature. This follows the [BIP 341 signature commitments](https://github.com/bitcoin/bips/blob/master/bip-0341.mediawiki#common-signature-message).

For example, the offline signer could authorize a known 300,000-sat savings input to pay a fixed 490,000-sat savings output. Later, two 100,000-sat swap claims supply the additional value and a 10,000-sat test fee. The claim keys sign the completed transaction; the cold-input signature remains unchanged. This consumes an existing savings output while replacing it with a larger one.

The authorization is finite. It covers one existing outpoint. After that outpoint is spent, the signature cannot authorize its successor, and a chain of future unknown outpoints cannot be assumed. Preparing several authorizations needs several existing inputs; creating small inputs just to stock authorizations could negate the benefit. Fixed output amounts also restrict withdrawal sizing.

The signature does not cap fees taken from additional inputs, and a holder can trigger the authorized move whenever it can supply sufficient extra value. The application still needs fee policy, input reservation, recovery, and revocation by spending the original input. A compromised host could misuse separately controlled online funds even though it cannot reduce the authorized savings output.

Hardware-wallet support and OpenBMT signer policy are separate gates. The isolated fixture does not put a real cold key on an offline device or prove a signing product supports this authorization. This construction is an optional maintenance tool to evaluate, not a reason to weaken the existing reserve-key policy or promise perpetual unattended cold-wallet management.

The second Core regtest transaction confirmed the illustrated 300,000 plus 100,000 plus 100,000 input construction with one 490,000-sat output. The original cold-input signature was unchanged after the claim inputs were appended. It had been prepared before those claim outpoints existed. Local checks rejected a decreased or redirected paired output under that signature. The transaction was 307 virtual bytes with a deliberately chosen 10,000-sat test fee. The fixture's assembly function does not use the cold-input secret, while the test program as a whole contains public fixture secrets. [Execution evidence](../../tools/merchant-acceptance-investigation/evidence/settlement-batching-2026-11-16.json)

## Ordinary on-chain payments through a working wallet

On-chain acceptance does not require the merchant to begin with Bitcoin or
to purchase receiving capacity. A standard customer payment can fund an empty
merchant wallet. The merchant can then pay a consolidation transaction's fee
from those receipts. The relevant design choice is who can sign the received
outputs while the merchant is busy serving customers.

A merchant-owned online working wallet can accept ordinary address payments
and periodically combine confirmed outputs into larger payments to offline
savings. The savings key need not be online to receive that payment. Customers
need no OpenBMT app, Payjoin support, channel, or swap service for this flow.
This controls the backlog by spending outputs after receipt; it cannot prevent
the initial creation of one or more outputs in each customer's transaction.

This would be a new receiving policy, not a silent expansion of the existing
[refund float](../FLOAT-RULES.md). That float is purpose-limited, does not
provide sale facts, and keeps savings out of its payment path. Reusing its
key-separation and wallet primitives is distinct from authorizing customer
receipts or unattended savings transfers.

The working-wallet proposal needs merchant authorization, independent savings
destination verification, durable input reservations, confirmation/reorg
handling, fee budgets, and recoverable settlement transactions. The controller
can stop issuing working-wallet addresses and route new eligible payments to
cold storage, but late or unsolicited payments to previously issued addresses
can still arrive. An online-balance cap is consequently an operational target;
it cannot impose a Bitcoin consensus rule that rejects excess receipts.
A compromised working key can also divert ongoing receipts it can spend, so a
balance cap is not a lifetime theft limit. Savings keys remain separate.

### Ordinary payment experiment

The isolated [on-chain batch probe](../../tools/merchant-acceptance-investigation/onchain-batch-probe/README.md)
uses Bitcoin Core 32.1 on a fresh local regtest chain. Both merchant wallets
start empty. A payer sends 100 separate ordinary transactions to 100 distinct
merchant P2WPKH addresses, 10,000 sats each. All receipts confirm before the
merchant constructs its batch. The savings address is imported into a wallet
with private keys disabled; it is a public synthetic fixture, not evidence of
physical cold-key custody.

The merchant signs one transaction consuming all 100 outputs and paying one
savings output. Three independently signed alternatives passed Core's mempool
checks; only the 1 sat/vB version was broadcast and confirmed. These fee rates
are explicit scenarios, not current network recommendations.

| Settlement fee scenario | Signed transaction size | Merchant fee | Net savings output | Fee share of gross receipts |
| --- | --- | --- | --- | --- |
| 1 sat/vB | 6,829 vB | 6,829 sats | 993,171 sats | 0.6829% |
| 10 sat/vB | 6,829 vB | 68,290 sats | 931,710 sats | 6.829% |
| 100 sat/vB | 6,829 vB | 682,900 sats | 317,100 sats | 68.29% |

Gross receipts remain 1,000,000 sats in every alternative. The fee is a separate
expense deducted once from the settlement proceeds. The confirmed transaction
left zero working-wallet outputs and one savings output. A 1,000 sat/vB funding
attempt was refused because the receipts could not pay its fee. No initial
merchant deposit, provider, Lightning operation, or Payjoin exchange was used.
[Execution evidence](../../tools/merchant-acceptance-investigation/evidence/onchain-batching-2026-11-16.json)

This establishes an ordinary-payment transaction path, not an unattended
OpenBMT controller or safe instant fulfillment. It does not exercise continuous
inflow, policy enforcement, lost-device recovery, fee replacement, reorgs,
hardware signing, or a high-fee backlog. Payer transaction fees and the future
cost of spending the new savings output are excluded from the table.

### Security and operating choices

| Receiving arrangement | Ordinary customer wallets | Routine signing authority | Main consequence |
| --- | --- | --- | --- |
| Direct to offline savings | Standard address payment | Owner signs batches after the outputs are known | Strong savings-key isolation, with periodic owner participation. Small outputs accumulate between batches. |
| Separate working wallet with automatic savings batches | Standard address payment | Merchant-controlled online working key | No per-batch cold signature, but unswept receipts have online-key exposure. |
| Working wallet with a merchant-owned policy signer | Standard address payment | Separate signer checks destinations, fees, and inputs | May isolate authority from checkout software; still an online signing system, with deployment and recovery work. No additional device is assumed universally acceptable. |
| Payjoin alongside ordinary receiving | Customer must support Payjoin for its benefits | Merchant signs eligible contributed inputs | Can consolidate during a cooperating payment. Plain address payments remain supported separately. |

The best candidate for an automated standard-wallet path is the separate
working wallet, with direct-to-cold receiving available for merchants who
prefer signing batches themselves. An additional policy signer is a security
option to compare, not a claim that a general phone keystore enforces Bitcoin
destination restrictions. The previously tested limited cold-input
authorization can be evaluated as an optional settlement optimization; it
cannot provide an infinite chain of unknown future authorizations.

### Economic protection without pretending costs disappear

Batching saves repeated transaction overhead and destination outputs. Each
received output still contributes input weight when spent. In this probe the
merchant spent roughly 68 vB per received payment. At a fixed fee rate the
absolute cost of consuming an input changes little with its value, so a larger
average ticket has a lower percentage settlement burden. Choosing a different
script changes costs and signing/security requirements; cheaper recipient
spending may also shift output costs to the payer.

A controller should reserve that future expense when quoting and measuring
merchant economics, while keeping estimated costs separate from fees actually
paid. A merchant could absorb it as an operating expense or choose an explicitly
priced acceptance policy. Neither creates free block space. Automatic
consolidation during affordable conditions reduces the risk of carrying a
large backlog into a fee spike; it cannot guarantee an affordable future
window.

Fee-engine integration also needs current policy assumptions. OpenBMT's existing
receipt cost view floors its estimate at 1 sat/vB. Bitcoin Core 30 changed its
default relay minimum to 0.1 sat/vB while leaving wallet fee defaults unchanged;
that release explicitly warns that propagation and confirmation depend on
network adoption and conditions. Fractional-fee support is an optimization to
evaluate, not an assumed market price or a guarantee of affordable settlement.
[Core release notes](https://bitcoincore.org/en/releases/30.0/#policy)

When the fee ceiling and online exposure target conflict, the merchant needs a
specific exception path: approve a more expensive savings transfer, retain the
working funds temporarily, or direct new payments to cold addresses. Direct
cold receiving preserves the on-chain option while deferring its own input
costs and signing work. It does not make arbitrary tiny payments economical.

## Payjoin as an additional receiving capability

For a compatible customer wallet, Payjoin lets the receiver contribute inputs and modify its own output during the customer's payment. This can combine an existing merchant output with new proceeds. The payer must participate and validate the resulting transaction. Plain address payments cannot be retroactively combined through this protocol. [BIP 78](https://github.com/bitcoin/bips/blob/master/bip-0078.mediawiki) describes the consolidation opportunity; [BIP 77](https://github.com/bitcoin/bips/blob/master/bip-0077.md) changes the messaging model rather than removing the need for participation.

Payjoin belongs in the comparison as an optional compatibility improvement. It cannot establish universal protection for arbitrary small on-chain payments. Cold inputs still require the appropriate signatures or a deliberately constrained authorization, and additional inputs increase immediate transaction weight.

An on-chain acceptance policy must also handle ordinary payments, stale fee information, payments to old addresses, and unsolicited outputs. Economic minimums reduce exposure but cannot guarantee that every output remains economical under arbitrary future fees. The existing [manual merge offer](../OWNER-COINS-RULES.md#6-the-line-about-receipts-and-the-offer-to-merge-them) remains useful cleanup rather than complete prevention.

Receiving to a cold address requires no spending signature. Spending or merging its existing outputs later requires the appropriate authorization; the economic experiment does not simulate cold consolidation.

## Economic experiment

The isolated [money model](../../tools/merchant-acceptance-investigation/money-model/README.md) compares synthetic policies using explicit receipt streams, action costs, capacity, delays, and online-funds caps. An unavailable or deferred payment contributes no revenue. Pending actions do not create usable capacity. Outputs created by settlements incur a separately estimated future input cost.

Results are recorded alongside the probe source and evidence. Hypothetical costs demonstrate feasibility boundaries; they are not live quotes, production forecasts, or evidence that a route or liquidity seller exists. The starting confirmed receiving capacity is an explicit external-capital assumption whose acquisition is outside the steady-state model.

### Observed operating limits

The model passed nine accounting and resource-reservation tests and completed 44 scenario-policy replays. A controlled comparison uses 1,000 receipts of 1,500 sats, initially confirmed receiving capacity and an online-funds cap of 250,000 sats each, a 50,000-sat export threshold, and a 120-tick settlement delay. Each completed export costs a hypothetical 1,000 sats plus 1,000 ppm. The only change between the two rows is the permitted number of outstanding exports.

| Outstanding exports allowed | Accepted sales | Blocked unpaid sales | Completed savings outputs | Actual modeled fees paid | Fees reserved for unfinished exports | Peak online exposure |
| --- | --- | --- | --- | --- | --- | --- |
| One | 708 | 292 | 8 | 8,984 sats | 1,071 sats | 249,000 sats |
| Four | 1,000 | 0 | 25 | 26,275 sats | 4,204 sats | 229,500 sats |

The four-export replay ends with four unfinished exports and 225,000 sats still online, including their reserved principal. Those actions are not counted as completed savings. The result shows a throughput, cost, and exposure trade-off from bounded concurrency under the chosen deterministic delay; it does not establish a node's HTLC limits, actual swap-service concurrency, failure rate, or a competitive total price. The fee budget and hot cap remain enforced for every event. [Replay summaries and source fingerprints](../../tools/merchant-acceptance-investigation/evidence/money-model-2026-11-16.json), [test log](../../tools/merchant-acceptance-investigation/evidence/money-model-validation.log)

For contrast, directly receiving the same 1,000 payments on-chain creates 1,000 modeled outputs. At the explicit stress assumption of 2,000 sats to consume each output, all 1,500-sat outputs have nonpositive marginal value. That is a future-fee scenario, not a current fee quote or a claim that those coins can never be spent. No merchant receiving fee is charged in that baseline; payer fees and later consolidation are excluded.

The low-cap replay exposes a separate incompatibility: a 15,000-sat hot cap cannot form the configured minimum savings output after the selected 14,000-sat fixed export fee. It accepts three 5,000-sat receipts and blocks subsequent sales instead of inventing a larger buffer or silently weakening the cap. A product must detect such incompatible limits during setup and when fee conditions change.

## Release criteria

Before this can be described as near-automatic merchant acceptance, the design needs evidence for:

1. Setup by an ordinary merchant without manual channel engineering, with the funding and recovery requirements made concrete.
2. A representative month of small and large payments, bursts, fee spikes, service failures, delayed settlements, and offline signing periods. Measure accepted sales, blocked sales, actual fees, online exposure, receiving outages, resulting output values, and merchant interventions.
3. Verified funding before payment secrets are released under the chosen policy, and recoverable state before any external side effect.
4. Restart, lost-device recovery, fee replacement, timeout handling, and independent exit with a service unavailable.
5. Working alternatives to any default seller or swap service; different names pointing to the same dependency are insufficient.
6. No silent transition from an offline wallet to online signing, and no implicit conversion of an unavailable Lightning payment into an uneconomic on-chain payment.

The checkout should remain a single merchant experience. The underlying accounting must retain the distinctions needed to keep that experience honest.
