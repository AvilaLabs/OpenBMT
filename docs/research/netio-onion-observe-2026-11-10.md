# The network driver dials onion peers through Tor (2026-11-10)

**Question.** With a Tor proxy set, does `openbmt-net-io` dial Tor v3
(onion) peers, group them as NR-93 says, and keep the chain and the block
checks working with them among its peers, on mainnet?

**Answer.** Yes. In a 20-minute run through Tor the driver learned 128
onion addresses from `addrv2` gossip, started 10 onion dials, and 4 became
ready peers, in 4 different network groups. Two of the four stated the
compact-filter service. Never more than 4 of 8 slots were onion, as NR-94
says. No onion peer closed or was dropped in the 20 minutes. The 6 dials
that did not end in a peer failed on Tor's side: 3 "host unreachable" and 3
that did not answer within the 20 seconds that NR-26 allows a proxy
exchange.

## What was run

- Tor 0.4.9.16 from the Tor Browser bundle 16.0.1, as a client only: its
  own data directory, one SOCKS port on the loopback address, no bridge, no
  onion service of its own. It ran only for this run and was stopped by its
  process number.
- `crates/openbmt-net-io/examples/observe.rs` on mainnet, release build,
  with `OPENBMT_PROXY` set to that port: the real `Driver`, default seeds,
  8 peers, `FileStorage`, one watched script that nobody pays.
- A scratch data directory that held only the stored headers of an earlier
  observe run (tip 975335, so the run still had 24 blocks to catch up) and
  no address book. The book started empty: every address came from the seed
  lookups through Tor or from peers.
- 1,200 seconds, one run. The example now prints onion counts and prints
  every onion name as `<onion>`. This note holds no address and no key.

## What was seen

| What | Result |
|---|---|
| Onion addresses in the book | 0 at the start, 128 at the end |
| Onion dials started | 10 |
| Onion dials that failed | 6: 3 host unreachable, 3 no answer in 20 s |
| Onion peers ready | 4 (4 distinct) |
| Groups of those 4 (NR-93) | 4 different groups |
| Onion peers that state compact filters | 2 of the 4 |
| Most onion peers ready at once | 4, with 8 slots (the cap of NR-94) |
| First onion peer ready | 58.5 s |
| Peers ready (target 8) | 8 at most, first reached at 699 s |
| Onion peers closed or dropped | none |
| All other closes | 9 ("peer closed the connection", clear-net) |
| Other drops | 1, NR-12: protocol 70015 |
| Blocks checked | 24 (975336 to 975359) |
| Blocks fetched in full | 9, in the first 41 s, while catching up |
| Blocks checked without a full download | 15 |
| Seed lookups through Tor | 17 answered of 21 |
| Tip | 975335 to 975359 |
| System lookups | none: with a proxy no name reaches the resolver |

- The ready peers went from 1 to 7 in the first 111 s and then stayed at
  7 until 699 s, so the onion peers were 4 of the first 7 and 4 of the
  final 8. The draw alternates between the kinds with equal chance while the
  onion count is under the cap (NR-94); the cap held.
- 128 is 16 groups of 8, the most the book keeps of onion addresses (NR-22).
  The count suggests that the book was full for onion addresses and that the
  gossip held at least as many as it takes; the spread over the groups was
  not looked at.
- The first block fetched in full came 15 s into the run and the last at 41
  s. The blocks after were checked with two filter peers in two groups
  (NR-42). Whether an onion peer was one of the two filter peers for a
  given block was not recorded.
- Each onion dial gave the proxy its own name and password (NR-96). Whether
  Tor put each on its own circuit was not looked at on Tor's side.
- The driver sent `sendaddrv2` before `verack` (NR-95, NR-13) and read
  `addrv2`: the 128 addresses came that way, as the seeds give none.

## Not covered

- The 20-second limit on the exchange with the proxy (NR-26) is short for an
  onion service, where Tor builds a rendezvous circuit. 3 of the 10 dials
  ended at that limit. Whether a longer limit would turn them into peers
  was not tried.
- Which of the two filter peers of a block were onion peers.
- A run longer than 20 minutes, a restart with onion anchors (NR-24), or an
  onion peer that misbehaves.
- The encrypted transport (BIP324) to onion peers: the counter tries it as
  for every peer, and how many of the 4 used it was not recorded.
- Tor's own view of the circuits.

The log of the run is kept outside the repository.
