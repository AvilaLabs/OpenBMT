# openbmt-signer

The rules under which the OpenBMT Key app signs with key 2, as a Rust library
with no network, no clock and no storage. The caller passes the time in
seconds and a `persist` closure; the library hands that closure the new state
as bytes and releases a signature only after the closure returns `Ok`.

The specification is [docs/SIGNER-RULES.md](../../docs/SIGNER-RULES.md). Each
rule has an ID (SR-01 and on). Every refusal names its rule
(`Refusal::rule()`), and every test is named for the rule it proves.

**This crate has had internal review only. No independent security audit has
been done yet.** Do not use it with real funds.

## The calls

| Call | What it does | Rules |
|---|---|---|
| `SignerState::register` | Reads the wallet description once and finds this device's slot (and key 3's). Starts with empty state and the first values. | SR-39, SR-60 to SR-63 |
| `SignerState::review` | Reads a PSBT file and checks everything that needs no key. Changes nothing. Returns a `Review` or a `Refusal`. | SR-01 to SR-25, SR-33 to SR-36, SR-40 to SR-43, SR-50 |
| `SignerState::sign` | Takes the `Review` itself, checks the kinds of the outputs, the change index and the ceilings again at the current state and time, signs, stores the new state, and returns the signed file. | SR-51 to SR-56 |
| `enroll_destination`, `remove_destination` | Owner calls on the Key app's own screen: the allowlist. | SR-30 |
| `set_ceilings`, `set_fee_ceilings` | Owner calls: the six ceilings and the two fee ceilings, inside the compiled-in bounds. | SR-37, SR-40 to SR-42 |
| `raise_change_mark`, `raise_receive_mark` | Owner calls: raise the change mark or the receive mark. | SR-34, SR-70 |
| `correct_clock` | Owner call: confirm a clock correction. | SR-31 |
| `cosign_batch` | Co-signs an address batch with `openbmt_core::batch::Approver`, keeping the receive mark in this one state. | section 10 |
| `to_bytes`, `from_bytes` | The `OBMK` snapshot, strict. `from_bytes` takes the network and this device's account key again and re-checks every invariant. | wire rules of `openbmt-core` |

Every call that changes state works on a copy, hands the bytes of the copy to
`persist`, and replaces the state only if `persist` returns `Ok`. A refusal and
a failed store are different errors (`Refused` and `Persist`), and a refusal
never reaches the store.

## How the rules map to the code

| Rules | Where |
|---|---|
| SR-01 to SR-15 (reading the file, inputs) | `src/analysis.rs`: `analyze` |
| SR-20 to SR-25 (outputs), SR-34 | `src/analysis.rs` (hints), `src/review.rs`: `classify` |
| SR-33, SR-35, SR-36, SR-40 to SR-43 | `src/review.rs`: `limits`; `src/state.rs`: `plan_log` |
| SR-30 to SR-32, SR-37 | `src/state.rs` |
| SR-50 | `src/review.rs`: `Review` |
| SR-51 to SR-56 | `src/sign.rs` |
| `OBMK` snapshot | `src/snapshot.rs` |
| Refusals and their rule IDs | `src/refusal.rs` |

Some choices worth knowing:

- The file is read with `bitcoin::Psbt::deserialize`, which refuses PSBT
  versions above 0 and repeated keys. The size limit is checked first.
- The returned PSBT is built from scratch (SR-09): the unsigned transaction,
  the verified previous transactions, the witness scripts and key-path hints
  the signer derived, the signatures it checked and its own. Nothing else of
  the incoming file is copied.
- Signing uses `sign_ecdsa` only (RFC 6979 nonces, low S) over the BIP143 hash
  the signer computes from its own witness script and the amount in the
  previous transaction. Each signature is verified before it is returned.
- The signed size is the signer's own: two 72-byte signatures per input in a
  clone of the transaction. A test checks it against a really signed and
  finalized transaction.
- The time the rules use is the larger of the clock and the latest time seen.
  Only signing and enrolling move the latest time seen.

## What it deliberately does not do

These are the limits from section 11 of the rules.

- It does not know whether a payment is owed. Any figure that comes from the
  owner phone is the owner phone's claim. The ceilings bound what a false
  claim can cost.
- It counts signatures, not confirmed spends. The totals can only be too
  high, never too low.
- It does not see the chain: not whether an input is still unspent, nor how
  far in the future a lock time is.
- It depends on the Key app's clock for waits and windows. Someone who can
  unlock the Key app and set its clock forward can end the 24-hour wait early
  and empty the day and week totals.
- It is a promise of the Key app's software. A malicious build keeps none of
  these rules.
- It binds key 2 only. Key 1 and key 3 together can spend without the Key
  app.
- Anti-exfil signing is not in the rules yet. RFC 6979 nonces make the
  signatures reproducible, but do not stop a malicious build from choosing its
  own.

The library also leaves to the app around it: unlocking and wiping, refusing
to run under root or with USB debugging, accessibility services or screen
overlays, and labeling every figure from the owner phone as its claim. The
app also keeps the stored state: `from_bytes` checks the shape of a snapshot,
not where it came from, so the app must store it where nothing else can
write it or put back an older copy. Erasing
secret key material (SR-56) is best effort: the compiler and the operating
system can leave other copies in memory.

## Tests

`cargo test -p openbmt-signer`. There is at least one test for every rule ID
(a test checks this against the rules file), named `sr_NN_...`. Each refusal
test checks the exact `Refusal`, the rule it names, and that the state bytes
did not change. The fixtures use fixed keys and build PSBTs the way an honest
owner phone does: full previous transactions, witness scripts and key-path
hints for all three keys. The end-to-end tests sign with key 2, add a
signature from key 1 or key 3, and finalize the transaction. The signatures
and the witness script are checked against a BIP143 hash that the test writes
out again itself, so they do not rest on the code under test.

Other tests cover persisting (a failing store changes nothing; a working one
receives exactly the bytes of the new state; a refusal never reaches it), the
time boundaries (24-hour wait, day and week windows, clock correction), every
ceiling at its exact edge, and the snapshot (round trips, every truncation
and extra byte refused, every single-bit flip either refused or canonical,
and each re-checked invariant with an edited snapshot). Fuzz targets
`signer_state_decode` and `signer_review` are in `fuzz/` at the repository
root.
