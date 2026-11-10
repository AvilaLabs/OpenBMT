# Research note: a different layer 2? (2026-11-09)

Research note on layer 2 alternatives to Lightning (2026-11-09).

Three lanes: Lightning problem inventory, L2 design survey, theoretical
limits. Technical sources only.

## Theory

- Instant, final, trustless payment to a receiver with no prior enforceable
  position (channel, escrow, pre-signed state, confirmed coin) is not possible
  on today's Bitcoin. Every design that claims it moves trust somewhere.
- Without covenants, shared off-chain state needs all parties to pre-sign
  every update. The smallest soft fork that changes this is CTV, possibly
  with CSFS.

## Lightning: problems inherent to two-party channels

- Inbound liquidity: someone must lock capital toward the receiver.
- The receiver must be online, or rely on a third party to hold the payment.
- Each user has an on-chain footprint, so force-closes are costly when fees
  spike.
- Larger payments fail more often, because balances along a route are hidden.

## Lightning: problems that are economic or UX-driven, amplified by structure

- Hub concentration: capacity Gini 0.977 on 2026-03-05 (Glassnode); top-10
  nodes hold 62–85% of public capacity, per vendor sources.
- Dependence on LSPs and trampoline routers.
- Custodial dominance (claimed above 80%; not measurable from public data).

## Designs

- **None meets the purist bar today without a soft fork.**
- Closest without a fork:
  - Ark (Second's bark; mainnet 2026-06-19; clArk and hArk). Instant
    out-of-round receives trust that the sender and server do not collude.
  - SuperScalar (regtest/signet, no audit, mainnet locked). An LSP
    architecture.
- Better with a soft fork: Erk, covenant Ark, Lark equivocation bonds,
  LN-Symmetry, John Law's factories. These need CTV, TEMPLATEHASH, CSFS or
  APO.
- Activation status:
  - The CTV client reportedly signals from 2026-03-30 to 2027-03-30, with
    about 0% signaling in May 2026 (vendor source, unverified).
  - The BIP 446/448 TEMPLATEHASH bundle is in Inquisition 29.4.
  - There is no convergence between the two camps.
- Fail the bar: Spark and Mercury (key-deletion trust), Fedimint, Cashu,
  Liquid, drivechains, BitVM2 bridges (1-of-N, slow exit).

## Open gap

No proposal gives a merchant instant, offline, bounded-loss receive under a
cryptographic rule rather than an economic one.
