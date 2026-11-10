# The network driver on the real Bitcoin network (2026-11-10)

**Question.** Does `openbmt-net-io`, which had only been run against one
Bitcoin Core node on regtest, find peers through the DNS seeds, sync
headers, keep peers, resume from its stored files and stay quiet and small
on the real network?

**Answer.** On signet and on mainnet it found and held up to 8 peers in 8
different network groups, took the headers, saw unconfirmed transactions
and stopped cleanly in under a second. A restart resumed from the stored
tip and did not fetch the headers again. It never raised a note, so no
peer was caught breaking a rule. What this does **not** show is the block
path: no new block arrived in any mainnet run (about 29 minutes in all, and
the explorer also showed none in that time), and none on signet in 3
minutes. Section "What looks wrong or surprising" lists the things that
were unexpected or could not be seen.

## What was run

The example is `crates/openbmt-net-io/examples/observe.rs`. It starts the
real `Driver` with `DriverConfig::new(network, watch_from)` (the network's
default DNS seeds, 8 peers, no proxy, no local peers), `FileStorage` in
the data directory, `SystemClock` and `OsRandomness`. It watches one script
that nobody pays: the P2WSH script of the SHA-256 of the bytes
`openbmt observe example`. So filters are asked for and compared, and
nothing can match. It reads events until the time is up, calls `shutdown()`
and prints totals.

`watch_from` is the tip height the header store has when the example
starts: the stored tip if the data directory has `headers.bin`, otherwise
the network's start block (height 0 on signet, the built-in checkpoint at
height 967,680 on mainnet). The example reads the file the same way the
driver does. Blocks above that height are checked (NR-40).

To run it again (the build and the run used the debug profile):

```text
~/.cache/openbmt/guard.sh env CARGO_TARGET_DIR=$HOME/.cache/openbmt/target/probe \
    cargo build -p openbmt-net-io --example observe --locked
~/.cache/openbmt/target/probe/debug/examples/observe \
    <bitcoin|signet> <data dir> <seconds> > run.log
```

The program only dials out to public peers found through the seeds. The
driver never relays, so it sends no transaction, address or script of ours.
No Tor and no proxy were used. No Bitcoin node was started on this machine.
The runs were made on 2026-11-10 between 08:49 and 09:24 UTC.

The runs:

1. signet, 180 seconds, fresh data directory.
2. mainnet, 600 seconds, fresh data directory.
3. mainnet, 120 seconds, the same data directory as run 2 (a restart).
   This run was made twice. The first time the example did not yet print
   the tip's hash, so it was run again with the same directory (and the
   directory then held run 3's own stored files). Run 3 below is the
   second one; the first one is described where it differs.
4. Not asked for: mainnet, 900 seconds, the same directory again, to try
   to catch a block, because runs 2 and 3 had none.

The logs are in `~/.cache/openbmt/logs/` and hold peer addresses, so they
are not in the repository. The three data directories were deleted.

## Results

Times are seconds since the example started. "Ahead" is the `ahead_since`
field of the `Peers` event.

### Run 1: signet, 180 seconds, fresh

| Fact | Value |
|---|---|
| Start tip | 0 (genesis) |
| Final tip | 330,219 |
| First ready peer | 0.4 s |
| 8 ready peers | 0.7 s |
| First moment no peer ahead | 18.2 s (after the last of 166 `Chain` events) |
| Two filter peers in different groups | 0.5 s |
| Blocks fetched in full | 0 |
| Events | peers 191, chain 166, sighting 55, also announced 347, note 0, stopped 1 |
| Distinct sighted txids | 55 (53 from 1 peer, 2 from 2 peers) |
| `headers.bin` / `book.bin` | 26,417,562 / 20,234 bytes |
| Time to stop | 0.8 s |

### Run 2: mainnet, 600 seconds, fresh

| Fact | Value |
|---|---|
| Start tip | 967,680 (the checkpoint) |
| Final tip | 975,280 (4 `Chain` events, all between 0.8 s and 1.5 s) |
| First ready peer | 0.5 s |
| Most ready peers | 8, first reached at 166.4 s (5 at 0.6 s, 6 at 0.8 s, 7 at 50.8 s) |
| First moment no peer ahead | 1.5 s (when the last headers arrived) |
| Two filter peers in different groups | 0.5 s |
| Blocks fetched in full | 0 |
| Events | peers 73, chain 4, sighting 1,913, also announced 11,830, note 0, stopped 1 |
| Distinct sighted txids | 1,913 (1,687 from 1 peer, 221 from 2, 5 from 3) |
| `headers.bin` / `book.bin` | 608,042 / 25,701 bytes |
| Time to stop | 0.0 s |
| Peak resident memory | 11,504 KiB (last sample), 10 threads in every sample |

### Run 3: mainnet, 120 seconds, restart on the run 2 directory

| Fact | Value |
|---|---|
| Start tip | 975,280, hash equal to run 2's final tip |
| Final tip | 975,280 (no `Chain` event) |
| First ready peer | 0.6 s |
| Most ready peers | 8, first reached at 95.2 s (5 at 1.0 s, 6 at 1.7 s, 7 at 75.6 s) |
| First moment no peer ahead | 0.6 s (the first ready peer) |
| Two filter peers in different groups | 0.7 s |
| Blocks fetched in full | 0 |
| Events | peers 24, sighting 340, also announced 1,055, note 0, stopped 1 |
| `headers.bin` / `book.bin` | 608,042 / 58,809 bytes |
| Time to stop | 0.1 s |

The first attempt of run 3 (no hash printed) had the same shape: 7 peers
at 0.6 s, 8 peers at 11.9 s, 331 sightings, no `Chain` event, no note,
`book.bin` 43,153 bytes. Run 4 (900 seconds): first ready peer 0.7 s,
8 peers at 86.6 s, 2,218 sightings, 14,462 also announced, no `Chain`
event, no note, no block, `book.bin` 61,477 bytes, `headers.bin` unchanged
at 608,042 bytes.

### Peer sets by run

Every `Peers` event in every run had as many network groups as ready peers,
and as many filter groups as filter peers (the example computes groups with
the watcher's `network_group`), so 8 peers were always in 8 groups. The
number of ready peers that state the filter service, from the last `Peers`
event: signet 7 of 8,
run 2 5 of 8, run 3 7 of 8, run 4 7 of 8. Two filter peers in different
groups were there at the first moment two peers were ready: 0.5 s, 0.5 s,
0.7 s and 0.7 s. This is the services flag the peer claims (NR-12), and
for peers that came from the seeds it is also what the seed claimed.

## Notes the driver emitted

None, in any run. The driver raises a note for a stored file that does not
read back, a failed save, a script that was not watched, a seed lookup that
failed, a local peer, and a peer dropped for breaking a rule (NR-25). No
note of any kind appeared, so there was no stored file that failed to read
back (the restart read both files without a note), no failed save, no
failed lookup of a seed name and no peer dropped for a rule.

| Note | Signet | Run 2 | Run 3 | Run 4 |
|---|---|---|---|---|
| any | 0 | 0 | 0 | 0 |

## Disconnects

The driver tells the app of a disconnect only when the peer broke a rule
(`misbehaved` is yes), and then only as a note with the reason. A peer
that closed, timed out or failed in the handshake is dropped without any
event. So the reasons for these could not be recorded:

| Reason | misbehaved | Seen |
|---|---|---|
| any rule broken (NR-25) | yes | 0 times in all runs |
| any other disconnect | no | reason not exposed |

What the events do show is the count of ready peers falling between two
`Peers` events, which is a lower bound on the peers lost (two lost
between two events show as one fall):

| Run | Falls in the ready count |
|---|---|
| 1 signet | 1 (8 to 7, at 22.1 s) |
| 2 mainnet | 1 (6 to 4, at 0.9 s, 0.1 s after the first headers arrived) |
| 3 restart | 3 (6 to 5, at 1.8 s, 21.2 s and 42.9 s) |
| 4 mainnet | 3 (1 to 0 at 0.7 s, 2 to 1 at 0.8 s, 5 to 4 at 2.0 s) |

## Blocks

No block arrived after the headers were caught up, so there is no
per-block record of the kind asked for.

| Run | Blocks joined | When | Fetched in full |
|---|---|---|---|
| 1 | 330,219 (heights 1 to 330,219) | 0.6 s to 18.2 s | 0 |
| 2 | 7,600 (967,681 to 975,280) | 0.8 s to 1.5 s | 0 |
| 3 and 4 | 0 | | 0 |

The blocks of runs 1 and 2 are all above the tip the example started with,
because of how `watch_from` was chosen (see the surprises). No `Block`
event came for any of them, so none was fetched in full. Whether each one
was settled by two equal filters cannot be shown from the events: the
driver sends no event for a block settled by filters. What can be said is
that a block is fetched in full when a filter matches, when two filters
differ or when fewer than two answered in 60 seconds (NR-42), and that no
block was fetched in any run. The tip of the explorer was 975,280 at
09:06 and again at 09:24 UTC, the same block as the example's. That block
carries a header time of 08:52:26 UTC, about a minute before run 2 began,
so the chain went about 31 minutes without a block. The four mainnet
runs together cover about 29 minutes of it.

## Data volume

Two things were measured. Neither is the bytes of the driver's sockets
alone.

- `rchar` in `/proc/self/io` does not count socket reads on this machine:
  it moved by 8,801 to 12,446 bytes in runs 1 and 2, which is the size of
  the files and `/dev/urandom` reads. It is useless for network volume. In
  runs 3 and 4 it moved by 634,649 to 667,941 bytes, which is the stored
  files being read and written about.
- The bytes the machine's network interfaces (loopback excluded) received
  and sent between the start and the end of the example. This counts
  everything else on the machine in the same period.

| Run | Received | Sent |
|---|---|---|
| 1 signet, 180 s | 172,805,121 | 8,425,665 |
| 2 mainnet, 600 s | 312,432,833 | 11,322,181 |
| 3 restart, 120 s | 800,243 | 510,528 |
| 4 mainnet, 900 s | 13,695,461 | 9,193,856 |

In run 2 the sockets alone were also read once a minute with
`ss -tinp` (`bytes_received` summed over the open sockets of the process;
sockets that had closed are not in it):

| Minute | Open sockets | Received on them |
|---|---|---|
| 0 | 6 | 16,369,119 |
| 1 | 7 | 137,879,582 |
| 2 | 7 | 250,172,432 |
| 3 | 8 | 287,933,660 |
| 4 | 8 | 288,076,559 |
| 5 | 8 | 288,250,394 |
| 6 | 8 | 288,392,273 |
| 7 | 8 | 288,544,996 |
| 8 | 8 | 288,728,776 |
| 9 | 8 | 288,885,739 |

So about 288 MB came in the first 3 minutes and then about 150 to
180 KB a minute. The 7,600 mainnet headers are 81 bytes each on the
wire, about 616 KB from one peer or about 5 MB if all 8 are asked, so the
headers do not account for the 288 MB (see the surprises).

## Memory and threads

Sampled with `ps -o rss=,nlwp= -p <pid>` once a minute.

| Run | Samples (KiB) | Threads |
|---|---|---|
| 2 mainnet, 600 s | 10,516 10,696 11,028 11,068 11,076 11,116 11,204 11,320 11,424 11,504 | 10 each time |
| 4 mainnet, 900 s | 9,056 9,228 9,316 9,336 9,360 9,428 9,476 9,592 9,704 9,808 9,948 10,016 10,204 10,240 10,348 | 10 each time |
| 3 restart, first attempt | 4,572 (at 2 s), 9,000 (at 27 s) | 1, then 10 |

Peak: 11,504 KiB (about 11.2 MiB) and 10 threads. Run 1 (signet) was not
sampled, though its `headers.bin` is 26 MB. Resident memory rose in both
long runs, about 100 KiB a minute; the runs were too short to say whether
it levels off.

## The explorer comparison

After the runs, with a plain `curl` and no extra headers:

| Source | Tip height | Hash at that height |
|---|---|---|
| The example, end of run 3 and of run 4 | 975,280 | `000000000000000000009860f9fd06da31dab41575f51fcf571c6b4360300e33` |
| `blockstream.info/api/blocks/tip/height` (09:24 UTC) | 975,280 | same, from `/api/block-height/975280` |

They agree.

## What looks wrong or surprising

1. **No block was seen on either network.** This is the biggest gap. The
   block path (filters from two peers, the 60-second wait, the full block,
   `Chain` after a new tip) did not run on a live network. The explorer
   agrees that there was no block, so nothing was missed, but this check
   proves nothing about the path.
2. **`watch_from` at the start tip makes a fresh start check every block
   since the checkpoint.** The example gave the driver the start block's
   height. All 7,600 blocks that joined while syncing on mainnet, and all
   330,219 on signet, were above it, so the driver is meant to have
   checked them (NR-40). In run 2, about 288 MB came in during the first 3
   minutes on the open sockets, then about 150 KB a minute. The headers
   are about 5 MB at most. That the rest is filters is an inference from
   NR-40, not something the events show, since the events say nothing
   about filters. Whether this is how the counter is meant to start,
   or whether it should start `watch_from` at the network's tip, is a
   question for the design, not a thing seen failing. A counter that
   starts at the network's tip would not have this cost.
3. **No visibility into filter work or disconnects.** The events carry no
   count of filters asked, filters compared or blocks settled by filters,
   and no reason for a disconnect unless the peer broke a rule. So several
   items asked for (how each block was settled, the disconnect reasons, and
   whether a restart dialed the stored anchors first) could not be
   observed. On the last, run 3 had 5 ready peers at 1.0 s and run 2 had 5
   at 0.6 s, so the restart was not faster; that neither shows nor
   disproves that the anchors were dialed first.
4. **The 8th peer is slow on mainnet.** Signet had 8 peers in 0.7 s. On
   mainnet 8 peers came after 166.4 s, 11.9 s, 95.2 s and 86.6 s in four
   runs. In run 2 the count fell from 6 to 4 at 0.9 s. Why was not looked
   into. The driver spends 30 seconds before it dials an address again
   (NR-70) and asks the seeds at most every 5 minutes (NR-72); whether
   either explains this was not checked.
5. **Peers are lost with nothing reported.** The ready count fell 1 to 3
   times in every run (above), mostly in the first seconds, and in run 3
   again at 21.2 s and 42.9 s. Nothing was reported for any of them. They
   could be peers that refused, closed or timed out; the driver does not
   report those.
6. **Sightings are mostly seen from one peer.** Of 1,913 sighted
   transactions in run 2, 1,687 were reported from 1 peer, though 11,830
   `AlsoAnnounced` events followed, roughly 6 more announcements for each
   sighting. So the other peers announce the same transactions, but not
   before the first fetch has returned. That is as designed (NR-53).
7. **Memory grows steadily** in both long runs (about 100 KiB a minute) and
   was still rising at the end. Not investigated.
8. **The signet run began receiving transactions at 8.8 s**, while 18 seconds
   of header sync were still going. Nothing wrong; recorded because it shows
   sightings do not wait for the headers.
9. **The restart did what was hoped.** The stored tip and hash were
   resumed, no `Chain` event came, and only about 0.8 MB came in for the
   whole 120 seconds on the machine's interfaces, against 312 MB in run 2.
   This is listed because the same comparison also shows how much of run
   2's volume was the first start (see 2).

## What this does not show

- A block on a live network, as above.
- Anything about Tor, a proxy or a bad peer.
- An independent view of which peers were real nodes: the peer counts and
  groups are the driver's own.
- Release-profile speed: all runs used the debug profile.
- Whether any of the 24-hour bans work in practice: no peer was banned.

## Addendum: a 45-minute run that saw blocks

A later run on mainnet lasted 2,700 seconds with one watched script that
has never been paid, so that new blocks went through the filter path.
Three blocks arrived (975,281 to 975,283). The saved tip at the end was
975,283.

| | |
|---|---|
| Ready peers | 8, first at 0.7 s, all 8 at 96 s |
| Two filter peers in different groups | 0.7 s |
| Transactions sighted | 8,453, all distinct |
| Blocks that joined the chain | 3 |
| Blocks settled by two equal filters | 1 |
| Blocks downloaded in full | 2 (1.56 MB and 1.61 MB) |
| Peers that closed the connection | 26 |
| Peers dropped for an unanswered ping (NR-14) | 1 |

**A defect shows in this run.** Two of the three blocks were downloaded
in full although nothing paid the watched script, each about 61 seconds
after its header joined the chain: the 60-second limit of NR-42 ran out
with fewer than two filter answers. And seven of the 26 peers that
closed the connection did so within 0.3 seconds, or a few seconds, of a
new header joining the chain.

The cause, as read from the code and from Bitcoin Core's behavior: the
manager asks two filter peers for a block's filter as soon as the header
joins the best chain, whichever peer the header came from. A peer that
has not yet received that block itself does not know the hash it is
asked about, and Bitcoin Core disconnects a peer that asks for filters
up to an unknown block. A peer that has the block but whose filter index
has not caught up sends no answer. Either way the answer never comes,
the counter waits the full 60 seconds and then downloads the block, and
it loses filter peers at every new block.

The check against one Core node on regtest could not show this: there,
the peer that is asked is always the peer the header came from.

This is safe for the shop (the fallback is the full block, which is
checked) but costs about 1.5 MB and a minute for each block, and it
sheds peers. While a payment is waiting for its confirmation the counter
downloads every block at once anyway (NR-44), so the minute does not
delay a sale. The fix is to ask a peer about a block only after that
peer has itself shown the block; it is tracked as the next change to
the manager.

## Addendum 2: the same run after the fix

The manager now asks a peer for a block's filter only after that peer
has itself shown the block (NR-33, NR-40). The 45-minute run was
repeated on mainnet with the same never-paid watched script.

| | Before the fix | After the fix |
|---|---|---|
| Blocks that joined the chain | 3 | 8 |
| Blocks settled by two equal filters | 1 | 8 |
| Blocks downloaded in full | 2 | 0 |
| Time from the header to "checked" | about 61 s for two of three | 1.4 to 2.4 s |
| Peers that closed the connection | 26 | 10 |
| Of those, within seconds of a new header | 7 | 0 |

Nine of the ten closed connections in the second run came in the first
73 seconds, while the first peers were being dialed; one came later, at
a moment when no header had arrived for four minutes. The saved tip at
the end was 975,293.

This is one run on one day. It shows the defect gone in that run; it
does not show how the manager behaves around a reorganization on the
live network, which did not happen in either run.
