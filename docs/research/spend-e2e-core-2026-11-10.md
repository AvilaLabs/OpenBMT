# The spend path against Bitcoin Core (2026-11-10)

**Question.** Does a transaction that `openbmt-spend` plans, the signer signs,
`combine` joins and `finalize` finishes get accepted by Bitcoin Core, and do
Core and the library agree about what the transaction is?

**Answer.** Yes, in every case run. All eight steps passed in three runs
after the library's last functional change, two of them after its last edit
of any kind. The first run failed at step 6 and showed a real difference
between Core's files and the signer's (see "What the run found").

## Method

`scripts/spend-e2e-core.sh` runs the whole check on regtest and prints a
PASS or FAIL for each numbered step. The pieces:

- Bitcoin Core v32.1.0 from `~/.local/opt/bitcoin-core-32.1/bin` only, with a
  fresh data directory under `~/.cache/openbmt/`, removed on every exit. The
  script refuses a `bitcoind` that is not the one in that directory and never
  touches `~/.bitcoin`. The node runs with `-listen=0` and `-txindex=1` (the
  example reads previous transactions with `getrawtransaction`).
- The wallet is the usual 2-of-3 `wsh(sortedmulti(2, ...))` with BIP48 keys.
  The keys come from fixed test bytes (`[n; 32]` for key n) in the example
  `crates/openbmt-spend/examples/spend_regtest.rs`: test only, never use.
- Core holds the wallet watch-only (a blank wallet with private keys
  disabled, the description imported as an active multipath descriptor), and
  key 3 as a second Core wallet with the private account key in its receive
  and change descriptors.
- The planner, `combine` and `finalize` run in the example, which reads Core's
  `listunspent` and `getrawtransaction` output as one line per coin. The
  signer is the real `openbmt-signer`, called by the same example as key 1 or
  key 2 in separate runs, each with its own state file. The signer's
  ceilings are raised to their highest on first use, as the owner would on the
  Key app's screen, so that the run meets the builder's rules and not the
  owner's limits.
- Versions: Rust 1.98.1, jq 1.8.1.

## Results

| Step | What was checked | Result |
|---|---|---|
| 1 | The description imports into a blank watch-only Core wallet. Core's derivation of seven receive and seven change addresses equals the library's. Five receive addresses are funded (40,000, 55,000, 70,000, 85,000 and 100,000 sat, one block after each, so the depths are 5 to 1). | PASS |
| 2 | The example reads the coins and plans a refund of 120,000 sat at 2,500 sat per 1000 vB: the three oldest coins (depths 5, 4, 3), change to change index 0 (Core's change address 0), fee 995 sat over 398 vB (`ceil(398 * 2500 / 1000)`), change 44,005 sat. | PASS |
| 3 | Core's `decodepsbt` reports the plan's fee, three inputs, two outputs with the planned amounts and addresses, the plan's transaction ID, and three key-path hints on every input. `analyzepsbt`: every input has its UTXO and none is final, next role `signer`. | PASS |
| 4 | The signer signs as key 1 and as key 2 in separate runs with no refusal; its fee and virtual size equal the plan's (995 sat, 398 vB) and it shows the second output as change at index 0. `combine` gives two signatures on each input and `finalize_plan` the transaction with the planned transaction ID. | PASS |
| 5 | `testmempoolaccept` allows it. Core's fee equals the plan's. Core's vsize is 397 or 398 (the plan counts signatures at their longest; real ones are shorter by one byte or two), so the rate is 995/397 = 2,506 or 2,500 sat per 1000 vB, at or above the 2,500 asked. `sendrawtransaction` returns the plan's transaction ID. After a block the refund address holds 120,000 sat and the watch-only wallet shows the 44,005 sat change at change index 0. | PASS |
| 6 | A second refund of 150,000 sat at 3,000 sat per 1000 vB (two coins, change index 1). Core's key-3 wallet signs it with `walletprocesspsbt`, the signer signs it as key 1 and as key 2. Keys 1 and 2: Core's `combinepsbt` and `finalizepsbt` give the same transaction bytes as `finalize` (byte-equal). Core's key 3 with the signer's key 1: `combine` and `finalize` succeed, the result is byte-equal to Core's own finalize of the same two files, `testmempoolaccept` allows it, it is sent and confirmed, and the change is at change index 1. `testmempoolaccept` allows the keys 1 and 2 version too (same transaction ID, other witness). | PASS |
| 7 | `send_all`: both remaining coins (the two change coins) go to one Core address, no change output; the amount paid is the coins minus the fee; Core allows it at a rate at or above 2,000 sat per 1000 vB; after a block the address holds everything and the watch-only wallet has no coin and balance 0. | PASS |
| 8 | Two fresh coins are planned. The example puts a signature made for input 1 in place of key 2's signature on input 0. `finalize` refuses the file (the signature by the named key on input 0 does not verify). The example then builds the transaction by hand with the same bad witness (a debug path in the example only; the library has none), and Core's `testmempoolaccept` refuses it: `mempool-script-verify-flag-failed (Signature must be zero for failed CHECK(MULTI)SIG operation)`. | PASS |

## What the run found

- **Core writes previous transactions without their witnesses.** The first
  run of step 6 failed in `combine`: the file Core's `walletprocesspsbt`
  returns carries each previous transaction (`non_witness_utxo`) serialized
  without its witnesses, so it is shorter than the signer's copy (137 bytes
  against 246 in the run) and not equal to it byte for byte. The library
  compared the field for equality and refused the copy under SP-50. The
  transaction ID is the same, and the ID commits to every output, so the
  library now treats two previous transactions as the same when their IDs
  are the same (SP-50, with a test). The signer was not involved: it reads
  such a file as it should (SR-10 compares the hash).
- **Core's finalize and the library's finalize agree byte for byte.** Both
  put the signatures in the order of their keys in the sorted script, take the
  first two when three are present, and use the same signatures, so the
  bytes are the same. Nothing differed.
- **The planned size is one or two virtual bytes above the real one.** At
  three inputs, Core's vsize was 397 or 398 against the plan's 398: the
  slack of counting every signature at 72 bytes (SP-31).

## What is NOT covered

- Only regtest, and only Bitcoin Core 32.1 as the other implementation. No
  hardware signer, no other wallet software.
- `testmempoolaccept` applies Core's standard policy together with consensus
  script verification. A consensus-only check (without the policy flags) was
  not run separately, and no high-S signature was sent to Core (the library
  refuses one under SP-53; Core refuses it by policy).
- The case where the change is dropped into the fee (SP-23), a plan with
  unconfirmed coins (`min_confirmations` 0) and the consolidation to the
  change script (SP-24) are covered by the library's tests and by the
  signer's own review of the file, not against Core.
- The mixed case is Core's key 3 with the signer's key 1. Core's key 3 with
  the signer's key 2 and Core's key 1 or 2 were not run.
- The forced transaction of step 8 has one kind of wrong signature (made for
  another input). A signature by a key outside the script, one made over
  another transaction and a wrong hash type are in the library's tests.
- Nothing is broadcast to a network other than the private regtest node, and
  no Tor path is used.
- The signer's ceilings were raised for the run. The ceilings and the
  allowlist are not exercised here; the signer's own tests and
  `scripts/signer-interop-core.sh` do that.
- No fee estimation and no replacement (fee bump) of a spend-path transaction.
- The fuzz target `spend_combine` was built (`cargo check` of the fuzz
  project) but not run.

## What did not fit

- A step that signs with Core as key 1 or key 2 as well as key 3: Core's
  wallets for those keys would need the same import as key 3's, and the
  signer-interop script already shows Core signing as keys 1 and 3.
- A run against a Core with `-txindex=0`: the example would need the
  previous transactions from the wallet (`gettransaction`), which the script
  could supply; the current script starts the node with `-txindex=1`.
- A step on change-index handling after a restore (the signer's change mark
  starts at 0 and the caller must supply the next unused index).
