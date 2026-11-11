# Signer rules: what the Key app refuses to sign

Status: specification for `crates/openbmt-signer`, which implements it.
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
here: refusing to run under root, USB debugging, accessibility services or
screen overlays, and labeling every figure that came from the owner phone (a
dollar amount, a sale number) as the owner phone's claim. The library takes
no such figures at all. The PIN lock, the wipe after too many wrong tries
and the enrolment screens are in section 12.

## 1. What the signer keeps

State lives on the Key app and nowhere else. No message from another device
can change it; every change below is made on the Key app's own screen.

- **The registered wallet.** The full wallet description, checked at setup,
  and which of its three key slots is this device's own.
- **Receive mark.** One past the highest receive index the Key app has
  co-signed in an address batch. It never goes down. The owner can raise it
  on this device (SR-70).
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
- **Latest time seen.** The largest time at which the Key app has signed a
  transaction or enrolled a destination (SR-31).

## 2. Reading the transaction file

- **SR-01.** The file is a BIP174 PSBT, version 0. Anything else is refused,
  including PSBT version 2 and a file with bytes after its last map.
- **SR-02.** The file is at most 8 MiB **[to tune, E8]**. The transaction has
  at least 1 and at most 1,000 inputs, and at least 1 and at most 100
  outputs.
- **SR-03.** The unsigned transaction has version 2. Its lock time is a block
  height (below 500,000,000). Every input's sequence number is 0xFFFFFFFD or
  higher, so no input carries a relative time lock. Any block height is
  accepted, the current tip's included, so a transaction with an
  anti-fee-sniping lock time passes.
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
  checked, and a bad one is refused. A signature that is not in low-S form
  counts as bad. A signature for any other key is refused. If this device's
  own valid signature is already there for every input, the signer reports
  "already signed" and signs nothing.
- **SR-15.** Each input amount is at most 21,000,000 BTC, and the total is
  added with overflow checks.

## 4. Outputs

Every output is exactly one of three kinds.

- **SR-20. Change.** The output's key-path hints name the change keychain (1)
  and an index, under the same matching rule as SR-12; the script the signer
  derives for that index equals the output's script; and the index is below
  the change mark plus 20 (SR-34). An output that meets the first two
  conditions with an index at or beyond that bound is refused, whether or
  not it would be the one taken as change. At most one output is change: if
  more than one qualifies, it is the first in the transaction's order. An output
  whose hints name the change keychain but whose script is not the derived
  one is not change. It is an "other destination". The screen shows "change
  back to your wallet" only for an output that passed this rule.
  Change is proved by its derivation, never by its place: it may be first,
  in the middle or last.
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
  has checked that itself, so the per-transaction ceilings have nothing to
  measure. The fee is held to the fee rules and, like every fee, to the day
  and week ceilings for other destinations (SR-36). The review calls it
  "moves coins inside your wallet".

## 5. Allowlist and time

- **SR-30.** A destination is enrolled only by a call that the Key app makes
  from its own camera and keyboard: a script of one of the kinds in SR-22,
  and a label of 1 to 64 characters and at most 255 bytes of UTF-8, none of
  them a control character. At most 64 entries, and no script twice. It
  becomes active 24 hours after enrollment. Removing an entry takes effect
  at once.
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
  seconds after. Older entries count for nothing and are dropped from the
  spend log at the next signature.
- **SR-33.** Every signature counts in the totals, whether or not the
  transaction is ever broadcast. The signer cannot see the network, so it
  never assumes that a transaction it signed is dead. There are two
  exceptions, and both are exact:
  - Signing the very same transaction again (same transaction ID) is not
    counted twice and needs no room under the ceilings. It gives out the
    same signatures as before (SR-53). This holds while the first
    signature is still in the spend log, which is 7 days.
  - A new transaction that spends every input of an earlier signed one can
    never confirm together with it. The two count once, at the larger of
    each amount (other destinations, allowlisted destinations, fee), dated
    at the later signature. The joined entry keeps the new transaction's ID
    and inputs. This covers a fee bump. If the new transaction covers
    several earlier ones in this way, it is joined with the most recent of
    them only and the rest stay counted in full, because two earlier
    transactions that do not conflict with each other could both confirm.

  Any other transaction counts in full, even if it conflicts with an earlier
  one. A transaction that would take the spend log past its size (section 1)
  is refused.
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
- **SR-39.** There is no restore mode. A Key app made from typed words
  is a new Key app: no destinations, the first values above, both marks
  at 0 and no totals. Every rule applies to it as to any other, and
  nothing is skipped because the words were typed. Whoever holds the
  words can sign with any wallet software, so a mode that loosened the
  rules would protect nothing; keeping them protects the holder from a
  request made by a taken-over owner phone during a recovery. Before a
  restored Key app signs a larger payment the holder, on its own screen,
  takes the wallet again and checks it against the kit, raises the
  receive mark once (SR-70), enrols the destination and waits the 24
  hours of SR-30, and raises the ceilings if the payment needs it
  (SR-37). A second key is needed for any payment, so the wait gives a
  thief with one key nothing.

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
  the owner reviews again. A review made for another wallet is refused.
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
  a valid checksum, three different keys. The network is mainnet, testnet,
  signet or regtest.
- **SR-61.** Exactly one key slot holds this device's own account public key,
  in full.
- **SR-62.** If the Key app was given key 3's public key directly (by QR from
  the device that made it), exactly one other slot holds it, in full.
- **SR-63.** A registered wallet is never replaced. A different wallet is a
  new setup with empty state ("Move to a new wallet").

The key ceremony (`docs/DESIGN.md` §2.1, steps 2, 4 and 5) passes three
public things between the devices: key 3 to the Key app, the Key app's answer
to the owner phone, and the wallet setup back to the Key app. They are read in
`openbmt-signer` (`ceremony.rs`); the Key app keeps key 3 and does the checks
of the setup (`openbmt-key-ffi`). The owner phone's own rules for the same
steps are OC-40 to OC-46 in `docs/OWNER-COINS-RULES.md`.

- **SR-64.** Key 3 reaches the Key app only as text with its origin:
  `[<8 hex digits>/48h/<coin>h/0h/2h]<key>`, where `<coin>` is 0 on mainnet
  and 1 on the other networks, `'` may stand for `h`, and the key is an
  `xpub` or `tpub`, or the multisig P2WSH `Zpub` or `Vpub`, which is read as
  the plain form of the same key. Surrounding white space is ignored and
  nothing else is. Refused: a key without its origin (a bare key with a
  fingerprint typed beside it is not an origin), an origin of another form or
  path, text that is not a key or is longer than 256 characters, a key that
  is not at the account level (depth 4, last step 2h), and a secret key, which
  is never echoed. In the Key app the scan of key 3 takes, besides a plain
  text code, a `crypto-account`, `crypto-hdkey` or `bytes` UR (one part or
  several, read by the shared `UrScan`). A finished UR goes through the
  library's key text call (UR-31): the text it gives is taken exactly as
  scanned text is, a code it declines shows its words and the scan step
  stays, and a refusal of the call or no answer shows the words of a reader
  that failed. Pasting takes text only; pasted `ur:` text is refused.
- **SR-65.** A single-signature form (`ypub`, `zpub`, `upub`, `vpub`) is
  refused, and so is a multisig form of another script type (`Ypub`,
  `Upub`). The words say that the key is for a wallet that one key spends from,
  and which export to choose instead: the one for a shared wallet of native
  segwit type.
- **SR-66.** Key 3 is for the same kind of network (main, or test) as key 2,
  or, on a device with no key 2, as the wallet. Key 3 is not key 2: neither
  its account key nor its master fingerprint is key 2's.
- **SR-67.** The Key app keeps key 3 in its stored state as soon as it is
  taken, before any wallet exists; taking another key 3 replaces it. A wallet
  is accepted only if exactly one of its keys is key 3 in full (account key,
  fingerprint and path), and, on a device that holds key 2, that key is another
  slot (SR-61, SR-62). When key 3 is stored, a key 3 given with the wallet must
  be the same key. The wallet setup of the ceremony is refused when no key 3
  has been taken. The three refusals of this rule are told apart by their
  rule strings in the Key app's library, not by their text: `key3-missing`
  (no key 3 taken), `key3-differs` (the key 3 given is not the one taken) and
  `key3-not-in-wallet` (no key of the wallet is the key 3 taken: a swapped
  key 3). A wallet setup of the ceremony that passes every check is kept
  as pending, not registered: it is stored (so a restart reopens on it) and
  shown for the key holder to compare with the owner phone and the counter.
  While it is pending nothing is co-signed, no pairing is offered and no
  other change of the setup is made. The key holder's "they match" registers
  it (SR-63 applies from then on); "they differ" drops it and returns the Key
  app to the state before the setup, with key 2 and key 3 kept. Either is
  refused, with nothing changed, when no wallet is pending.
- **SR-68.** The Key app's answer (`OBMv`) and the wallet setup (`OBMw`) are
  strict frames: the magic, version 1, the exact length, flags 0 or 1,
  defined network bytes, keys at the account level for the network's kind, an
  answer whose keys differ, and a setup whose description is the canonical text
  of a valid wallet. Bytes decode only if writing the value again gives the
  same bytes.
- **SR-69.** The Key app's answer holds only public values: the network, key
  2's fingerprint and account public key (not on a device with no key 2), key
  3's as stored, and the Key app's own root. No secret is in it.

When key 2 is on a hardware signer the Key app holds no key 2, and the owner
phone would otherwise be the only device that names key 2's slot. So the Key
app takes key 2's public part from the signer itself, the same way it takes
key 3 (SR-64 to SR-67), and checks the wallet against it. Both phones take
key 2 on their own; the Key app's answer never carries it.

- **SR-84.** On a Key app that holds no key 2, key 2 reaches the Key app only
  as text with its origin, read exactly as key 3's is (SR-64, SR-65), after
  key 3 is taken and before any wallet exists. It is kept in the Key app's
  stored state as soon as it is taken; taking another key 2 replaces it until
  the wallet is accepted. Taking it is refused, with nothing changed, when
  key 3 is not taken yet, when the Key app holds key 2 itself, and once a
  wallet is waiting or accepted. The Key app's answer (SR-69) does not carry
  it.
- **SR-85.** A secret key given as key 2 is refused with words of its own, and
  is never echoed. Nothing is changed.
- **SR-86.** Key 2 is not key 3: neither its account key nor its master
  fingerprint is key 3's. The same holds the other way round: a key 3 that is
  the key 2 taken is refused. Nothing is changed.
- **SR-87.** Key 2 is for the same kind of network (main, or test) as key 3,
  and a key 3 taken later is for the kind of the key 2 taken. Nothing is
  changed.
- **SR-88.** On a Key app that holds no key 2, the wallet setup of the
  ceremony is refused, with nothing changed, when no key 2 has been taken. A
  Key app that holds key 2 itself needs none taken.
- **SR-89.** On a Key app that holds no key 2, the wallet setup of the
  ceremony is accepted only if slot 2 of the wallet is exactly the key 2
  taken: the same account key, the same master fingerprint and the same
  origin path. Any other key in slot 2, or the key 2 taken in another slot, is
  refused and nothing is accepted.
- **SR-90.** The key 2 taken is part of the Key app's stored state, in an
  `OBM7` envelope of version 4; it needs key 3, and it decodes only if key 2
  is a key at the account level of key 3's kind, is not key 3, and the Key
  app holds no key 2 of its own. A state of an older version reads as "no key 2 taken", and a
  wallet accepted before this rule stays accepted: only a new acceptance of a
  ceremony wallet needs key 2 taken.

## 10. Other things the Key app co-signs

Address batches follow `openbmt-core` (`Approver::check_and_cosign`), in
this order: the batch names this wallet by full hash, has a valid shape,
carries key 1's signature, is for a counter on the signed device list, starts
exactly at the receive mark, and holds the addresses the Key app derives
itself. The new receive mark is stored before the co-signature is released.

- **SR-70.** The owner can raise the receive mark on this device, and only
  there. It only goes up. A restored Key app starts with the mark at 0, and
  the shop has issued addresses up to the index its recovery kit records, so
  the owner raises the mark to that index before the next batch. Indexes
  below the mark are never co-signed again.

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
  early and empty the day and week totals. So can someone who sets the clock
  back, confirms that as a correction (SR-31) and sets it forward again.
  That person already holds the unlocked key 2 device.
- **They do not protect the stored state.** The library checks the shape of
  the state it is given, not where it came from. Keeping the state where
  nothing else can write it or put back an older copy is the Key app's job.
  The PIN lock (section 12) keeps the state unreadable to someone who has
  the phone and not the PIN; it does not stop that person from putting an
  older copy of the stored file back.
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

## 12. The Key app's lock and its screens

The Key app keeps the library's state behind a PIN, and enrols cash-out
addresses from its own camera. SR-71 to SR-73 are kept by `openbmt-key-ffi`,
which holds the Key app's store and calls `openbmt-lock` for the frame, the
cost and the PIN rules (`docs/LOCK-RULES.md`, LC-01 to LC-07: the owner app
keeps its key 1 in the same frame); SR-74 to SR-79 by the Android app
(`apps/key-android`), with the tries, the waits and the wipe of
`apps/android-common` (`LockModel.kt`), which the owner app uses as well.
It also pairs a Box (`docs/BOX-RULES.md`, BX-10 to BX-17, and AS-78 to AS-82
of `docs/SESSION-RULES.md`): SR-80 and SR-82 are kept by `openbmt-key-ffi`,
SR-81 and the screens of SR-82 by the Android app.

- **SR-71. The locked frame.** What the Key app stores is a frame `OBMx`,
  version 1: the Argon2id parameters (memory in KiB, passes and lanes, each 4
  bytes), a 16-byte salt, a 12-byte nonce, the length of the ciphertext, and
  the ChaCha20-Poly1305 ciphertext of the stored state (`OBM7`), with the
  whole header as associated data. The parameters are 64 MiB, 3 passes and 1
  lane. A file that asks for more than 256 MiB, 10 passes or 4 lanes, or for
  no passes or no lanes, is refused before Argon2 runs. The salt and the
  nonce come from the platform's random source. Every write has a new nonce,
  and a write whose nonce would repeat the last one under the same key is
  refused. Decoding is strict: version 1, the exact length, and only bytes
  that writing the frame again would give. Every write of the state goes
  through the frame. The Android Keystore's seal stays outside it, so that
  the device and the PIN are both needed. The frame and the cost are LC-01,
  LC-02 and LC-04.
- **SR-72. The PIN.** A PIN is at least 6 digits, or a passphrase of at least
  8 characters, and at most 128 characters in either case (`pin-weak`,
  `pin-long`, LC-03). It is set before a new or restored key is made, so that
  the first store is already locked (`pin-first` until then), and it is set once
  (`pin-set`). A state stored by an older build, with no frame, opens only
  into the step that sets a PIN (`pin-needed`): setting it stores the same
  state behind the PIN, with nothing changed if the store fails, and the
  app goes on.
- **SR-73. Locked.** While a PIN is waiting, the Key app answers its setup
  state (locked), its lock status and the unlocking, and refuses every other
  call with the rule `locked`, before it reads what the call brought. A wrong
  PIN is refused with the rule `pin-wrong` and changes nothing. A frame whose
  ciphertext was changed gives the same answer; a frame that is cut,
  lengthened or of another form is refused as unreadable (`OBMx`). The PIN
  is changed with the current one, under a new salt, in one rewrite of the
  stored state.
- **SR-74. Key files stay out of backups.** The files that hold the key (the
  stored state and the tries file of SR-75) are in the app's no-backup
  folder, and the manifest excludes every domain from cloud backup and from
  device transfer.
- **SR-75. Wrong tries and waits.** The app counts wrong PINs in a small file
  of its own: the count, and the time of the last wrong one. It writes the
  count plus one before each attempt, and 0 after a right PIN. A refusal that
  is not `pin-wrong` gives the step back. After 1 to 4 wrong tries the next
  attempt waits for nothing; after 5, 30 seconds; after 6, 1 minute; after 7,
  5 minutes; after 8, 15 minutes; after 9, 1 hour. From the 5th wrong try
  the screen says how many tries are left before the key is wiped, and the
  time left to wait counts down. A clock behind the time of the last wrong
  try restarts the wait from now, and that time is stored.
- **SR-76. The wipe.** The 10th wrong try deletes the sealed state file and
  the Keystore key, then the tries file, and shows that the phone's copy of
  the key was wiped. It says that the money is not lost, because the wallet
  needs two of three keys and the other two still exist, and that the next
  step is to restore the key from its 24 words on this phone, or to tell the
  owner.
- **SR-77. Locked on start and on return.** At start, and after the app has
  been in the background for more than 60 seconds, the app shows the unlock
  screen before any other. The 60 seconds include the time spent in the
  system's file picker or permission dialog: the app notes the time when it
  hands over to the system, and when the result comes back (a file picked,
  the picker cancelled, the permission answered) or the app is resumed after
  more than 60 seconds, the result is dropped unread, the session is dropped
  and the unlock screen comes first, as after any other relock. It draws a
  "working" page before it asks the library to unlock, because the key from
  the PIN takes about a second.
- **SR-78. Cash-out addresses come from the camera.** The call takes the text
  the Key app's camera read and the label typed on the Key app. The text is
  a bare address or a `bitcoin:` URI (BIP 321), and only its address is
  taken. Refused, each with its own words: an address of another network, a
  script kind outside SR-22, a label outside SR-30, a 65th entry and an
  address already enrolled. The state is stored before the call returns. The
  screen has no way to paste or type the address; it shows the address in
  groups of four and says that it becomes usable in 24 hours on this phone's
  clock.
- **SR-79. The list.** Each entry shows its label, its address, the time it
  was enrolled, and either "active" or the time left until it is, counted
  from the current time of SR-31. "Remove" asks for a confirmation and takes
  effect at once (SR-30).
- **SR-80. The invitation.** `make_invitation` makes an invitation (BX-10)
  from 17 bytes of the platform's random source, stores the state with it and
  an end 30 minutes after this phone's clock (BX-11), and only then returns it:
  its 30 characters in six groups of five separated by hyphens, the end in
  seconds since 1970 and the seconds left. A failed store gives no invitation
  and changes nothing. One invitation is open at a time: a second call replaces
  the first. `open_invitation` gives the invitation that is open and has not
  run out on this phone's clock, or nothing; it is the same after the app is
  closed and opened again. `cancel_invitation` forgets it, stores first, and
  is not an error when none is open. All three are refused while a session is
  open.
- **SR-81. "Pair a Box".** The home screen has a "Pair a Box" button. With no
  invitation open the screen offers to make one. With one open it shows the 30
  characters in two lines of three groups, large and in a fixed-width type,
  with the words that say it works once and for 30 minutes, to type all 30
  characters into the Box's own setup page, and not to type them on a shared
  computer or send or photograph them. The time left counts down each second on
  this phone's clock. At zero the screen says the invitation has run out and
  offers a new one. "Cancel this invitation" forgets it. Leaving the screen, or
  the app going to the background, takes the characters off the screen at once;
  the invitation stays open until it runs out, is cancelled or is used. The
  window carries FLAG_SECURE like every screen of the app (SR-74).
- **SR-82. "Box found".** A session that holds a Box's claim (AS-78) shows its
  line with the title "Box found": the Box's key in full in groups of eight
  hexadecimal characters, the four fingerprint words of that key large on one
  line, and the three checks of AS-81 each with its result. The words say to
  approve the settings that add the Box first and then to approve only if the
  four words are the same, in the same order, as on the Box's own screen. A
  claim that fails a check shows what failed and cannot be approved. Approving
  makes the welcome and spends the invitation; declining signs nothing and
  spends it too; a session that is stopped without an answer spends nothing
  (AS-82). A line whose four words could not be made says so and has nothing
  to compare.
- **SR-83. A change of light and dark.** A change between light and dark while
  the app runs does not finish and start the activity. The activity reads the
  palette of the new mode, sets the window colour, the bar colours and the
  status-bar icons again, asks for the insets again, and asks the screens to
  be drawn again in place. A page's padding for the bars is worked out
  whenever its views are built, so no gap shows above its band. The Key app
  draws the screen from the library's state, the same pick as at start: a
  locked app comes back as the unlock screen, never as a screen that skips the
  PIN, and a screen that cannot be drawn from state gives way to the one the
  state names. Nothing is drawn while the screen is cleared, or while a file
  picker or the camera is open.
