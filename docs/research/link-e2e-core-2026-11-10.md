# The link against a relay and Bitcoin Core (2026-11-10)

**Question.** Do the owner's heartbeat and the counter's check-ins and records
travel through the relay (`docs/LINK-RULES.md` section 6), does the owner end
up with a copy of the counter's chain of the same length and head, does a
relay that goes away and comes back leave the shop working on-chain and the
link catching up, and does the relay turn away a device that is not a member?

**Answer.** Yes, in the run made after the script's last edit: all six steps
passed. An earlier run of the same script stopped at step 1 because of a
mistake in the script's own text parsing (it read `hbstate` fields wrongly);
that run is not counted.

## Method

`scripts/link-e2e-core.sh` runs the whole check on regtest and prints PASS,
FAIL or NOT RUN for each numbered step. The pieces:

- Bitcoin Core v32.1.0 from `~/.local/opt/bitcoin-core-32.1/bin` only, one
  private regtest node serving compact filters, in a fresh directory under
  `~/.cache/openbmt/` that is removed on every exit. The script refuses a
  `bitcoind` that is not the one in that directory and never touches
  `~/.bitcoin`.
- The relay is the `relay` example of `openbmt-link` with its fixed test
  identity and a `members` file holding the owner's and the counter's keys.
  Its directory is the same before and after the stop in step 4, so the
  mailboxes are the ones it had.
- The counter is `counter_regtest` and the owner `owner_regtest`, both peers
  of the node, both started with `--regtest-single-peer`, and both given
  `--relay` and `--relay-key`. Nothing is carried between them by the script:
  the heartbeat, the records, the check-ins and the receipts all go through
  the relay's TCP port. The owner sends a heartbeat every 3 seconds; the
  counter's signed age limit is 60 seconds.
- The keys come from fixed test bytes (`[n; 32]`): the owner 11, the counter
  13, the device that is not a member 15. Test only, never use.
- `link_probe` (`openbmt-courier`) prints those keys and acts as the device
  that is not a member.

## Results

| Step | What was checked | Result |
|---|---|---|
| 1 | The counter took heartbeats the owner sent through the relay; `hbstate` shows the last as verified and 0 seconds old; the link reports the relay reachable. | PASS |
| 2 | A sale (500 cents) is paid and a block mined: it reaches Confirmed. The owner's copy of the counter's chain then has the same length (4 records) and the same head as the counter's own. | PASS |
| 3 | The owner's copy holds the counter's check-in (record 0) at its place, `check_in_held` is true, the owner has raised no alert and the courier refused nothing. The counter shows a last check-in time. | PASS |
| 4 | The relay is stopped. The heartbeat on the counter ages to 61 seconds, above the signed limit of 60; the counter's link says the relay is not reachable; a new sale still reaches Confirmed on-chain; the counter's chain grows to 7 records while the owner's copy stays at 4. | PASS |
| 5 | The relay is started again on the same directory with the same key. The heartbeat is fresh again (0 seconds) and the owner's copy catches up to the counter's 8 records and the same head. | PASS |
| 6 | A device whose key is not in the members file is refused when it tries to make a mailbox (LK-21), to put a message (LK-20) and to fetch (no mailbox exists for it, so the fetch is refused too). | PASS |

## What is NOT covered

- Only regtest and Bitcoin Core 32.1 beside the examples. Both examples talk
  to one node on loopback; the health rules (WR-41) are replaced by
  `--regtest-single-peer`, as in `scripts/instant-e2e-core.sh`. Instant
  acceptance is not exercised; the sales are confirmed by a block.
- One counter, one owner and one relay, all on one machine; no network
  between them, no packet loss, no slow links.
- A relay that restarts with a different identity, a restart of the counter
  or the owner in the middle of a run, and a chain of more than one message
  of records are covered by the unit tests of the crates (LK-51 to LK-63),
  not by this script.
- The fetch in step 6 is refused because the stranger has no mailbox; a
  member's mailbox being unreadable by others is a rule of the relay
  (`openbmt-link`) tested there, not here.
