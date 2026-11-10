# Research note: self-managing merchant Lightning node (2026-11-09)

Research note on a self-managing merchant Lightning node (2026-11-09).

Three lanes: building blocks, costs and operations, products and purist fit.

## Correction

Splice-out does **not** restore inbound liquidity. Receiving moves capacity
from the peer's side to the merchant's side. Splicing the merchant's side out
to cold storage leaves the peer's side depleted. Inbound is consumed by every
sat received, so a receive-mostly merchant must keep buying inbound (Phoenix
charges 1% plus mining fees for added liquidity) or spend over Lightning.

## Costs

Modelled for $30k/month over Lightning, estimates:

| Fee rate | Monthly cost | Share of volume |
|---|---|---|
| 1 sat/vB | ~$200 | 0.7% |
| 5 sat/vB | ~$315 | 1.0% |
| 20 sat/vB | ~$750 | 2.5% |

- Cards for comparison: $600–900 (2–3%).
- Customers pay about 0.4% routing if they pay from Phoenix.

## Operations

- "Never has to manage" is not realistic. The minimum is a weekly review,
  liquidity top-ups, upgrades, a restore test each quarter, and pausing
  during fee spikes.
- Hot exposure is about $400–700.
- A force-close during an extreme fee spike can cost more than the channel
  funds.
- If the Box dies, hot funds are stuck for days to weeks.

## Building blocks

- **Splicing:** spec merged 2026-03-23 (BOLTs #1160).
  - CLN: `spliceout` to an address; multi-channel splice scripts (#8450).
  - Eclair: most mature.
  - LDK: partial.
  - LND: no splicing.
- **Liquidity ads:** not standardized. #878 closed; #1153 still open. Few
  sellers on mainnet (vendor claim). Amboss Magma is the main marketplace.
- **Watchtowers:** LND has a tower; CLN uses TEOS (last push 2025-10-09); none
  for Eclair or LDK. Zero-fee commitments change fee-bump needs.
- **Remote signing:** VLS 0.14.0 (2025-11); production readiness unverified.
- **Tooling gap:** no stack ships automatic splicing.

## Existing products

- Alby Hub sweeps to cold storage automatically, but via Boltz swaps.
- NodeGuard sweeps node funds to a multisig, for institutions.
- phoenixd is ACINQ-dependent.
- BTCPay offers a POS on the merchant's own node.
- The novelty would be the integration only.

## Purist fit

- Own node and own channels: accepted.
- Single LSP: tolerated, not an end state.
- A multi-seller market: positive, but thin in practice.
- Signing keys must stay online. Claim "bounded loss", never "cold".
