# Terminal rules: the shop's own point of sale

Status: specification, written before the code. Each rule has an ID so that a
test can name the rule it proves. Every row of the merchant capability map is
listed in `docs/terminal-map.md` with the rule that covers it, or with the
reason it is later or not part of OpenBMT.

These rules make the counter app a complete point of sale. The shop rings up
the whole sale in OpenBMT and takes the payment in OpenBMT, by bitcoin, cash
or card. It replaces the shop's register; it does not sit beside one. A shop
that serves customers at a counter (a store, a café, a market stall, a
tourist shop) can run every sale through it.

They add to `docs/COUNTER-RULES.md` and change none of its rules unless a
rule here says so by number. Where a counter rule already does the job, the
rule here names it and adds only what is missing.

## 0. What the terminal is

- **TM-01. The devices.** An Android phone or tablet at the counter, running
  the counter app, and the owner's own Android phone, running the owner app.
  Nothing else is needed to sell. A receipt printer, a cash drawer, a barcode
  scanner, a card reader and a hardware wallet are optional, and every rule
  here works without them unless it is about them.
- **TM-02. No server of ours.** OpenBMT runs no server and the shop needs no
  account with OpenBMT. The shop's records live on the shop's devices. When
  the shop uses an outside service (a card processor, an accounting program),
  the shop's own device talks to it with the shop's own sign-in, and nothing
  passes through OpenBMT.
- **TM-03. No Box needed.** No rule here requires a Box. A shop that runs one
  may use it where a rule says so; a shop without one loses nothing that a
  rule here promises.
- **TM-04. One record of the shop.** Every sale, payment, refund, drawer
  movement, stock change and staff action is a record in the counter's
  record log (CR-40 order of writing), in the Rust library, so that the owner
  app and the books read the same facts. There is no second ledger in the
  app. A record is written before the screen says it happened.
- **TM-05. Exact money.** Amounts are whole numbers of the minor unit of the
  shop's currency (CR-193) or of satoshis. No amount is ever a floating-point
  number. Every rounding is named by a rule.
- **TM-06. Who did it.** Every record that a person causes names the person
  (a cashier, a manager, or the owner) and the counter. A name typed into a
  field is never taken as who someone is; only a signed-in PIN or the owner's
  signature is.
- **TM-07. Selling is not spending.** Nothing a cashier or a manager can do at
  the counter moves the shop's bitcoin, changes where it is paid or changes
  how it is signed. That stays with the owner's keys (§14).

## 1. Items

- **TM-10. An item.** An item has a name (1 to 48 characters), a price in the
  shop's currency or "price at sale" (TM-24), an optional category, optional
  barcodes, a tax group (TM-40), and a flag for whether its stock is counted
  (§11). The counter keeps up to 5,000 items.
- **TM-11. Categories.** Items sit in categories the shop names, up to two
  levels deep, and an item may be shown in more than one category. The
  counter's sale screen opens on the categories.
- **TM-12. Variants.** An item may have variants (a size, a colour), up to
  three choices of up to 20 values each. Each variant has its own price,
  barcode and stock.
- **TM-13. Modifiers.** An item may have modifier groups (milk, extra shot,
  no onion). A group is required or optional, has a least and a most number
  of choices, and each choice has a price, which may be zero. A group can be
  used by many items.
- **TM-14. Barcodes.** An item or variant has up to five barcodes. EAN-13,
  EAN-8, UPC-A, UPC-E and Code 128 are read. A scan that matches one item adds
  it; one that matches none says so and offers to make an item with that
  code (manager or owner only).
- **TM-15. Finding an item.** The cashier finds an item by category, by
  typing part of its name or code, or by scanning (TM-110, TM-111).
- **TM-16. Editing items.** A manager at the counter or the owner on the
  owner phone makes, edits and archives items. Every change is a record
  (TM-04). An item that has been sold is archived, never deleted, so that old
  sales still name it. A change reaches the shop's other counters through the
  owner's copy of the records (`docs/LINK-RULES.md`) and is shown with who
  made it.
- **TM-17. A list from a spreadsheet.** The owner imports items from a CSV
  file and exports them to one, with the columns name, category, variant,
  price, tax group, barcode, counted and on hand.
  An import shows every row it would add or change and every row it refuses,
  with the reason, before anything is written, and writes all or none.

## 2. The sale

- **TM-20. Ringing up.** The cashier adds items, chooses variants and
  modifiers when an item has them, changes quantities, removes lines and
  clears the sale. The sale screen always shows each line, the discounts,
  the tax, the tip and the total.
- **TM-21. Same item twice.** By the shop's setting, the same item with the
  same choices is either one line with a count, or a new line each time.
- **TM-22. A note.** A line or a sale may carry a note of up to 140
  characters. A sale's note is CR-212's.
- **TM-23. A price change for one sale.** A cashier with the permission
  (§10) may change a line's price for this sale only, with a reason. The
  receipt and the records keep the item's own price beside it.
- **TM-24. An amount typed in.** The keypad takes an amount for an open item
  ("price at sale") or a sale with no items at all.
- **TM-25. Parked sales.** A sale may be parked and picked up again at the
  same counter, by anyone signed in. A parked sale has no payment on it. Up
  to 20 are kept; the oldest the shop allows (by default a day) is cleared
  with a record.
- **TM-26. The sale is frozen before payment.** When the first payment of a
  sale starts, its lines, prices, discounts, tax and total are written as
  one record and never change. A change after that is a refund or a new
  sale (§7).
- **TM-27. The customer's screen.** The customer sees the lines, the total
  and the payment code on a second display joined to the counter (a screen
  on HDMI or USB-C, or a cast screen), or on the counter's own screen turned
  around. What the customer's screen shows is written in
  `docs/screens/counter.md`.
- **TM-28. Kitchen tickets.** For counter service with food, the shop may
  name a printer as a kitchen printer and choose which categories go to it.
  When a sale is paid, its lines in those categories print there with their
  modifiers and notes. Kitchen screens and table service are not part of
  these rules (§19).

## 3. Discounts, fees and tips

- **TM-30. A discount by hand.** A cashier with the permission takes an
  amount or a percent off a line or the whole sale, with a reason when the
  shop asks for one.
- **TM-31. Discounts the shop sets.** The owner or a manager sets named
  discounts: an amount or a percent, on the sale or on some items or
  categories, with optional limits (a least and most spend, a largest amount
  off, items that must be in the sale, dates, days and hours). A discount the
  shop marks automatic is applied when its limits are met; others are chosen
  by the cashier.
- **TM-32. Codes.** A discount may need a code, typed or scanned, with dates
  and an optional single use per code.
- **TM-33. Several discounts.** The shop chooses whether discounts add up or
  only the largest applies. Items marked "never discounted" take none.
- **TM-34. Who may.** The shop chooses which roles may use each discount and
  whether a reason is asked. A discount's button has the name and colour the
  shop gives it. A discount is turned off, on, archived or restored, never
  deleted once used.
- **TM-35. Fees and a service charge.** The shop may set fees (an amount or a
  percent) that the cashier adds or removes, and a service charge added
  always or by hand. Each says whether it is taxed.
- **TM-36. Tips.** Tips are CR-187 to CR-192, with these additions: a tip is
  chosen before payment, on the customer's screen when there is one; the
  owner chooses whether the percent is of the amount before or after tax; a
  cash or card part of the sale may carry its own tip. There is no changing a
  tip after a bitcoin payment; a card tip is the processor's (§6).
- **TM-37. The order of the sum.** Line prices with modifiers, then line
  discounts, then sale discounts, then fees and the service charge, then tax
  (TM-41), then the tip. Each step is rounded to the minor unit half away
  from zero, once, at that step.

## 4. Tax

- **TM-40. Tax groups.** The owner sets tax rates (a percent to four decimal
  places, or a fixed amount per item) and groups them; each item or modifier
  has one group. Up to 8 rates per sale.
- **TM-41. Added or included.** The shop chooses whether prices include tax
  or tax is added. Tax is worked out per rate on the sale's taxed total after
  discounts, rounded half away from zero to the minor unit. An included tax
  is shown as the part of the price that is tax.
- **TM-42. Exempt.** A cashier with the permission removes tax from a line or
  a sale with a reason, or marks the customer exempt (TM-120). The receipt
  says so.
- **TM-43. Cash rounding.** Where coins are not small enough, the owner sets
  a step (for example 0.05) for cash payments; the cash total is rounded to it
  and the difference is a record of its own.

## 5. Taking payment

- **TM-50. Ways to pay.** Lightning, on-chain bitcoin, cash, card (§6), and
  other ways the owner names (a cheque, a voucher), which are recorded only.
  The owner chooses which are shown and in what order. Bitcoin is shown first
  unless the owner chooses otherwise.
- **TM-51. Bitcoin.** A bitcoin payment is a sale of the counter rules, with
  all of them: the code of CR §2 and §12, instant acceptance within the
  owner's limits (CR §1), late, short and over payments (CR-12, CR §8), and
  Lightning first when it is on. The terminal hands it the amount; it adds
  nothing to how a bitcoin payment is judged.
- **TM-52. Cash.** The cashier types what the customer handed over, or taps
  one of the notes the counter suggests. The counter shows the change. A cash
  payment is recorded with what was handed over, what was taken and the
  change, against the open drawer (§9). A cash payment cannot be taken when
  no drawer is open on that counter.
- **TM-53. Paying in parts.** A sale may be paid in several parts by several
  ways. Each part keeps its own state and its own refund limit. The sale is
  paid when the parts reach its total; until then it shows what is still
  owed. A bitcoin part is a bill part of CR-246; a cash, card or other part is
  a part of this section.
- **TM-54. Paid later.** A sale with parts still owed may be parked (TM-25)
  with its paid parts kept. It is not paid until the rest is.
- **TM-55. A part cannot be paid twice.** A part that is being paid blocks
  other parts of the same amount until it ends. A part whose outcome is not
  known (a bitcoin payment not yet seen, a card result lost) stays unknown and
  holds its amount: the counter never invites a second payment of it until it
  is settled (TM-63, CR §8).
- **TM-56. The card fee line.** When the owner turns it on and enters their
  card rate (a percent and a fixed amount), the sale screen shows the card fee
  the shop would pay on this sale. When the customer pays in bitcoin, the
  same line is shown struck through. The owner chooses either to show it only,
  or to take that amount off for bitcoin as a discount (TM-31). It is never
  added to a card payment: that is a surcharge, which card networks and some
  places restrict.
- **TM-57. Offline.** Without internet, the counter takes cash and other
  recorded ways, and card when the shop's processor allows offline card
  payments (TM-66). It does not take bitcoin, because it cannot see the
  payment arrive: it says so on the bitcoin button, and the sale stays open
  for another way to pay. It keeps ringing up, parking, printing and
  recording. When the internet returns it says so and bitcoin is offered
  again.

## 6. Cards

Card payments go through the shop's own card processor. The card network's
steps (holding an amount, taking it later, voids, adjusting a tip, disputes)
live inside this section and touch nothing else.

- **TM-60. One way in.** The counter talks to every processor through one
  card module boundary in the app. Each processor is a separate module built
  only into the builds that include it; the plain build has no card code at
  all. The module is told an amount, a currency and a sale part, and answers
  with one of: paid, declined, cancelled, unknown.
- **TM-61. Saved before it is asked.** Before a card payment starts, the part
  and a reference of its own are written. The processor is given that
  reference. A crash at any moment leaves a part that the counter can match
  against the processor.
- **TM-62. Paid means the processor said so.** A card part is paid only when
  the processor's answer says completed for that reference. A declined part
  may be tried again on the same part.
- **TM-63. Unknown is never declined.** A lost answer, a timeout, a crash or a
  reader that went away makes the part unknown. The counter asks the processor
  about that reference before anything else; until the answer comes, the
  part's amount stays held and the cashier is told plainly not to take the
  card again.
- **TM-64. The shop's key on the counter.** Where a processor needs a secret
  to run, the owner makes a limited key in the processor's own dashboard that
  can take payments and refund them to the card they came from, and cannot
  pay out or move money anywhere else, and scans it into the counter. It is
  sealed with the Android keystore (as the PINs are) and never leaves the
  counter. The setup screen says what it can do and how to cancel
  it in the dashboard. A shop with a Box may keep it on the Box instead.
  **[verify]** for each processor that such a limited key exists.
- **TM-65. Refunds by card.** A card refund is asked of the processor against
  the original payment, by a manager or the owner, and follows TM-62 and
  TM-63 in reverse.
- **TM-66. Offline cards.** Where the processor offers offline payments and
  the owner turns them on, the counter shows the processor's limit and keeps
  each offline card part as unknown until the processor confirms it.
- **TM-67. Processors.** Modules in this order: Square, Stripe Terminal,
  Adyen, Mollie. Others are added as each allows outside apps, each with what
  it needs written beside its module.
- **TM-68. No card data.** The counter never sees, stores or logs a card
  number; it keeps only the processor's payment ID, the brand and the last
  four digits the processor returns.

## 7. Receipts, past sales and refunds

- **TM-70. What a receipt says.** The shop's name and the header and footer
  it sets (a return policy, for example), the date, the counter, who served,
  each line with its choices, discounts, fees, tax by rate, tip, total, each
  part and how it was paid, change, and for bitcoin the amount in sats and
  the rate used. A refund, an exchange or a reprint says so.
- **TM-71. Three ways.** Printed (TM-160), shown as a code the customer's phone
  camera reads as plain text, or sent from the counter's own share sheet
  (email or messages through the shop's own apps). The customer chooses, or
  none. Printing can be set to always.
- **TM-72. Gift receipt.** A receipt without prices.
- **TM-73. Finding a sale.** By its receipt number, by scanning the receipt's
  code, by date, by item, by amount, by who served, by how it was paid and by
  state. A sale's note and customer may be changed after it is paid, as a
  record.
- **TM-74. Reprint.** Any past sale, refund or exchange may be printed or
  shown again.
- **TM-75. A refund.** A refund is of whole lines, some of a line's quantity,
  or an amount, against a paid sale, with the tax of those lines. It is
  allowed only to someone with the permission and only up to what the sale's
  parts can still give back. Each part refunds its own way: bitcoin by the
  refund rules of the counter (CR §7: at the dollar price or the bitcoin
  amount, the shop's setting, to an address or invoice the customer gives and
  the counter checks; a Lightning sale back in cash or on-chain, CR-111);
  cash from the drawer; card through the processor (TM-65).
- **TM-76. A return policy.** The owner may set a time limit for returns. Past
  it, a manager may still refund with a reason.
- **TM-77. An exchange.** Returned lines and new lines in one step: the
  difference is paid or refunded.
- **TM-78. A return without its sale.** A manager may take back an item with
  no sale found, only as store credit (TM-80), with a reason.
- **TM-79. What refunds cannot do.** A refund once given is not undone. A
  card processor's fees are not returned by a refund. The screen says both
  before the refund is made.

## 8. Store credit

- **TM-80. Credit.** The shop may give a customer credit instead of a refund,
  or for a reason a manager names. A credit has an amount in the shop's
  currency and a code printed or shown as on a receipt. It is the shop's own
  promise; no bitcoin moves.
- **TM-81. Paying with credit.** Credit is a way to pay (TM-50) up to what is
  left on it. What is left is shown and printed.
- **TM-82. Where it is kept.** A credit is kept on the counter that gave it
  and in the owner's copy. In a shop with several counters, the owner's copy
  tells the others; until a counter has heard, it accepts only credit it gave
  itself, and says so.

## 9. The drawer and the day

- **TM-90. A drawer.** Each counter has at most one drawer open. It is opened
  with the cash in it at the start, counted by the person opening it.
- **TM-91. Cash in and out.** With a reason: cash added, cash paid out for
  something, cash taken to the safe, cash paid out as tips. Each is a record
  that changes the cash expected.
- **TM-92. Opening without a sale.** With a reason, by someone with the
  permission. It changes nothing expected.
- **TM-93. Closing.** The person closing counts the cash. The counter shows
  what was expected, what was counted and the difference, and writes them.
  After close nothing can be added to that drawer.
- **TM-94. One person's drawer.** The shop may give a drawer to one person
  for their shift; only they take cash into it.
- **TM-95. The bank.** The owner may record cash taken to the bank, with the
  amount, against closed drawers, and see a difference from what was
  expected.
- **TM-96. The end of the day.** Closing the day shows, in this order: open
  sales and parked sales left, open drawers to close, card parts still
  unknown, bitcoin payments still waiting; then the day's report (TM-130).
  The day can be closed with items left, which the report lists.
- **TM-97. A shift.** A cashier's shift is their signing in and out (CR §14).
  The shop may ask for a shift report before signing out: their sales, their
  cash, their tips, their refunds.
- **TM-98. The drawer's history.** A manager or the owner reads every drawer,
  open or closed, with every movement and who made it.

## 10. Staff

- **TM-100. Three roles.** Cashier, manager and owner. Cashiers and managers
  sign in at the counter by name and PIN (CR-140 to CR-150, CR-84, CR-85);
  the owner acts through the owner phone.
- **TM-101. Permissions.** The owner chooses, for cashiers and for managers,
  each of: price change for one sale, discounts by hand, tax removal, refunds,
  returns without a sale, store credit, opening the drawer without a sale,
  cash in and out, editing items, stock changes, seeing reports, closing the
  day. A screen shows what each role can do.
- **TM-102. Spending is not a permission.** No role at the counter can be
  given TM-07's actions; there is no switch for them.
- **TM-103. The log.** Every price change, discount, tax removal, refund,
  store credit, drawer opening, cash movement, stock change and item change is
  in a log the owner reads, with who, which counter and when.

## 11. Stock

- **TM-110.** An item or variant whose stock is counted has a number on hand.
  A paid sale takes its quantities off once; a refund puts them back if the
  person refunding says the goods came back.
- **TM-111.** Stock is changed by hand with a reason, set to a number, or
  marked out of stock. An item out of stock is shown so at the sale screen.
- **TM-112.** A count: of everything or of chosen items. A partial count
  changes only what was counted. A count can be left and finished later,
  including after a restart, and shows the differences before it is applied.
- **TM-113.** The owner exports stock and count differences as CSV.

## 12. Customers

- **TM-120.** A customer record has a name and any of a phone, an email and a
  note, and a tax-exempt flag. It is kept on the shop's devices only.
- **TM-121.** A customer is added at the counter or found and put on a sale.
  Their past sales and credit are shown.
- **TM-122.** The owner edits or deletes a customer, and exports the list.
  Deleting removes the name and contact from the customer record; the sales
  keep their amounts.
- **TM-123.** The shop can ask for consent before keeping contact details,
  with its own words, and keeps the answer with the record.

## 13. Reports and the books

- **TM-130. The day's report.** Sales, refunds, discounts, tax by rate, tips,
  fees, each way to pay, cash expected and counted, store credit given and
  used, and bitcoin received in sats with its value at the time of each sale.
- **TM-131. Other reports.** By staff, by item, by category, by hour, for any
  span of dates, on the counter (with the permission) and on the owner phone.
- **TM-132. Exports.** CSV of sales, lines, payments, refunds and drawers, for
  any span, from the owner phone, and the books export of
  `docs/BOOKS-RULES.md`. Layouts that QuickBooks and Xero import are added
  **[verify]** against each program's own import documentation.
- **TM-133. Bitcoin in the books.** Each bitcoin part carries the rate, its
  source and its time, as the books already record it.

## 14. The shop's money

- **TM-140. Three levels.** In setup and in the owner app the owner sees:
  sell today with the starter wallet on the owner phone, with a limit the
  owner sets (`docs/START-SIMPLE-RULES.md`); savings on a hardware wallet,
  paired with its public information only and checked on its own screen
  (`docs/WALLET-RULES.md`); savings on a spare phone kept offline with the
  Key app, signing by QR. The third is offered only after it passes the
  purist review; until then it is not shown. An offline laptop is not on the
  main path.
- **TM-141. Moving and spending.** The owner moves funds and spends from the
  owner phone, signed by the shop's keys (`docs/SPEND-RULES.md`), with the fee
  and the destination checked on the signing device.
- **TM-142. Small coins.** The cost of spending small coins later is shown,
  and combining them follows D9 of `docs/DESIGN.md`.
- **TM-143. A lost device.** A lost counter is replaced by pairing a new one;
  no money was on it. A lost owner phone is recovered from the shop's
  wallet words or hardware wallet, without asking OpenBMT; the records are
  recovered from the counters and the owner's copy.

## 15. Lightning without a Box

This section changes SL-73.

- **TM-150. The second copy.** Shop Lightning's second copy (SL-20) is kept on
  the owner phone, sealed for it, sent over the shop's link through any relay
  (which sees only ciphertext). A shop with a Box may keep it on the Box, as
  SL-73 says now.
- **TM-151. The copy may lag.** The counter does not wait for the owner phone
  to take each update. It keeps the age of the last copy taken, and the owner
  app shows it. The owner sets a limit: when the last copy is older than it,
  the counter stops taking Lightning and offers on-chain, and says why.
- **TM-152. Restoring an old copy.** A node restored from a copy that may be
  old never broadcasts its own channel state; it asks each peer to close the
  channel, and the funds come back on-chain. **[verify]** that the Lightning
  library handles a stale restore this way.
- **TM-153. Who else is involved.** Setup says plainly that a channel provider
  opens the shop's channels and pays for them as its terms say, never holds
  the shop's money, and that the shop can close a channel without it.

## 16. Hardware

- **TM-160. Printers.** Receipt and kitchen printers that take ESC/POS, over
  Bluetooth, USB or the local network. The counter tests a printer with a
  sample page when it is added.
- **TM-161. A cash drawer.** Joined to the receipt printer and opened by it,
  when a cash part is paid and by TM-92.
- **TM-162. Scanners.** A Bluetooth or USB scanner that types like a keyboard,
  and the counter's own camera. If the scanner is lost or the camera fails,
  the counter says so and typing the code still works.
- **TM-163. Card readers.** The readers and tap-on-phone that the shop's
  processor supports, through its module (TM-60). Pairing and readiness are
  shown on the counter.
- **TM-164. A status screen.** One screen shows the network, the Bitcoin check
  (CR §1), Lightning, the printer, the scanner, the card reader, the owner
  link, the last Lightning copy, and the version.

## 17. Privacy

- **TM-170.** The shop's data stays on the shop's devices (TM-02). The shop
  may show its privacy policy as a code at the counter.
- **TM-171.** A customer may ask for their data; the owner exports it or
  deletes it (TM-122).
- **TM-172.** The payment code shows the customer only what they need to pay:
  the amount and where to pay, never other sales or the shop's wallet.

## 18. Setup

- **TM-180.** First run asks for the shop's name, its currency, its kind of
  place (`docs/GIVING-RULES.md`), its tax, its ways to pay, and then
  the wallet (TM-140). A shop can sell in cash before the wallet is set, and
  bitcoin is offered as soon as it is.

## 19. What these rules do not do

- **Integration with another register.** Square, Clover, Lightspeed and the
  rest are not part of these rules.
- **iPhone and iPad.** Android only for now.
- **Table service.** Seats, checks split by seat, moving tables, courses and
  kitchen screens.
- **Selling online.** An online store sharing items and stock, online orders,
  pickup, shipping and delivery.
- **Several locations.**
- **Gift cards, loyalty, subscriptions and marketing.**
- **A live link to outside programs.** The local API, webhooks and a live
  sync to accounting programs.
- **Payroll, banking and loans.** Not part of OpenBMT.
