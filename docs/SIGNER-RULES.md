# Signer rules: what the Key app refuses to sign

Status: specification for `crates/openbmt-signer`, written before the code.
Each rule has an ID so that a test can name the rule it proves. The rules
implement `docs/DESIGN.md` §5.4 (Rule 3 and "The OpenBMT Key app"). Where this
file is more exact than the design, this file is what the code follows.

These rules are the Key app's half of the security claim. The Key app holds
key 2. A thief who controls the owner phone can build any transaction and add
key 1's signature to it. What stops the theft is that the Key app refuses to
add the second signature unless every rule below holds, and shows the owner
exactly what it will sign.

The signer is a library with no network, no clock and no storage of its own.
The caller passes in the time and the stored state, and supplies a "store this
first" step. A rule that fails returns a refusal that names the rule. Nothing
is signed and no state changes.

Some protections belong to the app around this library and are not listed
here: unlocking the Key app and wiping the key after too many wrong tries,
refusing to run under root, USB debugging, accessibility services or screen
overlays, and labeling every figure that came from the owner phone (a dollar
amount, a sale number) as the owner phone's claim. The library takes no such
figures at all.

## 1. What the signer keeps

State lives on the Key app and nowhere else. No message from another device
can change it; every change below is made on the Key app's own screen.

- **The registered wallet.** The full wallet description, checked at setup,
  and which of its three key slots is this device's own.
- **Receive mark.** One past the highest receive index the Key app has
  co-signed in an address batch. It never goes down.
- **Change mark.** One past the highest change index the Key app has signed
  for. It never goes down. The owner can raise it on this device (SR-34).
  The design's "high-water mark" is these two marks: receive and change
  addresses are numbered separately, so each has its own.
- **Allowlist.** Cash-out destinations, each with a label and the time it was
  enrolled. An entry becomes active 24 hours after that time.
- **Ceilings.** In satoshis: per transaction, per day and per week, one set
  for destinations on the allowlist and one for all other destinations. Plus
  the fee ceilings (SR-40 to SR-42).
- **Spend log.** For every transaction the Key app has signed in the last 7
  days: its ID, the time, the amounts and the inputs it spent. The day and
  week totals come from it. It holds at most 10,000 entries and 100,000
  inputs; when it is full the signer refuses until old entries age out.
- **Latest time seen.** The largest time the caller has ever passed in
  (SR-31).

## 2. Reading the transaction file

- **SR-01.** The file is a BIP174 PSBT, version 0. Anything else is refused,
  including PSBT version 2.
- **SR-02.** The file is at most 8 MiB **[to tune, E8]**. The transaction has
  at least 1 and at most 1,000 inputs, and at least 1 and at most 100
  outputs.
- **SR-03.** The unsigned transaction has version 2. Its lock time is a block
  height (below 500,000,000). Every input's sequence number is 0xFFFFFFFD or
  higher, so no input carries a relative time lock.
- **SR-04.** No input appears twice.
- **SR-05.** No input is already final (no final script or final witness
  field).
- **SR-06.** No input or output carries Taproot fields, a redeem script, or a
  hash preimage field. The wallet is native P2WSH and uses none of them.
- **SR-07.** Every input's signature-hash type is absent or `SIGHASH_ALL`.
- **SR-08.** If the file carries global extended public keys, each one is a
  key of the registered wallet with the same origin. The signer never learns
  a wallet from a transaction file.
- **SR-09.** Unknown and proprietary fields are ignored and are not copied
  into the file the signer returns. The signer builds that file itself. It
  holds the unsigned transaction, the previous transactions, the witness
  scripts and key-path hints the signer derived, the signatures it checked
  (SR-14) and its own. Nothing else from the incoming file is passed on.

## 3. Inputs

- **SR-10.** Every input carries the full previous transaction
  (`non_witness_utxo`). Its hash equals the input's previous transaction ID,
  and the output index exists. The amount and script of that output are the
  only ones the signer uses. (Without this a signer can be lied to about
  input amounts: CVE-2020-14199.)
- **SR-11.** If an input also carries a `witness_utxo`, it equals that output
  exactly.
- **SR-12.** Every input belongs to the registered wallet. The signer reads
  the key-path hints in the file only to learn which keychain (0 receive, 1
  change) and which index is claimed. A hint counts only if its fingerprint
  and path match a key slot of the registered wallet, and then the public key
  it names must be the one the signer derives for that slot; if it is not,
  the file is refused. Other hints are ignored. All counted hints of one
  input agree. The signer then derives the script for that keychain and
  index from its own registered description and compares it with the script
  of the output being spent. An input with no usable hint, or whose script
  differs, is refused.
- **SR-13.** If an input carries a witness script, it equals the one the
  signer derived. The signer signs with its own derived script, never with
  one from the file.
- **SR-14.** A signature already in the file for one of the wallet's keys is
  checked, and a bad one is refused. A signature for any other key is
  refused. If this device's own valid signature is already there for every
  input, the signer reports "already signed" and signs nothing.
- **SR-15.** Each input amount is at most 21,000,000 BTC, and the total is
  added with overflow checks.

## 4. Outputs

Every output is exactly one of three kinds.

- **SR-20. Change.** The output's key-path hints name the change keychain (1)
  and an index, under the same matching rule as SR-12; the script the signer
  derives for that index equals the output's script; and the index is below
  the change mark plus 20 (SR-34). At most one output is change: if more
  than one qualifies, it is the first in the transaction's order. An output
  whose hints name the change keychain but whose script is not the derived
  one is not change. It is an "other destination". The screen shows "change
  back to your wallet" only for an output that passed this rule.
- **SR-21. Allowlisted destination.** The output's script equals an allowlist
  entry that is active at the current time (SR-31).
- **SR-22. Other destination.** Any other output. Its script is one of:
  P2WPKH, P2WSH, P2TR, P2PKH, P2SH. Anything else is refused: data outputs,
  bare multisig, bare public keys, unknown witness versions and non-standard
  scripts.
- **SR-23.** An output to one of the wallet's own receive addresses is not
  change. It is an "other destination" and counts against those ceilings.
  A second output on the change keychain is treated the same way.
- **SR-24.** No output has a zero amount. The outputs are added with overflow
  checks, and their total is at most the input total.
- **SR-25.** A transaction whose only output is change (SR-20) is allowed.
  It joins many small payments into one coin, which a shop needs to do from
  time to time. Everything but the fee returns to the wallet, and the signer
  has checked that itself, so only the fee rules apply. The review calls it
  "moves coins inside your wallet".

## 5. Allowlist and time

- **SR-30.** A destination is enrolled only by a call that the Key app makes
  from its own camera and keyboard: a script of one of the kinds in SR-22,
  and a label of 1 to 64 characters, none of them a control character. At
  most 64 entries, and no script twice. It becomes active 24 hours after
  enrollment. Removing an entry takes effect at once.
- **SR-31.** The current time is the larger of the time passed in and the
  latest time seen. Moving the clock back therefore changes nothing. The
  latest time seen is stored with every signature and every enrollment.
  If the clock is behind the latest time seen, the signer says so. A clock
  that once ran far ahead would otherwise leave the totals stuck. The owner
  can confirm a clock correction on this device. The correction only makes
  things stricter: the latest time seen becomes the clock's time, every
  allowlist entry enrolled after that time starts its 24-hour wait again,
  and every spend-log entry dated after that time is re-dated to it.
- **SR-32.** The day total covers the last 24 hours and the week total the
  last 7 days, both measured back from the current time (SR-31). A signature
  is in the day total while the current time is less than 86,400 seconds
  after its date, and in the week total while it is less than 604,800
  seconds after. Older entries are dropped from the spend log.
- **SR-33.** Every signature counts in the totals, whether or not the
  transaction is ever broadcast. The signer cannot see the network, so it
  never assumes that a transaction it signed is dead. There are two
  exceptions, and both are exact:
  - Signing the very same transaction again (same transaction ID) is not
    counted twice and needs no room under the ceilings. It gives out the
    same signatures as before (SR-53).
  - A new transaction that spends every input of an earlier signed one can
    never confirm together with it. The two count once, at the larger of
    each amount (other destinations, allowlisted destinations, fee), dated
    at the later signature. The joined entry keeps the new transaction's ID
    and inputs. This covers a fee bump. If the new transaction covers
    several earlier ones in this way, it is joined with the most recent of
    them only and the rest stay counted in full, because two earlier
    transactions that do not conflict with each other could both confirm.

  Any other transaction counts in full, even if it conflicts with an earlier
  one.
- **SR-34.** A change index must be below the change mark plus 20. An honest
  transaction builder uses the next unused index, so this always holds for
  it. After signing, the mark becomes one past the change index used, if that
  is higher. A refusal under this rule names the index that was asked for.
  The owner can raise the mark on this device, and only there. A restored Key
  app starts with the mark at 0, so its first transaction usually needs
  this. The reason for the rule: change sent to an index far beyond the
  others would still belong to the wallet, but a restore would not find it
  without knowing where to look.

## 6. Ceilings

Amounts are in satoshis. "A" is the total of allowlisted outputs in the
transaction and "O" is the total of other destinations.

- **SR-35.** O is at most the per-transaction ceiling for other destinations,
  and A is at most the per-transaction ceiling for allowlisted destinations.
- **SR-36.** O plus the fee, plus the day total for other destinations, is at
  most that day's ceiling; the same for the week. A plus the day total for
  allowlisted destinations is at most that day's ceiling; the same for the
  week. The fee is counted with the other destinations because it also
  leaves the wallet for somewhere that is not on the allowlist. For one
  transaction the fee has its own ceiling (SR-41).
- **SR-37.** Ceilings are changed only by a call the Key app makes from its
  own screen. Lowering takes effect at once. Raising takes effect at once
  too: it is the owner's own device and own decision, and the 24-hour wait
  for new destinations (SR-30) is what protects a cash-out.
- **SR-38.** First values, used until the owner sets their own **[to tune]**:
  other destinations 100,000 per transaction, 300,000 per day and 1,000,000
  per week; allowlisted destinations 5,000,000, 10,000,000 and 30,000,000.
- **SR-39.** Restore mode starts from empty state and these first values. The
  ceilings apply in restore mode exactly as they do otherwise.

## 7. Fee

The fee is the input total (from the full previous transactions, SR-10) minus
the output total. The size is the signer's own calculation for the fully
signed transaction, with two signatures of the largest size per input (72
bytes each, with the hash-type byte).

- **SR-40.** The fee rate is at most the fee-rate ceiling: 150 sat/vB at
  first, adjustable on this device up to a fixed bound of 1,000 sat/vB.
- **SR-41.** The fee is at most the absolute fee ceiling: 200,000 sat at
  first, adjustable on this device up to a fixed bound of 2,000,000 sat.
- **SR-42.** The fixed bounds are compiled in. Nothing sent from another
  device can change a fee ceiling.
- **SR-43.** The fully signed transaction's weight is at most 400,000 units.

## 8. Signing

- **SR-50.** The signer first returns a review: every non-change output with
  its address (in the registered wallet's network), amount and kind (and the
  label, for an allowlisted one); the change amount, only if SR-20 passed;
  the fee and fee rate; the input count and total; and what would remain
  under each ceiling. The review holds the parsed transaction. Signing takes
  the review itself, never the file again, so what was shown is what is
  signed.
- **SR-51.** Signing works out the kind of every output, the change index
  and the ceilings again, from the transaction held in the review and the
  state and time as they are then. Another transaction may have been signed
  since the review, or a destination removed. If an output's kind or the
  change index is no longer what the review showed, the signer refuses and
  the owner reviews again.
- **SR-52.** The signing key's account public key equals the registered
  wallet's own slot, in full. A key that only shares its fingerprint is
  refused.
- **SR-53.** Signatures are ECDSA over the BIP143 hash, computed from the
  signer's own derived script and the amount from SR-10, with `SIGHASH_ALL`,
  RFC 6979 nonces and low S. The same file always gives the same signatures.
- **SR-54.** The signer verifies each signature it made before it returns.
- **SR-55.** The new state (spend log, change mark, latest time seen) is
  handed to the caller's "store this first" step. The signed file is returned
  only if that step succeeds.
- **SR-56.** The signer erases its copies of secret key material when it is
  done with them. This is best effort: the compiler and the operating system
  can leave other copies in memory.

## 9. Setup: registering the wallet

- **SR-60.** The description parses under the strict rules of `openbmt-core`:
  P2WSH `sortedmulti`, 2-of-3, BIP48 paths for the right network, `<0;1>/*`,
  a valid checksum, three different keys.
- **SR-61.** Exactly one key slot holds this device's own account public key,
  in full.
- **SR-62.** If the Key app was given key 3's public key directly (by QR from
  the device that made it), exactly one other slot holds it, in full.
- **SR-63.** A registered wallet is never replaced. A different wallet is a
  new setup with empty state ("Move to a new wallet").

## 10. Other things the Key app co-signs

Address batches follow `openbmt-core` (`Approver::check_and_cosign`), in
this order: the batch names this wallet by full hash, has a valid shape,
carries key 1's signature, is for a counter on the signed device list, starts
exactly at the receive mark, and holds the addresses the Key app derives
itself. The new receive mark is stored before the co-signature is released.
Settings and pairings follow `docs/DESIGN.md` §2.9 and are specified with
those parts.

## 11. What these rules do not do

Stated plainly, because the design promises no overclaiming.

- **They do not know whether a payment is owed.** The Key app has no sales
  records. "This refund is for sale 123" is the owner phone's claim. The
  ceilings bound what a false claim can cost.
- **They count signatures, not confirmed spends** (SR-33). The totals can
  only be too high, never too low.
- **They do not see the chain.** The signer cannot tell whether an input is
  still unspent, or how far in the future a lock time is. A signed
  transaction that is held back and broadcast later was still counted when
  it was signed.
- **They depend on the Key app's clock for waits and windows.** Someone who
  can unlock the Key app and set its clock forward can end the 24-hour wait
  early and empty the day and week totals. That person already holds the
  unlocked key 2 device.
- **They are promises of the Key app's software.** A malicious build of the
  Key app keeps none of them. That is what the separate release key, the
  reproducible builds and the second key holder are for (`docs/DESIGN.md`
  §5.4).
- **They bind key 2 only.** Key 1 and key 3 together can spend without the
  Key app. That is the purpose of key 3, and why it is stored away from the
  shop.
- **Anti-exfil signing is not in these rules yet.** RFC 6979 makes the
  signatures reproducible, which lets a second build check them, but it does
  not stop a malicious build from choosing its own nonces.
