# Spend rules: building, joining and finishing a transaction

Status: specification for `crates/openbmt-spend`, written before the code.
Each rule has an ID so that a test can name the rule it proves. The rules
serve `docs/DESIGN.md` §2.4 (refunds), §2.10 (approval session) and §11
(cash-out), and they are written to meet `docs/SIGNER-RULES.md`: where the
two meet, the signer's rules decide, because the signer is the part that
refuses.

The signer reviews a transaction file and adds one signature. This library
is what comes around it:

1. **Plan.** From the coins of the wallet and a request (payments, a fee
   rate), choose coins and work out the exact transaction.
2. **PSBT.** Write the plan as a PSBT version 0 file that the signer accepts
   without a refusal.
3. **Combine.** Join the signatures that two signers made on copies of that
   file, checking every signature before it is taken.
4. **Finish.** Turn two signatures per input into a transaction the network
   takes, and check that transaction again from scratch.

It has no network, no clock, no storage and no randomness. The same coins
and the same request always give the same plan, byte for byte. The caller
supplies the coins, the fee rate and the change index.

## 1. Coins

A coin is a previous transaction (the whole of it), an output index, and the
keychain (receive or change) and index of the wallet script it pays.

- **SP-01.** The previous transaction's ID equals the outpoint's
  transaction ID, and the output index exists in it. A coin that fails is
  refused.
- **SP-02.** The script of that output equals, byte for byte, the wallet's
  script at the coin's keychain and index, as `openbmt-core` derives it. A
  coin that fails is refused. Nothing in this library derives a script by
  hand.
- **SP-03.** The same outpoint is not given twice.

Every coin handed in is checked, including coins that are not eligible by
depth (SP-10). A bad coin makes the whole plan an error. It is never left
out silently.

## 2. Choosing coins

- **SP-10.** Only coins with at least `min_confirmations` confirmations are
  spent. Callers pass 1 to spend confirmed coins only. A coin with no
  confirmation is spent only when the caller passes 0, which is a decision
  the caller makes in the open.
- **SP-11.** The choice is deterministic. Eligible coins are taken oldest
  first (most confirmations first), ties broken by outpoint (transaction ID
  bytes, then output index). The plan takes the shortest run of that order
  that pays for the request. The inputs of the transaction are in the same
  order. There is no randomness.
- **SP-12.** With `send_all`, every eligible coin is spent, to exactly one
  payment, with no change. The request carries that payment's script and
  the amount 0; the amount paid is the total of the coins minus the fee. It
  serves a cash-out of everything and a consolidation (SP-24).

## 3. Outputs

- **SP-20.** The outputs are the payments in the order given, then the
  change output if there is one. The order is not sorted (BIP 69 is not
  used). The signer's rules do not fix an order, and a change output that
  is always last is easy to pick out on the chain; that cost is accepted
  here and is an open point.
- **SP-21.** Change goes to the wallet's own change script at
  `change_index`, and its amount is the coins' total minus the payments
  minus the fee. The caller supplies the index; the signer accepts it only
  below its change mark plus 20 (SR-34).
- **SP-22.** A payment is to one of the script kinds SR-22 allows (P2WPKH,
  P2WSH, P2TR, P2PKH, P2SH) and is at least the dust limit of that kind
  (294, 330, 330, 546 and 540 satoshis). An amount of zero, a data output,
  a bare script or an unknown witness version is refused. Amounts are at
  most 21,000,000 BTC in total.
- **SP-23.** A change amount below the dust limit of its script (330
  satoshis) is not made. It is added to the fee, and the plan says so: the
  plan holds the amount that was dropped.
- **SP-24.** A payment to one of the wallet's own scripts is refused, with
  one exception. SR-23 would let the signer sign an output to the wallet's
  own receive address (it counts as another destination), and SR-20 would
  count a second change-keychain output as another destination too; this
  library refuses both, because an honest request never needs them. The
  exception is SR-25: with `send_all`, one payment to the wallet's change
  script at `change_index` is allowed. The signer reads it as a
  transaction that moves coins inside the wallet. The wallet's own scripts
  that are known here are the scripts of the coins handed in and the change
  script at `change_index`; a payment to any other script of the wallet
  cannot be told from a payment to someone else.
- **SP-25.** At least one payment is required. There are at most 100
  outputs in total (SR-02), change included, and at most 1,000 inputs.

## 4. Size and fee

- **SP-30.** The size is worked out from the known shape: version, lock
  time, 41 bytes per input without its witness, each output by its script
  length, and per input a witness of an empty item, two signatures of 72
  bytes each (the longest a low-S signature with its hash-type byte can be)
  and the 105-byte witness script. The weight is checked against the
  `bitcoin` crate's own weight of a transaction with a witness of that
  shape. The virtual size is the weight divided by 4, rounded up, as in
  SR section 7. The fee is `ceil(vsize * rate / 1000)` with the rate in
  satoshis per 1000 virtual bytes, as an integer.
- **SP-31.** Signatures are counted at their longest, so the fee of a
  finished transaction is at least the asked rate times its real size. The
  real signatures are sometimes one or two bytes shorter, so the rate that
  is actually paid is a little above the asked one. How far above is
  bounded by that slack: at most two virtual bytes per input and one
  satoshi of rounding.
- **SP-32.** A rate of 0 is refused. A fee above 2,000,000 satoshis, or a
  fee rate above 1,000 satoshis per virtual byte, is refused. These are the
  fixed bounds the signer compiles in (SR-40, SR-41); the signer's own
  ceilings, which the owner sets lower, are applied by the signer. The fee
  checked is the fee that is paid, including a dropped change (SP-23).
- **SP-33.** The weight of the fully signed transaction is at most 400,000
  units (SR-43).
- **SP-34.** Every amount is added with checked arithmetic. Each coin and
  each output is at most 21,000,000 BTC. The total in always equals the
  total out plus the fee.
- **SP-35.** The transaction has version 2, lock time 0 (a block height, as
  SR-03 wants) and sequence 0xFFFFFFFD on every input (RBF signalling, no
  relative time lock). The signer's rules ask for no anti-fee-sniping lock
  time, and the library has no clock or chain height to set one, so the lock
  time is 0.
- **SP-36.** The plan exposes what a person must be shown: every payment,
  the change, the dropped change if any, the fee in satoshis and as a rate,
  the virtual size and the totals in and out.

## 5. The PSBT

- **SP-40.** The file is PSBT version 0. Every input carries the whole
  previous transaction and the previous output, the witness script, and the
  key-path hints of all three keys at that keychain and index. Nothing else
  is added: no signature, no hash type, no global extended public key.
- **SP-41.** The change output (and the one output of a consolidation, SP-24)
  carries the witness script and the key-path hints of all three keys, so
  that the signer can show it as change. No other output carries anything.
- **SP-42.** The file passes the signer's review with no refusal, and the
  signer signs it. Two signers' signatures on it can be combined and
  finished (SP-60).

## 6. Combine

`combine(base, signed)` takes the plan's file and the files the signers
returned. The keys it accepts are the three keys of each input's witness
script in the base, and the base's witness script has to hash to the script
being spent.

- **SP-50.** Every signed copy holds the same unsigned transaction as the
  base, and, for each input, the same previous transaction, previous output
  and witness script. A copy that differs is an error. A previous
  transaction counts as the same when its transaction ID is the same: the
  ID covers every output, and Bitcoin Core writes the previous transactions
  of a PSBT without their witnesses. A copy with a final
  script or witness on an input is an error.
- **SP-51.** A partial signature is taken only for one of the three keys of
  that input's witness script. A signature for another key is an error that
  names the key.
- **SP-52.** Only `SIGHASH_ALL` is taken. Another hash type, in a signature
  or in the input's hash-type field, is an error.
- **SP-53.** Every signature is checked against the BIP 143 hash of its
  input, made from the base's witness script and the amount in the base's
  previous transaction, before it is taken. A signature that does not verify
  is an error that names the key and the input. It is never dropped. A
  signature in high-S form is one that does not verify. A signature that is
  not strict DER does not parse; the file is then an error too.
- **SP-54.** The same key's signature met twice is kept once. The result
  does not depend on the order of the copies, and the base's own signatures
  are checked like the others.

## 7. Finish

- **SP-60.** `finalize` needs, for every input, valid signatures by two
  different keys of the input's script. The witness is: an empty item, the
  two signatures in the order of their keys in the sorted witness script,
  the witness script. A third signature is not used. Fewer than two, or the
  same key twice, is an error.
  A file with no input or no output is refused, by `combine` too: with
  nothing to check, every check would pass.
- **SP-61.** The input's keychain and index are read from its key-path
  hints; the script spent and the witness script have to equal what the
  wallet derives for them. A file whose witness script is not the wallet's
  is an error.
- **SP-62.** Before the transaction is returned it is checked from scratch,
  from the finished transaction and not from the working variables: its
  transaction ID equals that of the unsigned transaction; its outputs,
  version, lock time and sequences equal those of the file; each input's
  witness has the shape of SP-60 and its two signatures verify against the
  BIP 143 hash, in the way OP_CHECKMULTISIG matches them; the fee equals
  the inputs minus the outputs; no amount is out of range.
- **SP-63.** The weight of the finished transaction is at most 400,000
  units.
- **SP-64.** `finalize_plan` also holds the file to the plan: the unsigned
  transaction is the plan's, and the fee is the plan's fee.

## 8. All inputs are hostile

- **SP-70.** No function of the library panics on any input. Errors are
  values with plain text. The fuzz target `spend_combine` and the tests
  feed it arbitrary bytes.
- **SP-71.** `plan` and `to_psbt` are deterministic: the same arguments give
  the same plan and the same file.

## 9. What these rules do not do

- **No fee estimation.** The caller supplies the rate. A rate from a stale
  source is the caller's problem, and so is a fee that is wrong for the mempool.
- **No broadcast.** The finished transaction is handed back. Sending it,
  and over which network path, is for the caller.
- **No coin tracking.** The library does not know which coins are spent,
  reserved by another plan or replaced. It also does not know the chain's
  height or whether a coin's depth is still true; the caller gives it.
- **No check of the payee beyond the script's kind.** The library does not
  know whether an address is the customer's. SP-22 checks that the script is
  a standard kind and above dust, nothing more.
- **No signing.** Keys are not here. Signing is the signer's work, and the
  signer's rules (ceilings, allowlist, change mark) apply whether or not
  this library thinks a plan is fine.
- **No check that stands alone.** The final check of SP-62 is this library's
  own, made with the `bitcoin` crate's hashing and secp256k1. There is no
  consensus library here. Bitcoin Core's `testmempoolaccept` in
  `scripts/spend-e2e-core.sh` is the independent check.
- **Privacy is not solved.** Change is always last, and the lock time is
  always 0, so the transactions have a recognizable shape. Oldest-first
  choice can link many old receipts in one transaction. A caller that wants
  fewer links spends fewer coins by passing only some of them.
- **No replacement or fee bump.** A new plan over the same coins is a new
  transaction; the signer joins it with the old one in its totals (SR-33).
- **No Taproot, no other wallet shape.** One script shape (P2WSH 2-of-3
  `sortedmulti`) is built and finished.
