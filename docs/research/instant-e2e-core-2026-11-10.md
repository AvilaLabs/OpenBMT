# The owner's heartbeat against Bitcoin Core (2026-11-10)

Question: does the counter node take the owner watcher's signed heartbeat
(`docs/COUNTER-RULES.md`, CR-50 to CR-56), refuse the wrong ones, age it, and
judge the tip it names against the counter's own header chain, with a real
Bitcoin node on each side? And can instant acceptance run in the counter
node with the real health rules?

Short answer: the heartbeat path works end to end (steps 3a, 5, 6a, 7
below). Instant acceptance cannot be reached on one machine, and the real
health rules cannot hold there. Both have the same cause, which is in the
grouping rule and was not changed. Five steps are marked FAIL for that
reason, with the exact text the run printed.

## What was run

`scripts/instant-e2e-core.sh`, twice, on Bitcoin Core 32.1 from
`~/.local/opt/bitcoin-core-32.1/bin`, regtest only, fresh data directories
under `~/.cache/openbmt/`, removed on every exit.

- Node A and node B, both serving compact filters, both with
  `-connect=0` so that neither dials anyone by itself. B was joined to A
  with `addnode ... onetry` for steps 1 to 6 and parted from A (the outbound
  connection closed with `disconnectnode`) for step 7.
- The counter example (`counter_regtest`, `openbmt-counter-node`) is the
  one fixed peer of A. The owner example (`owner_regtest`,
  `openbmt-owner-node`) is the one fixed peer of B. Each has its own data
  directory and its own driver.
- The owner example makes heartbeats with `OwnerWatcher::heartbeat` and
  prints their bytes as hex. The script types the hex into the counter
  example's `heartbeat <hex>` command. How the bytes travel between two real
  devices is not part of this slice (the transport comes with the Box).
- The counter is started with the signed setting `heartbeat_max_age_secs`
  at its minimum, 60 seconds (`--heartbeat-max-age 60`), and with
  `customer_wifi` false (`--customers-off-wifi`), which the light check needs
  to accept anything at once without Tor. The default settings say true.
- Sales are $5.00 at $100,000 per bitcoin: 5,000 satoshis, under the $10
  per-sale limit and inside the counter's slice. Core's wallet pays them.
  The policy is the real one and was not loosened.

## The loopback limit

`openbmt_watch::network_group` puts every address that starts with 127 into
one group, called local, together with every private, link-local and CGNAT
address (`crates/openbmt-watch/src/netgroup.rs`, `v4_group`: `a == 127`). A
test of the library names `127.9.9.9` as local
(`crates/openbmt-watch/src/tests/netgroup.rs`). So nodes bound to
`127.0.0.1`, `127.1.0.1` and `127.2.0.1` are one group, and the plan of
three nodes in three groups does not exist on one machine. The rule was not
changed.

Three things follow, and the run shows each:

1. **WR-50 cannot hold.** It asks for peers in 3 groups, 2 of them with
   filters. The real answer was `TooFewPeerGroups` with 1 peer in 1 group
   and 1 filter group, for both examples.
2. **A payment is never seen by 3 groups.** The watcher counts a payment as
   seen only when 3 groups reported it (WR-44, `SEEN_GROUPS`), so the
   check mode stays `None`. A sale then shows `Waiting` with "Not yet seen by
   enough of the network", and the instant decision is not asked at all, so
   the heartbeat is never read. The reason "no independent watcher" cannot
   appear.
3. **More loopback peers would not help.** The peer manager refuses a
   second peer in a group that already has one (`ConnectRefused::SameGroup`
   in `openbmt-net`).

So both examples run with `--regtest-single-peer`, which replaces the health
answer with "healthy" in the examples only. The library is untouched. The
examples print what the real rules said (`real=TooFewPeerGroups`).

**Rules not exercised in this run:** WR-50 (groups and filter peers, with
real peers in more than one group), and WR-51 to WR-53 (the real answer stops
at WR-50, so the tip age, the peers-ahead timer and the clock check never
ran). WR-44 (3 groups) is not met by any payment. The instant decision
`evaluate` was not asked for any sale.

## Results

The same table on both runs.

| Step | What | Result |
|---|---|---|
| 1 | Counter and owner watcher healthy by the real rules | FAIL |
| 1b | Both examples reach their node, the tip is 150, both answer healthy under the switch | PASS |
| 2a | No heartbeat taken: the counter shows view none, not verified, last number 0 | PASS |
| 2b | A paid sale waits with the "no independent watcher" reason | FAIL |
| 3a | A fresh heartbeat from the owner watcher is taken and verified | PASS |
| 3b | A new paid sale shows `PaidInstant` | FAIL |
| 4 | `PosPaid` once, exposure on the ledger, released at the first confirmation | FAIL |
| 5 | A replayed heartbeat, one signed by the approver key and one naming another counter are refused, each for its own reason | PASS |
| 6a | With the age limit at 60 seconds, the heartbeat really ages past it (the script waits 65 seconds) | PASS |
| 6b | A new paid sale shows `Waiting` again with the no-watcher reason | FAIL |
| 7 | A tip the counter does not hold is "not verified" and no alarm; another block at a held height is "not verified" and logged as a disagreement | PASS |

What the run printed:

- Step 1: `health used=healthy real=TooFewPeerGroups peers=1` and
  `groups peers=1 groups=1 filter_groups=1`, for the counter and for the
  owner watcher.
- Step 2b: `Not yet seen by enough of the network.` (sale 0). Step 3b:
  `sale 1 state=Waiting received=5000 needs=5000 mode=None`. Step 6b: the
  same text for sale 2.
- Step 4: the sale of step 3b was `Confirmed` after one block and `PosPaid`
  was reported once, with the confirmation, not before it. Nothing went on
  the exposure ledger because nothing was accepted at once.
- Step 5: `CR-50: heartbeat number N is not above the last one, N` for the
  replay, `CR-50: heartbeat is not signed by the owner` for the approver's
  key, and `CR-50: heartbeat is for another counter`.
- Step 6a: `view=Verified policy_verified=true policy_age=66` against a
  limit of 60. The heartbeat is still verified, and its age is the counter's
  time minus the heartbeat's own time; the instant decision refuses an age
  above the signed limit (`OffReason::NoWatcher`).
- Step 7: B mined its own block 152 while parted from A. The owner
  watcher's heartbeat named it. The counter, at 151, did not hold it:
  `NotVerified rule=CR-53 disagreement=false`, not logged as a disagreement.
  Then A mined its own block 152. The next heartbeat still named B's block:
  `NotVerified rule=CR-53 disagreement=true`, and the counter's log has
  `heartbeat disagreement: ... (owner tip height 152)`.

## Where the unreachable steps are checked instead

The heartbeat decisions of steps 2b, 3b, 4 and 6b are checked with the real
counter, the real instant decision and the real policy, with the peers fed as
three network groups by hand to a fake driver, in
`crates/openbmt-counter-node/src/tests/heartbeat.rs`:

- `a_waiting_sale_is_accepted_when_the_first_heartbeat_is_taken`: with three
  groups and no heartbeat, `Waiting` with `InstantOff(NoWatcher)`; the first
  heartbeat gives `PaidInstant`, one `PosPaid` report, and 5,000 satoshis
  of exposure.
- `a_heartbeat_that_is_too_old_waits_again`: a second sale seen after the
  age limit is `Waiting` with the no-watcher reason.
- `what_is_not_a_good_heartbeat_is_refused_and_changes_nothing` and the
  other tests there: refusals, replays, a heartbeat from the future,
  unhealthy heartbeats, a disagreement at a held height, and a restart.

This is a fake network, not Core, and it is said so here. Release of
exposure at the first confirmation (CR-15) is checked in the counter's own
tests, not against Core in this slice.

## What is NOT covered

- **The real health rules with real peers**, and instant acceptance of a
  payment seen from three groups, against Core. Both need peers in 3 network
  groups, which one machine cannot give without changing the grouping rule
  or using addresses outside the local ranges (for example network
  namespaces with public-range addresses, which need privileges this check
  does not ask for).
- **The transport** of the heartbeat from the owner's device to the counter.
- **The owner's own watching of payments.** The owner watcher watches no
  script in this slice.
- **A heartbeat lost to a crash at a bad moment** on the owner side beyond
  the unit tests of the number file (written whole, then the bytes are
  returned).
- **Two clocks.** Both examples read the same system clock. A counter and an
  owner device with clocks that differ (CR-51, the 120-second limit) were
  checked in unit tests only.
- **A bad build.** A heartbeat is a claim by the same software family on
  another device. It guards against a dead or cut-off owner watcher, not
  against a bad build (CR-56).

## Where the existing crates did not fit

- **The grouping rule and the manager.** See "The loopback limit". It is
  the reason for five FAIL lines. It looks like a limit of regtest and not a
  defect: a counter on a real network has peers in real groups.
- **`Facts.heartbeat`.** The counter's `Facts` still carries a
  `Heartbeat` supplied by the caller, because the counter's own tests use it.
  The node now fills it from `Counter::heartbeat_for_policy`, so nothing in
  the node supplies a heartbeat the owner did not sign (CR-55), but the
  library does not enforce that for another caller.
- **`Reader::framed` reads only version 1.** The counter's stored state
  (`OBMG`) needed a second form for the heartbeat number, so `Desk` reads
  its own header. The desk is written as version 1 until a heartbeat has been
  taken, which keeps one spelling for each state.
- **`HeaderChain` has no "hash at height".** CR-53 needs to tell a different
  block at a held height from a block the counter has not got. The judgment
  uses what the trait gives: a hash that is not held at a height at or
  below the counter's tip is a disagreement.

## Notes

- The two runs of the script are in the same state as the table above.
- The owner's heartbeat number is the larger of the last number plus one
  and the clock in seconds, so that a lost or restored owner directory does
  not start again below the counter's stored number.
