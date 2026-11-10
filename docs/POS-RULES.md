# POS rules: matching the POS orders to the counter's sales

Status: specification for `crates/openbmt-pos`, written before the code.
Each rule has an ID so that a test can name the rule it proves. The rules
implement `docs/DESIGN.md` §2.2, §2.7, §5.3 (the Box, and "The owner phone
polls the POS itself"), §5.6 and §10 for any POS, without naming one.

The flow they describe works on every POS and needs only reading from it:

1. The shop adds a tender type named "Bitcoin" in its POS.
2. The cashier rings up the order in the POS as always, then enters the
   amount on the counter. The customer pays. When the counter shows green,
   the cashier closes the order in the POS with the Bitcoin tender.
3. The owner's device (and the Box, if there is one) reads the POS orders
   with a read-only sign-in and matches every Bitcoin tender to a counter
   sale. Anything that does not match is an alert. This catches a cashier
   who picks the Bitcoin tender and keeps cash, an order closed before the
   payment was safe, and a payment that never got an order.

Closing an order through the POS's own API is a per-vendor add-on (PS-81).
Nothing here depends on it. The Box (`docs/BOX-RULES.md` section 4) is the
one that uses it, and runs this matching on the counters' signed paid
notices instead of their records.

This library has no network, no clock, no storage and no vendor code. The
caller passes in the POS orders (through a reader it supplies), the
counter's sales and the time, stores what it is handed before it takes
effect, and draws what it is told. Times are whole seconds since 1970 UTC.

## 1. The model

- **PS-01.** An order has an id (1 to 64 bytes of printable ASCII, opaque),
  a location, a state (`Open`, `Closed` or `Voided`), a currency, a total in
  minor units, its tenders (at most 16), the times it was opened, last
  changed and, if closed, closed, and an optional reference note. An order
  that breaks a bound is refused whole, and a page that holds one is
  refused whole.
- **PS-02.** A tender has an id (the same form as an order id, unique within
  its order), a kind (`Bitcoin`, `Cash`, `Card` or `Other`), a name, an
  amount and a tip in minor units, a state (`Paid`, `Voided` or `Refunded`
  with the amount refunded, which is at most the amount plus the tip) and
  the time it was taken.
- **PS-03.** Text (a location, a tender name, a note) is 1 to 64 bytes and
  holds no control character. A POS that holds longer text is cut by its
  connector before it is handed in, at a character boundary. A reference is
  read from the bytes handed in, so a code that the cut splits is not found.
- **PS-04.** The tender's and the order's times are the POS's own. A sale's
  times are the counter's own. The two are different devices with different
  clocks, and the rules do not otherwise correct the difference (PS-08).
- **PS-08.** A tender timed up to 120 seconds **[to tune]** before the sale
  opened still matches it, in pass 2 (PS-33) and in pass 1 (PS-31, which has
  no time test at all). This clock slack is a setting. It is separate from the
  2 minutes of PS-41, which are about the payment and not about the clocks.
- **PS-05.** A sale as the matcher sees it (a "sale fact") is: the counter,
  the sale index, the amount and currency, when it opened, when it was
  first reported paid (if ever), whether it was reported unpaid after that
  and not paid again, whether it is final, and its reference. Facts are
  built only from the counter's signed records. The chain is verified whole
  first, and a chain that does not verify gives no facts. The Box has no
  records: it builds the same facts from the counter's signed paid notices
  (BX-40), which carry exactly these fields and nothing else; the
  reference is worked out from the counter's key and the sale index
  (PS-20). The counter folds its own records as PS-06 and PS-09 say to know
  which notices to send, so the two ways give equal facts.
- **PS-06.** Without PS-09, the facts are read from the counter's events in
  order. The sale is reported paid at the first of `PaidAtOnce`, `Confirmed`,
  `Override` or `Final`. It is reported unpaid after that by `Reopened`, and
  by `Replaced` unless the sale is confirmed at that moment, and is paid again
  by any of the four paid events. It is final at `Final`, until a `Reopened`.
  An event for a sale that was never opened, a second `Opened` for one sale,
  and a record body that does not decode are refused.
- **PS-07.** What the counter's events do not carry stays empty. The times are
  the counter's time of the judgment that wrote the record. A late payment on
  an ended sale (CR-19), a surplus and a refund are not part of a fact.
- **PS-09.** The counter writes a record `PosPaid` and a record `PosUnpaid`
  exactly when it reports a sale to the POS as paid or as unpaid again (CR-30,
  CR-32). For a sale that has one of them, the facts take "first reported
  paid" from the first `PosPaid`, and "unpaid after" is true when the last of
  the two records is `PosUnpaid`; nothing is inferred (PS-06 is not used for
  that sale). For a sale that has neither, which is every sale of a chain
  written before these records existed, PS-06 applies. "Final" is the record
  `Final` in both cases: the counter reports it to the books once, with that
  record.

## 2. What counts as a Bitcoin tender

- **PS-10.** What counts as a Bitcoin tender is a shop setting: the exact
  tender name or names (at most 8). A tender is a Bitcoin tender when its
  kind is `Bitcoin`, or when its name equals one of the names, compared
  without regard to ASCII case and surrounding spaces. Nothing is compared
  more loosely than that.
- **PS-11.** A tender of kind `Other` (or any kind) with another name is
  never guessed to be bitcoin. A tender named like the setting but of
  kind `Cash` or `Card` is counted, because more checking is the safe side.
- **PS-12.** Only a Bitcoin tender in state `Paid` on an order that is not
  `Voided` is a candidate for matching. Split tenders are fine: only the
  Bitcoin tender's own amount is compared with a sale.
- **PS-13.** The settings name the shop's currencies (one to four). A
  candidate in another currency is not matched and raises `UnknownCurrency`.
  An order's currency is checked when it is built, so a code the POS holds and
  the connector cannot read is the connector's error (`BadResponse`), not an
  alert.
- **PS-14.** The settings map each POS location, by exact text, to the
  counters that serve it (at most 32 locations, 16 counters each). A
  counter is in at most one location. A counter may instead be marked "no
  POS", for a counter that is used without one.
- **PS-15.** Which tenders are Bitcoin tenders is decided when an order is
  first taken into the ledger, with the settings of that moment, and only
  those tenders are kept. A name added to the settings later does not reach
  back into orders already held. A tender that was kept and is gone from a
  later version of its order is treated as removed (PS-45).

## 3. The sale reference

- **PS-20.** The reference of a sale is a pure function of the counter's
  key and the sale index: six characters, five taken from a tagged hash of
  the two and a sixth that checks them. The alphabet has 31 characters, with
  no `0`, `O`, `1`, `I` or `L`: `23456789ABCDEFGHJKMNPQRSTUVWXYZ`. The check
  character is the sum of each character's value times its place (1 to 5),
  modulo 31, so any one wrong character and any two neighbours swapped are
  seen.
- **PS-21.** A reference is found in free text. Case does not matter.
  Text is cut into runs of letters and digits, and a run of exactly six
  characters from the alphabet with a good check is a reference. If the text
  holds none, or more than one different one, nothing is found. "btc
  K7M2QX" finds K7M2QX.
- **PS-22.** The reference is a convenience for matching and never a proof
  of payment. It says which sale a cashier meant. It never makes a sale
  paid, never closes anything and never overrides the counter's screen.
  Only the counter's signed records say a sale is paid.
- **PS-23.** A reference names a sale when it is the reference of a sale of a
  counter at the order's location. If it names more than one (two sales whose
  codes happen to be equal), the first by counter and index that fits in
  amount and currency is taken. A six-character word or number in a note that
  passes the check and names no sale is ignored. Odds are about one in 31 for
  such a word to pass the check and then it must also name a sale, so the
  effect is a rare false alert at worst, never a match to the wrong sale of
  another amount.

## 4. Matching

- **PS-30.** Matching is deterministic: the same orders, facts, settings and
  time give the same matches and alerts, whatever the order of the input.
  Ties are broken by time, then by the ids, never by the order given.
- **PS-31.** Pass 1, by reference. A candidate's note (or else its order's)
  that holds a reference naming a sale of a counter at the order's location
  is matched to that sale when the amount and currency are equal. If more
  than one sale has the reference, the first by counter and index that
  fits is used. A candidate whose reference names no sale at its location
  goes on to pass 2.
- **PS-32.** A reference that names a sale but whose amount or currency
  differs is not a match: it raises `AmountDiffers`, the tender takes no
  part in pass 2, and the sale is claimed so that it does not also raise
  `PaymentWithoutTender`. A reference that names a sale already matched to
  another tender raises `DuplicateReference` on the later tender, and that
  tender also takes no part in pass 2. The earlier tender (by time, then
  ids) keeps the sale.
- **PS-33.** Pass 2, by amount and time. A candidate matches a sale at its
  location with an equal currency and an equal amount when the tender's time
  is no earlier than the sale's opening and no later than 30 minutes **[to
  tune]** after the sale was first reported paid. For a sale never reported
  paid the 30 minutes run from 15 minutes **[to tune]** after it opened.
  The amount compared is the tender's amount without its tip.
- **PS-34.** One sale matches at most one tender and a tender at most one
  sale. When several pair up, the earliest tender (by time, then ids) takes
  the earliest eligible sale (by opening, then counter and index), and a
  match is marked `by_order` when more than one tender or more than one
  sale was eligible, so the owner can see it was not exact.
- **PS-35.** A tip on a Bitcoin tender raises `BitcoinTip`, matched or not:
  the counter took no tip, so the tip was not paid in bitcoin.
- **PS-36.** Matches are kept between polls. A match by reference never
  changes while its tender stands. A match by amount and time is worked out
  again at each poll while its tender is a candidate and its sale is known, so
  a late tender cannot leave the pairs crossed. A match whose sale is no
  longer among the facts is kept as it is and not judged again. A match whose
  tender is refunded stays, and the sale stays taken. A match whose tender is
  voided, gone from the order, or on a voided order is freed (PS-38).
- **PS-37.** A paid sale of a counter that is in no location and not in
  no-POS mode is never matched and raises `PaymentWithoutTender`, so that a
  counter missing from the settings is seen.
- **PS-38.** Re-ring after a void. A sale whose matched tender was voided, or
  removed from its order, or whose order was voided, is free to match again.
  If a later Paid Bitcoin tender of the same location, currency and amount
  (within the window of PS-33), or one that carries the sale's reference,
  appears, the sale moves to it, and `VoidedAfterPayment` on the first tender
  is cleared as resolved: the sale was rung again under another order. While
  no such tender has appeared the sale stays with the voided one, so it is not
  a payment with no order. A refund is not a void and never frees the sale.
- **PS-39.** The first tender seen to carry a reference to a sale keeps it. A
  later tender with an earlier time that names the same sale is the duplicate
  (PS-32), because a match by reference is not worked out again. A match by
  reference is also taken before any match by amount and time, so a reference
  outranks a pairing by order.

## 5. Alerts

Each alert has a kind, a subject, why it was raised and what to do next,
both in plain words, since every unknown explains itself (§2.6).

- **PS-40.** `TenderWithoutPayment`: a candidate with no sale, once its time
  is 10 minutes **[to tune]** old.
- **PS-41.** `ClosedBeforePaid`: the matched sale was first reported paid
  more than 2 minutes **[to tune]** after the tender's time.
- **PS-42.** `PaymentWithoutTender`: a sale that is reported paid now (not
  unpaid again) for more than 10 minutes **[to tune]**, matched to no tender
  and not claimed by PS-32, on a counter that is not in no-POS mode. It is
  raised only while the sale was first reported paid less than 7 days ago, so
  that retiring a match (PS-73) cannot free a sale into an alert. A sale
  reported unpaid again raises nothing: its money is not safe and the goods
  are held. If the payment later comes back it is a sale reported paid and the
  rule applies from then.
- **PS-43.** `AmountDiffers`: see PS-32.
- **PS-44.** `TenderOnUnpaidSale`: the matched sale was reported unpaid
  afterwards and not paid again; or it was never reported paid and the
  tender is 10 minutes old.
- **PS-45.** `VoidedAfterPayment`: the matched sale is reported paid, and the
  order or the matched tender was voided or refunded in the POS, or the tender
  is gone from the order. A refund is raised at once. A void, or a tender gone
  from the order, is raised only once the order's last change is older than 10
  minutes **[to tune]** and no other tender has taken the sale (PS-38), so
  ringing the order again at once raises nothing. Either the customer is owed
  a refund or the cashier undid the order.
- **PS-46.** `DuplicateReference`: see PS-32.
- **PS-47.** `BitcoinTip`: see PS-35.
- **PS-48.** `UnknownCurrency`: see PS-13.
- **PS-49.** Every alert carries its why and its next step as text, with the
  amounts in the order's currency and no transaction ID, address or amount
  in satoshis (§10).
- **PS-50.** An alert is raised once. Its identity is its kind and its
  subject (a tender, or a sale), so a later poll that finds the same
  condition does not raise it again, and does not change when it was raised.
- **PS-51.** An alert is cleared only by a later state that resolves it (the
  tender gets its match, the sale gets its tender, the order is changed, the
  sale is rung again under another order), or by the owner's acknowledgement.
  An alert that cannot be worked out again because its sale is no longer among
  the facts stays open: not being among the facts is not a resolution.
- **PS-52.** An acknowledgement names an alert and a time. It moves the
  alert from open to acknowledged. It is an input: signing it as the
  owner's command is not part of these rules. An acknowledged alert is not
  raised again while its condition holds, and goes when the condition does
  or when it can no longer be worked out (its sale left the facts).

## 6. What leaves

- **PS-60.** Each poll returns what is new: the new matches, the alerts
  raised, the alerts cleared, and how many matches retired.
- **PS-61.** For the owner: the open alerts, oldest first, each with its
  text; and a summary (open, acknowledged, matches, matches by order and
  retired).
- **PS-62.** For the books: one line per match whose sale is final, with the
  POS order id and tender id, the sale (counter and index), the amount and
  the currency. A POS order is never what makes a sale money (CR-31): the
  books count at `Final`. Nothing in a line is a transaction ID, an
  address or an amount in satoshis (§10).

## 7. Storage

- **PS-70.** The ledger is changed by building the new value, handing its
  stored form to the caller's `persist`, and swapping only if that returns
  `Ok`. If it fails, the ledger is as it was, and so is the cursor.
- **PS-71.** The stored form is framed: the magic `OBMZ`, version 1, exact
  lengths, keys in strictly ascending order, no byte left over, and a
  tagged hash of everything before it at the end. It is read back whole:
  any truncation, any changed byte, an unknown tag or a bound broken is
  refused, and bytes that are accepted encode back to themselves.
- **PS-72.** Every size is bounded: 4,096 tracked orders, 16,384 matches,
  4,096 open and 4,096 acknowledged alerts, 16,384 sales, a cursor of 256
  bytes. A ledger that would grow past a bound refuses the change, and nothing
  is dropped silently. The refusal is PS-76.
- **PS-73.** Retirement. An order is retired, with its matches counted into
  one number, when it has not changed for 7 days **[to tune]**, every sale
  it is matched to is final and was first reported paid at least 7 days ago,
  and it has no open alert (a candidate left unmatched has acknowledged
  ones). Acknowledged alerts of a retired order go with it.
- **PS-74.** Each page of a sync, with its cursor, is stored as soon as it is
  read, before the next is asked for, so a POS that fails on a later page
  still makes progress. Matching and alerts are worked out once, on what is
  stored, after the last page read, and stored as a last write. A sync that
  stops (the POS fails, a page is refused, the store fails) says why and how
  many pages were kept, and what was kept stays. A page that breaks a bound is
  refused whole and not stored; the pages before it are. Reading the same
  pages again changes nothing and raises nothing again.
- **PS-75.** An order may come again as a newer version. The version with the
  later last change wins; when two have the same time within one sync the one
  with the greater stored form wins, so the result never depends on the order
  they came in. An older version never undoes a newer one.
- **PS-76.** A refusal for a full ledger names what is in the way, oldest
  first, at most 10 items: open alerts, orders waiting for a sale that is not
  final or not known, orders changed too recently. The owner can acknowledge
  the alerts named (PS-52) and the ledger can then retire what they held.
  Orders waiting for a sale are freed when the counter's facts say it is
  final, or by raising the bound.

## 8. The connector boundary

- **PS-80.** A reader gives a page of orders for a cursor: the orders, the
  next cursor and whether there are more. A page holds at most 500 orders;
  a sync pulls at most 64 pages and leaves the rest for the next poll. A
  reader that says there is more and does not move the cursor is refused.
  The reader is read-only, and a sync never calls anything else.
- **PS-81.** Closing an order through the POS's API is an optional
  capability of a connector. A close request names the order, the tender,
  the amount and the currency and carries an idempotency key: the tagged
  hash of the counter's key and the sale index. It is made only for a sale
  that is reported paid now (CR-30). A retry carries the same key, and the
  outcome says whether the order was already closed. The Box's use of it,
  and how it finds the order to name, are BX-43.
- **PS-82.** This crate holds no vendor code, no network and no sign-in.
  The scripted reader and closer it holds are test doubles.

## 9. What these rules do not do

- **They do not check payments.** The counter does (`docs/COUNTER-RULES.md`).
  A wrong fact handed in gives a wrong match.
- **They do not prove that goods were handed over for a payment.** A match
  says an order and a sale line up in amount and time, or by a reference
  the cashier typed.
- **They do not catch a cashier who rings the sale on the counter for less
  than the order.** The amount on the counter is the cashier's entry; the
  alert is `AmountDiffers` or `TenderWithoutPayment`, and only when the POS
  order's Bitcoin tender is for the full price.
- **They do not read a POS.** A connector does, and holds the sign-in.
- **They do not sign or send an acknowledgement.** That is an owner command.
- **They do not correct clock skew** beyond the slack of PS-08.
- **They do not link a late payment to an order.** A customer who pays a
  sale that already ended (CR-19) and whose order is then rung is seen as
  a tender on a sale that is not paid, or with no payment.
- **They do not settle two orders for one payment by themselves.** The
  alerts say which are involved; the owner decides.
- **They do not re-read orders already held** when the settings change
  (PS-15).
