# Request rules: asking a customer to pay from anywhere

Status: the library is built, in `crates/openbmt-requests`, with a test
named for each rule (`rq_NN_*`). The owner node keeps the book, takes the
desk's addresses, credits payments at the confirmation depth, takes them
back when their block leaves the chain and queues returns on the owner's
queue (`crates/openbmt-owner-node/src/requests.rs`); the owner library
carries them to the app as ops 216 to 226 and adds them to the books
(RQ-32). The owner app has no screens for them yet. A `bitcoin:` link
pasted or scanned as a return address is read with the BIP 321 reader of
`docs/URI-RULES.md`, and only its address is used (RQ-28).

A shop often needs to be paid by someone who is not at the till: a deposit
for a booking, an invoice to a business, a stall fee, a customer abroad, a
sale agreed over a message. These rules let the owner phone ask for that
money and follow it until it is paid. The owner phone makes the address,
keeps the request, watches for the payment and returns money when it must.
Nothing is sent by OpenBMT: the owner shares a link or a picture of the code
through whatever the owner already uses, and the customer pays it from any
wallet.

A counter can already make a short request at the till (`docs/COUNTER-RULES.md`
§21, CR-205 to CR-216): 1, 3 or 7 days, a fixed amount in bitcoin, paid when
confirmed. That stays as it is. The requests here are the owner's: longer,
priced in the shop's currency if the owner wants, with a due date instead of
an end, payments in parts, repeats and returns.

## 1. Where the addresses come from

- **RQ-01.** Requests are made on the owner phone of a shop paid to its own
  wallet (CR-227 to CR-238). A shop still paired through the Key app has the
  counter's requests only, and the owner app shows no "Request a payment"
  for it.
- **RQ-02.** The owner phone is a device of its own for addresses, the
  desk. Its id is the owner root. Its ranges come from the same allocation
  as the counters' ranges (WL-23, WL-25 to WL-27): no index is ever given to
  two devices, and a range of the desk is recorded in the allocation before
  its first address is used. A desk range holds 1,000 indexes. A new one is
  taken when the last is used.
- **RQ-03.** Each request has one address, the next unused index of the
  desk. The index is written to the request book (RQ-30) before the address
  is shown or shared, so a crash never gives the same address to two
  requests. An address is never given to a second request, even after the
  first is cancelled.
- **RQ-04.** Every address of the desk is in the owner phone's watch set
  from the moment it is written, and stays in it for good: a payment to a
  request that is paid, cancelled or years old is still seen (OC-01). Each
  request records the wallet its address was made from (that wallet's
  fingerprint), and its address is watched from that wallet's description
  for good, also after the shop moves to another wallet (SS-41): the
  desk's addresses count among those handed out. A request made after the
  move uses the new wallet.

## 2. What a request is

- **RQ-05.** A request has: a number, the note, the amount, how it is
  priced, the date it was made, the due date, the address, and, if it
  repeats, how often (RQ-24). The number counts up from 1 in the order
  requests are made and is never reused; it is shown as "No. 12" and is
  what a customer can quote back.
- **RQ-06.** The note is plain text of 1 to 64 characters with no control
  character (the check of CR-206), and is required: a request with no name
  on it cannot be told apart later. It stays on the owner phone and in the
  books. It is never put in the link or the code.
- **RQ-07.** A request is priced one of two ways, the owner's choice when it
  is made, and the way never changes afterwards.
  - **In bitcoin.** The amount is in satoshis and is what is owed, whatever
    the price does. A shop in Bitcoin Standard mode has only this way.
  - **In the shop's currency.** The amount is in the currency of the shop's
    settings (CR-193) and is what is owed. The satoshis asked for are worked
    out each time the request is shared (RQ-12).
- **RQ-08.** The amount in satoshis, or worked out from the currency, is at
  least the shop's smallest bitcoin sale (CR-75) and above the dust line
  (CR-17). A smaller one is refused with nothing written. The currency
  amount is at most 1,000,000 units of the currency, and the satoshi amount
  at most 21,000,000 bitcoin; a larger one is refused.
- **RQ-09.** The due date is a day in the shop's time zone, from the day the
  request is made to 366 days later. It is a date to show, not an end: the
  request stays open after it (RQ-17).
- **RQ-10.** A request is paid at the confirmation depth of the shop's
  settings, and at no fewer than one confirmation. It is never accepted at
  once and never paid over Lightning: the instant limits and the exposure
  ledger have no part in it (as CR-208). The owner phone learns of payments
  from checked blocks only (OC-01, OC-02).
- **RQ-11.** At most 1,000 requests are open at once. Making one more is
  refused, with the words of RQ-36, until one is paid or cancelled.

## 3. Sharing a request

- **RQ-12.** "Share" makes a quote. For a request in bitcoin the quote is
  the satoshis still owed (RQ-15). For one in the currency it is the
  currency still owed at the rate of the moment, from the shop's rate
  sources under the rules of a sale's rate (RT rules, CR-01 to CR-05): a
  stale rate makes no quote, and the share is refused with the words of a
  stale rate. A quote in the currency holds for 24 hours. The quote is
  written to the request (at most 64 per request; the oldest goes first)
  before anything is shared.
- **RQ-13.** What is shared is a BIP 321 link with the request's address,
  the quote's amount in bitcoin and the shop's name as the label, written
  as CR-83 writes a sale's, and nothing else: no note, no number, no
  message. "Share the link" gives the link to Android's share sheet as
  text, with one line above it from the words ("{shop} asks for {amount}.
  Request No. {n}, due {date}."). "Share the code" gives the code as a PNG,
  made and revoked as CR-214's images are. The owner phone sends nothing
  itself.
- **RQ-14.** Sharing again makes a fresh quote of what is still owed. An
  older link of the same request still pays the same address, and whatever
  arrives is counted (RQ-15); the older amount is simply no longer asked.

## 4. Matching what arrives

- **RQ-15.** Every output paid to a request's address in a checked block is
  a payment to that request, in the order of the blocks. An output of zero
  satoshis is not a payment and is not recorded. Each payment is
  credited as follows, and what is still owed is the amount less every
  credit, never below zero.
  - **In bitcoin:** its satoshis.
  - **In the currency:** its satoshis at the rate of the newest quote shared
    before the payment's block time. If that quote had run out at the
    block's time, or there was none, it is credited at the newest quote
    anyway and the request is marked "on an old price" (RQ-18), so the
    owner sees it and decides.
- **RQ-16.** A request is Waiting until something arrives, then Part paid
  while something is still owed, Paid when nothing is, and Overpaid when
  more arrived than was owed, by the excess. In the currency, an excess
  under the smallest bitcoin sale counts as Paid and not as Overpaid, and a
  shortfall under it shows "Almost paid: {amount} short" with the owner's
  choice "Count as paid", which is recorded and cannot be undone.
- **RQ-17.** A payment whose block is dated after the end of the due day,
  in the shop's time zone, marks the request "Paid late". It is counted as
  any other payment.
- **RQ-18.** A request in the currency credited at a quote that had run out
  is marked "On an old price". The owner can leave it, or share again to
  ask for what the rate of now says is still owed. Nothing is decided
  without the owner.
- **RQ-19.** A request that is cancelled stays in the book and its address
  stays watched. Anything that arrives after the cancel is listed as "Paid
  after it was cancelled" and is offered for return (RQ-26). A request that
  has received anything cannot be cancelled; it can be closed as it stands
  (RQ-20).
- **RQ-20.** "Close as it is" ends a Part paid request: what arrived stays
  in the books, nothing more is asked, and it leaves the list of open
  requests. A later payment to it is listed as "Paid after it was closed"
  and offered for return.
- **RQ-37. A block that leaves the chain.** When the shop's checked chain
  drops blocks, every payment recorded from a dropped block is taken back,
  and each request's state is worked out again from what remains (RQ-16).
  A payment in a block of the new chain is credited as any other. Where the
  returns of a request then come to more than it received, its queued
  returns are taken off the queue, newest first, until they no longer do,
  and so is a return that goes back to the queue later because its spend
  was replaced (RQ-29); a return already in a spend or done stays, and the request shows that
  more was returned than it now received, for the owner to decide. Nothing
  is paid or cancelled without the owner.

## 5. Due, overdue and asking again

- **RQ-21.** The list of requests has three parts, in this order: "Overdue"
  (past the due day and not paid), "Due soon" (due within 3 days), then
  "Waiting", each newest due date last. A request shows its note, its
  number, what is still owed and its due date.
- **RQ-22.** The owner app sets no alarm and starts no job (OA-89), so it
  does not remind anyone by itself. When it is opened, Home shows how many
  requests are overdue, and each request has "Send again", which makes a
  fresh quote (RQ-14) and opens the share sheet with the overdue line of
  the words above the link.
- **RQ-23.** The background watcher (OA-80 to OA-89) holds no request and
  learns no payment to one: it signs heartbeats only. A request's payment is
  counted when the owner app next follows the chain.

## 6. Repeats

- **RQ-24.** A request may repeat every week, every month or every year,
  from its due date. A month that has no such day uses its last day.
- **RQ-25.** When the owner app is opened on or after a repeat's next due
  date less the same lead as the first request had (its due date less the
  day it was made, at most 31 days), it makes the next request: a new
  number, a new address (RQ-03), the same note, amount and pricing, and the
  next due date. It is listed under "Ready to send" until it is shared once.
  Requests missed while the app was closed are all made, oldest first, at
  most 12 at a time. "Stop repeating" on any of them stops the series; the
  requests already made stay.

## 7. Returning money

- **RQ-26.** Money can be returned from any request that received
  something: an overpayment, a payment after a cancel or a close, or part
  or all of a payment the owner chooses to give back. The amount returned
  from a request is at most what it received less what was already returned
  from it, and is chosen in satoshis or in the currency at the rate of now.
- **RQ-27.** The customer's address is asked for and not guessed: the
  address a payment came from is never used, because it can belong to an
  exchange or another person. "Ask for an address" opens the share sheet
  with a line from the words ("{shop} would like to return {amount} for
  request No. {n}. Please reply with a bitcoin address of yours."). The
  owner then pastes or scans what the customer sent.
- **RQ-28.** What is pasted or scanned is a mainnet address of any standard
  kind (P2WPKH, P2WSH, P2TR, P2SH, P2PKH), or a BIP 321 link with one; an
  amount in the link is ignored. It is refused if it is not valid, is for
  another network, or is an address of the shop's own wallet. The accepted
  address is shown in groups of four with the amount and the request, for
  the owner to compare with the customer by another way if possible: a
  person who can change the messages between the shop and the customer can
  change the address.
- **RQ-29.** Each accepted return is queued, not paid at once. "Pay the
  returns" builds one spend with every queued return as a payment of its
  own (SP rules, `payments`), at the fee rate the owner chooses (FE rules),
  for the shop's wallet to sign (SP-80 to SP-87 for a wallet with one key).
  A return is marked done when the spend that pays it is confirmed, and goes
  back to the queue if the spend is replaced without it (OC-03). The books
  record each return against its request. A return is accepted only while
  fewer than 99 returns are queued or in a spend not yet confirmed, so a
  spend replaced without them never brings the queue above 99. A return
  whose payment the owner cancels in the queue ends as cancelled in the
  book: it no longer counts toward what was returned, and the money it would
  have paid can be returned again.

## 8. The request book

- **RQ-30.** The requests, their quotes, payments, returns and the desk's
  next index are kept in one file, the request book, magic `OBM=`, sealed
  with the owner app's storage key as its other files are, and written by
  atomic steps (new file, sync, rename, sync of the folder). The read is
  strict: another magic or version, a count over its limit, a field out of
  range or bytes left over is refused, and what decodes encodes to the same
  bytes. The read also refuses a book that contradicts itself: a quote whose
  satoshis are not what its amount and rate give, a credit of zero
  satoshis, more than 99 returns queued or in a spend, and a repeat whose
  series does not start with a first request of the same cadence, first
  due day and lead.
- **RQ-31.** A book that cannot be read is not replaced: the owner app
  shows the words of RQ-36 and makes no new request until it can be read,
  so an index is never handed out twice.

## 9. The books

- **RQ-32.** Each request is a line in the full ledger (BK rules): number,
  note, made, due, priced in, amount, each payment with its block time,
  satoshis and credit, the marks of RQ-16 to RQ-20, and each return. The
  end-of-day totals count a payment to a request on the day of its block.

## 10. Hostile input

- **RQ-33.** A pasted or scanned return address of any length or content
  never panics and is refused unless it meets RQ-28.
- **RQ-34.** A request book of any bytes never panics and is refused unless
  it meets RQ-30. A fuzz target feeds both.

## 11. Screens and words

- **RQ-35.** The owner app has "Request a payment" on Home, the list of
  RQ-21, the page of one request, the form of RQ-05 to RQ-09, and the
  return steps of RQ-26 to RQ-29.
- **RQ-36.** Every refusal and every screen has words from
  `docs/screens/owner-app.md`; until the words are written a placeholder
  constant `WORDS_NEEDED_RQ_xx` names the rule.

## 12. What these rules do not do

- They do not charge a customer later or take money without the customer
  paying. Bitcoin has no pull payments; a customer always pays.
- They do not send messages, emails or reminders. The owner shares; the
  app shows what is due.
- They do not take a payment over Lightning. A swap provider's invoice
  lasts minutes, not days.
- They do not check who sent a payment. Anyone who has the link can pay it.
