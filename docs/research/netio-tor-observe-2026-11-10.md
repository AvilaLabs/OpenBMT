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
"Why no filter peer". The fix was built the same day; its first run is in
"Second run: lookups through Tor" at the end, and did not reach it.

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

## Second run: lookups through Tor (2026-11-10)

**What was built.** The fix of "What would fix it": `socks::resolve` sends
Tor's `RESOLVE` request with a token of its own as name and password, and
refuses any reply that is not an IPv4 or IPv6 address that the book would
keep. With a proxy, a seed round asks each seed's `x49.` name three times,
at most 4 lookups at a time, and the answers enter the book as stated by a
seed to offer filters. A proxy that answers "command not supported" is
noted once and the seed names are dialed as peers, as before. NR-26 and
NR-72 say so. Unit tests against a fake proxy and driver tests with injected
hooks pass; they show 9 lookups for 3 seeds, 0 calls of the system's
resolver, at most 4 lookups in flight, and the fallback.

**The run.** The same Tor client (0.4.9.16, client only, SOCKS port on the
loopback address), the release build of `observe` on mainnet with the proxy
set, the data directory of the first run (the stored headers at 975315 and
an address book of about 20 KB), 20 minutes, one run.

| What | Result |
|---|---|
| Ready peers (target 8) | 1 at 1.7 s, 4 at most (first at 1101 s) |
| Peers offering filters | 0, at any point |
| First filter peer ready | never |
| Two filter peers in different groups | never |
| Seed rounds started | 0 |
| Seed lookups through the proxy | 0 answered of 0 |
| Lookups seen in Tor's log | none |
| Blocks fetched in full and checked | 11 (1.2 to 1.8 MB each) |
| Tip | 975315 to 975326 |
| Peers dropped | 3 for NR-42 (requests not answered in time), 1 for NR-14 |
| Transport | 7 distinct peers on v2, 3 on v1, 9 refusals of v2 |
| System lookups | none: no name is looked up by the system with a proxy |

**Which step fails: the lookups are never started.** No seed round began,
so nothing was asked of Tor. A round starts only when the address book has
nothing to dial (NR-72, as without Tor). This data directory already held
about 20 KB of addresses that earlier runs had heard of by `addr`, so the
book always had something to give, and the seeds, whose `x49.` names are
the only source of filter peers a Tor-only device has, were never reached.
The peers that were ready came from the stored anchors and the book. The
handshakes of the peers that were dialed worked (v2 and v1), and Tor
carried the traffic; no step after the dial failed.

So the fix is in the code and tested against a fake proxy, and it has not
been seen to work through Tor: `RESOLVE` against a real Tor was not
exercised by this run. The trigger is the open question. Asking the seeds
also when filters are wanted and the book holds no address with the filter
service stated by a seed would reach them with a populated book. That
changes NR-72 and was not done here.

The log of the run is kept outside the repository.
