# Research note: co-signed instant payments (2026-11-09)

Research note on co-signed instant payments (2026-11-09).

Three research lanes: protocol soundness, purist/legal/operators, wallet
adoption. Status: undecided. Nothing built.

## Base protocol (Robin Linus, gist updated 2026-08-07)

- Customer coins sit in a confirmed Taproot 2-of-2 (customer, co-signer).
- The co-signer pre-signs an exit gated by a "connector" output, so the
  customer's escape delay starts only when the customer starts it.
- The co-signer refuses conflicting spends. The merchant accepts at zero
  confirmations.
- No periodic refresh is needed; the exit cost is paid only at exit.
- Verdict: sound if the co-signer does not equivocate and never loses state.
  A customer colluding with the co-signer can double-spend.

## Our extensions, after review

| Extension | Verdict | Fix |
|---|---|---|
| E1 lock only on merchant request | Sound with rules | The lock binds payee script, amount and invoice. It is refused unless all unconfirmed ancestors are already locked. The co-signer issues a signed lock attestation the merchant checks offline. |
| E2 merchant release (void) | Sound | Per-payment, short-lived release; refused once the payment has descendants |
| E3 fee bump by replacement | **Unsound** (changes txid, breaks chained change and later payments) | Fee bumps by CPFP or anchor only |
| E4 threshold co-signer | **3-of-5 unsafe**: one malicious operator in two quorums can double-sign | Need 2t−n > f, e.g. 4-of-5 (tolerates 2 malicious, 1 offline) or n=3f+1, t=2f+1. Per-operator signed receipts make equivocation attributable off chain. |
| Bonds | On-chain slashing needs CSFS+covenant (soft fork); weak forms only | Off-chain (reputation/legal) until a soft fork. Bonds also add legal and capital risk. |

FROST for BIP340 is BIP 445 (draft, PR #2070). ChillDKG is a draft. No
production mainnet FROST wallet was confirmed.

## Purist objections (ranked)

1. "Reinvented PayPal using a 2-of-2" (Laz1m0v, Delving 2026-05-03): the
   merchant trusts the signer set.
2. Freeze power: the co-signer can refuse to sign; the answer is the exit,
   which is slow.
3. Liveness: the co-signer is offline, so the customer cannot pay until a
   fallback.
4. Privacy: the co-signer sees every spend. There is no full fix without
   losing the check.
5. Labeling: never call it self-custody (Adam Back on Green 2FA, Stacker News
   1593864, 2026-10-12).

## Legal (not advice)

- FinCEN FIN-2019-G001 §4.2.2: a signing-only co-signer with no balances is
  not a money transmitter.
- DFAL §3102(c): no "control" if the co-signer cannot prevent a transaction
  indefinitely. The exit makes that true; keep the delay clearly finite.
- Risks: operator fees and bonds; a long-delay reading; Samourai sentences
  (2025-11-19) as a warning for fee-taking operators.

## Adoption

- No partner wallet fits as is. Bitkey has per-spend server policy; Green is
  2-of-2 plus a CSV escape (not Taproot); Nunchuk has Taproot MuSig2.
- Path: spec plus SDK plus a reference co-signer, shipped first in one anchor
  wallet. It is inert without operators.
- Customer friction:
  - the first deposit must confirm (10–60 min);
  - the co-signer being offline blocks paying;
  - the exit takes days and costs fees;
  - the 25-transaction chain limit.

## Costs at 1–3 sat/vB (estimates)

| Step | Cost |
|---|---|
| Funding | ~150–450 sats |
| Payment | ~200–600 sats |
| Full exit | ~350–1,200 sats |
| Each extra connector or kickoff output | ~330 sats of dust |
