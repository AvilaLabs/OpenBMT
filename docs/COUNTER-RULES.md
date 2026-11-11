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
  proxy. The app's reading of the settings before the start (`pairing_read`)
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
  in the library and never from the app; if that source cannot be
  trusted to be unpredictable, the call is refused. The ceremony is in
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
  less, and the payment counts if it covers any of them. Asking again for a
  sale that is paid over Lightning already changes nothing. The cashier's
  screen says "Lightning, final" as the check line (CR-105) and says that a
  refund is given in cash or on-chain and never over Lightning.
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
  (CR-71) and an on-chain refund (CR-60) are both open. There is no refund
  over Lightning: the counter holds no key that can pay.
- **CR-112.** The driver asks the node for an invoice for each quote of a sale:
  the amount is the quote's satoshis times 1,000, the lifetime
  `QUOTE_SECS` (CR-05, LN-13) and the attempt is the next one for that sale
  and quote (LN-15). A new quote of the same sale (CR-05) asks for a new
  invoice and drops the old one. While a sale is open, unpaid and its
  invoice is live, the driver asks the node whether it is paid every 2
  seconds, at most one request at a time. It stops when the sale is closed
  (paid, cancelled, expired or final) or the invoice has run out. A node
  that cannot be reached, or whose reply is refused, is a note in the log
  and never a state of the sale. The driver holds no clock and no network:
  it hands out the next request with a number and takes the reply for that
  number; a reply for any other number is ignored.
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
  or the settings, and is never in a log or a screen.
- **CR-116.** The counter's status shows a Lightning line only when a
  credential is saved: "Lightning ready", or why not: the node cannot be
  reached, it refused the key, or the settings name another node now. The
  credential is kept with the node key it was tested against. When the signed
  settings name another node or none, or the saved credential does not read
  back, Lightning is off, the status says so, and it stays off until the
  credential is set up again; a sale then has no Lightning line (CR-113).

## 13. What these rules do not do

- **They do not check payments.** Proof of work, proofs, peers and
  conflicts are the watcher's. A wrong fact handed in gives a wrong
  screen.
- **They do not see a payment drop out of the network.** A light client
  cannot. A payment that never confirms is "stuck" (CR-18); the limits
  of `docs/DESIGN.md` §6 are what protect the shop.
- **They do not refund.** Surplus and cancelled sales with money on them
  are recorded for the owner's refund session (§2.4). A sale paid over
  Lightning is refunded in cash or on-chain, never over Lightning (CR-111).
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
  key. The bundle is version 2 when it carries a relay and version 1 when it
  does not, byte for byte as before; version 1 reads and has no relay. A
  version that is not 1 or 2, a route tag that is not defined, a name that
  is not a name, a port that is 0 and any byte left over are refused. The
  owner fills the relay from its link's settings when it shows the code;
  a route that cannot be written as a relay gives a code with none.
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
