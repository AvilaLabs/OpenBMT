# The network driver against Bitcoin Core (2026-11-10)

**Question.** Does `openbmt-net-io` connect to Bitcoin Core, sync headers,
report a payment seen before it is mined, hand over the block that holds
it, follow a reorganization, and start again from its stored headers
without downloading them again?

**Answer.** Yes, in this run: every step passed against one Core node on
regtest, in both runs after the script was fixed (the first run failed in
step 6 because of the script, not the driver; see below). The one thing
worth noting is under step 2: the node's start height was one low in two of
three runs.

## Setup

`scripts/netio-interop-core.sh` runs the whole check and fails at the first
step that does not hold. The driver is the example
`crates/openbmt-net-io/examples/regtest_node.rs`: the real `Driver` with
the real sockets, `FileStorage`, the system clock and `/dev/urandom`, one
fixed peer (the node, with `allow_local` because it is on this machine) and
one watched script. The example prints each event the app would see, one
line each. The script reads those lines, and drives the node with
`bitcoin-cli`.

The node printed `Bitcoin Core daemon version v32.1.0`. It ran on regtest
in a scratch directory with `-blockfilterindex=1 -peerblockfilters=1
-listen=1 -bind=127.0.0.1:18957 -txindex=0`, and was stopped at the end.
The script used only the binaries in `~/.local/opt/bitcoin-core-32.1/bin`
and removed the node and the scratch directory (the node's data and the
driver's files) on every exit. A wallet created for the test mined 150
blocks to itself. The
driver was told to check blocks above height 150, so the history is not
downloaded.

## Steps

The lines are from the last run. Hashes change from run to run.

| # | Step | Result |
|---|---|---|
| 1 | Node with filters; 150 blocks; a fresh address to watch | PASS |
| 2 | Handshake; first `Peers` event at the genesis tip; then one ready peer that offers filters | PASS |
| 3 | All 150 headers arrive as `Chain` events; no block at or below height 150 is downloaded | PASS |
| 4 | A payment to the watched address is a `Sighting` with its txid, 100,000 sat, from 1 peer | PASS |
| 5 | Mining a block: a `Block` that contains the payment, and `Chain` for height 151 | PASS |
| 6 | `invalidateblock` on the tip, two new blocks: `Chain` with 2 joined and 1 left; the new tip is fetched | PASS |
| 7 | Stop, mine one block, start again: starts at the stored tip, one header joins, the node sends 106 bytes of headers | PASS |

**2. Handshake and Peers.** The first line is the start, before any peer:
`peers 0 ... tip=0`. Then:

```
peers 1 filters=1 best=149 tip=0 ahead=yes
```

One ready peer that offers compact filters. `ahead=yes` is right at this
point: the peer claims a height 149 above a tip of 0 and has sent no
headers yet (NR-33). It goes back to `no` once the headers are in.

**3. Headers.** `chain joined=150 top=150 left=0` in one message of 150
headers, and `peers 1 filters=1 best=150 tip=150 ahead=no`. The node's
`getpeerinfo` showed 12,175 bytes of `headers` messages sent to the driver
(150 headers of 81 bytes each, which is 80 and a zero transaction count,
a one-byte count and the 24-byte message header).

**4. A payment before it is mined.**

```
sighting 0182...16b7 from=1 pays 100000
```

Core announced the transaction by `inv`; the manager asked for it, and the
driver handed the bytes on. This is the path the shop uses to accept a
payment before the block.

**5. The block.**

```
block 5f57...2265 471 pays 0182...16b7:100000
chain joined=1 top=151 left=0
```

With one peer there are fewer than two peers that offer filters, so the
manager fetches the block itself (NR-42). The block is 471 bytes and the
example finds the payment in it.

**6. A reorganization.** `invalidateblock` on that tip, then two blocks to
another address (see below), gave:

```
block 5f57...2265 471 pays 0182...16b7:100000
chain joined=2 top=152 left=1
block 20a2...c457 471 pays 0182...16b7:100000
block 7aad...992a 249 pays
```

The first line is the block that left, fetched before the reorganization.
The second is the reorganization. The third is the replacement block at
height 151, which holds the same payment (it went back to the node's
mempool and was mined again), and the last is the new tip.

**7. Restart.** The first run took 12,175 bytes of `headers` messages from
the node. After `quit` on the example's input, the driver wrote
`headers.bin` and `book.bin`; one block was mined while it was stopped. The
second run printed `peers 0 ... tip=152` before it had talked to anyone,
reached tip 153 with one header joined, and the node had sent 106 bytes of
`headers` messages to it: one header of 81 bytes, a one-byte count and 24 bytes
of message header. So
the driver asked from its stored tip and was not sent the chain again. The
script reads the 106 from the node's `getpeerinfo`
(`bytessent_per_msg.headers`), because the `Chain` events alone cannot tell:
headers the store already holds produce no event.

## Things seen that are not failures

- **A reorganization to the same block.** The first version of the script
  mined the two replacement blocks to the same address as the one it had
  invalidated. Core refused (`block not accepted`): the new block had the
  same parent, the same transactions and the same second as the one marked
  invalid, so it was the same block. The script now mines to a second
  address. This is the node, not the driver.
- **Start height lags.** In two of three runs the node's start height was
  149 on a node at 150, right after mining. The earlier note on the session
  (`net-interop-core-2026-11-10.md`) saw the same, once. It is a claim
  (NR-12) and the driver does not rely on it: the first `getheaders` brings
  the headers up to 150.
- **`ahead=yes` at the start**, as under step 2.

## What this does not show

- One honest peer, on one machine, on regtest. No bad peer on a real
  network, no seed, no Tor and no proxy. The SOCKS5 client and the seed
  fallback are tested against fake peers on loopback sockets in the crate's
  unit tests, not against Core or a Tor daemon.
- A peer on this machine is not allowed in the counter (NR-20); this check
  turns that off with `allow_local`.
- One peer, so no filter checks across two peers and no choice among peers.
- Headers saved when the driver stops. The 60-second save and the
  10-minute anchors are not reached in this run.
- Only Bitcoin Core 32.1. Other node software was not tried.
- Timing is not measured. The blocks are small.
