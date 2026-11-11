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
  person is and has checked the PIN, as for an override (CR-21).
- **CR-70.** A cashier or a manager can withdraw a request. A
  `RefundWithdrawn` record is written, naming the request number. The
  counter cannot know whether the owner has taken the request, so the
  withdrawal is a message and not a fact: the owner honours it only if the
  refund is still queued and not in an open session. A withdrawn request no
  longer counts for CR-67; it still counts for the cap (CR-69). A request
  that is withdrawn, or does not exist, cannot be withdrawn again.
- **CR-71.** A cash refund from the till (the shop's option, DESIGN §2.4) is
  a record `RefundPaidInCash` with the amount in the shop's currency, the
  cashier and the reason. It is valued at the sale's last quote and counts
  for CR-67, rounded up to a whole satoshi, and for the cashier's cap
  (CR-69). It asks the owner for nothing: the bitcoin is kept.
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
  it.
- **CR-86.** A cashier's action needs no PIN: opening, requoting and
  cancelling a sale, asking for or withdrawing a refund, and a cash refund.
  The actions of CR-21 (an override) and of CR-67 to CR-69 (a refund asked
  for or withdrawn by a manager) are made with the manager as the actor
  only after the PIN was checked. The app never names the manager as the
  actor without it. Each action names its actor, and the library keeps the
  name.
- **CR-87.** When the signed settings have Tor on, every connection of the
  app, the node's and the price fetch's, goes through the SOCKS5 proxy of
  the app's setting (`127.0.0.1:9050` unless the owner changed it), and no
  name is looked up by the phone. Before the node starts, and before every
  price fetch, the app asks the proxy for a SOCKS5 greeting. If the proxy
  does not answer, nothing is sent around it: the node is not started, no
  price is fetched, and the app says "Tor is on for this shop, but Orbot is
  not running." and "Open Orbot and turn it on." The manager is the one
  who acts. This is NR-26 applied: the proxy is the caller's. The node
  enforces it from the verified settings: `start` refuses to start, with
  this rule, when the signed settings have Tor on and the start has no
  proxy. The app's reading of the settings before the start (`settings_read`)
  is for display only and is not trusted.
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

## 10. What these rules do not do

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
