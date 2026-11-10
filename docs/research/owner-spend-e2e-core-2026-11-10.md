# The owner node's coins and sending against Bitcoin Core (2026-11-10)

**Question.** Does the owner's device learn the coins of the 2-of-3 wallet
from checked blocks alone, send a refund that two signers made, and follow it
through the peers, a restart, a block, a conflicting spend and a
reorganization, with its coin view equal to Core's at each stop?

**Answer.** Yes, in the run made. All eight steps passed in the last run, after
the last functional change. The runs before it found two things in the check
itself and one real defect in the network crate (see "What the runs found").

## Method

`scripts/owner-spend-e2e-core.sh` runs the whole check on regtest and prints a
PASS or FAIL for each numbered step. The pieces:

- Three Bitcoin Core v32.1.0 nodes from `~/.local/opt/bitcoin-core-32.1/bin`
  only, each with a fresh data directory under `~/.cache/openbmt/`, removed on
  every exit. They serve compact filters (`-blockfilterindex=1
  -peerblockfilters=1`) and listen on 127.0.0.1, 127.0.0.2 and 127.0.0.3, so
  that the driver counts them as three network groups (`host_group`, used
  only when local peers are allowed). They are connected to each other. The
  script refuses a `bitcoind` that is not the one in that directory and
  never touches `~/.bitcoin`.
- The owner node is the example `crates/openbmt-owner-node/examples/
  owner_spend_regtest.rs`, driven by lines on its standard input. The Key app
  (with key 2), key 1 and the owner's queue are in that process. Their states
  are kept in its data directory, so a restart does not forget which indexes
  were signed. The wallet keys come from fixed test bytes: test only, never
  use. The three nodes are its fixed peers. It sends through the main driver
  (the setting "send without Tor"), and says so in a note each time.
- Core holds the wallet watch-only (a blank wallet with private keys
  disabled, the description imported as an active multipath descriptor). The
  coin view is compared with `listunspent` as lines of `outpoint amount
  confirmations`.
- Versions: Bitcoin Core 32.1.0.

## Results

| Step | What was checked | Result |
|---|---|---|
| 1 | The description imports into the watch-only wallet. Core's derivation of the first two receive addresses equals the node's. Two payments (300,000 and 200,000 sat) are mined, the second five blocks deep. The node's two coins equal `listunspent`: same outpoints, amounts and confirmations (6 and 5). | PASS |
| 2 | A refund of 50,000 sat is queued and `spend` builds a session; the Key app approves, key 1 signs, the node takes the response. A transaction comes out, the refund entry is Sent, the transaction was handed to the driver once, and the node noted that it was sent without Tor. | PASS |
| 3 | All three mempools hold the transaction. The node's state for it is "seen back" from at least two groups other than the one it was first announced to (one peer asked, one fetched). | PASS |
| 6 | The node is stopped and started again on the same directory before the block. It holds the transaction and follows it again, the outbox holds one transaction and no session is open, the refund is still Sent, nothing was spent a second time, and each mempool still holds exactly one transaction. | PASS |
| 4 | A block confirms it. The state is confirmed, one deep. The node's coins equal `listunspent`: the change (249,527 sat, one deep) is a coin, the spent coin is not. | PASS |
| 5 | Four more blocks: five deep is not done. One more: six deep (the refund depth) is done, the refund is Paid and the outbox is empty. | PASS |
| 7 | A second refund is queued. A conflicting transaction that spends the same coins is made on a copy of the queue and mined with `generateblock`. The real transaction (sent to the peers) is replaced: its refund is back as Returned, the outbox is empty, no session is open, and nothing was signed again. | PASS |
| 8 | A third refund is sent and mined, one deep. The block is invalidated on all three nodes and two empty blocks take its place. The node holds the transaction again and sends it again, and its coins equal `listunspent` counted from 0 confirmations: the old coin three deep and the change at none. | PASS |

## What the runs found

- **A restarted node reported "not seen back" at once.** After the restart
  the node's first act is to send what it still holds. The network crate's
  manager had been given no time yet, so the transaction's ten minutes began
  at time 0 and were over at the first tick. The state went to `NotSeen` and
  the transaction was announced again in the first second. The manager now
  gives such a transaction its ten minutes from the first time it is given
  (NR-88), and a test names the case
  (`nr_88_a_transaction_held_before_the_first_time_is_given_waits_its_ten_minutes`).
- **The check counted its own commands wrongly.** The first runs of the
  script counted the example's commands in a subshell, so the wait for an
  answer asked for the wrong one and read nothing. The count now lives in the
  main shell, as it does in `scripts/counter-e2e-core.sh`.
- **The first receive derivation compared both keychains.** `deriveaddresses`
  on a multipath description returns the receive and the change addresses as
  two lists; the script now takes the first.

## What this does not show

- It does not use Tor. The sending is the "without Tor" setting; the second
  driver behind a proxy has unit tests with fake drivers, and no run against a
  real proxy.
- "Seen back" is announcements from peers that were not asked first. It is
  not proof that a miner has the transaction, and the node says so in the
  state's text.
- Three nodes of the same software on one machine are not a network. The
  groups are counted by loopback address.
- Marks that grow from the owner's own queue (a batch, a spend's change
  index) rescan nothing (`docs/OWNER-COINS-RULES.md`, OC-07). The run shows
  it: the view is ready right after the batch and after the spend, and the
  example never printed a note about checking blocks again. An earlier
  version rescanned from the birthday on every such growth and the view was
  not ready until the rescan had caught up. One window is left: a block the
  driver matched before it received the new script and that was mined after
  the script was made. `rescan` closes it on request.
- Reorganizations deeper than 100 blocks are the named state "deep reorg:
  rescan" and are covered by unit tests, not by this run.

## Reproducing

```
scripts/owner-spend-e2e-core.sh
```

Environment: `CORE_BIN`, `DRILL_DIR`, `DRILL_RPC_PORT` (first of three,
default 18980), `DRILL_P2P_PORT` (default 18990), `DRILL_KEEP=1`,
`OWNER_BIN` (a prebuilt `owner_spend_regtest`). A build goes through
`~/.cache/openbmt/guard.sh` when that file exists.
