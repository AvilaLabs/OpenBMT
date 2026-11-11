# Books rules: from verified records to the merchant's books and exports

Status: specification for `crates/openbmt-books`, written before the code.
Each rule has an ID so that a test can name the rule it proves. The rules
serve `docs/DESIGN.md` §2.4 (refunds), §2.5 (end of day), §2.7 (the books
never count unfinished money), §10 (books and tax) and §11 (cash-out).

The library takes the record chains the owner already holds, checks them,
reads the counter's events out of them and builds the books: one line per
sale, the outflows the caller adds, the totals for the end-of-day screen,
two CSV exports and an encrypted export file that the library can read back.

It has no network and no clock. It reads no file and writes one only through
a single helper that writes a file whole (BK-29). The same inputs always
give the same books and the same CSV, byte for byte. The caller supplies the
time, the shop's UTC offset, the randomness and the passphrase.

**The export is not tax advice, and it covers US rules only** (DESIGN §10).
Nothing here says what a merchant owes. It writes down what the records say.

## 1. What is read

- **BK-01.** Every chain is checked with `Chain::verify` against the signed
  device list before anything is read. The counter named for the chain must
  be on that list, and a counter is given once. A chain that fails, or a
  counter that is not on the list, refuses the whole build, and the error
  names the counter and, for a bad chain, the record.
- **BK-02.** Only records written by the counter itself are read, and of
  those only the kinds that hold sale events: sale, payment, override and
  conflict, refund. A check-in, a setting or a pairing record holds no money
  and is passed over. The refund kind holds three events (COUNTER-RULES
  CR-60 to CR-71): a request, a withdrawal and a refund paid in cash. A
  request and a withdrawal are asks to the owner, not money: they are read,
  checked as every event is, and are not a disposal. A refund that was paid
  is a disposal the caller gives (BK-12); the counter does not learn of it.
- **BK-03.** Each event is decoded with the counter's own decoder
  (`openbmt_counter::Event`). A body that does not decode, an event tag that
  is not known and an event whose kind is not the record's kind refuse the
  build, naming the counter and the record. Nothing is skipped.
- **BK-04.** A sale is opened once, before any other event for it. An event
  for a sale that was never opened, or a sale opened twice, refuses the build.
  A time that is not a date from 1970 to the end of the year 9999 refuses the
  build.

## 2. One sale

- **BK-05.** The events of a sale are folded in the order of the chain. The
  state is the one the last event gives: open, expired, cancelled, paid at
  once (accepted before a block), confirmed (the shop's depth), final (the
  refund depth), late payment, replaced or reopened. A late payment gives
  its state only to a sale that had not reached a payment of its own. A
  sale that was paid at once and later confirmed shows its confirmation and
  keeps the amount that was put at risk. A sale replaced by a conflict is
  no longer counted as paid until a new payment event is written.
- **BK-06.** The fields of a sale are what the records carry: the sale's
  UTC opening time, the amount and currency, the satoshis and the rate of
  the first quote and where that rate came from, the satoshis, the rate and
  the rate source of the last quote (the first quote's when the sale was
  never requoted), the time funds were first recorded, the satoshis
  received (BK-09), the late payments with their transaction IDs, any
  surplus owed and dust, any override with the person's role and name, and
  what the POS was told. Once the sale is final, the final record adds the
  transactions that paid it with what each paid, the earliest time one of
  them was first seen, and what the satoshis received were worth in the
  shop's currency by the sale's own quotes: the value at first seen. A
  field the records do not carry is never made up. It is an explicit
  unknown that says why it is unknown and what would supply it: the sales
  tax (unless BK-10 supplies it), the address, and, for a sale that is not
  final, the value at first seen and the transaction IDs of a payment that
  was not late ("the sale is not final; the final record names its
  transactions"). A conflict that replaces the payment makes the final
  record's fields unknown again until a new final record is written.
  Overpaid is read from a surplus owed. Underpaid is read as received below
  the satoshis of the last quote. That is a reading of the records and not
  the counter's own test: a quote that the counter wrote down and the sale
  never took (CR-59) counts as the last quote, and a part paid before a
  requote is valued by the counter at the rate of its own quote and here
  is not.
- **BK-07.** The opaque sale ID is the first 8 bytes of
  `tagged_hash("OpenBMT/books-sale/v1", counter device ID || index LE)`,
  shown as 13 Crockford base32 characters in groups of 4, 4 and 5
  (`ABCD-EFGH-JKMNP`); the 65th bit is zero. It hides the transaction ID,
  the address and the amount in satoshis: none of them is in it or can be
  found from it. It does not hide which sale it is. It is made only from
  public values, so it is stable for a sale, and anyone who has the counter's
  device ID and the sale index can compute it and test a guess. It is a hash
  and not a running number, so it does not show the order of sales.
- **BK-08.** Only a sale that received funds is an income line: a payment
  accepted at once, confirmed, final or late. Such a line is **counted**
  once the sale has reached final and **pending** until then (CR-31,
  DESIGN §2.7). A sale that expired, was cancelled with nothing received,
  was replaced or only has dust is in the full ledger and in nothing else.
  Dust is not an income line: it was below the dust line and is written down
  with its amount.
- **BK-09.** The satoshis received are the amount of the last confirmed or
  final event; or, for a sale with only late payments, their sum. When both
  exist, the final record settles it: it names the transactions that paid
  the sale, so a late payment among them is already in the amount and one
  that is not is added to it. If there is no final record, or it names only
  the 32 largest of more transactions (CR-32) and the late payment is not
  among them, the records do not say, and the amount is unknown. For a sale
  that was only accepted at once the amount is unknown, with the amount put
  at risk shown beside it.
- **BK-10.** Sales tax comes only from the optional `PosFacts` the caller
  passes (a tax amount in minor units for a counter's sale index, from the
  POS match). A tax amount for a sale that is not in the books refuses the
  build. Without it the sales tax is unknown.
- **BK-11.** The high-water mark of a counter is the highest sale index any
  of its events names (the sale index is the receive index the counter
  stored before it handed out the address, CR-03), or none when the counter
  has no events. It is carried in the header of the full ledger and of the
  encrypted export. The accounting CSV has no header rows, so that an
  accountant's import does not break (BK-17).

## 3. What goes out

- **BK-12.** A disposal is given by the caller: the time, the kind (refund of
  a sale, cash-out, or a network fee paid in bitcoin), the currency, the
  satoshis, the dollar value when known and the basis when known. A refund
  must name a sale that is in the books. The books keep the lines as given.
  A value or a basis that is not given is unknown and says what would supply
  it. Specific lots are not tracked.
- **BK-13.** A cash return from the till is a return line in the currency of
  its sale and moves no satoshis. It comes from the counter's record of a
  cash refund (`RefundPaidInCash`: the time, the sale and the amount in
  minor units) and from the caller, who may give more: the time, the sale and
  the amount. The sale must be in the books. The two are kept together in
  time order.
- **BK-14.** Every sum is checked. A sum that does not fit, or a list that is
  longer than the limit, is an error and never a wrapped or cut number.

## 4. The end-of-day totals

- **BK-15.** The totals are per day, once per UTC day and once per shop-local
  day for a fixed offset in minutes the caller gives. For each day: the
  number of sales that received funds, the sum of their amounts in minor
  units per currency, the sum of the satoshis that are known, the number of
  sales whose satoshis are unknown, and the number of sales still waiting
  (not final). A sale is on the day its funds were first recorded. Totals
  are of sales only. There is no wallet balance, and the outflows are not
  taken from them (§2.9).
- **BK-16.** Dates are civil dates computed from Unix seconds by the
  days-from-civil method, with no date library. An offset is from -720 to
  840 minutes. Dates are `YYYY-MM-DD`.

## 5. The two CSV exports

- **BK-17.** The accounting export has the columns `Date,Description,Amount`
  and one file per UTC calendar month. A month with no line has no file.
  A line is a counted sale (its amount), a refund, a cash-out or a fee (their
  dollar value, negative) and a cash return (negative). Pending sales are
  not in it. A disposal whose dollar value is unknown refuses the export,
  naming it, because the file does not guess a number. More than one
  currency in the lines refuses the export, because the file has no currency
  column. Lines are in time order.
- **BK-18.** The description of a line holds the opaque sale ID and fixed
  words, never a transaction ID, an address or an amount in satoshis. A line
  that is not about a sale holds fixed words only. The export has the three
  columns of BK-17 and no column for a transaction ID or an address, whatever
  the records carry.
- **BK-19.** The full ledger has every sale in every state, every disposal and
  every return, with the transaction IDs the records carry (those of the
  final record and the late payments), the rate source, the value at first
  seen, the time first seen, the satoshis, rate and source of the last
  quote, the unknowns and what each unknown needs. It can be made only by
  passing `RevealsPayments::Acknowledged`. The first line is a comment row
  with the sentence "This file reveals the shop's payments on the
  blockchain. Share it only with people you trust with that." The second is
  a comment row with the high-water mark of each counter (BK-11).
- **BK-20.** CSV is written as RFC 4180: UTF-8 without a byte order mark,
  `\r\n` after every row, a field in double quotes when it holds a comma, a
  double quote, a carriage return or a line feed, and a double quote written
  twice.
- **BK-21.** A text field that starts with `=`, `+`, `-`, `@`, a tab or a
  carriage return is written with a `'` in front, so that a spreadsheet does
  not run it. Numbers that the library produces are not text and are not
  changed. A negative amount has a leading `-`.
- **BK-22.** An amount is a decimal with the currency's minor digits (two for
  most, none for the yen, three for the dinar) and no thousands separator.
  The accounting export has dates, the ledger has `YYYY-MM-DDTHH:MM:SSZ`.

## 6. The encrypted export file

- **BK-23.** The file is `OBMY`, version 1, then the parameters, the salt, the
  time it was made, the high-water marks, and the ciphertext, which runs to
  the end of the file. Reading is strict: exact lengths, nothing left over,
  the marks sorted and without repeats.
- **BK-24.** The key is Argon2id (version 0x13) of the passphrase and the
  salt, 32 bytes, with memory 64 MiB, 3 passes and 1 lane. The parameters
  are in the header so that a later version can raise them.
- **BK-25.** The salt is 16 bytes from the caller's randomness, new for every
  file. Sealing fails if the randomness fails. The nonce of ChaCha20-
  Poly1305 is all zeros, which is sound only because the key comes from a
  fresh salt and so is never used for two files. The library never reuses
  a salt and never derives one from the content.
- **BK-26.** The ciphertext is the whole of the books as a framed bundle
  (BK-28), not the CSV, so that it can be read back. The associated data of
  the AEAD is the whole header. Opening gives the books or one error,
  "wrong passphrase or damaged file", whether the passphrase is wrong or
  any byte was changed, added or cut.
- **BK-27.** A header with memory above 256 MiB, passes above 10 or lanes
  above 4 is refused before Argon2 runs, so that a hostile file cannot keep
  the phone busy. An empty passphrase is refused when sealing and when
  opening. How long a passphrase must be is for the screen that asks.
- **BK-28.** The bundle is `OBM6`, version 1, with the counters and their
  high-water marks, the sales with every field and every unknown (including
  the rate sources, the last quote, the final record's transactions, first
  seen time and value), the disposals and the returns. Reading is strict:
  the final record's three fields are all there or all missing, and the
  transactions are in ascending order of ID, at most 32. It checks that the
  opaque IDs are the ones the keys give, the order, the references from
  refunds and returns, and the high-water marks against the sales.
- **BK-29.** The one file helper writes the bytes to a temporary name in the
  same folder, flushes it and renames it over the target, so that the target
  is either the old file or the whole new one.

## 7. Hostile input

- **BK-30.** No function of the library panics on any input. Errors are values
  that say their rule in words. A decoder is fed arbitrary bytes by the fuzz
  target `books_export_decode`.
- **BK-31.** The same books give the same CSV and the same bundle, byte for
  byte. Only the salt differs from one encrypted file to the next.

## 8. A sale paid over Lightning

The counter can take a payment over Lightning through the shop's own node
(`docs/COUNTER-RULES.md`, CR-109 to CR-111). Such a sale is final at once and
has no transaction. The record is `PaidByLightning`.

- **BK-32.** A `PaidByLightning` record makes the sale final: its state is
  final, it is counted (BK-08) from the time of the record, and its payment
  mode is Lightning, with the payment hash, the amount in millisatoshis and
  the time the node says it settled, if it says. The satoshis received (BK-09)
  are the millisatoshis rounded down; the value at first seen is the value
  the record carries; the time first seen is the time of the record; and
  the transactions the final record names are none, a known empty list and not
  an unknown. A sale with no such record has the payment mode on-chain, which
  is what every sale was before. Surplus (CR-111) and late payments are read
  as for any sale, and a late payment is added to the satoshis received (BK-09).
  The full ledger has four more columns at the end: the payment mode (`on-chain`
  or `lightning`), the payment hash, the amount in millisatoshis and the settle
  time. The accounting export is unchanged: it holds fixed words only (BK-18),
  and a Lightning sale is a counted sale in it like any other.
- **BK-33.** The bundle (BK-28) is version 2 when at least one sale was paid
  over Lightning, and then every sale ends with its payment mode: 0 for
  on-chain, or 1, the 32 bytes of the payment hash, the amount in
  millisatoshis (not zero) and the settle time as a flag and a time. A bundle
  with no such sale is written as version 1, byte for byte what earlier builds
  wrote, and version 1 is still read, with every sale on-chain. Reading is
  strict: a mode that is not known, an amount of nothing, a Lightning sale that
  is not final or is not its own payment, and a version 2 bundle with no
  Lightning sale (it would not encode back to itself) are refused.
- **BK-34.** A sale paid over Lightning is not read as underpaid against the
  last quote (BK-06). The invoice was made at the quote in force when it was
  asked for, a later quote may ask for more or for less, and the counter took
  the payment as paid in full. Overpaid is read as for any sale.

## 9. What these rules do not do

- **No tax advice, and US rules only.** The export writes down what the records
  say. Whether a sale is income, which value counts, and what a disposal
  costs or earns is for the merchant and the accountant. No gain or loss is
  worked out here.
- **No basis method.** Specific lots are not tracked, and no first-in or
  average method is applied. A basis is kept as the caller gave it, or it is
  unknown.
- **No fields the records do not carry.** The address is unknown. The dollar
  value at the time the payment was first seen and the transaction IDs of a
  normal payment are unknown until the sale is final. No stand-in is worked
  out from the quote's rate.
- **No rebuild from the chain.** The rebuild-from-chain tool of DESIGN §10 (the
  blockchain read with the wallet description from the recovery kit, using
  the high-water marks) is a separate piece of work.
- **No check of refunds against receipts.** A refund is not tested against
  what was received for the sale. That is the owner phone's assertion (§2.4).
- **No tax inside the accounting CSV.** It has three columns. Whether the
  counter's amount includes sales tax is the POS's fact, not the records'.
- **No privacy from the file's holder.** The opaque ID (BK-07) is not a secret.
  The full ledger and the header of the encrypted file show counter keys,
  and the full ledger shows transaction IDs.
- **No passphrase strength check, no key storage, no schedule.** Choosing the
  place, the time and the passphrase is for the app.
- **No clock, no network, no randomness of its own.**
