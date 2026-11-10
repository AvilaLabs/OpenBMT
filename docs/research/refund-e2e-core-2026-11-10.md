# A refund request from the counter to the owner's queue (2026-11-10)

**Question.** When a sale is paid and final and a cashier asks for part of it
back to an address a customer's wallet showed, does the request travel as a
record through the relay (`docs/LINK-RULES.md` section 6) to the owner, does
the owner's queue then hold exactly one refund for that sale, that amount and
that address, and does the counter refuse a second request that would take the
refunds over what the sale received (`docs/COUNTER-RULES.md` CR-60 to CR-74)?

**Answer.** Yes, in the run made after the script's last edit: all five steps
passed on the first run of the script.

## Method

`scripts/refund-e2e-core.sh` runs the whole check on regtest and prints PASS,
FAIL or NOT RUN for each numbered step. The pieces:

- Bitcoin Core v32.1.0 from `~/.local/opt/bitcoin-core-32.1/bin` only, one
  private regtest node serving compact filters, in a fresh directory under
  `~/.cache/openbmt/` that is removed on every exit. The script refuses a
  `bitcoind` that is not the one in that directory and never touches
  `~/.bitcoin`. The ports start at 19030 unless `DRILL_PORTS` is set.
- The relay is the `relay` example of `openbmt-link` with its fixed test
  identity and a `members` file holding the owner's and the counter's keys.
- The counter is `counter_regtest` and the owner `owner_regtest`, both peers
  of the node, both started with `--regtest-single-peer`, and both given
  `--relay` and `--relay-key`. The script carries nothing between them: the
  records and the owner's copy of them go through the relay's TCP port.
- The counter example has a `refund <sale> <sats> <address>` command, which
  reads the address with `refund_destination` as a customer's wallet would
  show it and asks for the refund as a cashier. The owner example applies the
  refund requests in its verified copy of the counter's chain to an owner's
  queue kept in memory over the same fixed test wallet (`RefundIntake`), and
  its `refunds` command lists the queue and the intake's list of refused
  requests.
- The keys come from fixed test bytes. Test only, never use.

## Results

| Step | What was checked | Result |
|---|---|---|
| 1 | A sale of 500 cents (5,000 sats) is paid and six blocks are mined. The counter shows it Final with 5,000 sats received. | PASS |
| 2 | The owner's copy of the counter's chain has the same length (5 records) and the same head as the counter's own. | PASS |
| 3 | A cashier asks for 2,000 sats back to a new Core address. The counter takes the request and numbers it 1. | PASS |
| 4 | The owner's queue holds exactly one refund: sale 0, 2,000 sats, to that address, cashier named, in the stage Queued, from the counter's key. The intake's list of refused requests is empty. | PASS |
| 5 | A second request for 5,000 sats is refused by the counter with CR-67 (5,000 on top of the 2,000 already asked is more than the 5,000 received). The owner's queue still holds the one refund and the list of refused requests is still empty. | PASS |

## What is NOT covered

- The check stops at the owner's queue. No approval session is opened, no
  transaction is built, signed or sent, and no refund is paid. The refund
  stays Queued.
- Withdrawal of a request, a refund paid in cash, a request that waits for
  the sale's depth, and a request the owner's queue refuses are covered by the
  unit tests of `openbmt-counter` and `openbmt-owner-node`, not by this
  script.
- Only regtest and Bitcoin Core 32.1 beside the examples, on one machine with
  one counter, one owner and one relay. The health rules (WR-41) are replaced
  by `--regtest-single-peer`, as in `scripts/link-e2e-core.sh`.
- In step 5 the script does not count records. That the refused request wrote
  no record is covered by the counter's tests (CR-67), and here by the owner's
  queue and list not changing.
- The owner's queue in the example is in memory and the persist function does
  nothing, so a restart of the example loses the queue (the intake's own
  position is kept in the data directory, which a restart would then not
  match). The tests of `openbmt-owner-node` cover the intake's restart with
  the owner's store.
