# The network driver through Tor on the real network (2026-11-10)

**Question.** With every connection sent through a Tor SOCKS5 proxy
(NR-26), does `openbmt-net-io` find peers on mainnet, follow the chain and
check blocks, and does it still get compact filters?

**Answer.** It follows the chain and checks blocks, but slowly finds peers
and never found a filter peer. In about 21 minutes it held 2 to 4 ready
peers of a target of 8, in as many network groups, and had **no** peer
offering compact filters at any point. Both blocks that arrived were
therefore downloaded in full and checked (NR-42), which is the designed
fallback and is correct, but costs about 1.5 MB per block through Tor.
The cause is understood and is in the driver's design, not in Tor; see
"Why no filter peer". It is not fixed yet.

## What was run

- Tor 0.4.9.16 from the Tor Browser bundle 16.0.1, signature checked against
  the Tor Browser Developers signing key, run as a client only with its
  own data directory and a SOCKS port on the loopback address. No bridge,
  no onion service, no system Tor.
- `crates/openbmt-net-io/examples/observe.rs` on mainnet with
  `OPENBMT_PROXY` set to that port: the real `Driver`, default seeds, 8
  peers, `FileStorage`, one watched script that nobody pays.
- A first pass of about 6.5 minutes to take the headers, then the main
  pass on the same data directory.
- No Bitcoin node ran on this machine. Peer addresses are left out of
  this note.

## What was seen

| What | Headers pass | Main pass |
|---|---|---|
| Length | about 6.5 min | about 21 min (cut short, see below) |
| Ready peers (target 8) | 2 to 5 | 2 to 4, mostly 2 or 3 |
| Peers offering filters | 0 | 0 |
| Network groups with a filter peer | 0 | 0 |
| New blocks joined | 6 | 2 |
| Blocks fetched in full and checked | n/a | 2 (1.47 MB, 1.65 MB) |
| "Peer closed the connection" notes | n/a | 17 |
| Unconfirmed transactions seen | n/a | several thousand |
| Notes about a peer breaking a rule | 0 | 0 |

- The tip moved from 975313 to 975315 in the main pass. Each block was
  joined to the chain, fetched and reported "checked" within about 5
  seconds of its header.
- No name was looked up by the system resolver: with a proxy the driver
  never calls it (NR-26, and the unit test of that rule).
- Each connection gave the proxy its own user name and password (NR-78),
  which is what makes Tor put peers on separate circuits. Whether Tor did
  so was not inspected on Tor's side.
- The main pass was meant to run 30 minutes. It stopped at about 21
  because the machine's disk filled from unrelated build output and the
  log could not be written. Nothing in the last minutes before that
  differs from the rest.

## Why no filter peer

Three rules meet here:

1. Without Tor, each seed is also asked under its `x49.` name, which gives
   only nodes that state the compact-filter service, and those addresses
   enter the book with that service stated by a seed (NR-72).
2. With Tor, no name is looked up. One seed name at a time is dialed
   through the proxy as an ordinary peer. That peer is whatever node the
   exit's lookup of the plain name gives, and the `x49.` names are never
   used.
3. What a peer says about another peer's services earns no preference
   (NR-21). So the addresses a Tor-only device hears by `addr` are dialed
   without regard to filters, and a filter peer is met only by chance.
   Few reachable nodes serve filters, so chance is slow.

A second limit sits behind the first. Every peer reached by name is in
one network group, because its address is not known (NR-20), so at most
one named peer is held. Dialing two `x49.` names at once therefore cannot
give the two filter peers in different groups that NR-42 needs. This was
tried in a test and is how the limit was found.

## What would fix it

Tor's SOCKS port can look a name up through Tor (the `RESOLVE` request)
and return an address without a connection to it. Asking the seeds'
`x49.` names that way, each on a circuit of its own, would fill the book
with filter peers stated by a seed, exactly as without Tor, and no name
would reach the system resolver. This needs a rule change (NR-26 and
NR-72 say "no name is looked up") and is not built.

Until then a Tor-only device works, and checks every block in full.

## Not covered

- Onion peers. None were dialed; the book takes them but the seeds do not
  give them.
- Encrypted transport (BIP324) through Tor.
- A run longer than 21 minutes, memory growth, or a restart through Tor.
- Whether peers were on separate circuits, seen from Tor's side.
- How often peers close a connection through Tor compared with without:
  17 closes in 21 minutes with 2 to 4 peers held is more churn than the
  clear-net run showed, and its cause was not looked into.
