# Six hours of the network driver on mainnet (2026-11-10 to 2026-11-11)

**Question.** Does `openbmt-net-io`, left running for six hours on the real
Bitcoin network, keep its peers, follow new blocks, stay quiet and keep its
memory bounded?

**Answer.** It held 8 peers within 41 seconds, followed all 74 blocks that
were mined in the six hours (start tip 975,336, final tip 975,410, the same
tip in the saved headers file), raised no note against any peer and stopped
in 0.1 s. Memory rose from 4 MB to 66 MB in the first three hours and then
grew by 1.6 MB in the last three. That last slope is small but not zero:
six hours do not show whether it levels off, so a longer run with a heap
profile is still to be done (below).

## What was run

The example `crates/openbmt-net-io/examples/observe.rs`, built in release
from commit `abf7dc1`, with `observe bitcoin <dir> 21600`: the network's
default DNS seeds, 8 peers, no proxy, one watched script that nobody pays
(as in `netio-mainnet-observe-2026-11-10.md`), and the headers file of an
earlier run as its start. A shell loop read `VmRSS` and `Threads` from
`/proc/<pid>/status` every 60 seconds. The process ran on a Linux laptop
(glibc allocator) on a home connection; a phone's allocator differs, so the
shape of the curve may not carry over exactly.

## Memory

Resident set size, per hour of the run (360 samples, one a minute):

| Hour | Lowest | Highest |
|---|---|---|
| 0 | 4.1 MB | 33.6 MB |
| 1 | 35.6 MB | 49.3 MB |
| 2 | 50.0 MB | 64.9 MB |
| 3 | 65.3 MB | 66.0 MB |
| 4 | 66.0 MB | 67.1 MB |
| 5 | 67.1 MB | 67.7 MB |

The thread count was 10 from the first minute to the last. The headers
file ended at 618,442 bytes and the address book at 27,865 bytes, so the
first three hours' growth is not the files: it is memory the process holds,
most likely its caches of filters, block hashes and peer state filling to
their limits. The last three hours grew by about 0.55 MB an hour. At that
rate a counter would gain about 13 MB a day; whether the rate falls to zero
is not settled by this run.

## Health

- First ready peer after 0.4 s; 8 ready peers first reached at 40.6 s;
  two filter peers in different network groups at 1.0 s.
- 74 blocks joined in 37 chain events; no chain was rolled back.
- 206,732 sightings of unconfirmed transactions were logged.
- No note was raised: no peer was caught breaking a rule.
- No onion peer was dialed, because the run had no proxy; the address book
  learned 50 onion addresses.
- Shutdown took 0.1 s.

## What looks wrong or is still open

- **The memory slope after hour 3.** It is small, but a counter runs for
  weeks. Next: a 24-hour run with the same loop and a heap profile at hours
  3, 12 and 24, to tell a bounded cache from a slow leak.
- **Bytes on the wire.** The process's `rchar` counter grew by 623,784
  bytes, which is far too little for 206,732 transaction announcements: the
  counter misses socket receives made with calls other than `read`, so it
  does not measure the traffic. The machine-wide figure for the same period
  covers other programs. The driver's own traffic is not known from this
  run; the 24-hour run should count it per socket.
- The run was on clear net only. The Tor path is covered by
  `netio-tor-observe-2026-11-10.md` and `netio-onion-observe-2026-11-10.md`,
  over minutes, not hours.

The log of the run is not published: it names peers' addresses.
