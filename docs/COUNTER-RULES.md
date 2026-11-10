# Counter rules: the life of a sale

Status: specification for `crates/openbmt-counter`, written before the
code. Each rule has an ID so that a test can name the rule it proves. The
rules implement `docs/DESIGN.md` §2.2, §2.3, §2.6, §2.7 and §6 for one
counter.

This library is the counter's judgment of a sale: when Bitcoin may be
offered, which address a sale gets, and which of the cashier's screens is
shown at each moment. It joins three parts that already exist and adds no
check of its own to them:

- what has arrived, from the watcher (`docs/WATCHER-RULES.md`);
- whether an unconfirmed payment may be accepted at once, from
  `openbmt_core::policy::evaluate`;
- the counter's signed address batches, settings and exposure ledger, from
  `openbmt_core`.

It has no network, no clock, no storage and no screen. The caller passes
in facts and the time, stores what it is handed before acting on it, and
draws what it is told.

## 1. Opening a sale

- **CR-01.** Bitcoin is offered only when the check is healthy (WR-50 to
  WR-53) at that moment. Otherwise opening is refused and the cashier is
  told to use another payment type. A clock mismatch (WR-53) refuses
  opening too: the price lock and the expiry cannot be trusted.
- **CR-02.** A sale gets the next unused address of the counter's current
  batch, and only that. An empty batch refuses the sale: "do not offer
  Bitcoin". An address is never used for two sales.
- **CR-03.** The counter stores its next index before the address is
  handed out. If the store fails, no address is handed out. A crash
  between the two wastes an index and never reuses one.
- **CR-04.** At most 120 new addresses are handed out per hour **[to
  tune]**. Past that, opening is refused until the hour has moved on.
- **CR-05.** A sale has an amount in the shop's currency and a quote: a
  rate the caller supplies, good for 5 minutes. When a quote runs out the
  caller supplies a new rate and the sale gets a new quote. The address
  does not change. All quotes of a sale are kept.
- **CR-06.** Satoshis are valued once, at the quote that was current when
  the counter first learned of them, and that value never changes. If
  none was current, the last quote is used: it is the one the customer
  was shown. If what has arrived shrinks, the newest valuations are
  dropped first.
- **CR-08.** What the sale needs, in satoshis, is the satoshis already
  valued plus the rest of the amount at the current quote, rounded up to
  a whole satoshi. A new quote moves only the part that has not been
  paid. Once enough has arrived the sale takes no new quote.
- **CR-09.** No sale opens while a rescan is pending after a restore or a
  reset (§5.5).
- **CR-07.** A sale with nothing received ends as "expired" 15 minutes
  after it opened **[to tune]**, or when the cashier cancels it. Its
  address stays watched for good.

## 2. What the cashier sees

The screen is one of the states below. Every state that is not final
carries two texts: why it is in this state, and what happens next (§2.6).

- **CR-10.** The states, and the color of each:
  - `NotPaidYet` (blue): nothing has arrived.
  - `PartPaid` (blue): less than the sale needs has arrived, beyond the
    tolerance. Shown as "Paid X of Y".
  - `Waiting` (blue): enough has arrived and it waits for a confirmation,
    with the reason from the instant decision.
  - `PaidInstant` (green): accepted at once, with the label of the check
    that was done and "could still be cancelled".
  - `Confirmed` (green): confirmed at the shop's depth, with the depth.
  - `Final` (green): confirmed at the shop's refund depth.
  - `DoNotHandOver` (red): the money is not safe, with a cause.
  - `Expired`, `Cancelled` (neutral): ended with nothing received.
  - `NeedsAPerson` (red): the sale holds more than the watcher can judge
    (WR-34).
- **CR-11.** The state is worked out again from the watcher's status
  every time it is asked for. Nothing is shown from memory except what
  CR-13, CR-14 and CR-16 say must stick.
- **CR-12.** Order of judgment, first match wins:
  1. the watcher says "needs a person": `NeedsAPerson`;
  2. a block that confirmed the payment left the chain and the payment is
     not confirmed now: `DoNotHandOver` (reopened by a reorg);
  3. a conflict was seen and the sale is not confirmed at the shop's
     depth: `DoNotHandOver` (replaced);
  4. confirmed at the refund depth: `Final`;
  5. confirmed at the shop's depth: `Confirmed`;
  6. the sale was accepted at once earlier (CR-13): `PaidInstant`;
  7. enough has arrived, unconfirmed: ask the instant decision.
     `Instant` gives `PaidInstant`; anything else gives `Waiting` with its
     reason, or `DoNotHandOver` (cannot check) if the check is not healthy;
  8. something has arrived, not enough: `PartPaid`;
  9. nothing: `NotPaidYet`, or `Expired` under CR-07.
- **CR-13.** Accepting at once is a step with a record. The counter adds
  the amount at risk to its exposure ledger, under the sale and not under
  a transaction (a fee bump changes the transaction), against the slice of
  the mode the decision used, and stores it before
  `PaidInstant` is returned for the first time. If the store fails or the
  ledger has no room, the state is `Waiting`. From then on the sale is
  "accepted at once" for good: a later worse answer from the instant
  decision does not take the green screen back, only rules 1 to 3 do.
- **CR-14.** A conflict on a sale that was accepted at once is a cancel.
  The counter records it in the pause tracker (instant pauses, §6),
  raises the cancel alarm, and the sale shows `DoNotHandOver`. The alarm
  stays raised until the owner clears it with a signed command
  (`openbmt_core::command`); nothing at the counter clears it. The same
  holds for resetting a pause and for releasing exposure by hand. A
  command is taken once: its number must be above the last one taken.
- **CR-15.** Exposure leaves the ledger when what the sale needs is first
  confirmed, or by the owner's signed command. Never on a timeout, and
  not when the sale turns red.
- **CR-16.** "Enough" is what the sale needs (CR-08) less the shop's
  tolerance (WR-32). More than that is paid in full; the surplus is
  recorded as owed to the customer. The instant decision is asked with
  the larger of what the sale needs and what arrived.
- **CR-17.** What arrived for a sale and is below 1,000 satoshis **[to
  tune]** in total is dust: it is logged and changes no state.
- **CR-18.** A payment that is seen and not confirmed for 2 hours **[to
  tune]** adds "stuck: not confirming" to the state's texts. The state
  does not change and the sale does not close.
- **CR-19.** A payment that arrives for a sale that expired or was
  cancelled is recorded on that sale, flagged for the owner, and shown
  as "late payment". It never changes another sale.

## 3. Overrides

- **CR-20.** A person can accept a sale that is `Waiting`, `PartPaid` or
  `DoNotHandOver`. The amount at risk goes into the exposure ledger as
  an override, stored before the screen changes. It is refused when the
  ledger has no room.
- **CR-21.** A cashier's override is refused above the per-sale instant
  limit, and when it would take that cashier's overrides of the last 24
  hours past the cashier's daily cap. A manager's is refused when it
  would take that manager's overrides of the last 24 hours past the
  manager's ceiling. The caller says who the person is and has already
  checked the PIN.
- **CR-22.** An override is recorded with who, when and how much. It
  does not clear a cancel alarm and does not make the sale `Confirmed`.

## 4. What leaves the counter

- **CR-30.** The sale closes in the POS as paid on `PaidInstant`,
  `Confirmed`, `Final` or an override, and on nothing else. A sale that
  turned `DoNotHandOver` after it was closed is reported to the POS as
  unpaid again.
- **CR-31.** The books count a sale as money only at `Final`. Before
  that it is pending.
- **CR-32.** Every change of state that CR-30 or CR-31 reports is also
  written as a signed record (`openbmt_core::records`), stored before it
  is reported.

## 5. Storage

- **CR-40.** Everything the counter must remember is handed to the
  caller's store before the step it belongs to takes effect: the address
  batches, the exposure ledger, the pause tracker, the counter's own
  state (address times, the cancel alarm, the last command number,
  override totals), each sale, and each record.
- **CR-41.** Where a step writes more than once, the order is chosen so
  that stopping between any two writes leaves the shop on the safe side:
  an address counted and not used, exposure counted and not shown as
  paid, a pause recorded and not yet shown.
- **CR-42.** A stored form is checked whole when it is read back. A sale
  whose stored form does not read back is shown as `NeedsAPerson`, never
  as paid.

## 6. The owner's heartbeat

Instant acceptance needs a second watcher that is not the counter's own
(`docs/DESIGN.md` §5.1, §5.2). The owner's device runs one and sends the
counter a small signed message (`openbmt_core::heartbeat`): a number, its own
time, the chain tip it sees (height and hash) and whether its own health
check says it is working. The counter takes it with
`Counter::heartbeat`, and the instant decision reads the result as the
`heartbeat` fact (`Counter::heartbeat_for_policy`). How the message travels
from the owner's device to the counter is not part of these rules.

- **CR-50.** Instant acceptance needs a heartbeat that the counter
  verified: signed by the owner root pinned at pairing, naming this
  counter, with a number above the last one taken. A heartbeat that fails
  any of these is refused and changes nothing.
- **CR-51.** The age of a heartbeat is the counter's time minus the
  heartbeat's own time, and it must be within the signed setting
  (`heartbeat_max_age_secs`); the instant decision checks that. A heartbeat
  timed more than 120 seconds ahead of the counter's clock **[to tune]** is
  refused, and its number is not used up.
- **CR-52.** The owner's watcher must say it is healthy. A heartbeat that
  says it is not is taken (its number counts) and makes the heartbeat "not
  verified" until a healthy one comes.
- **CR-53.** The tip the heartbeat names must agree with the counter's own
  header chain: the counter holds that hash at that height, and the height
  is at most 2 below the counter's tip. A tip the counter does not hold yet,
  at a height above its own tip by at most 2, is "not verified" for now (the
  counter may be the one that is behind) and is not an alarm. A different
  hash at a height the counter holds is "not verified" and is logged as a
  disagreement. The answer is worked out when the heartbeat is taken and not
  again as the chain moves; the next heartbeat is judged anew.
- **CR-54.** The last number taken is stored before the heartbeat counts. If
  the store fails, it does not count and its number is not used up. After a
  restart the counter has no verified heartbeat until a new one comes, and
  the stored number refuses replays.
- **CR-55.** Nothing but an owner-signed heartbeat counts: not the Box's
  "paid" notice, and not a signature of the approver root.
- **CR-56.** The heartbeat is a claim by the same software family on another
  device. It guards against a dead or cut-off owner watcher, not against a
  bad build: a build that signs "healthy" without watching anything is not
  seen by the counter. The outside check of `docs/DESIGN.md` §10 is the one
  that does not depend on this software.

## 7. What these rules do not do

- **They do not check payments.** Proof of work, proofs, peers and
  conflicts are the watcher's. A wrong fact handed in gives a wrong
  screen.
- **They do not see a payment drop out of the network.** A light client
  cannot. A payment that never confirms is "stuck" (CR-18); the limits
  of `docs/DESIGN.md` §6 are what protect the shop.
- **They do not refund.** Surplus and cancelled sales with money on them
  are recorded for the owner's refund session (§2.4).
- **They do not talk to the POS.** CR-30 says what to report; the
  connector reports it.
- **They do not know who a person is.** The list of managers and their
  PINs is not in the signed settings yet. The caller names the person
  and the role.
