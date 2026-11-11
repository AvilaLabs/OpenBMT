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
  4,096 open and 4,096 acknowledged alerts, 16,384 sales, a cursor of 1,024
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

## 9. Vendor readers

`crates/openbmt-pos-readers` holds one reader for each of two vendors,
Square and Clover. Each builds the text of one request for the orders that
changed since a cursor and reads the reply into a page of PS-80, or refuses
it whole. It has no network, no clock and no sign-in: the caller sends the
request with the sign-in it holds, hands the bytes of the reply in and stores
the page. Square is read-only (`docs/DESIGN.md` section 5.6, decided
2026-11-10): nothing in its reader names a payment or a change of an order.

The tests read fixtures that were written from the vendors' documentation,
not captured from a real account. What the documentation did not confirm is
marked **[verify, E5]** for Square and **[verify, E6]** for Clover: each such
point needs a real seller or merchant account before the reader is relied on.

- **PS-90.** A reply is read with `openbmt-json`: at most 8 MiB, at most 64
  levels of arrays and objects, at most 400,000 values, UTF-8 throughout, the
  grammar of RFC 8259, no key twice in one object (however it is spelled) and
  no byte after the document. A number is an integer that is exact (no
  fraction, no exponent, not `-0`) or the reader that needs one refuses it
  (PS-92). Any breach refuses the reply whole.
- **PS-91.** A field a reader needs that is missing, or `null`, or not of the
  type the vendor documents, refuses the reply whole; so does a reply that
  carries a service error (Square's `errors` array that is not empty). A state,
  kind or result the reader maps and does not know is refused only where it
  can change the reading of a Bitcoin tender or of an order that carries one:
  there it refuses the reply whole, and where it cannot (a cash payment, an
  order with no Bitcoin tender) the tender or order is read as the rules of
  PS-96 and PS-98 say, or left out, and the page goes on. Nothing is mapped by
  guessing. A field the reader does not need is ignored,
  and so is a field the vendor adds later, whatever it holds. An optional
  field the reader reads is checked when it is there.
- **PS-92.** An amount is a whole number of the currency's smallest unit from
  0 to 9,223,372,036,854,775,807: a negative or larger one is out of range, and
  one with a fraction or an exponent, or that is not a number, is of the wrong
  type. A time is read to whole seconds since 1970 in UTC. Square's is RFC 3339
  and strict: `YYYY-MM-DDTHH:MM:SS`, optionally a dot and one to nine digits
  (dropped, not rounded), then `Z` or `+HH:MM` or `-HH:MM`, capitals, a date
  that exists, no leap second, and not before 1970 or after the year 9999.
  Clover's is whole milliseconds since 1970, cut to seconds, not after the year
  9999. **[verify, E6: the unit of Clover's times]**
- **PS-93.** A currency is three capital letters; a tender or a refund in
  another currency than its order is refused (the model holds one per order).
  An id (an order's, a tender's, a payment's) is refused, never cut, unless it
  is PS-01. A location is refused, never cut, unless it is PS-03. A tender
  name is cut to 64 bytes at a character boundary (PS-03) and refused if it
  is then empty or holds a control character. A note typed by a person has
  each control character turned into a space, the ends trimmed and is cut to
  64 bytes; nothing left is no note.
- **PS-94.** The bounds of PS-01, PS-02 and PS-80 hold for what is read: a
  page of more than 500 orders, an order with more than 16 tenders, two
  tenders with one id, refunds of a tender larger than its amount and tip, a
  refund that names no tender of its order and a cursor above 1,024 bytes each
  refuse the page whole. A page size is 1 to 500. A cursor is the time in
  whole seconds from which orders are read (decimal, no leading zero, not
  after the year 9999), then, only while a page has more, a bar and the
  vendor's own marker (printable ASCII, no space). The empty cursor is time 0.
  A cursor that is not in that form is refused.
- **PS-95.** The Square request is a POST of a JSON body to
  `/v2/orders/search`: `location_ids` (the location text of the settings,
  which is Square's location id), `limit`, `cursor` when the cursor holds
  Square's, and `query` with `filter.state_filter.states` of `COMPLETED` and
  `CANCELED`, `filter.date_time_filter.updated_at.start_at` from the cursor's
  time and `sort` by `UPDATED_AT` ascending. It is the same text for the same
  input. No location in the settings is refused. A ListPayments request is
  a GET of `/v2/payments` with `begin_time` a day before the cursor's time,
  `location_id` of the first location, `sort_order` of `ASC`, `limit` (1 to
  100) and `cursor` when the previous payments reply gave one; every value is
  percent-encoded as in PS-97. **[verify, E5: the path, the inner names
  `states` and `updated_at`, the largest `limit`, whether Square accepts a
  start at the beginning of 1970, and the names `begin_time`, `location_id`,
  `sort_order` and `cursor` of ListPayments]**
- **PS-96.** The Square reply is `orders` (left out when none were found),
  `cursor` (absent means no more) and `errors`. An order has an `id`, a
  `location_id`, a `state` (`COMPLETED` is closed, `CANCELED` is voided, `OPEN`
  and `DRAFT` are open), `created_at` (opened), `updated_at` and, when closed,
  `closed_at` (kept if there), `total_money` and optionally a `version` (an
  int32, read and checked, not kept), a `ticket_name` or else a
  `reference_id` (the order's reference note) and `tenders` and `refunds`. A
  tender has an `id`, a `type`, `created_at`, `amount_money` (which includes
  the tip, so the model's amount is that less `tip_money`), optionally
  `tip_money`, `note` and `payment_id`. The types are `CARD`,
  `THIRD_PARTY_CARD` (card), `CASH` (cash) and `SQUARE_GIFT_CARD`,
  `BANK_ACCOUNT`, `WALLET`, `BUY_NOW_PAY_LATER`, `SQUARE_ACCOUNT`, `NO_SALE`,
  `OTHER` (other); another type is kept as a tender of kind other named
  "Other (" and the type's text, cut to 64 bytes with control characters
  turned to spaces, and ")", and is never a Bitcoin tender. Which tenders are
  Bitcoin stays PS-10:
  a tender of type `OTHER` whose `note`, or whose payment's
  `external_details.source`, equals a name of the settings (PS-10's
  comparison) is given that name, and any other is named "Other". The
  payment comes from the ListPayments replies the caller may hand in with
  the orders (every page of them; a payment on two pages is read once, from
  the later; the reply's `cursor` says whether there is another page): its
  `status` `CANCELED` or `FAILED` makes the tender voided, and
  `APPROVED`, `PENDING` and `COMPLETED` do not; another status leaves the tender
  paid, and refuses the reply only if the tender is a Bitcoin tender. A tender's
  reference is its `note`, else its payment's. A refund names its tender
  (`tender_id`) and counts while its `status` is `PENDING` or `APPROVED`; it
  does not while `REJECTED` or `FAILED`; another status is ignored, and refuses
  the reply only if the tender is a Bitcoin tender. A state the
  reader does not know on an order (not `OPEN`, `DRAFT`, `COMPLETED` or
  `CANCELED`) leaves the order out of the page unless it carries a Bitcoin
  tender, when it refuses the reply. The refunds that
  count are added up on the tender (a refund of nothing is refused) and a tender
  that is voided stays voided. The next cursor keeps the time and takes
  Square's marker when there is one; when there is none it takes the newest
  `updated_at` of the page (never earlier than the time it was read from), so
  the last order is read again, which PS-75 makes harmless. **[verify, E5:
  how a Square POS "Other" tender names itself (the documentation does not
  say), that `amount_money` includes the tip, the full list of tender types,
  that a cancelled payment is shown by the payments reply, and that an order
  rung up in the Square app is seen once it is paid]**
- **PS-97.** The Clover request is a GET of
  `/v3/merchants/<mId>/orders` with the query `filter` of
  `modifiedTime>=<milliseconds>` from the cursor's time, `expand` of
  `payments,payments.tender,refunds`, `limit`, `offset` (from the cursor's
  marker, else 0) and `orderBy` of `modifiedTime ASC`. Every byte of a value
  but a letter, a digit, `-`, `.`, `_` and `~` is written as `%` and two
  capital hex digits. The merchant id is 1 to 32 ASCII letters and digits, or
  it is refused. **[verify, E6: the operator and unit of the time filter, the
  `expand` text, the form of `orderBy`, and the largest `limit`]**
- **PS-98.** The Clover reply is `elements`, which is required. An order has
  an `id`, a `currency`, a `total`, a `paymentState` (`OPEN` and
  `PARTIALLY_PAID` are open; `PAID`, `PARTIALLY_REFUNDED`, `REFUNDED` and
  `CREDITED` are closed; another leaves the order out of the page unless it
  carries a Bitcoin tender, when it refuses the reply), `createdTime` (opened)
  and
  `modifiedTime`, optionally a `state` (read for its type only) and a `note`
  (the order's reference). Its location is the merchant id. It is never read
  as voided: a void shows on its payments. `payments` and `refunds` are
  an object with `elements` or an array. A payment has an `id` (the model's
  tender id, since the id of the tender type is the same for every payment of
  it), a `tender` with a `label` (the name; Bitcoin is PS-10 against it, and
  the kind is `Other`), an `amount` (without the tip), optionally a
  `tipAmount` and a `note`, a `result` and `createdTime`. `SUCCESS` is paid
  and `VOIDED` is voided; `FAIL`, `INITIATED`, `VOIDING`, `VOID_FAILED`,
  `AUTH`, `AUTH_COMPLETED`, `DISCOUNT`, `OFFLINE_RETRYING` and `PENDING` are
  not money taken and the payment is left out, so a payment that was paid and
  is now being voided is seen as gone (PS-45); another result leaves a payment
  out too if its tender label is not a Bitcoin tender name (PS-10), and
  refuses the reply, naming the result text, if it is. A refund of a payment
  that was left out is ignored. A refund
  names its payment and has an amount above zero; it is added up on that
  tender, which stays voided if it is. A page of exactly the limit has more,
  and the next cursor keeps the time and moves the offset on; a shorter page
  is the last, and the next cursor takes the newest `modifiedTime` of the page.
  The documentation gives no flag for the end. **[verify, E6: the end of
  pages, the time unit, that `amount` is without the tip, the shape of a
  refund and of an expanded list, that `paymentState` is the state to use, and
  what a voided order looks like]**
- **PS-99.** The readers are pure: the same bytes and settings give the same
  page or the same refusal, and the same input the same request text. The
  crate depends on workspace crates by path and on nothing else, holds no
  network, no clock and no sign-in, and reads nothing from a file. A refused
  reply is reported to a connector as `PosError::BadResponse` and a person is
  given the words of `say.rs`.

## 10. What these rules do not do

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
