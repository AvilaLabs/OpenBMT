# The peer session against Bitcoin Core (2026-11-10)

**Question.** Does `openbmt-net` complete the handshake with Bitcoin Core,
and does it fetch headers, filters, blocks and unconfirmed transactions
that the watcher's checks accept?

**Answer.** Yes, in this run: every step passed against one Core node on
regtest. One surprise, recorded under step 8: Core announces its tip with
an `inv` right after the handshake, even though the session has sent
`sendheaders`.

## Setup

`scripts/net-interop-core.sh` runs the whole check and fails at the first
step that does not hold. The driver is the example
`crates/openbmt-net/examples/regtest_peer.rs`: a blocking TCP loop around
one `Session`, with the system clock, that feeds every header it receives
to the watcher's `HeaderStore` for regtest. It opens a new connection for
each step.

The node printed `Bitcoin Core daemon version v32.1.0`. It ran on regtest
in a scratch directory with `-blockfilterindex=1 -peerblockfilters=1
-listen=1 -bind=127.0.0.1 -txindex=0`, and was stopped at the end. The
wallet mined 2,105 blocks to itself. That is one full `headers` message of
2,000 and a second of 105, so the continuation rule (NR-30) is used.

## Steps

The lines are from the last run. Hashes change from run to run.

**1. Node and chain.** PASS. Height 2,105, filter index caught up.

**2. Handshake.** PASS.

```
ready c49 2105
filters yes
sent version sendaddrv2 verack sendheaders
```

Services `0xc49` are `NETWORK`, `WITNESS`, `COMPACT_FILTERS`,
`NETWORK_LIMITED` and `P2P_V2`. The session sent its `version`,
`sendaddrv2`, `verack` and, after the peer's `verack`, `sendheaders`, in
that order (NR-13).

**3. Header sync.** PASS.

```
headers 2105
messages 2
tip 2105 19c07494ae2a59938c33006e7427294a3e39f2845dd336b4dc6151cc6c51da32
```

All 2,105 headers passed the header store (WR-05 to WR-07), and the tip is
the node's best block.

**4. A new block, mined while the session waits.** PASS.

```
waiting
announced-headers 1
tip 2106 18624f6ce79ebebf5b381f4e2f82e1dcecc42b53ab3ec68f72b239d6b56e89d8
```

The block came as a `headers` message, not an `inv`, as `sendheaders`
asks. The store's tip is the node's new best block.

**5. An unconfirmed payment.** PASS.

```
announced 0d5188480ae5020a4dc1c5e02abbc80f2bb30b78419688fd2655928a95de6906
tx 0d5188480ae5020a4dc1c5e02abbc80f2bb30b78419688fd2655928a95de6906 100000 yes
```

Core announced the transaction by `inv` of its txid (the session does not
send `wtxidrelay`), the session asked for it with the witness flag, and
the transaction paid 100,000 sat to the fresh address and has witness data.

**6. The block's compact filter.** PASS.

```
filter-bytes 9
match yes
filter-bytes 9
match no
```

The first line pair is the filter of the block that holds the payment,
matched against the payment's script. The second is the same filter
against an unrelated script. The filter names the block it is for, and the
driver checked that it is the block asked for.

**7. The full block.** PASS.

```
block-bytes 471
payments 1
payment 0d5188480ae5020a4dc1c5e02abbc80f2bb30b78419688fd2655928a95de6906 1 100000
```

`payments_in_block` read the block fetched by the session, against the
header store the same session filled, and returned the one payment, at
position 1 for 100,000 sat.

**8. Silence after the handshake.** PASS.

```
received version
received wtxidrelay
received sendaddrv2
received verack
received sendcmpct
received ping
received feefilter
received inv
event announced
event-block 034412ab3de7d7958a5ed3ad584cba3fbba5951994bb713800cca1f5e264368f
sent-bytes-after-handshake 0
node tip 034412ab3de7d7958a5ed3ad584cba3fbba5951994bb713800cca1f5e264368f
```

For 15 seconds the session sent nothing but its answer to Core's `ping`,
and was not dropped (NR-55). Core sent `wtxidrelay`, `sendcmpct` and
`feefilter`, which the session ignores (NR-03), and a `ping`, which it
answered. Core did not send a `getheaders` in this run.

The `inv` is a block entry, and the hash is the node's tip. So Core
announced its tip to a new inbound peer by `inv` although the session's
`sendheaders` was already on the wire. The likely reason is that Core
chose its announcement before it read `sendheaders`; this was not checked.
This costs the session one extra event, and
a driver that wants the header asks `getheaders`; the example does so in the
`announce` step. It is not a rule breach.

**9. A port that is not a Bitcoin peer.** PASS.

```
dropped NR-14: the handshake took 30 seconds or more
exit code 2 after 30 s
```

The Core RPC port accepts the connection and never answers a `version`.
The session ended it at the 30-second handshake limit instead of hanging.

## Things seen that are not failures

- **Start height lags once.** In an earlier run, the handshake came back
  with start height 2,104 on a node at 2,105, right after the burst of
  mining. The start height is a claim (NR-12) and Core states the height of
  its last tip notice. The script now tries the handshake again, once a
  second, up to 10 times, and prints any other height it sees. The last
  run needed no retry. The cause was not examined further.
- **The `inv` of the tip** under step 8.
- **Mining is slow.** The 2,105 blocks take about four minutes on this
  machine. That is the node, not the session.

## What this does not show

- One honest peer, on one machine, on regtest. No bad peer, no peer that
  lies about headers, filters or blocks, and no peer on the Internet.
- Only Bitcoin Core 32.1. Other node software was not tried.
- No Tor and no encrypted transport.
- Timing is not measured: no throughput, no latency, no behaviour under a
  slow or stalled peer, and the 2-minute ping was not reached.
- The blocks are small (one payment, one coinbase). Large blocks near the
  4,000,000-byte limit and filters of a busy block are not tested here.
- Only one session at a time. The manager of several peers, the address
  book and the filter checks across two peers (NR-20 to NR-26, NR-40 to
  NR-46) are later slices.
