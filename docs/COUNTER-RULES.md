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
- **CR-05.** A sale has an amount in the shop's currency and a quote: the
  amount in satoshis at the rate the caller supplies, good for 5 minutes.
  When a quote runs out the caller supplies a new rate and the sale gets a
  new quote. The address does not change. All quotes of a sale are kept.
- **CR-06.** A payment is judged against the quote that was in force when
  the payment was first seen (WR-46), or first reported if it was never
  "seen". A later quote does not change what an earlier payment had to
  be.
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
  the amount at risk to its exposure ledger and stores it before
  `PaidInstant` is returned for the first time. If the store fails or the
  ledger has no room, the state is `Waiting`. From then on the sale is
  "accepted at once" for good: a later worse answer from the instant
  decision does not take the green screen back, only rules 1 to 3 do.
- **CR-14.** A conflict on a sale that was accepted at once is a cancel.
  The counter records it in the pause tracker (instant pauses, §6),
  raises the cancel alarm, and the sale shows `DoNotHandOver`. The alarm
  stays raised until the owner clears it with a signed message; nothing
  at the counter clears it.
- **CR-15.** Exposure leaves the ledger at the payment's first
  confirmation, or by the owner's signed action. Never on a timeout, and
  not when the sale turns red.
- **CR-16.** "Enough" is the quote's amount less the shop's tolerance
  (WR-32). More than the quote is paid in full; the surplus is recorded
  as owed to the customer. The instant decision is asked with the larger
  of the quote and what arrived.
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
  `DoNotHandOver`. The amount still at risk goes into the exposure
  ledger as an override, stored before the screen changes. It is refused
  when the ledger has no room, and when it is above that person's limit
  in the signed settings.
- **CR-21.** An override is recorded with who, when and how much. It
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

## 5. What these rules do not do

- **They do not check payments.** Proof of work, proofs, peers and
  conflicts are the watcher's. A wrong fact handed in gives a wrong
  screen.
- **They do not see a payment drop out of the network.** A light client
  cannot. A payment that never confirms is "stuck" (CR-18); the limits
  of §6 are what protect the shop.
- **They do not refund.** Surplus and cancelled sales with money on them
  are recorded for the owner's refund session (§2.4).
- **They do not talk to the POS.** CR-30 says what to report; the
  connector reports it.
