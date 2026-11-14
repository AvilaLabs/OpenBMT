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
  WR-54) at that moment, except that being behind on blocks (WR-54) alone does
  not refuse opening: the sale opens and waits for a confirmation. Otherwise
  opening is refused and the cashier is told to use another payment type. A
  clock mismatch (WR-53) refuses opening too: the price lock and the expiry
  cannot be trusted.
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
- **CR-57.** A rate comes with its source: a label of 1 to 32 bytes from
  `a-z`, `0-9`, `-`, `+` and `.`, and the time, in Unix seconds, at which
  the caller got the rate. A label with any other byte, or none, is
  refused. The counter does not fetch rates and does not read the label;
  it writes down what the caller says. The source of the first quote is in
  the record `Opened`, and the source of every later quote in the record
  `Requoted` (CR-59).
- **CR-58.** A sale is not quoted at a stale rate. A rate that the caller
  got more than 600 seconds before the counter's time, or more than 60
  seconds after it, is refused, when a sale is opened and when it is
  requoted. Nothing is written when it is refused.
- **CR-59.** A new quote is written as a signed record `Requoted` (kind
  `Sale`): the sale, the time, what the whole amount needs at the new rate
  in satoshis, the rate and its source. It is stored before the sale takes
  the quote. If the process stops between the two writes, the record names
  a quote that the sale does not hold; the caller was told the requote
  failed, and asking again writes a second record.
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
- **CR-75.** The shop's signed settings name the smallest bitcoin sale, in
  satoshis (`min_sale_sats`; 0 means no floor). The first value is 0:
  the shop takes every sale above the dust line until the owner sets a
  floor. A sale whose amount, at the quote the counter
  makes for it (CR-05), is worth fewer satoshis than that is refused
  with `BelowMinimum`, before anything is written: no address is handed
  out, the desk does not change and no record is made. The cashier is
  told to take another kind of payment. The dust line (CR-17) is checked
  first, so when both apply the sale is refused as dust. A sale for
  exactly the floor opens. A new quote (CR-59) that would put a sale
  with nothing received below the floor is refused the same way and
  changes nothing; a sale that has been paid in part is not held to the
  floor again, because the customer has already paid into it. The
  counter counts, in memory only, the sales it refused for the floor
  since it started, so the owner can be told how often the floor turns
  sales away. Raising the floor is a tightening of the settings;
  lowering it, or turning it off, is a loosening and needs the Key
  app's co-signature like every loosening (§2.9 of the design).
- **CR-07.** A sale with nothing received ends as "expired" 15 minutes
  after it opened **[to tune]**, or when the cashier cancels it. Its
  address stays watched by the counter until the sale is settled (CR-76),
  and by the owner's wallet (CR-80).

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
     reason, or `DoNotHandOver` (cannot check) if the check is not healthy.
     The check is not healthy while the counter is behind on blocks
     (WR-54): a header is known above the height checked in full, so a
     conflicting payment in that block cannot have been seen. No sale is
     accepted at once then, and it waits for a confirmation;
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
  as "late payment". It never changes another sale. This holds until the
  sale is settled (CR-76); after that the counter no longer watches the
  address, and a payment to it is seen by the owner's wallet (CR-80).

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
  checked the PIN. The last 24 hours are a day by the clock and, when the
  heights are known, 144 blocks as well: an override counts until both have
  passed, so moving the date forward frees no cap. While no cashier is listed
  (CR-146) a name proves nobody, so the overrides of every cashier name count
  against one daily cap; with cashiers listed each person has their own.
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
  is reported. "Paid" and "unpaid" to the POS are the records `PosPaid` and
  `PosUnpaid`, written exactly when the report is made; "final" to the
  books is the record `Final`, which is written once, with the report. A
  chain written before `PosPaid` and `PosUnpaid` existed has none of them.
  The record `Final` says what the books need to know about the money:
  what has arrived and the depth reached; the transactions that pay the
  sale and what each pays, as the watcher names them at that moment
  (WR-31); the earliest time any of them was first seen, or, for a payment
  that was only ever seen in a block, the time the counter learned of the
  block; and what the received satoshis are worth in the shop's currency
  by the sale's own quotes (CR-06). The record names at most 32
  transactions. A sale with more standing transactions is final all the
  same: the record names the 32 that pay the most (the smaller transaction
  ID first among equal amounts), in ascending order of ID, and says there
  were more. The record `Opened` carries the first quote's source (CR-57).

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
  verified: signed by the owner root pinned at pairing, or by the watch key
  of the settings in force (CR-185, CR-186), naming this counter, with a
  number above the last one taken. A heartbeat that fails
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
- **CR-55.** Nothing but an owner-signed heartbeat, or one signed by the
  watch key of the settings in force (CR-186), counts: not the Box's
  "paid" notice, and not a signature of the approver root.
- **CR-56.** The heartbeat is a claim by the same software family on another
  device. It guards against a dead or cut-off owner watcher, not against a
  bad build: a build that signs "healthy" without watching anything is not
  seen by the counter. The outside check of `docs/DESIGN.md` §10 is the one
  that does not depend on this software.

## 7. Refunds: the cashier's side

A refund is two things in two places: the cashier asks at the counter, and
the owner's device pays in an approval session (`docs/DESIGN.md` §2.4,
`docs/SESSION-RULES.md` AS-50 to AS-57). These rules are the asking. A
sale's refund state is stored with the sale (`OBML`, version 2, written
only for a sale that has a refund entry; a version 1 sale has none) and the
request number in the desk (`OBMG`, version 3, written only once a request
has been made).

- **CR-60.** A refund request is a record, not a payment. The counter holds
  no key that can move funds. A request is a record of kind `Refund`
  (`RefundRequested`) on the counter's chain. It reaches the owner with the
  other records (`docs/LINK-RULES.md` LK-54 to LK-58) and becomes a queued
  refund there only after the owner's device has checked it. Nothing is
  paid until an approval session.
- **CR-61.** A request names: the sale's receive index; the amount, in
  satoshis or in the shop's currency, one of the two; the destination
  script; the cashier's name and role, as an override does (CR-21); the
  reason; the time; the satoshis the counter saw received for the sale
  at that time; whether the sale had reached the refund depth; and a
  request number, rising per counter from 1. Numbers rise and may skip: a
  number the desk gave out for a request that was never written (CR-73) is
  not used again, and the owner does not need them to follow one another.
  The signed settings have no
  "refund at the dollar price or at the bitcoin amount" choice, so the
  caller passes the choice with each call. A name and a reason are 1 to 64
  characters, at most 255 bytes, with no control character: the labels of
  AS-07. A value in the shop's currency comes with a rate and its source
  and is turned into satoshis at that rate, rounded down in the shop's
  favour; the rate is as fresh as CR-58 asks.
- **CR-62.** The destination comes from the customer's QR code and only a
  plain on-chain address is taken from it. `refund_destination` accepts a
  bare address or a `bitcoin:` URI and takes the address and nothing else.
  The amount, the label, the message and every other field are ignored,
  including a field named `req-`. This differs from the Key app's deposit
  reader on purpose: that reader refuses a code with a `req-` field, as
  BIP21 says, because it enrols an address; here the amount is the shop's
  and the code may carry anything. A text over 2,048 bytes, a character
  that is not printable ASCII, and an empty or unreadable address are
  refused.
- **CR-63.** An address for another network is refused. So is an address
  whose script is not one the Key app's signer would pay (P2WPKH, P2WSH,
  P2TR, P2PKH or P2SH, SR-22). The counter crate does not depend on the
  signer; it keeps its own copy of the list, and a test in a crate that
  depends on both checks the two agree.
- **CR-64.** A request for a sale the counter does not hold is refused.
- **CR-65.** A request for a sale for which nothing has arrived is refused.
  A sale that only holds dust, or that is `Expired` or `Cancelled`, has
  nothing to refund.
- **CR-66.** A sale that has not reached the refund depth (the setting
  `final_confirmations`) is not a refusal. The request is recorded and is
  shown as "waiting for depth" until the sale is final (DESIGN §2.4). It is
  one stored state with a flag: the view works the flag out again from the
  sale each time it is asked for, as CR-11 does for the screen. A reorg
  that undoes the payment puts the flag back.
- **CR-67.** A request is refused when its amount is zero, or when it
  would take what is asked for the sale past what was received for it.
  What is asked is the sum of the earlier requests that are not withdrawn
  and the cash refunds (CR-71), valued in satoshis at the sale's last
  quote. This holds across parts: two requests that together pass what was
  received make the second one refused.
- **CR-68.** A request whose destination is a script of the wallet's own, as
  far as the counter can tell (any script in its address batches, shown or
  not), is refused. The counter cannot check further: the owner and the Key
  app see every line.
- **CR-69.** A cashier's request is refused when it is above the largest
  single refund of the settings, or when it would take that cashier's
  refunds of the last 24 hours (requests, withdrawn ones included, and cash
  refunds) past the settings' daily refund cap. The refusal says a manager
  is needed. A manager's request is not held to either; it is held to CR-67
  and to the owner's and the Key app's own limits. The caller says who the
  person is and has checked the PIN, as for an override (CR-21). The daily
  refund cap is shared by every cashier name while no cashier is listed, and
  is each person's own once cashiers are listed, as the override cap is.
- **CR-70.** A cashier or a manager can withdraw a request. A
  `RefundWithdrawn` record is written, naming the request number. The
  counter cannot know whether the owner has taken the request, so the
  withdrawal is a message and not a fact: the owner honours it only if the
  refund is still queued and not in an open session. A withdrawn request no
  longer counts for CR-67 when a request is asked or a manager gives cash,
  but it still counts when a cashier gives cash (CR-71, CR-119), and it
  still counts for the cap (CR-69). A request that is withdrawn, or does
  not exist, cannot be withdrawn again.
- **CR-71.** A cash refund from the till (the shop's option, DESIGN §2.4) is
  a record `RefundPaidInCash` with the amount in the shop's currency, the
  cashier and the reason. It is valued at the sale's last quote and counts
  for CR-67, rounded up to a whole satoshi, and for the cashier's cap
  (CR-69). It asks the owner for nothing: the bitcoin is kept. For CR-67 a
  cashier's cash refund counts the withdrawn requests too (CR-119); a
  manager's does not. Cash goes out only against bitcoin at the shop's
  `min_confirmations` or deeper: the amount is checked against what of the
  sale is that deep, and a cash refund of a sale paid only by an unconfirmed
  payment is refused, for a manager too, until it confirms. Satoshis paid
  over Lightning count as confirmed. A bitcoin refund is not changed by this:
  it already waits for the final depth (CR-66).
- **CR-72.** For each sale the counter gives a `RefundView` for each entry:
  the sale, what was asked in satoshis, the state (`Requested`,
  `WaitingForDepth`, `Withdrawn` or `PaidInCash`), who asked and when. The
  words on the customer's receipt are the app's.
- **CR-73.** The writes of a request, in this order: the desk with the
  next request number, the record, then the sale. A stop after the desk
  wastes a number and never reuses one. The record is the truth: the
  counter's own records say which refunds exist, and the sale's refund
  entries are what those records say, in record order. When a counter is
  opened, and before any refund call acts, every sale's entries are made
  equal to its records: a request, a withdrawal or a cash refund whose record
  was written and whose sale was not is taken in and the sale is stored
  again. If that cannot be stored, the call is refused. The same is done at
  once after a call whose sale could not be stored. So after any stop the
  views show what was recorded, the room left counts it (CR-67, CR-69), and
  asking again cannot give a second record for the same ask: it gives a
  second refund only if the cashier means one. A call that fails after
  its record was written still reports the failure; the caller reads the
  views. The desk's last number is never below the highest in the records.
  Withdrawing and a cash refund write the record, then the sale, and follow
  the same rule.
- **CR-74.** These rules do not prove who the customer is. They do not stop
  a cashier, or a cashier with a manager's help, from asking for a wrong
  destination: the owner and the Key app see every line before it is paid,
  and the Key app limits refunds to unknown destinations. The counter does
  not learn whether a refund was paid: that would need a message from the
  owner back to the counter, which these rules do not have.
- **CR-119.** A cashier's cash refund (CR-71) is refused when it would take
  what is asked for the sale past what was received only if the sale's
  withdrawn requests are counted as well. The owner may already have the
  request in an open approval session (CR-70), and would pay it on top of
  the cash. The refusal says the owner is to be asked whether the request
  was dropped, and that a manager can then give the cash refund. A manager's
  cash refund, and a new request (the owner sees both), count the withdrawn
  requests as CR-67 says. A cash refund that is over what arrived without
  the withdrawn requests is refused under CR-67 as before.
- **CR-120.** Each refund entry keeps the chain height the counter's headers
  were at when the entry was made, when that height is above zero; a record
  without one reads back as height unknown. An entry counts toward a
  cashier's daily cap (CR-69) when any of these hold: it is from the last 24
  hours by the clock; both its height and the chain height now are known and
  the entry is fewer than 144 blocks old; its time is later than the clock
  now. So a cashier who moves the phone's date forward does not free the cap
  until a day of blocks has passed too. The refund records `RefundRequested`
  and `RefundPaidInCash` take the tags 22 and 23 when they carry the height
  (a `u32` after the other fields) and keep the tags 16 and 18, and their
  bytes, when they do not; the sale's refund entry takes the kinds 3 and 4
  for the same (a `u32` after the withdrawn flag) and keeps the kinds 1 and
  2. A build from before this one refuses the new tags and kinds.

## 8. Settling old sales

The counter watches every address it showed and keeps every sale in memory,
so a shop that sells every day would reach the network manager's limit on
watched scripts (NR-41) and grow without end. Its records are bounded
separately (CR-81). The owner's
wallet already watches every issued receive index for good and keeps a
receipt of every payment (OC-07, OC-10 to OC-12), so the counter can let an
old sale go once the owner holds everything the counter wrote about it.

The owner's mark (LK-55) says how many of the counter's records the owner
holds. The link hands it to the counter with `Counter::set_owner_mark`. The
mark is kept in memory only; after a start there is none until the owner's
next message of records received, and until then nothing is settled. A
mark that goes down is taken as it is: sales already settled stay settled,
and sales not yet settled wait for the owner's copy to catch up.

- **CR-76.** A sale is settled when all of these hold at the time the caller
  passes:
  1. it is not live (`Sale::is_live`): it ended or the books were told it
     is final;
  2. its last change is at least `SETTLE_SECS` (30 days **[to tune]**) old:
     the last change is the later of the time it ended and the time of the
     newest record that names it. A sale with neither time is never
     settled;
  3. none of its refund entries is `Requested` or `WaitingForDepth` (CR-72);
  4. every record that names it is at or below the owner's mark, that is,
     the number of the newest such record is below the number of records
     the owner holds;
  5. it is not unreadable (CR-42): a sale that does not read back is never
     settled and its slot is never removed;
  6. its exposure is not still counted in the ledger (CR-15): a sale
     accepted at once whose exposure has not been released is not settled.
  A sale that is live, or that the counter has no owner's mark for, is
  never settled.
- **CR-77.** `Counter::settle(store, now)` settles at most
  `MAX_SETTLE_PER_CALL` (256) sales in one call, the one whose last change
  is oldest first (the lower index first among equal times), and says how
  many it settled and whether more are waiting. For each sale it removes
  the stored sale (`Store::delete` of `Slot::Sale`) first and the sale from
  memory second. If the store refuses, the call stops at that sale, nothing
  is half removed in memory, and the fault is returned; the sales settled
  before it stay settled. A stop between the two steps leaves the sale in
  memory and its slot gone; when the counter is opened again a sale whose
  slot is gone is not loaded, which is the settled state. Settling writes no
  record: the records are the truth and the owner already holds them. The
  desk's numbers and the batches' issued ranges are not touched, so the
  index of a settled sale is never handed out again.
- **CR-78.** After a settled sale is gone, the counter does not judge it, does
  not report it and does not take sightings for its address (CR-80). A
  call that names its index is refused as for any sale the counter does not
  hold. The refund numbers in the records still count (CR-73).
- **CR-79.** The batches keep the time at which each script was shown or
  retired, in their stored form (`OBMC`, same version). A script that no
  sale of the counter holds, that was shown or retired at least
  `SETTLE_SECS` ago, leaves the counter's watch list: it is not in
  `Counter::watched_scripts()` after the next call to `settle`. This covers
  an address that was handed out and never used for a sale (CR-41), an
  address of a retired batch, and the address of a sale that was settled.
  The index stays issued: the range is kept and the index is never handed out
  again. Nothing is released while any sale is unreadable (CR-42), because
  the counter cannot tell which address it held. The counter does not
  learn what an address it does not hold received: that is the owner's.
- **CR-80.** What the counter does not watch any more, the owner's wallet
  does (OC-07, OC-10 to OC-12): every issued address is watched, all the
  time while the wallet's addresses fit the live watch, and by a check once a
  month after that (OC-22 to OC-26). A late payment to the address of a
  settled sale is not reported by the counter; the owner's coin view sees it
  and records its receipt, at once or at the next monthly check. A shop
  that wants the counter to flag such a payment must not settle: there is no
  switch for it.
- **CR-81.** The counter's records leave its memory. Every record is in the
  store's record log (`Store::append_record`); the counter keeps the number
  of records, the hash of the newest (the head) and the records from `base`
  up, and nothing about the ones below. Records below `base` are read back
  with `Store::read_records(from, max)` when something asks for them.
  1. **Where `base` is.** `base` is `len` less what memory holds, and what
     memory holds is the larger of `KEEP_RECORDS` (1,024 **[to tune]**)
     and the number of records above the owner's mark (CR-76), but never
     more than `MAX_UNSENT_IN_MEMORY` (16,384 **[to tune]**). A mark that
     is not known counts as zero. Records above the mark are what the link
     sends, so they stay in memory; when the owner has not taken more than
     `MAX_UNSENT_IN_MEMORY` of them, the newest stay and the older ones are
     read from the store when they are sent (a send reads at most one
     message's worth of them at a time). The cap bounds memory at about 3
     MB for records of the usual size whatever the owner does.
  2. **When it moves.** `base` is set when a record is written and when
     `set_owner_mark` is called, and it only goes up: a mark that goes down
     does not bring records back into memory, they are read.
  3. **The calls.** `records_len()`, `head()`, `records_base()`,
     `records_in_memory()`, and, each given the store, `record_at(n)`,
     `records_from(from, max)` (at most `MAX_READ`, 1,024, records, oldest
     first, fewer at the end, none past it) and `hash_at(n)` (the hash of
     record `n - 1`, `GENESIS` for 0, the head for the length, `None` above
     it). A record or hash from memory needs no read. Nothing hands out
     the whole chain.
  4. **What is checked on a read (CR-42).** A record read from the store
     must parse, be the number asked for, be signed by this counter, follow
     the record before it in the same read (record 0 follows `GENESIS`),
     and, when it is the record just below `base`, be the one the first
     record in memory follows. Otherwise the call returns
     `StoreFault::Damaged { seq, why }`; a store that cannot answer returns
     `StoreFault::Read`. Either way nothing in memory changes and the
     caller goes on.
  5. **LK-55 and LK-56.** The check of a mark below `base` reads the hash
     with `hash_at`; a wrong head there is ignored as one above `base` is
     (LK-55). A mark that goes down below `base` is taken, and the resend
     of LK-56 reads its records from the store, oldest first, with the
     same limit of messages in a step. A read that fails in either is a
     note, once, and the step goes on; the last mark stands and nothing is
     sent that was not read whole.
  6. **At start.** `Counter::load` reads the record log from record 0 in
     chunks of 256, checks each record as `Chain::verify` does (author,
     signature, number, `prev`, check-in claims; it is `Verifier` in
     `openbmt_core::records`), and keeps only the newest
     `MAX_UNSENT_IN_MEMORY`. A log
     that cannot be read, or has a record that does not parse, refuses the
     start (`LoadError::RecordsUnread`); one that does not verify refuses
     it as before (`LoadError::Records`). The same pass builds what the
     counter needs from the whole history: the newest record that names
     each sale (CR-76), and the refund entries of each sale and the
     highest refund number (CR-73). Both are then kept up as records are
     written, so that no call reads the history again.
  7. **The record file.** `DirStore` keeps the records in the one file it
     always did, in the same format, and an index of 8 bytes for each 64
     records (the offset of every 64th). Its load reads the file once to
     check the framing, cut a torn tail and make the index; a read seeks to
     the nearest mark and reads forward. A file that changed after the
     index was made is an error on read, not a wrong record.
  8. **A failed append.** `DirStore::append_record` writes the record,
     flushes the file's data and, for a file it made, the directory, all
     before its index moves. If any of that fails the write is taken back
     (the file cut to its length before and flushed, or the file it made
     removed and the directory flushed) and only then is the error
     returned: a failed append leaves the log as it was, so that the
     retry does not write the same number twice. If the take back fails
     too, no further record is appended (each is refused as a store fault,
     CR-40) until the store is opened again, which reads the file as it is.
  A start with 100,000 records took about 11 seconds on the development
  laptop (debug build, signatures checked) and held 16,384 records.
- **CR-82.** `openbmt-counter-node` calls `settle` from the node's pump once
  at start, as soon as the owner's mark is known, and then once an hour, and
  never more often than once a minute. The list of scripts the driver is
  started with (`watched_scripts()`) is then the shorter one. The node keeps
  a line in its log when it settles: the count only, no index and no
  address. A store fault in a settle is a note and changes nothing else.
- **CR-83.** The payment request the customer scans is the sale's
  address and what it needs now (CR-08) as a BIP 21 URI. The text of the
  QR code is `BITCOIN:` and a bech32 address, both in upper case, so that
  the address is in the alphanumeric set, then `?amount=` and the amount
  in bitcoin (`amount` stays lower case, as BIP 21 has it). The amount has
  at most 8 decimals, no trailing zeros and no exponent, and is left out
  with its `?` when it is zero. An address that is not bech32 is left as
  it is. The same request in lower case is given for the cashier's
  "copy". The code is made at error-correction level M in one segment, and
  is handed to the app as its side length and its modules, most
  significant bit first, row by row.

## 9. The Android counter app

`apps/counter-android` is the cashier's screen. These rules are the app's
own; the library judges sales and prices and knows nothing of them.

- **CR-84.** The manager's PIN is 6 to 12 digits. It is kept as
  PBKDF2-HMAC-SHA256 with 600,000 iterations and a random 16-byte salt, in
  one file in the app's `noBackupFilesDir`. It is written in the order of
  CR-40 by atomic steps (new file, sync, rename, sync of the folder), so a
  crash leaves the old PIN or the new one. The PIN is never in the settings, the
  logs or a screen.
- **CR-85.** A PIN is compared in constant time. A wrong try holds the PIN
  for 1 second, then 2, 4, 8 and so on, up to 5 minutes. While it is held
  the PIN is not looked at, and the screen says how long to wait. The count
  is written before the answer is given, so closing the app does not reset
  it. The answer is acted on only while the app is shown (CR-200). The wait
  is measured by the time since the phone started, not by its date, so
  moving the date ends no wait; after a restart of the phone the wait starts
  again in full.
- **CR-86.** A cashier's action needs no PIN: opening, requoting and
  cancelling a sale, asking for or withdrawing a refund, and a cash refund.
  The actions of CR-21 (an override) and of CR-67 to CR-69 (a refund asked
  for or withdrawn by a manager) are made with the manager as the actor
  only after the PIN was checked. The app never names the manager as the
  actor without it. Each action names its actor, and the library keeps the
  name.
- **CR-87.** When the signed settings have Tor on, every connection of the
  app, the node's and the price fetch's, goes through a SOCKS5 proxy, and no
  name is looked up by the phone. The proxy is the app's own Tor (CR-201,
  the choice "Built-in Tor", which a new counter has) or Orbot at the app's
  setting (`127.0.0.1:9050` unless the owner changed it, the choice "Use
  Orbot", CR-203). Before the node starts, and before every price fetch, the
  app has the proxy: the built-in Tor's port once Tor has a circuit, or, with
  Orbot, the answer to a SOCKS5 greeting. If it has none, nothing is sent
  around it: the node is not started, no price is fetched, and the app says
  so (CR-204 for the built-in Tor; for Orbot "Tor is on for this shop, but
  Orbot is not running." and "Open Orbot and turn it on. If it listens at
  another address, the manager can change it under Network."). The Network
  screen, reached from home and from that screen, holds the proxy's address
  of Orbot; it is saved behind the manager's PIN. The manager is the one who
  acts. This is NR-26 applied: the proxy is the caller's. The node enforces
  it from the verified settings: `start` refuses to start, with this rule,
  when the signed settings have Tor on and the start has no proxy. The app's
  reading of the settings before the start (`pairing_read`) is for display
  only and is not trusted.
- **CR-88.** The node runs only while the app is on the screen. It starts
  when the activity becomes visible and is stopped, cleanly, when it stops
  being visible, so that it lets go of its folder. The screen is kept on
  while the app is open. The app has no service and no notification. The
  About screen and the README say so.
- **CR-89.** The price is fetched by the app on the schedule of RT-40 to
  RT-45: a fixed interval of 60 seconds, never because a sale opened; at
  most 64 KiB read; a timeout of 20 seconds on the whole fetch; no cookies,
  no cache, no redirect, no compression; one fixed generic User-Agent for
  every shop. The reply is given to the library's `rate_parse`, the samples
  to `rate_combine`; the app parses no JSON.
- **CR-90.** The camera reads the codes of pairing and nothing else: the
  Key app's offer and the owner phone's counter bundle (CR-91, CR-92). The
  permission is asked for only when the manager taps Scan, never at start,
  and if it is refused or "don't ask again" was chosen the app says so in
  words and says that pairing needs the camera. The camera is open only
  while the scan page is on the screen and is released on the first code, on
  Cancel, when the app is not visible and when the scan fails. Nothing is
  kept from the frames: a frame is read once, zeroed, and never written to
  disk, logged or sent anywhere, and the bytes of the code are handed to the
  pairing call like any other, to be read by the library's strict decoder.
  The manifest asks for the camera as a feature that is not required.

## 10. Pairing the counter

These rules implement `docs/DESIGN.md` §2.9 for the counter's side of the
pairing ceremony. The ceremony, the checks and the stored record are in
`crates/openbmt-counter-node` (`paired`) and run behind
`crates/openbmt-counter-ffi` (`pair_begin`, `pair_match`, `pair_differ`,
`pair_cancel`, `pair_finish`); the screens are the app's (CR-98). The
Key app shows its offer as a code; the counter scans it, shows its own
request as a code with four words; the owner phone carries the request in an
approval session; the Key app shows the words; the person compares them on
both screens; the owner phone shows a counter bundle and the counter scans
it.

- **CR-91.** The ceremony begins from the offer's bytes and from nothing
  else the app supplies but a name and the time. The counter makes its own
  identity key and a fresh 32-byte number from the system's random source,
  in the library and never from the app; if that source gives nothing or
  cannot be trusted to be unpredictable, the call is refused. The ceremony is in
  memory only: a restart forgets it, and a new `pair_begin`, a refusal, a
  finish, `pair_differ` and `pair_cancel` each forget it with its key and
  number. A call with no ceremony is refused with words that say to begin
  again from the Key app.
- **CR-92.** A bundle is taken only after the person has said "They match",
  and only within 10 minutes of the start. "They differ" ends the ceremony,
  forgets the key and the number, and says in words what happened and to
  start again from the Key app. A bundle that is not a bundle leaves the
  ceremony as it was; a bundle for another counter, another try or another
  wallet, or whose pairing is not signed by both pinned roots, is refused
  and ends the ceremony. The checks run in this order: the match, the
  bundle's encoding, the pairing, the settings (CR-94, CR-95), the first
  batch (CR-96).
- **CR-93.** The two roots, the wallet check and the network are the
  offer's and are never taken from the bundle, the app or a setting. The
  offer names the network in one byte and it is part of what the four words
  are made from, so an offer for another network gives other words and a
  request made for it is refused by the Key app.
- **CR-94.** The settings in the bundle are verified against the two pinned
  roots with no earlier settings, so both signatures are needed. Their
  version must be above the pairing's settings floor (the version in force
  on the Key app when it signed): an equal or lower version is refused.
  Settings that cannot be read or are not signed as they must be are
  refused.
- **CR-95.** This counter must be on the settings' device list and among
  the settings' counters. Settings that do not name it are refused, and the
  words say that the owner must add the counter and pair it again.
- **CR-96.** The first batch, when there is one, goes through the batch path
  of the library as any batch does: this counter, this wallet, both
  signatures, no overlap. Its smallest size is the counter's own number
  (`BATCH_MIN_COUNT`, 1000, the library's default) and nothing in the bundle
  or the record can lower it. A batch that does not pass refuses the whole
  bundle.
- **CR-97.** What a counter keeps after pairing is one framed record
  (`OBMt`, version 1, or version 2 with a relay, CR-131): the identity
  secret, the two roots, the wallet check, the network, the settings floor,
  the signed settings, the first batch, the peers and, in version 2, the
  relay. It is decoded strictly (exact length, no trailing bytes, every
  part decoded and re-encoded to the same bytes) and holds nothing that was
  not checked. `start` takes the record in place of separate fields, reads
  it and runs CR-94 to CR-96 again on it before anything starts; a record
  that does not read or holds a secret that is not a key is refused. The
  record holds the counter's key, so the app seals it (CR-98) and its text
  is never printed: the debug form hides the secret.
- **CR-98.** The app's pairing screens are the camera's: scan the offer,
  show the request as a code with its four words and "They match" and "They
  differ", then scan the bundle. The words are the most prominent thing on
  the screen. There is no pairing text, no paste and no file. The record
  that `pair_finish` returns is sealed with the Android Keystore in the
  app's `noBackupFilesDir` and handed to `start` at every start; the
  ceremony under way is never kept, and a restart says so in words and
  starts over.
- **CR-99.** The peers in the bundle are hints: at most 8, each a literal
  address and port in its canonical spelling (no names, port not 0), sorted
  and without repeats, at most 64 bytes each. They only choose whom the
  counter asks first. Nothing is checked against them and nothing that
  moves money depends on them.

## 11. The own node as a second check

These rules join the merchant's own node (`docs/NETWORK-RULES.md`, NR-98
to NR-103; `docs/WATCHER-RULES.md`, WR-60 to WR-65) to the counter. The
setting is read in `crates/openbmt-counter-node` (`ownnode`) and behind
`crates/openbmt-counter-ffi` (`own_node_check`, `own_node_retry`); the node
passes it to the driver and keeps the health line; the judgment is in
`crates/openbmt-counter`; the screens are the app's. The own-node tier of
the instant decision already exists in `openbmt_core::policy`, with its own
limits in the signed settings: these rules only say when the counter uses it.

- **CR-100.** The own node's address is a setting of this counter, not of the
  shop. It is a hint of where to ask, and what makes the node trusted is the
  name itself (NR-98). It is set on the counter by the manager's PIN
  (CR-84 to CR-86; the counter has no other PIN), kept in the app's own
  folder with the Orbot address and the names, never in the pairing record,
  and removed with the same PIN. What is kept is one canonical text: a Tor
  v3 name, a colon and a port. The text typed may have no port, and then
  the port is the network's usual one (8333, 18444 on regtest, 38333 on
  signet). A name that is not a Tor v3 name, and a port that is not from 1
  to 65535, are refused with words, and nothing is kept. A start that is
  given an address that is refused starts without an own node and says so
  in the own node's health line; it never starts with an address that the
  library did not accept.
- **CR-101.** A home-network, loopback or clear-net address is never the own
  node (NR-99). The words say that a home-network address cannot prove which
  machine answers, and where to find the node's Tor address. The counter
  app has no way to add an ordinary peer: the peers come with the owner's
  pairing (CR-99).
- **CR-102.** The own node needs Tor. When the counter has no Tor proxy
  (CR-87: the signed settings have Tor off), turning the own node on is
  refused with words that say to turn on Orbot first, and a start that has
  an own node and no proxy starts without it (CR-100).
- **CR-103.** The node passes the own node to the driver, takes its reports
  on the channel of its own (NR-103) and keeps one tracker for the health
  line (WR-63). A report is given to the sales that know the transaction
  (WR-61). The node asks the own node about the unconfirmed payments of the
  live sales when it starts, when the own node connects, and after each
  sighting of the light client that a sale took (WR-65), at most 256 at a
  time. It never reads the own node's reports as sightings, and it takes no
  header, block or transaction from it (NR-101, NR-102).
- **CR-104.** Every sale is judged with the own node's health line
  (`Counter::judge_with_own_node`, `override_sale_with_own_node`). The
  instant decision uses the own-node tier, with the limits of the signed
  settings, only when the sale's status says the own node counted (WR-62):
  the own node reported the payment, can be used now, and the light client
  saw it too. Otherwise it uses the light tier, as without an own node. A
  report is kept in memory only and is not in the stored sale (WR-65).
- **CR-105.** The screen always says which check was done. A sale that
  waits shows one line for the check: the light check alone; the light check
  because the own node saw the payment but cannot be used just now (the
  fallback of WR-64 is never silent); the own node and the light check;
  the own node alone, waiting for the light check; the Box and the light
  check; the Box alone. A sale that was accepted at once says its check in
  its own words (CR-10, "Paid (your node checked it)").
- **CR-106.** Every record of a sale accepted at once names the check:
  the label shown and the mode the limits were taken from (CR-13, CR-32). An
  acceptance by the own node is recorded as "your node" and the node mode,
  an acceptance by the light check as "light check" and the light mode.
- **CR-107.** The counter's status shows the own node's health line
  (WR-63, WR-64) in words, with the time it was last heard, and a Try again
  that asks the driver to dial the own node again (NR-100). With no own
  node set the status shows no line at all. The line is shown on the status
  screen of the app; the check of a sale is shown on the sale screen
  (CR-105).
- **CR-108.** With no own node set nothing changes: no line, no words, no
  own-node tier, whatever any report said, and `judge` is the same call as
  `judge_with_own_node` with `OwnNodeHealth::not_configured()`.

## 12. Lightning at the counter

These rules join the invoices of the shop's own Lightning node
(`docs/LIGHTNING-RULES.md`, LN-01 to LN-23; `docs/DESIGN.md` section 8) to
the counter. The library of that document builds the requests, reads the
replies and checks the invoice; the driver that asks the node is in
`crates/openbmt-counter-node` (`lightning`); the judgment of a paid sale is in
`crates/openbmt-counter`; the transport and the screens are the app's. The
node's public key is part of the signed settings (LN-03); the credential the
counter holds is not, and never leaves the counter.

- **CR-109.** A sale is paid over Lightning when the shop's own node says
  the invoice made for it was settled for at least what a quote of the sale
  asks (LN-14). `Counter::lightning_paid` takes the node's facts: the
  payment hash, the amount received in millisatoshis and the time the node
  says it settled. The sale is then paid and final at once, with no instant
  limit, no exposure and no wait for a block (DESIGN section 8): it shows
  `PaidLightning` (green, "Paid over Lightning"), the POS is told it is paid
  (CR-30), the books are told it is final (CR-31) and the sale is no longer
  live (`books_final`; it does not expire, cannot be cancelled and takes no
  new quote). The writes are the records `PaidByLightning` (kind `Payment`),
  the surplus of CR-111 if there is one and `PosPaid`, then the sale (CR-41).
  Nothing is written and the call is refused for a sale the counter does not
  hold, for a sale that has ended or is final (the payment stays in the
  node, which is the one place that has it), for a sale that was paid
  another way (accepted at once, accepted by a person, or with enough on its
  address) and for a payment below the least any quote of the sale asks: the
  invoice of an earlier quote may be paid after a newer quote asks more or
  less, and the payment counts if it covers any of them (CR-112 keeps those
  invoices watched). Asking again for a
  sale that is paid over Lightning already changes nothing. The cashier's
  screen says "Lightning, final" as the check line (CR-105) and says that a
  refund for it is given in cash, never over Lightning.
- **CR-110.** What the sale keeps of the payment is the payment hash, the
  amount in millisatoshis and the settle time if the node gave one. They are
  in the stored sale (`OBML` version 3, written only for a sale paid over
  Lightning, so every earlier file reads back and every earlier sale is
  written as before) and in the record `PaidByLightning`, which also holds
  what the satoshis received are worth in the shop's currency by the quote in
  force (CR-06). The record is read strictly: a payment for nothing, a byte
  too few or too many, and a stored form that is not final or has ended do
  not read. The books carry them (`docs/BOOKS-RULES.md`, BK-32 onward).
- **CR-111.** The address of a sale that is paid over Lightning stays
  watched, and the two payments are not mixed. What had arrived on the
  address before the node's payment, above the dust line, is recorded as
  owed to the customer (CR-16, `SurplusOwed`). What arrives after is a late
  payment (CR-19): recorded on the sale, flagged for the owner, shown as "Late
  payment", and never a change of the sale's state. A refund of a sale paid
  over Lightning uses the refund paths of section 7: what the node received
  counts as received and as at the refund depth at once, so a cash refund
  (CR-71) is open for all of it. An on-chain refund (CR-60) is open only up
  to what arrived on-chain (CR-217). There is no refund over Lightning: the
  counter holds no key that can pay.
- **CR-217.** The owner's phone pays a refund only against payments it sees
  on the chain, and a payment over Lightning through the shop's own node is
  not on the chain. (A payment through a swap provider is: CR-224.) So the
  counter offers only a cash refund (CR-71) for a sale whose payment is
  all Lightning through the shop's own node, and refuses a request for a
  bitcoin refund of it (CR-60) with "Refund in cash", for the cashier: "This
  sale was paid over
  Lightning, and the owner's phone can only check payments it sees on the
  chain, so a bitcoin refund cannot be sent." and "Give the refund in cash
  from the till." A sale paid partly over Lightning and partly on-chain can
  be refunded on-chain up to what arrived on-chain, less the requests that
  stand (withdrawn ones do not count here), and the rest in cash. A request
  past that is refused the same way. A sale that no Lightning payment
  settled has no such limit. The sale's details do not offer the request
  while nothing is left to ask on-chain. The owner's side does not change.
- **CR-112.** The driver asks the node for an invoice for each quote of a sale:
  the amount is the quote's satoshis times 1,000, the lifetime
  `QUOTE_SECS` (CR-05, LN-13) and the attempt is the next one for that sale
  and quote (LN-15). A new quote of the same sale (CR-05) asks for a new
  invoice and drops nothing: the invoice of an earlier quote is still the
  shop's node's, and the customer may have scanned it, so the driver keeps
  every invoice of a sale, each with its own lifetime, until that has run out
  (`QUOTE_SECS` after the quote, with the grace of the lookups) or the node
  says it is settled, cancelled or unknown. The invoice of the sale's current
  quote is looked up every 2 seconds while the sale is open and unpaid. An
  earlier invoice of an open sale, and every invoice of a sale that is closed
  (paid, cancelled, expired or final), is looked up every 10 seconds. At most
  one request is out at a time for the whole driver. A node that cannot be
  reached, or whose reply is refused, is a note in the log and never a state
  of the sale. The driver holds no clock and no network: it hands out the
  next request with a number and takes the reply for that number; a reply for
  any other number, or for an invoice the driver no longer holds, is ignored.
  An earlier invoice of an open sale that the node says is settled is a
  payment of the sale (CR-109); one of a sale that is closed is a late payment
  (CR-117). When the counter starts again, the driver holds nothing, so for
  each sale from the counter's records it asks Core Lightning by label (the
  label carries the sale and the attempt, LN-15, which is the quote's place in
  the sale) for the invoice of every quote whose lifetime has not run out,
  for a sale that is open and for one that is over, and watches those that
  come back, an invoice already paid while the counter was stopped included.
  The current quote of an open sale asks as it did when the sale began. A
  label the node does not know is dropped. LND has no labels, so after a
  restart only the current quote's invoice is made again there.
- **CR-113.** The sale shows its on-chain code at once and never waits for
  Lightning. When the invoice has passed LN-17 against the node key of the
  signed settings, the code becomes the code of LN-18, in capitals when LN-19
  allows. If no invoice has come 6 seconds after the quote, or it fails a
  check, the on-chain code stays and one line says that Lightning is not
  available for this sale, with the reason under the details (LN-22): the
  cashier takes another kind of payment if the customer has only Lightning
  and tells the owner what the details say. A sale opened with no Lightning
  credential, or while Lightning is off (CR-116), has no such line.
- **CR-114.** The app talks to the node over HTTPS only, with no redirect, a
  body of at most 1,000,000 bytes, a connect timeout of 20 seconds through Tor
  and 10 directly, and a read timeout of 15 seconds. It goes through Orbot's
  SOCKS proxy, with the name unresolved so that the proxy resolves it, when
  the host ends in `.onion` or the shop's Tor setting is on (CR-87), and
  directly otherwise. A home-network host is allowed, because the certificate
  is pinned and every invoice must be signed by the node key of the settings
  (LN-12, LN-17). The handshake is accepted only when the library accepts
  the leaf certificate (`CertPin::accepts`); the host name is not checked and
  no authority is asked. One worker thread makes the requests the node hands
  out.
- **CR-115.** The credential is set on the counter behind the manager's PIN
  (CR-84 to CR-86): for LND the `lndconnect://` address, scanned or pasted;
  for Core Lightning the address, the rune and the certificate, one field
  each. The setup words say that the counter holds a key limited to
  invoices, that the node must run a current release (LN-04) and that
  running Lightning means watching liquidity, updates and backups (DESIGN
  section 8). The connection is tested as LN-20 says, against the node key of
  the signed settings; if the settings name no node, nothing is tested or
  saved and the words say that the owner has not named the shop's Lightning
  node yet. The credential is saved only after the test passes, sealed like
  the counter's other secrets (CR-98) in the app's `noBackupFilesDir`,
  and removing it deletes it at once. It is never part of the pairing record
  or the settings, and is never in a log or a screen. The screen where it is
  typed is kept out of screenshots, recordings and recent apps
  (`FLAG_SECURE`).
- **CR-116.** The counter's status shows a Lightning line only when a
  credential is saved: "Lightning ready", or why not: the node cannot be
  reached, it refused the key, or the settings name another node now. The
  credential is kept with the node key it was tested against. When the signed
  settings name another node or none, or the saved credential does not read
  back, Lightning is off, the status says so, and it stays off until the
  credential is set up again; a sale then has no Lightning line (CR-113).

- **CR-117.** An invoice of a sale that the node says is settled when the
  sale is over (cancelled, expired or final) or was paid another way (accepted
  at once, by a person, or with enough on its address), or when the sale was
  already paid over Lightning by another invoice, is a late payment, as a
  payment to the sale's address after it ended is (CR-19): recorded on the
  sale, flagged for the owner and shown as "Late payment". Nothing else
  changes. `Counter::lightning_late` writes the record `LateLightning` (kind
  `Payment`, tag 21: the payment hash and the millisatoshis received, which
  are not zero), then the sale, then reports `Report::LatePayment` with the
  whole satoshis received. The sale lists the payment hash where it lists a
  transaction, with at least one satoshi, so that the stored sale does not
  change. The invoice that paid the sale, and one recorded already, change
  nothing. The books list it as a late payment with the hash in place of the
  transaction ID. A write that fails leaves the invoice in the driver, which
  asks again.
- **CR-118.** The driver keeps at most 8 invoices for one sale. A quote past
  that gets no invoice: the sale keeps its on-chain code and shows the line
  of CR-113 that Lightning is not available for this sale, with the reason
  under the details: "this sale has had too many quotes for Lightning". The
  earlier invoices stay watched (CR-112).

## 13. What these rules do not do

- **They do not check payments.** Proof of work, proofs, peers and
  conflicts are the watcher's. A wrong fact handed in gives a wrong
  screen.
- **They do not see a payment drop out of the network.** A light client
  cannot. A payment that never confirms is "stuck" (CR-18); the limits
  of `docs/DESIGN.md` §6 are what protect the shop.
- **They do not refund.** Surplus and cancelled sales with money on them
  are recorded for the owner's refund session (§2.4). A sale paid over
  Lightning is refunded in cash, or on-chain up to what arrived on-chain
  (CR-217), never over Lightning (CR-111).
- **They do not talk to the POS.** CR-30 says what to report; the
  connector reports it.
- **They do not know who a person is.** The list of managers and their
  PINs is not in the signed settings yet. The caller names the person
  and the role.

## 13. Talking to the owner through the Box

These rules join the counter to the owner through a relay, which is
normally the Box (`docs/LINK-RULES.md`, LK-30 to LK-63). The relay is a
hint and adds no authority (LK-09): the owner's key is the root the counter
pinned at pairing, the relay's key is checked by the relay's own hello
(LK-32), and a wrong relay only fails to deliver. The encodings are in
`crates/openbmt-core` (`relay`, `bundle`), the record and the link in
`crates/openbmt-counter-node` (`paired`, `relaylink`, `link`), the calls in
`crates/openbmt-counter-ffi` (`link_step`, `link_state`, `take_relay`) and
`crates/openbmt-owner-ffi` (`counter_bundle`, `counter_relay`). The thread
and the screen line are the app's.

- **CR-130.** The counter bundle may carry a relay: one route and the relay's
  key. The route is a literal address and port (not port 0), or a version 3
  onion name of 56 characters, without `.onion`, and a port. The key is a
  key. The relay is carried as the owner's signed relay of CR-132 (the
  `OBMz` frame, behind a length byte), signed by the owner root. The bundle
  is version 2 when it carries a relay and version 1 when it does not, byte
  for byte as before; version 1 reads and has no relay. A version that is
  not 1 or 2, a route tag that is not defined, a name that is not a name, a
  port that is 0, a relay that is not a whole frame and any byte left over
  are refused. The owner fills the relay from its link's settings and signs
  it when it shows the code; a route that cannot be written as a relay gives
  a code with none. The counter keeps the relay only if the owner root it
  pinned in that same pairing signed it. A bundle whose relay is signed by
  anyone else, or whose relay was changed after it was signed, is refused
  with words (the code from the owner's phone was changed or damaged: make a
  new one on the owner's phone), nothing is kept and the ceremony is used up.
- **CR-131.** The counter keeps the relay with its pairing record. The
  record (`OBMt`) is version 2 when it holds a relay and version 1 when it
  does not; version 1 reads and has none. CR-97 applies to both: strict
  decoding, and every part decoded and encoded again to the same bytes.
  `finish` keeps the relay of the bundle it took.
- **CR-132.** The owner can hand a counter that was paired before the shop
  had a Box its relay by itself: the relay as a frame of its own
  (`OBMz`, version 1), signed by the owner root over the relay. The counter
  takes it only if the signature is by the owner root it pinned at pairing;
  a frame that is not read, or is signed by anyone else, is refused with
  words and changes nothing. The call returns the new record for the app
  to keep in place of the old one, and the link starts from it at the next
  start. The frame has no number and no time: a copy taken again is the
  same record, and an old copy can only point the counter at a relay that
  does not deliver (LK-09).
- **CR-133.** A relay is made only from a key, a route that is read in its
  canonical spelling, and, for an onion route, a name of 56 characters
  from `a` to `z` and `2` to `7`. Anything else is refused where the relay
  is made, with the rule number in the text. The owner's proxy is no part
  of it: a counter uses its own.
- **CR-134.** The counter starts its link, and only when its record holds a
  relay, with the courier of the owner's role table (the owner is the one
  device it talks to) on its own data folder, and the link's default
  intervals. A direct route is reached direct, unless the verified settings
  have Tor on (CR-87), when it is reached through the proxy as an address.
  An onion route is reached through the proxy, whatever the setting says.
  With no proxy where one is needed the link does not start and the
  counter does, and the screen says that Orbot is needed. A second link on
  the same data folder at once is not started (LK-51).
- **CR-135.** One turn of the link is called from one thread of the app
  every 5 seconds, and from the manager's Try again. The relay is called
  with the counter's lock not held: what a Box that does not answer costs is
  a turn of the link, never a sale. What the owner's heartbeat made the
  counter report is held for the next `pump`. With no relay the call is
  refused and says so.
- **CR-136.** The home screen shows a line for the link only when the
  counter has a relay. It says "Connected to the Box" when the last call
  worked. When it did not it says so in words, with the time the failures
  began, and whether the Box refused the counter or could not be reached;
  Try again calls the Box at once. The words follow `docs/SCREEN-RULES.md`.

## 14. Cashiers, the till lock and the day's totals

These rules give the counter a list of cashiers who sign in by name and PIN,
and one screen of the day's totals (`docs/DESIGN.md` §2.2, §2.5, §2.9). The
list, the PINs and the lock are the app's (`apps/counter-android`); the
records and the totals are the library's (`crates/openbmt-counter`,
`crates/openbmt-counter-ffi`), and the owner reads the records with
`crates/openbmt-books` (BK-35 to BK-37).

- **CR-140.** The counter keeps a list of at most 30 cashiers, local to the
  app, in one file of the app's `noBackupFilesDir`. The file is written in the
  order of CR-40 (a new file, a sync, a rename, a sync of the folder) and its
  first line is a version line. It is read whole and refused whole when any
  line is not in the exact form the app writes, when two names are the same
  ignoring case, when a name is the manager's, when a count or a time is not
  in canonical form or when there are more than 30 lines; the app then shows
  its fault screen and nobody is signed in. The file is left out of cloud
  backup and device transfer, like the PIN and the pairing, and the manifest
  and the data extraction rules say so.
- **CR-141.** A cashier has a name and a PIN. A name is 1 to 24 characters
  with no control character and no space at either end, is not the manager's
  name and is not another cashier's, ignoring case. A PIN is 4 to 8 digits. It
  is kept as the manager's PIN is (CR-84): PBKDF2-HMAC-SHA256 with 600,000
  iterations and a random 16-byte salt, written with the entry, never in a
  record, a log or a screen. Each cashier has a count of wrong tries and a hold
  of their own, as CR-85 says, so one cashier's wrong tries never hold
  another's PIN.
- **CR-142.** The manager's PIN opens the screen "Staff". There the manager
  adds a cashier (the manager types the name, and the new cashier types the PIN
  twice), removes a cashier, and sets a new PIN for one (the cashier types it
  twice). A change is written to the file first and then to the library as a
  record (CR-143); a change that the file cannot take changes nothing, and a
  record that the library cannot write is reported with words, and the app
  says the owner was not told. A new PIN for a cashier clears that cashier's
  count and hold, and is not a record.
- **CR-143.** The library writes one record for each cashier added and each
  cashier removed: a record of kind `Pairing`, which no sale, payment,
  refund or override reader takes, with a body of the tag (32 added, 33
  removed), the time (u64 LE), the cashier's name and the manager's name, each
  a length (u8) and the text. The names are labels (CR-61). No PIN, hash, salt
  or count is in a record, and a new PIN is not a record. The decoder is
  strict: a tag that is not one of the two, a name that is not a label and any
  byte left over are refused, and a decoded body encodes to the input. The
  call is `Counter::record_staff`, behind the op `staff_record` of the app's
  call protocol. The record goes through the stored path of CR-40, and on an
  error the chain is unchanged. A record of this kind is no sale event: the
  counter's load skips it, as it does a check-in.
- **CR-144.** The owner's library reads the record strictly, as it reads every
  event, and shows each change in the records and the books with words
  (BK-35 to BK-37).
- **CR-145.** Adding cashiers on the counter is not a loosening
  (`docs/DESIGN.md` §2.9). A cashier is held to the per-sale limit and the
  daily override cap of the signed settings (CR-21), and the shop's exposure
  total (CR-20) bounds every override of every person together, so another
  cashier adds no room. The roles table lists managers, not cashiers, as what
  the owner adds; the owner learns of each cashier from the record (CR-143)
  and not from a signed setting.
- **CR-146.** With no cashier listed the app behaves as it did before this
  section: the first start's cashier name is the actor and nobody signs in.
  The first cashier added turns the till lock on; removing the last turns it
  off. The change takes effect at once, and a person signed in when the lock
  turns off is forgotten. The manager who adds the first cashier is signed in,
  since the manager's PIN has just been checked. With no cashier listed the
  cashier's daily caps are shared by every name (CR-21, CR-69), so a new name
  brings no new cap, and changing the name the counter shows needs the
  manager's PIN. The counter keeps the number of cashiers listed from the
  staff records (CR-143); a counter that had cashiers listed before it kept
  that number starts with none counted, the stricter side, until the next
  change of staff.
- **CR-147.** While cashiers are listed, the first screen after the node
  starts, and the first after a lock, is "Who is at the till?": the cashiers'
  names and the manager's. A name, then that person's PIN, opens home. Home
  shows "At the till: <name>" and a Lock button. Nothing else of the app is
  reached before a person has signed in, and a wrong PIN signs nobody in.
- **CR-148.** Sales, cancels, refunds asked for and withdrawn, and cash refunds
  name the signed-in person as the actor: a sale and a cancel in the record
  (CR-151), a refund as CR-61 says. A cashier's override uses the signed-in
  cashier and asks for no second PIN. The manager option of an override or of
  a refund still needs the manager's PIN (CR-86), whoever is signed in. A
  manager signed in with the PIN acts as the manager in the actions that CR-86
  lets a cashier make, without typing it again; the sign-in is the PIN check.
- **CR-149.** The app locks when the person taps Lock, and when it was out of
  view for more than 5 minutes. It never locks while a sale's screen is open:
  the lock waits until that screen closes and then happens. Who is signed in
  is held in memory only: an app that was stopped starts locked, and a sign-in
  is never written to a file.
- **CR-150.** A PIN that CR-85 holds is not looked at, on the sign-in screen
  as everywhere, and the screen says how long to wait. A sign-in is a PIN
  check of CR-85 for that person, counted and stored before it is compared.
- **CR-151.** The records `Opened` and `Cancelled` carry the person at the till
  who opened or cancelled the sale, when the caller names one: the role (u8)
  and the name, after the fixed fields. A record without them reads as before,
  has none and encodes to the same bytes, so a chain written earlier is read
  and checked as before. A name that is not a label (CR-61) refuses the call
  before anything is written. The calls are `Counter::open_by` and
  `Counter::cancel_by`; `open` and `cancel` name no one.
- **CR-152.** The op `day_totals` counts the counter's own record log between
  two times the app gives, the start of the day (counted) and its end (not),
  from the phone's clock and time zone: local midnight to now. The library has
  no clock and no zone. It reads the records from the newest back, 256 at a
  time, until a block holds events and none is from the start on, and then
  folds what it read, oldest first. A day that needs more than 20,000 records
  is refused with words, and never counted in part. A day that ends before it
  starts is refused. The lines per person are in the answer only when the call
  asks for them.
- **CR-153.** What a day counts. A sale counts on the day it was opened, with
  its state as of the newest record. Paid: the records show it accepted at
  once, confirmed, final, accepted by a person or paid over Lightning (CR-30),
  and not replaced or reopened since; with its amount in the shop's currency
  as it was when the sale opened, its satoshis (those received, or at risk for
  one accepted at once or by a person, or the Lightning amount rounded down)
  and a split into on the chain and over Lightning. Waiting: opened, not paid,
  not ended and not stopped, with the satoshis it asks for at its last quote;
  a payment that has arrived and was not yet accepted or confirmed is in no
  record, so such a sale is counted as waiting. Stopped: replaced or reopened
  and not paid since, which is not a wait (CR-12). Ended: cancelled or
  expired. Overrides, refunds asked for (not withdrawn ones; withdrawn ones are
  counted apart) and cash refunds count on the day of their own record, with
  the satoshis put at risk or asked for and, for a cash refund, its amount.
  Sales in two currencies are refused, as are sums that do not fit, with
  words.
- **CR-154.** A cashier sees the totals of CR-153 without a PIN. The lines per
  person (sales opened, overrides, refunds asked for and cash refunds, by the
  role and the name the record carries; sales whose record names no one are
  counted apart) are asked for from the library, and shown, only after the
  manager's PIN. At most 64 persons are named; the others are counted as left
  out.
- **CR-155.** The screen "Today" says what each total is, says why a sale
  waits and what happens next (`docs/DESIGN.md` §2.6), shows no wallet balance,
  and says that the owner's phone has the full books and the export. Its words
  follow `docs/SCREEN-RULES.md`.
- **CR-156.** These rules do not make the list of cashiers an authority. A PIN
  on a phone stops a casual hand and nobody else: it does not stop a person who
  holds the unlocked phone and the manager's PIN, or who takes the phone's
  storage apart. The list is not in the signed settings, is not backed up and
  is not on the owner's phone; a phone that is lost or reset loses it, and
  the owner's records still name every cashier who was added or removed.
- **CR-177.** The screen comes back. The counter keeps which screen it shows
  (home, the list of sales, a sale's screens, or another) as a small value,
  and holds no picture of a screen. When the app leaves the screen and comes
  back, that screen is drawn again from the library's state at that moment:
  a sale shows its state as it is now ("Paid", "Do not hand over"), which may
  have changed while the app was away; another screen gives way to home. At
  a new start of the app (after the process was ended) the counter asks the
  library for its sales and opens on the screen of the one sale that is not
  yet ended, on the list of sales when more than one is not ended, and on home
  when none is. A sale is ended when it is expired, cancelled, paid by
  Lightning or paid at depth (final). The till's lock (CR-147) still comes
  first: a locked till shows "Who is at the till?", and the screen follows the
  sign-in.
- **CR-178.** A change of light and dark. A change between light and dark
  while the app runs does not finish and start the activity. The activity
  reads the palette of the new mode, sets the window colour, the bar colours
  and the status-bar icons again, asks for the insets again, and asks the
  screens to be drawn again in place. A page's padding for the bars is worked
  out whenever its views are built, so no gap shows above its band. The
  counter draws its current screen again from state as CR-177 says (a screen
  that is not a sale, the list or home gives way to home), and a locked till
  draws "Who is at the till?" again.

## 15. Taking the owner's settings after pairing

These rules join the counter to the settings the owner signs after pairing
(`docs/DESIGN.md` §2.9). They arrive through the link as messages of kind
Settings (`docs/LINK-RULES.md`, LK-64 to LK-66). The checks and the file are
in `crates/openbmt-counter-node` (`settings`, `node`, `link`), the record in
`crates/openbmt-counter`, and the start in `crates/openbmt-counter-ffi`.

- **CR-160.** A message of kind Settings is read as the `OBMS` frame of
  `openbmt_core::settings` (strict: exact length, at most 64 KiB, and the
  bytes encode again to the same bytes) and checked with
  `SignedSettings::verify` against the two pinned roots and the settings in
  force: the owner's signature, valid values, a version above the one in
  force, and the Key app's signature when the change loosens. A version that
  is not above the one in force is dropped without a refusal: the owner
  sends again until it sees the counter's record (CR-162). Any other failure
  changes nothing and gives a note with the rule and words. Once taken, every
  limit, depth, tolerance and cap is read from the new settings by the next
  call that needs it. A sale already open keeps the underpayment tolerance
  it was opened with, the one value a sale stores; the depths and caps are
  read again each time the sale is judged or a person acts on it.
- **CR-161.** The newest settings taken are kept in one file of the node's
  folder, `settings.bin`: the `OBMS` frame and nothing else, written whole
  (temporary file, flush, rename, flush the folder, as CR-40) before the
  settings are used. If the write fails the settings are refused and nothing
  changes.
- **CR-162.** When settings are in force the counter writes a `Setting`
  record: the version (4 bytes little endian), 0 for a tightening or 1 for a
  loosening, and the 32-byte hash of the settings (37 bytes). The owner's
  copy of the counter's chain shows which version the counter holds by it. A
  record that cannot be written is noted and tried again at each turn of the
  link and of `pump`. A counter that has a link writes one at its first start
  for the settings of its pairing, marked loosening, and again at any start
  where the settings in force have none. A record body is never read as a
  sale event.
- **CR-163.** At start the node reads the settings of the pairing record and
  the settings file and uses the newer. The file must be signed by the owner
  root, and by the Key app root if it carries that signature, be valid and
  have a version above the record's; one that does not read, or does not
  check, is a fault and the counter does not start. It does not fall back to
  the record, whose settings may be looser. A file that is not above the
  record's is left as it is, noted, and the record's settings are used. The
  start also refuses settings of a version below the highest one a `Setting`
  record of the counter's own chain names (an old copy of the file put back,
  or the file gone).
- **CR-164.** Settings that change what the node was started with, the Tor
  switch and the Lightning node, take effect when the node starts again. The
  link's state says a restart is needed, and Home says so in words. The app
  starts the node again at the next moment no sale screen is open, and CR-87
  then applies as at any start.
- **CR-165.** Settings that do not list this counter, on the device list and
  among the counters, are taken and kept. No new sale is opened: the refusal
  says the counter was removed from the shop by the owner and to ask the
  owner. Sales already open are judged and settled as before, and the
  records still go to the owner. Home says so in words.
- **CR-166.** A message of kind Device list is noted and dropped. The device
  list travels inside the settings, and nothing sends it separately.

## 16. Telling the owner that a batch is held

The owner sends each batch of addresses again until the counter's own
records show that the counter holds it (LK-70 to LK-73). These rules give the
counter the record that shows it.

- **CR-167.** When the counter takes a signed batch of addresses from the
  owner (`Counter::take_batch`, which the node's `add_batch` calls), it
  writes one record that says so. The record is of kind `Setting`, and its
  body is one tag byte (34), the first index of the batch (u32 LE), the first
  index after it (u32 LE) and a 32-byte digest, the SHA-256 of the
  text `OpenBMT/batch-held/v1` and the signed batch's wire bytes, so that it
  covers the range and both signatures. The order is that of CR-40: the batch is
  checked first, as a copy of the batches would take it and with nothing
  stored; the record is then written; the batches are stored last. Only a
  batch that would be taken is recorded. The first batch of a pairing, which
  the node adds at its start, is not taken this way and has no record.
- **CR-168.** A batch the counter already holds, with the same range and the
  same digest, is taken as nothing: no change, no second record and no
  refusal. If the record exists but the batches do not hold the range (the
  store failed after the record), the batch is stored again and no second
  record is written. A batch that overlaps a held range and is not the same
  is refused as before (`OverlapsExisting`). The digests are read from the
  records when the counter starts, so a restart changes none of this.
- **CR-169.** The body of CR-167 is read strictly: a tag other than 34, a cut
  body, a byte left over and an end not above the start are refused, and
  re-encoding a decoded body gives the input. The books, the totals and the
  readers of sale events take no meaning from a record of this body. A
  `Setting` record of CR-163 has a body of 37 bytes and this one has 41, and
  both are read strictly, so a body is read as at most one of the two.

## 17. The watch key

The owner phone can keep following the chain and telling the counters so
while its owner app is closed (`docs/OWNER-APP-RULES.md`, OA-80 to OA-89).
That watcher holds no key of the owner: not the owner root, not Key 1. It
signs with a key of its own that the signed settings name.

- **CR-185.** The settings carry an optional watch key: 0, or 1 and then 32
  bytes, after the Lightning node. It is a public key of its own, never on the
  device list and never a counter or a Box (`validate` refuses it), and no
  wallet or identity key is derived from it or into it. Naming it or changing
  it loosens (instant sales go on on the word of a key that is not the owner
  root), so it needs the Key app's signature like any loosening; removing it
  tightens. It reaches the counters in the settings, by the link (LK-64 to
  LK-66), and no counter is paired again for it.
- **CR-186.** A heartbeat is taken if it is signed by the owner root, or by
  the watch key of the settings in force under the tag
  `OpenBMT/watch-heartbeat/v1`, which no owner-root signature uses. The frame
  is the same, and the counter tries the root and then the key. The checks of
  CR-50 to CR-54 are unchanged, and the numbers are one line: a heartbeat of
  either signer uses up numbers for both. With no key named, or after the
  settings remove it, a heartbeat signed by it is refused and changes
  nothing. A watch-key signature on anything but a heartbeat is not read: the
  link takes the watch key's messages only as heartbeats (LK-74).

## 18. Tips at the counter

The owner can have the counters ask the customer for a tip. The tip goes to
the shop's wallet with the sale, in the same payment. Paying it on to staff
is the owner's business, as with card tips. A tip straight to a staff
member's own wallet is not part of these rules.

- **CR-187.** The settings carry `tips_on` and up to three suggested tips, in
  whole percent from 1 to 50 (`validate` refuses anything else). At first tips
  are off and the percents are 10, 15 and 20. They are written in the
  extension block that follows the watch key, after the shop's currency byte
  (CR-193), as a flag, a count and the percents. The block is written, in a
  message of version 2, only when tips are on, or the percents are not 10, 15
  and 20, or the currency is not USD: settings with tips off, the first
  percents and USD are the bytes every earlier build wrote (message version 1,
  the same hash), and read as tips off. A message of version 2 whose block
  holds USD, tips off and the first percents is refused (it would have been
  written as version 1). Neither turning tips on or off nor
  changing the percents changes what a counter accepts or how much is at
  risk: the tip is part of the sale's total and every check is made on the
  total (CR-189). Both directions are tightenings, so the owner phone signs
  alone. A change that also loosens something is loosening as ever. The
  counter offers the percents, in the order the settings give them, when
  `tips_on` is set (`Counter::tip_choices`), and none when it is not. It
  does not refuse a tip it is given when tips are off: asking is the
  caller's to honour.
- **CR-188.** A suggested tip is its percent of the sale's amount, rounded
  half up to a whole minor unit of the shop's currency:
  `floor((amount * percent + 50) / 100)`. The customer chooses one of the
  suggested tips, types an amount, or chooses no tip, before the code shows;
  the screen is "Add a tip?" with a button for each suggested tip, "Other
  amount" and "No tip". The code is for the amount and the tip together.
  The counter app shows that screen after the keypad and before the code,
  only when the library offers tips: one large button for each suggested
  tip, written "{percent}% · {tip}", then "Other amount" and "No tip", in
  big type and with nothing else on it, since it faces the customer. "Other
  amount" opens the keypad for the tip, and a tip above the sale shows the
  words of CR-190 on that keypad, which stays. A tip of nothing is no tip.
- **CR-189.** `Counter::open_with_tip` opens a sale with the amount of the
  sale itself and the tip. The sale is for the total: the quote, the
  satoshis it needs, the underpayment tolerance, the valuation of what
  arrives, the instant limits, the exposure and the refund ceiling (CR-67)
  are all made against the amount and the tip together, and the smallest
  bitcoin sale, the dust line and the largest amount (CR-75, CR-17, CR-05)
  are judged on the total. A sale with no tip is the sale of every earlier
  build. The record `Opened` carries the two as separate fields and the
  person at the till (CR-151): a sale with a tip is written with the tag 20,
  which holds the tip (never 0) after the rate source and before the person,
  and a sale without one is written with the tag 1, byte for byte as before;
  the tag 1 reads as a tip of 0 and the tag 20 with a tip of 0 is refused.
  The stored sale (`OBML`) is version 4 when it has a tip. The report
  `BooksFinal` gives the sale's own amount, as the POS and the books know
  it.
- **CR-190.** A tip above the amount of the sale itself is refused
  (`OpenRefused::TipTooLarge`), with nothing written. The words are "Tip not
  accepted": the tip is more than the sale itself, so it was not added; type a
  smaller tip, or choose No tip.
- **CR-191.** The day's totals (CR-153) have a line for the tips: how many of
  the paid sales carried one and their sum, in the shop's currency. A sale
  counts on the day it opened and is a paid sale as CR-153 says, so a sale
  that was replaced or is waiting adds no tip. The tallies of CR-153 hold the
  sales' own amounts, without the tips. The lines per person (CR-154) have
  the tips of the paid sales that person opened. The screen says "Tips" and,
  for each person, "Tips taken by {name}".
- **CR-192.** A refund may ask for up to what was received, which is the
  amount and the tip (CR-67). The view of a refund (CR-72) says how much of
  it is tip: the refunds of the sale are counted oldest first against the
  sale's own amount, and what goes past it, up to the tip, is tip. A
  withdrawn request counts for nothing. A sale with no tip says 0.

## 19. The shop's currency

A shop prices in one currency, held in its signed settings, chosen from a
fixed list: US dollar (USD), euro (EUR), pound sterling (GBP), Japanese yen
(JPY), Australian dollar (AUD), Indian rupee (INR), South Korean won (KRW),
Indonesian rupiah (IDR), Singapore dollar (SGD), Canadian dollar (CAD),
Mexican peso (MXN), Brazilian real (BRL), Turkish lira (TRY), Thai baht
(THB), South African rand (ZAR), or Bitcoin Standard, which
prices in satoshis and uses no price source at all. Every counter of the
shop prices in it and every limit of the settings is in it. A counter in
another currency than the shop's is not part of these rules. The sources of
a price and the band of each currency are in `docs/RATE-RULES.md` (RT-01,
RT-03, RT-24).

- **CR-193.** The settings carry the shop's currency: one byte, 1 for USD, 2 to
  9 for EUR to SGD in the order above, 10 for Bitcoin Standard (`SAT` is the
  code a sale records for it) and 11 to 16 for CAD to ZAR in the order above.
  The currencies added after Bitcoin Standard take the bytes after it, so the
  byte of every earlier currency is unchanged. It is the first part of the
  extension block that follows the watch key, and the tips block of CR-187
  follows it. The block is written, in a message of version 2, only when the
  currency is not USD or the tips are not off with the first percents: settings
  of a dollar shop with those tips are written, hashed and signed exactly as
  before, in the wire form of version 1. A version 1 form reads as USD with tips
  off. A version 2 form whose currency byte is 0 or above 16, one whose block
  holds USD, tips off and the first percents (it would have been written as
  version 1), one whose block is cut short or has bytes left over, and a form of
  any other version are refused. The block is in the bytes that are signed.
- **CR-194.** Every money value of the settings (the three instant limits
  and their budgets, the refund limits, the cashier's override cap and the
  manager's ceiling) is in whole units of the shop's currency, next to the
  satoshis that counters enforce. For Bitcoin Standard a unit is a satoshi,
  so a limit's satoshis and its whole units are the same number, and
  `validate` refuses settings in which they are not.
- **CR-195.** A change of currency loosens, whatever else it does (every
  limit becomes another number, which cannot be sorted), so it needs the
  Key app's signature. The same change must give every money value again in
  the new currency: a limit that is on and has the same satoshis and the
  same whole units as in the settings in force is a value carried over from
  the old currency, and settings that carry one are refused when they
  replace those in force, naming it. A limit that is off is not a value.
  The field `currency` is listed by `changed_fields`.
- **CR-196.** A sale is in the currency of the settings in force: the sale
  and its `Opened` record carry that currency's code, whatever currency the
  caller names. The amount is in the minor units of that currency (ISO
  4217: yen and won have none), and the rate is in minor units per bitcoin.
  The counter app's library refuses a sale in another currency than the
  settings' (`sale-currency`). The app takes the currency from the settings
  in force, not from its price sources: its keypad has the currency's
  minor digits, its words name the currency by its code, its price is the
  one of the sources of that currency (RT-40), and when the settings in
  force name another currency than the one it started with, it starts the
  node again.
- **CR-197.** With Bitcoin Standard the amount typed is satoshis and the
  quote is the amount: the rate is exactly 100,000,000 minor units (of a
  satoshi) per bitcoin, no source is asked, and the rate and source the
  caller gives are not read. The sale records the source `sats`, at the time
  of the sale, as CR-57's label, and no price. A new quote (CR-05) and the
  amount of a refund given in the shop's currency (CR-61) are made the same
  way. The dust line (CR-17) and the smallest bitcoin sale (CR-75) apply to
  the satoshis as before. The counter app's keypad then takes whole satoshis
  with no point, its amount line and its totals say the sats once, it shows
  no price line and asks no price source.
- **CR-198.** The owner phone changes the currency in the editor of the
  settings (AS-74 to AS-77) by an edit that names it. The edit must give
  every limit again, and each is typed in the new currency: a limit left as
  it is, or one that cannot be told from the old one, is refused with CR-195
  in the words of the refusal. The satoshis of a limit are its whole units
  at the price of the new currency the edit gives (AS-75), refused outside
  that currency's band; for Bitcoin Standard no price is given or asked and
  the satoshis are the units. The preview says the currency, and that the
  change needs the Key app.
- **CR-199.** The Key app's line for a settings item lists the currency as a
  field, named `Currency`, with the choice written in full (`US dollar (USD)`),
  and each limit's value in the currency's name as its whole units (`$10`
  for a dollar shop, `10 euros`, `10 sats`), so that the key holder sees in
  what the limits are before co-signing.
- **CR-200.** The steps that check a PIN (CR-85, CR-150) run only while the
  app is shown. The check runs on a thread of its own and its answer comes
  back to the screen thread later; when the app was hidden in between, the
  answer is dropped: nothing is unlocked, the action that waited for the PIN
  does not run, no sign-in is made and no wrong-PIN words are shown. The try
  was counted before it was compared, as CR-85 says, so closing the app
  does not clear it; the person types the PIN again when the app is shown.
  The app keeps a flag that is set when it is shown and cleared when it is
  hidden, and no answer is acted on while it is clear.

## 20. The built-in Tor

- **CR-201.** The counter app runs the Tor Project's C Tor itself, as the
  owner app does (OA-97): the runner, the service and the strict reader of
  Tor's control socket are the same code, in `apps/android-common/tor/`,
  which only the owner and counter builds compile and package. The Key app
  makes no network connection and has none of it, nor `libtor.so`. Tor runs
  only when the signed settings in force have Tor on (CR-87, CR-163) and the
  choice is "Built-in Tor" (CR-203). With Tor off in the settings, or with
  Orbot chosen, no Tor of the app's runs, and one that was running is let go
  of when the node starts again. Tor has one SOCKS listener on 127.0.0.1 at a
  port Tor picks; when Tor reports its first circuit the app gives that
  address, `127.0.0.1:port`, to the node, the price fetches and the
  Lightning transport, where Orbot's address was. The app holds one lease on
  the run while it is shown and lets go of it when it is hidden (CR-88); the
  last lease let go of shuts Tor down, and a start that comes after the app
  was hidden does not take one. The link's proxy (CR-134) is not changed.
- **CR-202.** Tor's data and cache folders are inside the app's
  `noBackupFilesDir`, in the folder `tor`, made readable, writable and
  searchable by the app alone; nothing of Tor is backed up (CR-140). The
  command line has the options OA-98 lists and no others, no `torrc` is
  written, and a control socket left by an earlier run is deleted before Tor
  starts. `libtor.so` is taken from the AAR pinned in `docs/DEPENDENCIES.md`
  and nothing else of it is, by the script both builds call
  (`apps/android-common/tor/tor-library.sh`), which checks the AAR's SHA-256
  first and stops the build if it differs.
- **CR-203.** Settings has a row "Tor", shown only while the shop's settings
  in force have Tor on. It opens a page with the words of the choice in use
  and two choices, "Built-in Tor" and "Use Orbot", and no third: there is no
  way to turn Tor off in the app. "Built-in Tor" is chosen with the manager's
  PIN. "Use Orbot" goes on to the screen of Orbot's address (CR-87) and is
  chosen by saving it behind the PIN. The choice is kept in the config file
  as a last line, `tor: builtin` or `tor: orbot`; the address of Orbot is kept
  for either choice. A new counter is saved with "Built-in Tor". A config
  file without the line, from before the choice, is read as "Use Orbot" at
  the address it has, and stays so until the choice is changed. A choice is
  saved first, and then the app starts again with it. "Built-in Tor" says
  "Tor", "This app runs Tor itself, so Orbot is not needed." and "Nothing to
  do. To use Orbot instead, choose Use Orbot." "Use Orbot" says "Tor through
  Orbot", "This app sends everything through Orbot, which must be installed
  and running." and "Keep Orbot running. To stop needing it, choose Built-in
  Tor."
- **CR-204.** With the built-in Tor the screen says "Starting Tor", "This app
  is connecting to the Tor network, which can take up to a minute." and "Wait
  on this screen." while the app waits, for up to 90 seconds, for Tor's first
  circuit. If Tor stops, does not load, does not connect in time, or does not
  answer, nothing is sent: the node is not started, no price is fetched, and
  the screen says "Tor did not start", "This app could not connect to the Tor
  network." and "Check that the phone is online, then try again. If it keeps
  failing, choose Use Orbot.", with a button "Try again" that starts again,
  with a new run if the old one ended, and a button "Tor" that opens the
  choice (CR-203).

## 21. Requests to send, and a note on a sale

A shop can take a deposit days ahead from a customer who is not at the till,
and match it with the balance later. The cashier makes the sale a request:
the same sale, the same address and the same checks, with a longer life. A
note names it. Nothing is sent by the counter: the cashier shares the code
by any means the phone has.

- **CR-205.** A request to send is a sale (`Counter::open_with_extras`) that
  lasts 1, 3 or 7 days, the cashier's choice, in place of CR-07's 15
  minutes. It has one quote, made when the request is made, and the quote
  holds for the whole life of the request instead of the 5 minutes of CR-05:
  the satoshis are fixed when the request is made, and a move in the price
  before the payment is the shop's. It needs the same things any sale needs
  to open (CR-01 to CR-05, CR-75, CR-17): the rate is fresh, and the
  satoshis are above the shop's smallest bitcoin sale.
- **CR-206.** Any sale may carry a note: plain text of 1 to 64 characters,
  with no control character (the check of a person's name, CR-61). It is set
  when the sale opens (`SaleExtras`) or added once later
  (`Counter::add_note`); a sale that has a note takes no second one. A note
  that is not plain text of 1 to 64 characters is refused with nothing
  written: it says "Note not accepted", "A note must be plain text of 1 to 64
  characters, so it was not saved." and "Type it again, shorter." for the
  cashier. The note is written as a record of its own, a `Sale` record with
  tag 24 (the time, then a length byte and the text), after the record of the
  opening for a sale opened with one, and in the stored sale (CR-211).
- **CR-207.** A request takes no new quote: `Counter::requote` refuses it,
  and the one quote stays in force until the request ends.
- **CR-208.** A request is never accepted at once and never paid over
  Lightning. It is paid when what arrived is at the confirmation depth of the
  settings, as a sale above the instant limits is: until then it waits, with
  nothing counted in the exposure ledger. The shop's node is not asked for an
  invoice for it. Part paid, overpaid, a payment after the request ended, an
  override and a refund are judged as for any sale (CR-12, CR-16, CR-19,
  CR-61 to CR-74).
- **CR-209.** A request ends as a sale does. With nothing received when its
  days run out it expires, with the words of an expired sale (CR-07), and a
  request that has received something does not expire. The cashier can cancel
  it from its details like a sale (CR-07). A payment that arrives after it
  ended is a late payment (CR-19).
- **CR-210.** The counter sees a payment whenever it runs, and a request is
  stored with the other sales: a request made before the counter was closed
  and paid while it was closed shows Paid when the counter starts again and
  has caught up with the chain, by the same steps as any sale (CR-42, CR-163).
  No other mechanism looks for it.
- **CR-211.** The stored sale (`OBML`) is version 5 when the sale is a
  request or has a note, and holds at least one of the two: the fields of
  version 1, the tip (0 for none), the Lightning payment (a flag, and the
  fields of version 3 when it is 1), the refund entries, then the request
  (a byte: 0, or 1, 3 or 7 days) and the note (a flag; then a length byte and
  the text). A request has one quote, which lasts the request's days, and has
  no Lightning payment. A sale with neither is written as before, so every
  earlier file reads back, as a sale with no note and no request, and every
  earlier build's file is unchanged. The read is strict, as CR-42 says.
- **CR-212.** The note is the owner's and the books': it is carried on the
  sale's line in the books' full ledger (BK-41), and goes to no other device
  than the owner's. The POS reader is told of a sale as before and is never
  told its note: the record of a note changes nothing it is sent.
- **CR-213.** On New sale, under the keypad's main button, the counter app has
  a quieter button "Request to send". It opens a page with the note field,
  which says "Optional. Like a booking name or date.", and the choice of
  lifetime as three chips, "1 day", "3 days" and "7 days", and a button "Make
  the request". The request is made with the amount typed, the tip step is
  not asked, and the app goes on to the page of the request (CR-214). A note
  the library refuses shows the words of CR-206 on the page and makes no
  request.
- **CR-214.** The page of a request shows the code, the amount and the words
  below it: "Request ready", "The customer can pay this request from anywhere
  until {date}. The amount in bitcoin is fixed at {sats}." and "Share the
  link or the code with the customer. The counter shows Paid once the payment
  is confirmed.", for the cashier. It has two buttons. "Share the link" gives
  Android's share sheet the payment link: the payment request URI the sale
  already shows, with its amount and label (CR-83). "Share the code" gives it
  the code as a PNG image. Nothing is sent by the app itself. Each image
  is made in the app's own folder under a fresh random name and shared at
  its own address. When the page is left, when another image is shared and
  when the app starts, every address given out is revoked and every image
  deleted. Back goes Home. The page of a request that
  is open again, from the list of CR-215, says "Waiting for payment", "This
  request is open until {date}. It is paid once the payment is confirmed." and
  "Nothing to do.", for nobody; one that expired says what an expired sale
  says.
- **CR-215.** Home lists the open requests under the heading "Open requests",
  each with its note, or its amount when it has none, the newest first. A
  row opens the page of CR-214. A request that is paid or has ended is not in
  the list.
- **CR-216.** A sale's details show its note under "Note", or a button "Add a
  note" when it has none, which asks for the note as CR-213 does and adds it
  once (CR-206).

## 22. Lightning through a swap provider

The shop can take Lightning with no node of its own: a swap provider it
picks makes the invoice the customer pays and locks its own bitcoin in an
output that only the counter can claim, to the sale's own address
(`docs/SWAP-RULES.md`, `docs/DESIGN.md` section 8). The provider never holds
the shop's money. The library of the swap is `crates/openbmt-swap`; the
driver that follows it is in `crates/openbmt-counter-node` (`swap`); the
judgment of a paid sale is in `crates/openbmt-counter`; the transport and the
screens are the app's. It has no network, no clock and no randomness of its
own, as the driver of section 12 has none.

- **CR-218.** The signed settings name the provider, or none
  (`Settings::swap`): a name of up to 32 bytes, its Tor v3 onion name, the
  path prefix of its API and the most the shop takes over Lightning for one
  sale, in satoshis (at most 25,000,000, the library's own cap, SW-02). It is
  off by default. Settings that name a Lightning node and a provider are
  refused: one Lightning path at a time. The settings are version 3 when they
  name a provider. Naming, changing or removing the provider is not a
  loosening, because the claim pays the shop's own address and no money can
  be sent anywhere new, so the owner phone's signature alone suffices. A
  change takes effect when the counter starts again, as CR-164 says for the
  node.
- **CR-219.** A sale that opens with a provider set, and a quote of it that
  is within the shop's maximum and the provider's live limits, gets a swap:
  the driver reads the provider's terms if they are older than SW-63 allows,
  then asks for a reverse swap for the quote's satoshis with a fresh preimage
  and claim key, made from the operating system's randomness, and the sale's
  own receive address as the claim address. A new quote asks again, and
  every earlier swap of the sale stays followed (as CR-112 keeps its
  invoices), at most 8 for a sale. The sale shows its on-chain code at once
  and never waits. When a reply has passed every check of SW-20 to SW-28 the
  code becomes the combined code of the own-node path (LN-18), carrying the
  invoice. If none has passed 20 seconds after the quote (the request
  crosses Tor), or the amount is outside the limits, the code stays
  on-chain and one small line says "Lightning is not available for this
  sale." A provider that cannot be reached, or that refuses, is "Lightning off for now" in the sale's
  details, and never a state of the sale.
- **CR-220.** One swap is followed by the machine of SW-71, one per swap,
  and each is saved through the store (a slot of its own, `swap-N-M.bin`) in
  the counter's folder before its invoice is shown, so that a restart can
  never lose a lockup it could claim. The secrets are made in memory only,
  and the file is the framed encoding of the swap library with the counter's
  own record of the lockup and the claims (magic `OBM(`, version 1). The
  status is asked for as the machine says (SW-71). A lockup is taken from the
  status bytes or from the counter's own watch of the lockup address, a sighting
  of a peer or a block, and is checked by SW-30 before it is believed; the
  counter's own watch alone tells whether it is in a block (SW-72).
- **CR-221.** When the lockup is in the mempool and the sale's quote is
  within the instant limits of the check that saw it (the same policy as
  CR-13: the heartbeat, the conflict watch, the groups of peers, the slice)
  and the exposure ledger has room, the sale is paid at once over Lightning
  (`Counter::swap_paid`): the counter claims at once, the amount at risk is
  counted in the ledger as for an instant on-chain sale and leaves it when
  the lockup is in a block (CR-15). Otherwise the sale waits for the lockup's
  first confirmation, shows "Waiting for one block", then claims and is paid.
  The sale keeps the payment hash and what the claim pays its address, as
  CR-110 keeps what the node received, and is final and told to the POS and
  the books as in CR-109. The stored sale is version 6 when it holds a swap
  payment. A sale whose lockup is on the way does not expire. What the
  claim pays the sale's address is the sale's own payment and never a late
  one (CR-19). A sale paid before its lockup was in a block whose lockup is
  then replaced, or whose swap fails before the claim is in a block, is a
  cancel (CR-14): `Counter::swap_cancelled`.
- **CR-222.** The claim pays the sale's address at a fee rate that starts
  at 2 sat/vB or at the recent rate of the blocks the counter has checked
  (FE-13, from the fee samples it makes of them and keeps in memory only),
  whichever is higher, and at 2 sat/vB when the higher rate would be refused
  by SW-41 or no sample has been made. It signals replacement (SW-40) and is
  built again at a higher fee (SW-46) every block it stays out of a block:
  50% more, and at least 2 sat/vB more. It is never built at or after the
  provider's timeout (SW-44), and never above the limit of SW-41. A claim
  still unconfirmed `MIN_CLAIM_BLOCKS / 2` blocks before the timeout is
  told to the owner (words "Lightning claim waiting"). The swap is done
  when the shop's own claim, or an earlier one it built, is in a block
  (SW-72, SW-75).
- **CR-223.** A swap that fails before the claim (the invoice ran out, the
  provider could not lock, the lockup is refused by SW-30, the provider took
  it back, the timeout) leaves the sale unpaid and shows "Lightning payment
  unfinished". The sale itself goes on: the same code takes bitcoin
  on-chain, and the cashier may take another payment.
- **CR-224.** A bitcoin refund of a sale paid through a swap is open as
  for any sale that arrived on-chain: the claim is on the chain, to the
  shop's own address. It is limited, as CR-67 says, to what the sale
  received, which is what the claim pays; a sale paid through the shop's own
  node stays cash only (CR-217).
- **CR-225.** The app reaches the provider over plain HTTP/1.1 (the codec of
  SW-80) to its onion name, through the counter's own Tor, as it reaches
  the other onion services, and never around it: with no Tor, nothing is
  sent. A request is sent at a time. The reply is at most 64 KiB and is
  given to the driver whole; a connect or a read that does not end in 60
  seconds fails. The driver gives the app the bytes to send and takes the
  bytes that came back.
- **CR-226.** Every state and refusal of this section has words under
  `docs/SCREEN-RULES.md`. The cashier is told: "Waiting for one block",
  "Lightning payment unfinished" and "Lightning off for now", and sees the
  line of CR-219. The owner is told "Lightning claim waiting" and has the
  settings row "Lightning through a swap provider" (CR-218): a provider, an
  onion address, the largest sale over Lightning, and "Turn on" and "Turn
  off".

## 23. Pairing with the shop's own wallet

A shop that uses its own wallet has no Key app. The owner phone shows the
offer, carries the request and shows the bundle; the counter pins the
wallet's description and makes its own receive addresses from it
(`docs/WALLET-RULES.md` §3). What replaces the Key app's second signature is
the person at the counter, who compares the counter's first address with
the wallet's own screen and the four words with the owner phone. The codecs
and the checks are in `crates/openbmt-core` (`pairing_own`) and
`crates/openbmt-counter-node` (`paired`); the address book is in
`crates/openbmt-counter`; the screens are the app's. A counter is paired one
way or the other for its whole life: the rules of section 10 and of batches
apply to a counter paired with the Key app, and the rules of this section to
a counter paired with the shop's own wallet.

- **CR-227.** The owner phone's offer is the frame `OBM-`, version 1: the
  network in one byte, the owner root, the settings floor (the settings
  version in force on the owner phone when it made the offer, u32 little
  endian) and the pinned wallet (`OBM)`, WL-21) whole. It names no Key app.
  It is decoded strictly (exact length, no trailing bytes, every part
  decoded and re-encoded to the same bytes) and the pinned wallet must pass
  WL-21 and WL-22. The network must be the main network, and the one the
  description is for.
- **CR-228.** When the offer reads, the counter derives the wallet's first
  receive address (receive chain, index 0, WL-20) and shows it, with the
  kind of wallet, before anything else. The person compares it with the
  first receive address on the wallet's own screen. "Same address" goes on
  to the request; "Different" ends the ceremony and nothing is kept. The
  counter makes no key and no number before "Same address".
- **CR-229.** After "Same address" the counter makes its identity key and a
  fresh 32-byte number as CR-91 says, and shows its request, the frame
  `OBM.`, version 1, as a code with four words: the counter's identity, its
  name, the number, a binding to the offer and the counter's signature over
  them. The binding and the four words are made from the whole offer under
  their own tags (`OpenBMT/pair-own-binding/v1`, `OpenBMT/pair-own-session/v1`),
  so a request or words made for a Key app offer never match an own-wallet
  offer, nor the reverse. The owner phone shows the same four words. "They
  match" and "They differ" and the ceremony's life in memory are as CR-91
  and CR-92 say, and its 10 minutes run from the offer.
- **CR-230.** The owner phone's bundle is the frame `OBM/`, version 1: the
  pairing entry (the hash of the pinned wallet, the counter, its name, the
  number, the settings version) signed by the owner root under the tag
  `OpenBMT/pairing-own/v1`, the signed settings (`OBMS`), the first signed
  range (`OBM*`, WL-24), the peers (CR-99) and, if any, the relay (CR-131).
  It is taken only after "They match" and within the 10 minutes. The checks
  run in this order: the match, the bundle's encoding, the pairing (this
  counter, this number, the hash of the offer's pinned wallet, the owner
  root's signature), the settings (CR-231), the range (CR-232). A bundle
  that is not a bundle leaves the ceremony as it was; any other refusal ends
  it.
- **CR-231.** The settings in the bundle need the owner root's signature
  only, a version above the offer's settings floor, valid values, and this
  counter on the device list and among the counters (as CR-95). They are the
  counter's first settings and not a change from earlier ones, so they need
  no counter confirmation (`docs/APPROVAL-RULES.md`): the person at this
  counter has just compared the address and the words.
- **CR-232.** The first range must be signed by the owner root, name this
  counter and have a valid shape (WL-23, WL-24, WL-28), and hold at least
  `BATCH_MIN_COUNT` indexes (1000). Nothing in the bundle or the record can
  lower that number.
- **CR-233.** What the counter keeps after pairing is one framed record,
  `OBM:`, version 1: the identity secret, the owner root, the pinned wallet,
  the network, the settings floor, the signed pairing, the signed settings,
  the first signed range, the peers and the relay if any. It is decoded and
  sealed as CR-97 says, and `start` runs CR-230 to CR-232 again on it, the
  match and the time aside, before anything starts.
- **CR-234.** A sale's address is the pinned wallet's receive address at the
  next index of the counter's range book (WL-20, WL-27). The book is kept in
  its own slot (`ranges.bin`, frame `OBM;`, version 1: every signed range
  the counter accepted, in the order it accepted them, and the next index of
  each), written whole before the address is shown, as CR-03 says. At start
  every range in the slot is checked again against the owner root and this
  counter; a slot that does not read or check is a fault and the counter
  does not start. An empty slot is made from the record's first range. A
  book that is used up refuses the sale as CR-02 says for an empty batch. The
  scripts the book handed out are watched as a batch's are.
- **CR-235.** Later ranges arrive through the link as messages of kind
  Range, each the `OBM*` frame. A range is taken when it passes CR-232 and
  overlaps no index the counter has handed out and no range still in use
  (WL-25, WL-26); otherwise nothing changes and a note gives the rule and
  words. A range taken is recorded as CR-167 records a batch, with tag byte 35 and
  the digest of the text `OpenBMT/range-held/v1` and the range's wire bytes,
  but in the other order: check, store, record. The record tells the owner
  that the range arrived (LK-77), so it is never written before the range is
  stored. A range already stored and recorded is taken as nothing. A range
  already stored with no record (the record failed after the store) gets
  its record when it arrives again, and nothing else changes. A range
  recorded but not stored (a counter that stopped between the two in an
  older version) is stored and no second record is written. The body is read
  as strictly as CR-169 says. A counter paired with the Key app drops a Range message with a
  note, and a counter paired with the shop's own wallet drops a Batch
  message with a note: "This counter makes its own addresses."
- **CR-236.** Settings after pairing are checked as section 15 says, with
  the owner root alone in place of the two roots. The class of the change is
  computed from the settings in force and the new ones
  (`openbmt_core::approval::classify`, AP-01, AP-02). A Free change is taken
  with the owner's signature alone. A Loosen or Redirect change is refused:
  it needs a counter confirmation or a wallet signature
  (`docs/APPROVAL-RULES.md`), and neither is joined to the settings yet. The
  note says the change must be confirmed at a counter. The `Setting` record
  of CR-162 is written as there.
- **CR-237.** The app's screens are the camera's, as CR-98 says: scan the
  owner phone's offer, the first-address screen, the request as a code with
  its four words and "They match" and "They differ", then the bundle. The
  first-address screen shows the address as the largest thing on it, in
  groups of four characters, and the kind of wallet beneath it. The same
  scan reads either kind of offer; the code's frame says which ceremony
  follows.
- **CR-238.** Every state and refusal of this section has words under
  `docs/SCREEN-RULES.md`.

## 24. Confirming a change at a counter

A shop that uses its own wallet has no Key app to sign a loosening of its
settings. Instead, a person at one of the shop's counters confirms it
(`docs/APPROVAL-RULES.md` §2): the counter shows what the change loosens
and four words, the owner phone shows the same four words, and a manager's
PIN signs it. A phone taken from the owner cannot loosen the shop's limits
on its own. This section is the counter's part. A change that sends payments
somewhere else (Redirect, AP-01) needs the wallet's own signature and is
not in this section; it stays refused as CR-236 says.

- **CR-239.** Settings that pass every check of CR-236 except the
  confirmation of a Loosen change are not refused: they are kept as the
  counter's one change to confirm, in the node's folder (`confirm.bin`, the
  `OBMS` frame and nothing else, written as CR-161), and are not in force.
  Newer such settings replace older ones. They are dropped when settings of
  the same or a higher version come into force, and seven days after they
  arrived. A Redirect change is refused as before.
- **CR-240.** While a change waits, the manager's menu has "Change to
  confirm". It shows each field the change loosens, with its value now and
  its value after, from `settings::changed_fields` (AP-01), and the
  removal of the last counter in force (AP-12). A limit is shown in
  satoshis, the number the counter enforces, whenever its satoshis change;
  in the shop's currency only when the satoshis stay the same, so a change
  cannot show a lower amount while it raises the limit. A change of the
  counters or of the devices shows, before and after, each entry that is not
  the same on the other side: the device's key in full, in groups of four
  as the Key app writes it (AS-39), and a counter's share as a percent, or
  "None". A counter swapped for another or given a larger share thus never
  reads as no change. A change of Shop Lightning (AP-13) shows the
  provider's key in full and its address, the key of the device that keeps
  the second copy in full, the cap and the most for one opening. Then come the settings version and the four words
  of AP-03 for the settings' hash, large. It has two buttons, "Confirm with
  manager PIN" and "Not now". Home shows a line that a change waits, for the
  manager.
- **CR-241.** "Confirm with manager PIN" asks for the manager's PIN
  (CR-84). With the right PIN the counter signs the confirmation of AP-04
  for the settings' hash with its identity key, and writes it as a
  `Setting` record of its own chain: one tag byte (36), the settings hash
  (32 bytes) and the signature (64 bytes), 97 bytes. The owner phone reads
  it from the counter's records, as it reads every record; nothing else
  carries it. A wrong PIN signs nothing and counts toward CR-84's lockout.
  The confirmation does not put the settings in force on this counter: they
  come into force only as CR-242 says, on every counter alike.
- **CR-242.** A message of kind Settings may carry the frame `OBM<`,
  version 1: the `OBMS` frame, then the confirming counter's id (32 bytes)
  and its signature (64 bytes). It is read as strictly as CR-160 reads
  `OBMS`. The counter takes a Loosen change from it when the signature
  verifies under AP-04 for these settings' hash and that counter, and the
  counter is in the settings now in force (AP-05); every other check of
  CR-160 and CR-236 applies as for `OBMS`. A confirmation from a counter
  not in force, for another hash or with a bad signature is refused with a
  note, and nothing changes. `settings.bin` keeps the `OBM<` frame whole,
  and CR-163 reads either frame at start.
- **CR-243.** The body of CR-241 is read strictly: a tag other than 36, a
  cut body or a byte left over is refused, and re-encoding gives the input.
  The books, the totals and the readers of sale events take no meaning from
  it. Bodies of 37 (CR-162), 41 (CR-167), 41 with tag 35 (CR-235) and 97
  bytes are each read as at most one kind.
- **CR-244.** Every state and refusal of this section has words under
  `docs/SCREEN-RULES.md`.

## 25. Bills, tabs and the till's reference

A table splits its check, a guest pays a deposit and then the rest, a bar
keeps a tab open all night. A bill is the amount owed in the shop's money,
paid in parts: each bitcoin part is an ordinary sale with every rule of the
sections above, and a part paid another way is written down by the cashier.
A tab is a bill whose amount grows while it is open. The till's reference
(its check or receipt number) can ride on a sale or a bill, so that what the
till closed as bitcoin can be matched against what arrived
(`docs/BOOKS-RULES.md` §13). Status: specification, written before the code.

- **CR-245.** A sale or a bill may carry the till's reference: 1 to 32
  characters of `A-Z`, `a-z`, `0-9`, `-`, `_`, `/`, `#` and `.`. It is set
  when the sale or the bill is opened, or added once later; one that already
  has a reference takes no second one. A reference that does not fit is
  refused with nothing written. It is written as a `Sale` record with tag 25
  (the time, then the sale's index or the bill's number as CR-247 says, a
  length byte and the text). The counter does not read it; the books and the
  match of `docs/BOOKS-RULES.md` §13 do. Unlike a note (CR-212), the
  reference is also given to the POS reader with the sale's events, because
  it is the till's own number.
- **CR-246.** A bill has a number, an amount in the minor units of the
  shop's currency (CR-193), above 0 and at most the largest amount of
  CR-05, a kind (a bill or a tab) and a state: open, settled, closed short
  or cancelled. Its number is the counter's next bill number, a 32-bit count
  from 1 that is stored before it is used, as CR-03 stores the next index,
  so a number is never given twice.
- **CR-247.** A bill is written as `Sale` records: tag 26 when it opens (the
  time, its number, its kind, its amount and the currency's code), tag 27
  when a tab's amount changes (the time, its number, the new amount), tag 28
  for a part paid another way (the time, its number, the amount and the way:
  1 cash, 2 card, 3 other), tag 29 when it ends (the time, its number and how
  it ended: 1 settled, 2 closed short, 3 cancelled), and tag 30 when a sale
  becomes one of its parts (the time, its number and the sale's index). Each
  record is stored before the state it names, as CR-59 stores a quote.
- **CR-248.** A bitcoin part is a sale opened for the bill, or a sale already
  open or paid that the cashier joins to it, such as a paid deposit request
  (CR-205). A sale is a part of at most one bill, and joining it is final.
  The part's amount is the sale's amount; its tip is not part of the bill
  (CR-189). Every rule of the sale holds for the part as for any sale:
  quotes, limits, Lightning, overrides, refunds.
- **CR-249.** What a bill has received is the sum of its parts that count:
  a bitcoin part counts with its amount in the shop's money once the sale is
  paid by its own rules (accepted, or confirmed for a request); a part paid
  another way counts with the amount the cashier typed. A part that is
  pending counts for nothing until it is paid. The bill is settled, and
  written so (tag 29), when what it has received reaches its amount; the
  part that passes it is not cut short, and any excess is that sale's
  overpayment, judged as CR-16 judges it.
- **CR-250.** The counter helps the cashier split what is left: it shows
  what is left and, when asked for a number of people from 2 to 20, the
  amount each, rounded up to a minor unit, the last share taking what is
  left over. Each share is opened as a part only when its customer is ready.
- **CR-251.** A tab's amount may be raised while it is open, and lowered to
  no less than what it has received. A bill's amount does not change. A tab
  is settled only when the cashier says it is finished and what it has
  received reaches its amount; until then it stays open whatever has
  arrived.
- **CR-252.** A bill that has received less than its amount can be closed
  short with the manager's PIN (CR-84); what was not paid is written in the
  record's amount, and the books show it. A bill with no part that counts
  and none pending can be cancelled; one that has received something or has
  a part pending cannot, and its parts are refunded by the rules of section
  7 if they are to be refunded.
- **CR-253.** Bills are stored in their own file, `bills.bin`, magic `OBM[`,
  version 1, written by atomic steps: the next bill number, then each open
  bill (number, kind, amount, currency, reference, its parts: the sale's
  index, or the amount and way of a part paid another way) and each bill
  that ended in the last 7 days, oldest first. At most 256 bills are open
  and a bill has at most 32 parts; a new bill or part past those is refused
  with words. The read is strict and bounded, as CR-42 reads sales.
- **CR-254.** The Paid screen of a sale shows the sale's number and, when it
  has one, the till's reference, large enough to copy into the till's
  payment note, because a fiscal till may need them on its receipt
  (`docs/BOOKS-RULES.md` §13).
- **CR-255.** Every state and refusal of this section has words under
  `docs/SCREEN-RULES.md`, listed in `docs/screens/counter.md` and checked
  as every other words are; the labels of the counter app's bill screens
  are plain and under 28 characters.

## 26. Switching from the starter wallet

A shop that started simple (`docs/START-SIMPLE-RULES.md`) takes its first
payments into a starter wallet whose key is on the owner phone. When the
shop moves to a key made offline, each counter has to start paying into the
new wallet. A counter pins its wallet at pairing and refuses any change that
sends payments somewhere new (CR-236), so this section is the one way
through, and it is open only once and only to a counter that pinned a
starter wallet. It cannot trust the owner phone alone, since a taken or
hacked owner phone would send a wallet of its own: the person at the
counter checks the new wallet's first address on the new wallet's own
screen, and a manager's PIN confirms it. The codecs and checks are in
`crates/openbmt-core` (`pairing_own`) and `crates/openbmt-counter-node`
(`paired`); the screens are the app's.

- **CR-256. Marked at pairing.** The owner phone's offer of CR-227 may be
  version 2: version 1's fields, then one byte of flags, of which only bit 0
  is defined (the pinned wallet is a starter wallet) and every other bit must
  be zero. The binding and the four words of CR-229 are made from the whole
  offer, so the flag is covered by the person's check of the four words.
  The record of CR-233 becomes version 3 with the same byte after the
  peers and the relay (version 2 is the record with a relay). A version 1
  or 2 record, and a version 1 offer, has no flag. A counter whose record has no
  flag refuses every switch.
- **CR-257. The switch.** The owner sends the switch as a message of kind
  Switch (`docs/LINK-RULES.md` LK-78), the frame `OBM]`, version 1: this
  counter's id (32 bytes), the hash of the wallet now pinned (32 bytes), the
  new pinned wallet (`OBM)`, WL-21) whole, the first signed range for the
  new wallet (`OBM*`, WL-24), and the owner root's signature over all of
  them under the tag `OpenBMT/switch-own/v1`. It is decoded strictly (exact
  length, no trailing bytes, every part decoded and re-encoded to the same
  bytes).
- **CR-258. Checked before it waits.** The counter takes a switch to check
  only when all of these hold, in this order: its record has the starter
  flag (CR-256); the signature is the pinned owner root's; the counter id is
  its own; the hash is that of the wallet it pins now; the new wallet passes
  WL-21 and WL-22, is for the same network and is not the wallet pinned now;
  the range passes CR-232 for this counter and the new wallet. Any other
  switch is refused with a note naming the rule, and nothing changes.
- **CR-259. One switch waits.** A switch that passes is kept in the node's
  folder (`switch.bin`, the `OBM]` frame and nothing else, written as CR-161)
  and changes nothing yet. Sales go on into the wallet pinned now. A newer
  switch replaces it. It is dropped seven days after it arrived, and the
  owner phone may send it again.
- **CR-260. The manager checks the address.** While a switch waits, home
  shows a line for the manager and the manager's menu has "New wallet to
  check". It shows the new wallet's first receive address (receive chain,
  index 0, WL-20) as the largest thing on the screen, in groups of four
  characters, the kind of wallet beneath it, and a line saying to compare it
  with the new wallet's own screen (the stick's result screen, its "Check
  words I wrote" mode, or the signer's screen), not with the owner phone. It
  has two buttons, "Same address" and "Different". "Different" drops the
  switch and writes the record of CR-262 with the refusal byte.
- **CR-261. The manager's PIN, then the switch.** "Same address" asks for
  the manager's PIN (CR-84). A wrong PIN changes nothing and counts toward
  CR-84's lockout; "Different" needs no PIN, since it changes nothing. With
  the right PIN the counter, in this order: writes the range book of CR-234
  again as version 2, which holds the old wallet's ranges closed (no index
  of them is handed out again), each naming its wallet, the old wallet's
  description so that its handed-out scripts stay watched, and the new
  wallet's first range; writes the record of CR-233 again with the starter
  flag cleared, the pairing itself kept as it was made; writes the record of
  CR-262; deletes `switch.bin`. From then on the wallet in force is the one
  the book's open range is for. At start, a book whose open range is for a
  new wallet while the record still has the starter flag and `switch.bin`
  holds the switch to that wallet finishes the switch from the second step;
  any other mismatch of book and record is a fault, as CR-234 says. Every
  later sale's address is the new wallet's. A sale already showing an
  address of the old wallet ends as it would have. A sale is kept by its
  address index, so the new wallet's first range starts above every index
  the owner phone has given out for the old wallet, and a counter refuses a
  range that holds the number of a sale it already has. The new wallet's
  first sale is therefore not at its index 0: a wallet program that opens
  the new wallet from its words needs a gap limit above that index, as the
  handbook says for any shop with more than one counter.
- **CR-262. Told to the owner.** The switch is a `Setting` record of the
  counter's own chain: one tag byte (37), one byte (1 switched, 0 refused at
  "Different"), the hash of the new pinned wallet (32 bytes) and the
  counter's signature with its identity key over the tag, the byte and the
  hash under the tag `OpenBMT/switched-own/v1` (64 bytes), 98 bytes. It is
  read as strictly as CR-243 says, and a body of 98 bytes is read as this
  kind only. The owner phone reads it from the counter's records, as it
  reads every record.
- **CR-263. Only once.** A counter whose record has no starter flag,
  including every counter that has switched, refuses every switch as
  CR-258 says. To change a wallet in any other case, the counter is paired
  again.
- **CR-264.** Every state and refusal of this section has words under
  `docs/SCREEN-RULES.md`, listed in `docs/screens/counter.md`.


## 27. A sale from the till

A till that works with the Box (the Odoo module, `docs/BOX-RULES.md` §5c)
asks one counter to take a payment of a set amount. The cashier starts it
on that counter, and from then it is an ordinary sale with every rule of the
sections above. These rules are the counter's side of BX-76 to BX-78.
Status: specification, written before the code.

- **CR-266. What is taken.** A POS notice from the Box (BX-76) that is a
  sale request is taken only from the Box of the settings in force, sealed
  to this counter, with the shop's currency, an amount above 0 and at most
  the largest amount of CR-05, a reference of at most 40 printable ASCII
  characters, a lapse time not past, and a till's sale the counter does not
  already hold. Anything else is
  dropped with a note and nothing sent. At most 8 requests wait; one more
  is dropped with a note, and the Box's own limit and lapse (BX-76) tell
  the till.
- **CR-267. What the cashier sees.** While a request waits, the counter's
  home screen shows "From the till" with its amount, its reference and the
  minutes left, and the buttons Start and Not now. The oldest request not
  lapsed is shown first. The amount cannot be changed at the counter. Not
  now hides it from the screen and sends nothing; the request lapses at the
  Box as BX-76 says.
- **CR-268. Start.** Start opens a sale for that amount exactly as a sale
  typed at the counter (sections 2 and 3: the counter's own rate, address
  and quote), and writes a `Sale` record with the next free tag naming the
  till's sale (its 16 bytes), stored before the sale's first quote is shown,
  as CR-59 stores a quote. The request's reference becomes the sale's
  till's reference (CR-245) if it fits that rule, and is otherwise only
  shown. The counter then sends the Box a signed "opened" notice naming the
  till's sale (BX-77). Every later notice of that sale names it too: paid,
  final and unpaid again.
- **CR-269. A cancel.** A cancel for a request not started removes it with
  nothing sent. A cancel for a sale already started does not end it: the
  counter shows "The till cancelled this sale" on that sale, and the cashier
  ends it as any sale is ended. A payment that arrives anyway is a payment
  as any other: it is counted, shown and can be refunded. The counter never
  ends a sale, and never marks one paid, on the Box's word alone.
- **CR-270. After a restart.** Waiting requests are kept in memory only, as
  the Box keeps them (BX-76), so a restart forgets them. A sale already
  started keeps its till's sale from its record and goes on naming it.
- **CR-271.** Every state and refusal of this section has words under
  `docs/SCREEN-RULES.md`, and the tests name these rules.

## 26. The starter line

A counter paired with the starter wallet is told by the owner phone what
that wallet holds, its limit, whether the owner asked to stop bitcoin sales
above it, and every payment out of it (`docs/START-SIMPLE-RULES.md` SS-32,
SS-38, `docs/LINK-RULES.md` LK-79). The line needs the shop's Box.

- **CR-272. Taking a line.** A counter takes a starter line only while it is
  paired with the starter wallet and not switched (CR-256, CR-261), only
  when the owner root's signature verifies, and only when its number is
  higher than the last one taken. The last line taken is kept in the node's
  folder in a file of its own, written as CR-161 and read strictly at
  start. A line that fails any check is dropped with a note and changes
  nothing.
- **CR-273. The stop.** When the last line taken says stop, holds at least
  the limit, and is not older than the heartbeat age limit of the settings
  in force, "New sale" offers no bitcoin and says why (SS-38). With no line,
  or a line older than that, the counter offers bitcoin as before, and,
  when the last line said stop, Home says that the limit cannot be checked.
  The counter takes no other meaning from the line: a sale already open is
  not changed.
- **CR-274. The payments.** Each payment in a line that the counter has not
  shown before goes on Home as a card with its amount and time, and says
  whether the owner phone made it, until someone at the counter taps that
  they have seen it. The cards and what was seen are kept across a
  restart. "Today" lists the day's payments from the starter wallet.
- **CR-275. After the switch.** A counter that has switched (CR-261) drops
  its starter line and its cards, and takes no more lines.
- **CR-276.** Every state and refusal of this section has words under
  `docs/SCREEN-RULES.md`.

