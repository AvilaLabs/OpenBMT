# OpenBMT design (draft 3.1, 2026-11-09, after the round-3 single-component check)

Status: design, with the first code under way. The shared rules crate
(`crates/openbmt-core`) exists and is tested; no app is built yet and nothing
has been tried on a device. Every claim about another system cites a source in
§17. **[verify]** marks a claim that still needs a
primary check or a device test before we depend on it.

Draft 3.1 applies three security reviews:

- docs/research/security-review-2026-11-09.md (round 1: findings S1–S17,
  decisions Q1–Q4);
- docs/research/security-review-round2-2026-11-09.md (round 2: findings A, W,
  U and P, each with a decided fix);
- docs/research/security-review-round3-2026-11-09.md (round 3: findings
  R3-1 to R3-10, a single-component check, each with a decided fix).

Section 12 maps the findings to where they are handled.

## 0. The standard

OpenBMT has to pass three tests at once, and every choice in this document is
checked against all three.

1. **Easy.** A merchant, a cashier or a customer who has never seen it should
   get it right the first time. If something is confusing, that is a defect.
2. **Best in class.** On every metric a merchant or customer would compare
   (setup time, taps, time to "paid", fees, refunds, books), it matches or
   beats the best product available, custodial ones included.
3. **Purist-approved.** No custody, no trusted server, honest about what is
   verified, private by default, built on open standards. **A real purist
   objection is a veto, not a trade-off** (project rule, 2026-11-09). The veto covers
   anything that lets a third party hold, move, freeze or redirect funds, or
   that gives it keys, the wallet description or addresses. Such an idea is
   dropped. It is not kept as an option, a slot or a later phase. An optional
   service that sees only metadata is allowed if all three hold: it is not the
   default; it is labeled at the point of choice; and it is listed in "Who
   sees what" (§4.1).

A fourth rule follows from them: **choice without confusion.** Where merchants
want different things (which keys, how long to wait, which node), they choose.
Every option offered must be secure and purist-acceptable on its own, so there
is no wrong answer. One option is marked as recommended. Choices are asked as
plain questions, and unsafe combinations are never offered.

A fifth rule: **every security claim in this document must be literally true.**
No overclaiming. If a claim holds only under a condition, the condition is
written next to it. If we cannot check something, the screen and the document
say "not verified" and say what was checked instead.

These are internal design reviews. No independent security audit has been done
yet. The design has been through three of them: round 1 from six perspectives
(a remote thief, insiders, a hostile customer, a Bitcoin wallet engineer, a
privacy and purist critic, and losses without a thief), round 2 from three
further perspectives (an adversary, a wallet and recovery engineer, and a
usability critic who also played the hostile purist), and round 3 as a
single-component check that asked whether any single component or person could
cause an unbounded loss. The merged findings and the decisions are in the three
files named above. The built software still needs tests, an outside code review
and an audit before anyone calls it secure.

**What we claim, restated after round 3.** Once two launch conditions hold, no
single compromised OpenBMT component, device or person can spend the wallet.
The conditions are:

1. **Key 3 is made outside OpenBMT software and away from the key 2 device**
   (§2.1).
2. **The Key app's release key is held by a second, independent person**, not
   the main app's signer (§5.4).

Sales proceeds and goods are protected up to the stated caps, and until the
owner's reconciliation catches a bad counter, build or Box (§6, §10). Until
both conditions hold, the claim does not hold, and the documents and screens
say so.

Where two tests pull apart, we look for a design that satisfies both before we
accept a trade-off. Trade-offs that remain are listed in §16.

## 1. What it is

OpenBMT (Bitcoin Merchant Terminal) lets a shop take bitcoin at the counter the
way it takes a card. The money goes straight into the merchant's own wallet.
That wallet needs two of three keys to spend, and the merchant holds all three.
The sale closes in the POS system the shop already uses.

It is three pieces of software:

- **The OpenBMT app** (Android) has two modes:
  - **Counter mode** runs on the shop's phone or tablet. It holds no keys. It
    shows the payment and confirms it. It holds signed batches of addresses,
    not the wallet description (§5.1).
  - **Owner mode** runs on the owner's own Android phone. It holds key 1,
    queues refunds and cash-outs for the approval session (§2.10), signs the
    shop's settings, and shows the books. Owner mode is Android-only in v1. An
    iPhone owner app is a research item (§15).
- **The OpenBMT Key app** is a separate, small signer for key 2. It is the
  second authority for everything that moves money, changes trust or loosens
  safety (§2.10). It asks for no network permission, has its own release key,
  and talks by QR or by a file on USB-C or SD card. It never updates itself
  (§5.4).
- **The OpenBMT Box** is the merchant's own small server: the POS connectors,
  record sync and, optionally, their own full node (§5.3). It holds no key.

## 2. The experience

These flows are the spec. The architecture exists to make them true.

### 2.1 Merchant setup: a key ceremony, 7 steps, target 15 minutes to a test sale

Setup creates keys on purpose, one on each device that will keep it. Key 3's
secret is never seen by any OpenBMT-published software, or by a device that
holds another key. It is the only time the owner does this. There are no waiting periods in setup. The
15-minute limit is a requirement. If a trial breaks it, we cut steps, not
safety.

1. **Install and name the shop.** Business name, currency, logo. The app also
   asks two plain questions:
   - "How do you take sales?" The answers are "On my own Box", "On a hosted
     Box" (labeled, §5.3) or "No Box, I tender by hand" (§5.6).
   - "Can customers join this Wi-Fi?" If yes or unsure, the counter uses
     cellular, or the counter's Tor switch is turned on (§5.1). If no, the
     staff network is fine.
2. **Make key 3 (recovery) outside OpenBMT software.** The owner makes it with
   a wallet app from a different publisher (★ nothing to buy; for example
   Nunchuk, **[verify that it exports a BIP48 P2WSH multisig key with origin,
   E8]**). The app runs on the counter tablet before it becomes the counter,
   in airplane mode. A hardware wallet is the stronger choice. The owner writes
   the words on paper or steel and checks them with the app's own backup
   check. Only key 3's public key (xpub) goes to the Key app, by QR. The tablet
   is then **factory reset** before OpenBMT is installed on it. The Key app
   never makes or shows key 3's words. The screen says "do this step alone":
   key 3's words and key 2's words are written in separate sittings, with no
   one watching.
3. **Make key 2 on the spare phone, in the Key app.** It is created offline.
   The Key app shows the words once. The owner writes them down by default and
   picks them back from an in-app word list, never the system keyboard. They
   are kept with this device, because one key is not enough to spend (skipping
   shows a plain warning). The Key app refuses to show words while an
   accessibility service, overlay permission, non-system keyboard or USB debugging is on, and the
   screen blocks screenshots (FLAG_SECURE). Honest note: these checks are
   promises of the Key app's own software, and the phone that shows the words
   could keep them. That is why a hardware wallet is the stronger choice. Key
   2's words must never be stored with key 3's.
   - **Where key 2 lives (★ the spare phone, at home):** a refund the cashier
     starts waits for the next approval session (§2.10), or is paid in cash
     from the till (§2.4). **On site:** the phone sits in the back office or
     with a manager, and sessions are easier. A hardware wallet is the other
     choice (§5.4). The app explains this in two lines. The merchant picks.
4. **Make key 1 on the owner phone.** Owner mode creates key 1, sets its PIN
   or passphrase (§5.4) and shows its short fingerprint. No seed words by
   default.
5. **Build and confirm the wallet.** Owner mode combines the three public
   parts into the wallet description and sends it to the Key app by QR. The
   Key app checks byte for byte that the key 2 and key 3 slots hold the exact
   xpubs it knows (key 3's came by QR in step 2). A bad owner phone can then corrupt only the key 1 slot.
   Then **every device shows the same 4-word checksum of the whole wallet
   description and all three fingerprints**, and the owner compares them. A
   hardware wallet also has the wallet registered on it and shows the first
   receive address on its own screen, to compare with the owner phone. The
   same screen then shows the **pairs screen**: "Any two of these can move all
   your money", every pair (1 and 2, 1 and 3, 2 and 3), and who holds each
   key. The owner confirms or goes back.
6. **Print the recovery kit offline.** The one-page kit (§5.4) goes to a
   printer directly or as a PDF on a USB drive, never through a cloud
   service. It holds hints, not secrets. It prints the checksum words, and any
   reprint is compared against them. The owner checks off "stored safely".
7. **Connect the counter and the POS, then run a test sale.** The counter
   shows its own pairing QR, and the Key app scans it directly. The counter
   shows the two pinned roots as 4-word fingerprints, and the owner compares
   them with both phones (§2.9).
   The owner phone gets its own read-only sign-in to the POS (§5.3). If the
   owner wants, a **practice mode on signet** runs the test sale with no real
   money first. Then a $1 sale paid from any wallet shows up in the POS as
   paid and in the books. The screen also shows the owner the line "If every
   instant sale failed you would lose at most $X" (§6).

Counts, honestly: 7 steps. Step 2 needs the counter tablet before it is set
up as the counter, and the tablet's factory reset adds time **[to measure]**.
Steps 3 to 5 and 7 need the spare phone, the owner phone and the counter
device in hand, so the owner should have all three before starting. We have
not timed it; **[to measure]** (E1–E3 phones, then a timed trial). The most
time goes to writing down words twice, in two sittings.

The words "xpub", "descriptor" and "multisig" never appear on these screens.
They appear in the recovery kit and under Advanced, for people who want them.
A hardware wallet's own screen may use its own words. The app tells the owner
what to expect before that screen appears.

### 2.2 Cashier sale: 3 taps

1. Ring up the sale in the POS as usual.
2. Choose **Bitcoin** as the payment type.
3. The counter screen shows the amount in dollars, a QR code, and "Tap or scan
   to pay" on Android (iPhone customers scan). The cashier waits and reads one
   of four plain states:
   - **Green plus a chime: "Paid, hand over goods".** The screen also says
     what was checked, for example "Paid (light check)" (§6).
   - **Green: "Paid, could still be cancelled for about N min".** This shows
     for an instant sale. The goods can go. The risk is the shop's limit.
   - **Blue: "Not paid yet, wait".** Neutral. It shows while a sale is open,
     and for a sale over the instant limit it also says why and what happens
     next (§2.6).
   - **Red: "Do not hand over goods".** It shows for any state where the money
     is not safe (§2.6). A manager can override it. A cashier can override it
     only up to the per-sale instant limit (§2.9). Every override is logged
     and counted as exposure (§6).
   - The cashier never chooses a network, never sees a fee, and never types an
     address.
4. The sale closes in the POS on its own, with the receipt and inventory
   handled there.

If the connection is down at the start, the counter does not offer Bitcoin
(§5.1). The cashier is told to choose another payment type.

### 2.3 Customer payment

| Customer has | Steps |
|---|---|
| Android phone | Tap the phone on the counter device, then confirm in their wallet (1 tap + 1 confirm) **[verify per wallet, E1]** |
| iPhone | Scan the QR, then confirm (1 scan + 1 confirm) |
| Any other phone | Scan the QR, then confirm (1 scan + 1 confirm) |

Tap is Android-only. iPhones do not open a wallet from a `bitcoin:` tag in the
background, and we will not send iPhone customers through a web page that a
server could change (§7).

- **Optional protection: check before you confirm.** The counter screen shows
  the amount and the **first characters after `bc1q` and the last characters
  of the address in large type**, with the words "You can check these in your
  wallet". A QR shows only an address and an amount. A hidden or fake code can
  show a different address. The check protects the customer, and it is
  optional. The last 6 characters are mostly a checksum, which is why the
  first characters are shown too.
- **Price lock: 5 minutes.** The bitcoin price is locked for 5 minutes, and
  both screens show the dollar amount. At the end of the lock the price
  refreshes on its own and the QR's amount changes. The address stays the same
  for the open sale (§5.5). The customer who is still at the counter does not
  have to start over.
- **Payments are valued at first-seen time.** The dollar value of a payment is
  taken at the moment it was first seen on the network. This is the value used
  for "paid in full" and for the books.
- **Underpayment tolerance.** The owner sets how much short still counts as
  paid. The recommended default is small, for example a few cents or 0.5%
  **[to tune]**. The tolerance never covers more than the shop is willing to
  lose on one sale. Past the tolerance, it is "Paid $8.00 of $10.00".
- **Wrong amount:** "Paid $8.00 of $10.00". The cashier taps "Ask for the
  rest" (a new code for the difference) or "Cancel (customer still owed $8.00)"
  (§2.4). Refunds of an overpayment follow §2.4.
- **Late payment after expiry:** it is still detected and recorded, never
  lost, and flagged to the owner. A late payment never turns a different sale
  green (§5.5).
- **The tag is empty when no sale is open.** The counter does not keep serving
  the last sale's code after it closes.
- **Dust to a shown address** (a tiny output below a threshold) is ignored and
  logged. It does not open, pay or close a sale.
- **Proof of payment.** On request the counter gives a counter-signed receipt
  that carries the transaction ID, as a short ID or a QR. The customer can
  show it later. The transaction ID appears only here, never in the POS fields
  or the accountant's CSV (§10).

### 2.4 Refunds: cancel or refund, queued, paid in a session

1. The cashier opens the sale and taps **Refund**. If a payment has already
   arrived and the sale is not complete, the button "Cancel sale" is renamed
   **"Cancel (customer still owed $X)"** and does the same thing.
2. The customer shows a receive QR from their wallet, and the cashier scans it.
3. The item goes into the **refund queue** with the cashier's name. The owner
   gets a push the same day: "1 refund waiting. Open an approval session when
   you are with key 2" (§2.10). The customer gets a receipt saying the refund
   is approved by the owner, usually the same day. All queued refunds go out
   in one transaction with one output per refund. Each line is shown
   separately on the owner phone and the Key app. A customer who cannot wait
   can be refunded in cash from the till (below), where the shop allows it.

Refund rules:

- **Refund depth is its own setting.** A refund is paid out only after the
  original payment reaches the **refund depth** (§2.7). The default is "Final",
  higher for large sales. It is separate from the depth for handing over
  goods. A queued refund shows "waiting for depth" until then. Otherwise a
  customer could be refunded and then cancel the first payment.
- **Shop option: refund in cash from the till.** The cashier gives cash and
  records a return. The bitcoin is kept, nothing is queued and no alert fires.
  Cash refunds count toward the cashier's daily cap.
- **Never more than was received.** A refund cannot exceed the amount actually
  received for that sale, including a refund in several parts.
- **Daily dollar cap per cashier.** Past it, a refund needs a manager. A
  **manager PIN above a dollar threshold** also confirms the customer is
  present. Both amounts are shop settings, in dollars, not counts. Raising
  them is loosening (§2.9).
- **The refund QR accepts only a plain on-chain address.** It ignores any
  amount, label, or other field in the code. The refund amount is always set by
  the shop, never by the code.
- **Warnings only on anomalies.** Both screens show the original sale, the
  amount refunded so far and the cashier's name. A warning appears only when
  something is unusual: a destination used before for another sale, or an
  amount over the threshold. A new address alone is not a warning, since
  wallets often differ.
- **The Key app limits refunds to unknown destinations.** A refund destination
  is not on the Key app's allowlist, so the Key app enforces its own
  per-transaction, per-day and cumulative weekly ceilings in satoshis for such
  destinations (§5.4). These ceilings change only on the Key app's own screen.
  The Key app has no sale records, so "never more than was received" is the
  owner phone's assertion, not the Key app's.
- **Optional proof.** The shop can require the customer to show the matching
  transaction ID in their wallet before the refund is approved.
- **Who pays the refund's network fee** is a shop setting: the shop, or the
  customer (the fee comes out of the refund). It is printed on the receipt.

The shop sets once whether refunds go back at the dollar price or at the
bitcoin amount paid. That setting is printed on the receipt. A shop may also
set a return window, after which refunds go back at the bitcoin amount paid, so
price moves cannot be used to profit from returns.

### 2.5 End of day: 1 screen

Totals in dollars and bitcoin, the number of sales, anything still waiting for
confirmation, and **Export** (§10). There is no day-close step. Optionally,
there is a weekly cash-out reminder (§11). The counter shows these as sales
totals. It never shows the wallet balance (§2.9).

### 2.6 Every waiting or unknown state explains itself

Any state that is not "Paid" or "Failed" shows two things on screen, as text:
why it is waiting, and what happens next. For example: "Not paid yet, wait.
Sales over your instant limit wait for the network to lock them in. Usually
about 10 minutes. You can hand over the goods after that, or let the customer
go and the sale will close on its own." Waiting is blue or neutral, never
alarming.

**Red "Do not hand over goods".** If a payment was replaced, dropped, or
reopened by a chain reorganisation, the counter shows a separate red screen.
It does not look like "waiting". A manager can override it with a PIN or by
signing in. A cashier can override it only up to the per-sale instant limit.
The override is logged with the name, the time and the amount, and it counts
as exposure (§6). It is blocked when the exposure total is full. A manager
override also counts against the manager's own dollar ceiling, and every
cashier has a daily override cap (§2.9).

**Cause-specific wording.** The wording says what actually happened. The
counter cannot always know the cause, so it says what it saw:

- **Replaced:** "A different payment replaced this one. The customer may have
  cancelled it. It was seen at 2:14 pm."
- **Dropped by the network:** "The network dropped this payment. This can
  happen without anyone cheating, for example when fees rose. The customer
  may need to pay again."
- **Reopened by a reorg:** "A block that confirmed this payment was replaced.
  The payment is back to waiting."
- **Unknown:** "We saw this payment disappear. We do not know why."

**Stuck state.** If a payment is seen but does not confirm for a long time
(its fee is too low), the sale shows "Stuck: not confirming". The owner gets a
notice and can speed it up if the shop pays the fee, or wait. The sale stays
open. It does not close falsely.

### 2.7 No silent losses

Requirement: a merchant must never lose a cancelled
payment without knowing, and must understand up front that it can happen.

- **Told at setup, in one sentence, with a choice.** "Instant sales up to $10
  (or $25 with your own node): for about 10 minutes, a dishonest customer could
  cancel, like a bad bill. You'll be alerted when your device sees it. If every
  instant sale failed you would lose at most $X." The shop picks its limit,
  including zero. The screen also says: "A cancel sent privately to a miner
  shows up only when the next block is found" (§6.2).
- **Words on every sale: Paid, Confirmed (1 block), Final.** An instant sale
  shows "Paid, could still be cancelled for about N min". At 1 block it shows
  **"Confirmed (1 block)"**. **"Final" is reserved for the refund depth**,
  which the shop sets (§2.4, §6). "Final" is depth-based: more blocks on top
  make a reversal less likely. It never claims zero risk.
- **The books never count unfinished money.** POS and books mark an instant
  sale pending until it reaches the refund depth.
- **Cancel alarm.** When the device sees a cancel: a loud alert on the counter
  screen and a push to the owner's phone: "Payment for $18.50 at 2:14 pm was
  replaced by another payment." A proof record is saved (§6.2). The sale
  reopens in the POS as unpaid.
  - **The alarm cannot be dismissed at the counter.** Only the owner clears it,
    on the owner phone.
  - **The owner phone runs its own watcher** for the shop's addresses, so a
    silenced or compromised counter cannot hide a cancel. Instant needs a
    live watcher (§6).
- **Weekly line.** "This week: 412 sales, 409 final, 3 confirmed once. 0
  cancelled. 0 pauses." The number is real, so trust is earned, not asserted.
- **The honest comparison, shown once at setup.** Card chargebacks can land
  months later **[verify the window before it appears on a screen]**. A
  bitcoin payment that is several blocks deep is, in practice, very hard to
  reverse. We will not call it final forever. The risk window for instant
  sales is about 10 minutes and visible. The risk for deeper payments is a
  chain reorganisation, which is rare and which the app also shows (§2.6).

### 2.8 Confusion points we design out

| # | Confusion | Fix |
|---|---|---|
| 1 | Key jargon | Plain words. Jargon only in the recovery kit and Advanced. |
| 2 | Having to buy hardware | Default second key is a device the owner already has; hardware is a choice, never a requirement (§5.4) |
| 3 | Losing the recovery kit | A checklist at setup, then a yearly physical inspection of key 3 and a reminder every few months to confirm the kit is where the owner left it |
| 4 | Customer's wallet is Lightning-only | §8. On-chain-only v1 says so plainly on the screen: "This shop takes on-chain bitcoin." |
| 5 | Wrong amount paid | Partial, over and late states, each with one-tap actions; owner-set tolerance |
| 6 | Waiting for confirmation | Instant limit (§6) plus an explained wait |
| 7 | Exchange-rate disputes | Price locked 5 minutes and shown on both screens; rate source recorded |
| 8 | Fee surprise | Customer's wallet shows the network fee; the shop can offer a bitcoin discount (§9) |
| 9 | Bad connection | Clear banner; detection continues; the sale never closes falsely; the counter does not offer Bitcoin when offline (§5.1) |
| 10 | Key 2 chores | One habit: the approval session, weekly by default (§2.10) |
| 11 | Who can change what | Roles (§2.9); the counter shows settings but cannot change them |
| 12 | Fake "Sent" screen on a customer's phone | The counter only shows green when it has seen the payment; the cashier is told this on the red screen |

### 2.9 Roles and authority

Three roles. The app enforces them. They are not an honor system.

| Role | Can do | Cannot do |
|---|---|---|
| **Owner** (in Owner mode, with key 1) | Everything on the owner phone: tighten settings, queue loosening for the approval session, add or remove devices and managers, approve refunds and cash-outs (with key 2), reset the instant pause, see balances and books | Loosen anything without the Key app's co-signature |
| **Manager** | Override a red screen (logged, counted as exposure, within a dollar ceiling), approve large refunds at the counter by PIN, view sales reports | Change money settings, pair devices, see the wallet balance, move money alone |
| **Cashier** | Ring up sales, start a refund, cancel a sale, override a red screen up to the per-sale instant limit and the cashier's daily override cap, see sales totals | Approve anything that moves money, change any setting, dismiss the cancel alarm, see balances |

Rules:

- **Loosening needs the Key app; tightening does not.**
  - **Loosening** is any change that lets more money leave or be accepted at
    risk, or that removes a check or an alert: a higher instant limit or
    exposure total, a lower depth, a lower rate limit, a larger refund cap, a
    new or widened destination, a new counter, manager or POS connection, a
    longer window, Tor or node checks turned off, and a higher override
    ceiling. A change we cannot classify counts as loosening.
  - **Tightening** is the opposite (a lower limit, a deeper depth, removing a
    device or manager, a pause). It takes effect at once on the owner phone,
    is signed with key 1, and needs no co-signature.
  - A loosening change is queued, then approved line by line in the approval
    session (§2.10), where the Key app shows the old and the new value. It is
    signed by key 1 and co-signed by the Key app. Counters accept it only with
    both signatures. This replaces the 24-hour delays: no clock or witness can
    be trusted on a compromised phone, and a second device can.
  - **Fallback when key 2 is a hardware wallet that cannot sign settings:**
    loosening waits for a delay counted in **blocks on each counter's own
    header chain** (default 144 blocks **[to tune]**), and is announced on
    every paired device first.
- **Pairing uses two roots.** The counter shows its own pairing QR, and the
  Key app scans it directly, so the owner phone does not name the counter. The
  counter pins the owner phone's root and the Key app's root, and shows both as
  4-word fingerprints for the owner to compare with both phones. The Key app
  shows "Pair counter X" and signs the pairing. Re-pairing needs both roots. A
  two-root-signed device list is kept. It includes the Box's key, and any
  identity not on it is ignored. A forged sync message cannot
  raise a limit, move the address index or change a destination, because those
  need both signatures.
- **Manager sign-in.** The owner adds a manager by name in Owner mode (a
  loosening change). The manager has a PIN of at least 6 digits. 5 wrong tries
  lock that manager for 15 minutes; 10 lock the manager until the owner resets
  it **[to tune]**. Removing a manager is tightening and reaches counters at
  their next check-in (below).
- **Check-ins.** Each counter reports a signed record head to the owner phone
  at fixed intervals (default 15 minutes **[to tune]**). A missed check-in
  alerts the owner.
- **The counter shows settings read-only.** It cannot change them. It never
  shows wallet balances.
- **All records are signed and hash-chained.** Each sale, refund, setting and
  override is a signed record that includes the hash of the one before. The
  owner phone keeps the latest hash (the "head"). A record that was deleted or
  edited breaks the chain and the owner phone reports it.

### 2.10 Approval session

One habit replaces the key 2 chores. The Key app is the second authority for
everything below, and it checks each item on its own screen against its own
records.

**What goes in:** refunds, cash-outs, address batch refills, loosening
settings, new counter pairings, the update checksum comparison (§5.4) and the
health check. A new destination is not queued here: it is enrolled on the Key
app itself (§5.4).

**The flow:**

1. **The owner phone queues items** as they come up. Nothing moves yet.
2. **A prompt** comes weekly by default, monthly if the owner chooses. The
   owner can also start a session at any time.
3. **The owner phone shows one animated QR** in the BC-UR format. Where the
   data is large it writes a file instead, and the owner carries it by USB-C
   or SD card. The Key app needs no network for either.
4. **The Key app lists each item** with its own checks and labels: the wallet
   description it holds, its allowlist, satoshi ceilings, high-water mark,
   change index bound and fee ceiling (§5.4). Any dollar figure is labeled
   "from the owner phone". The owner approves or declines each line.
5. **The Key app shows one QR back** (or writes one file) with its signatures
   and co-signatures. The owner phone adds key 1, and broadcasts or
   distributes the results.

A declined line returns to the owner with a reason. A refund waiting for the
next session can be paid in cash from the till (§2.4). If key 2 is away, a
speed-up (§9) waits for the next session too. If key 2 is a hardware wallet,
transactions are signed on it one at a time and settings use the block-count
fallback (§2.9). The Key app can still run on a spare device as an approver
for batches, settings and pairings (§5.4). The number of QR frames for a large transaction is not
measured **[verify, E8]**.

## 3. Best-in-class scorecard

Targets are what we must reach. "Best today" is the best competitor value we
found; many competitors publish no numbers, and we will measure our own.
Card percentages and dollar fees below are **[verify]** and stay off merchant
screens until sourced.

| Metric | Best today | OpenBMT target |
|---|---|---|
| Setup to first sale | Square: same day; BTCPay: hours, technical | ≤ 15 min with a key ceremony (7 steps, no waiting periods); own node optional **[to measure]** |
| Cashier taps | Card terminal: 1–2 | 3, counting "Bitcoin" in the POS |
| Customer steps | Card tap: 1 | Android 1 tap + 1 confirm; iPhone and any other phone 1 scan + 1 confirm |
| Time to "Seen" | Lightning POS: about 1–5 s **[verify]** | < 5 s (E3) |
| Time to "Paid", small sale | Lightning POS: about 1–5 s | < 10 s within the instant limit |
| Processor fee | BTCPay, Numo, Zaprite: 0% | 0% processor fee on receiving. Spending the money later costs the merchant network fees (§9) |
| Network fee | Card: 1.5–3.5% to the merchant **[verify]** | Paid by the customer: about $0.11–0.42 flat at 1–3 sat/vB **[verify]**. Spending each receipt later costs the merchant about 104 vB |
| Custody | BTCPay: merchant single key | Merchant 2-of-3; no one else holds a key |
| Closes the sale in the POS | Square bitcoin: native | Square, Clover, Lightspeed-style gateway |
| Refunds | BTCPay: refund flow | 3 cashier steps, queued, paid in an approval session; cash-from-till option |
| Books | Rarely offered | One-tap QuickBooks/Xero CSV plus a full ledger; owner-held copy |
| Who can see the wallet | BTCPay host: yes (xpub) | Only the merchant's devices. The counter holds address batches, not the wallet description. A hosted Box host (optional) sees orders and revenue, not addresses (§4.1) |
| Explained states | Rare | Every wait says why and what's next |
| Honest check labels | Rare | Every payment says what was checked: light check, or your node |

## 4. Purist checklist

| Red flag (source) | Our answer |
|---|---|
| Sharing an xpub reveals all history (Stacker News 112663) | The wallet description stays on the owner phone, the Key app and the merchant's own Box. The counter gets signed address batches only. No third party receives the description. |
| Gap-limit exhaustion hides payments (Stacker News 408359) | A shown address is never reused. Each counter has its own contiguous index range. Wide lookahead; late payments still detected; the kit records an explicit import range (§5.4, §5.5) |
| Public Electrum/Esplora servers leak addresses and IP (Stacker News 1586239, 1336584) | No third-party indexer. Payments are checked over the Bitcoin P2P network directly, or by the merchant's own node. Tor is one switch for the counter. Spends go out over Tor by default. |
| Light clients trust peers about the best chain (Core PR #18876 debate) | Status text says what was checked: "Seen by 3 peers (not verified by this device)", "Confirmed (light check)", "Confirmed (your node)". Never "fully verified" unless the merchant's own node said so, and the counter still checks proof-of-work (§5.1). |
| 0-conf treated as settled after full-RBF (Peter Todd, Stacker News 93588) | Instant only within a limit the shop sets and can afford to lose. The light check has a lower cap and says "Paid (light check)". The screen never calls it final (§6). |
| Custodial processors, KYC | No custody. No account tied to identity. No company in the payment path. The optional sync relay and hosted Box are not in the payment path (§5.3). |
| Trusted server decides where money goes (BTCPay third-party hosting docs) | Addresses come from batches made on the owner phone and co-signed by the Key app or spot-checked on a hardware wallet. A server cannot add one, and neither can a bad OpenBMT update alone (§5.1). |
| iPhone tap needs a web page that a server could rewrite | Removed. iPhone customers scan the QR. No page, no server in the address path (§7). |
| Push services see message text and timing | Wake-up pushes carry no content. UnifiedPush through the merchant's own Box is the default. FCM is a labeled fallback (§5.3). |
| Telemetry | None: no analytics, no crash upload. Every network contact is listed (§4.1, §13) |

### 4.1 Who sees what

The apps send no analytics or crash data to anyone. These are the contacts
they do make, what each party sees, and how to reduce it. Optional services
are marked, and none is a default.

| Party | What it sees | How to reduce it |
|---|---|---|
| Bitcoin peers (light check) | The counter's IP address, that it follows the chain, and which blocks it asks for | Counter's Tor switch; your own node |
| The shop's ISP or mobile carrier | That the device speaks Bitcoin and when; with Tor, only that it uses Tor | Tor; your own node on your own line |
| DNS seeds (peer discovery at start) | The IP of a device looking for peers | Tor; peers from your own node **[design]** |
| Rate sources (price) | That a device fetched the price, and when; no sale or address | Two or more sources on a fixed schedule; fetch over Tor |
| POS vendor (Square, Clover, other) | Every order, amount and the "Bitcoin" tender; opaque sale IDs only, never an address or transaction ID | "No Box, I tender by hand" still lets the POS see tender type; no way to hide the order from the POS itself |
| Push services | With UnifiedPush on your own Box: nothing outside. With the FCM fallback (labeled): that a wake-up happened, and when. No content | Use the Box's UnifiedPush |
| Box host (optional, hosted) | Orders, amounts and timing: how much the shop sells. Never addresses, transaction IDs or the wallet description | Run the Box yourself |
| Update channel (our site, F-Droid, GitHub) | The downloader's IP, and which version | Download over Tor; updates are manual |
| Customers' phones | The address and amount in the QR for that sale | A fresh address each sale; nothing about other sales |
| Optional metadata-only services | Whatever the choice screen says at the point of choice | Off by default |

The Box is reached by a Tor onion service or on the local network. No domain
and no TLS certificate company is needed **[design, R5]**.

## 5. Architecture

```
 customer phone        counter device (Counter mode)        OpenBMT Box (merchant-owned)
 ──────────────        ─────────────────────────────        ────────────────────────────
 Android wallet ◄─NFC─► signed address batch (no wallet      merchant's full node
 any wallet     ◄─QR──  description), emulated tag, QR         (optional)
                       header chain + light check, or         POS connectors (polling)
                       the merchant's own node                record sync (ciphertext)
                       explained status                       UnifiedPush wake-ups
                                                              holds NO key
 owner phone (Owner mode, Android)        spare phone / hardware wallet
  key 1, queues and signs, runs its       key 2 (OpenBMT Key app: second authority,
  own watcher, polls the POS itself         QR or USB-C/SD file, no network permission)
                                          recovery key (key 3), words stored away
```

### 5.1 Counter mode

- Holds **signed batches of addresses**, not the wallet description. Owner mode
  derives a batch from the wallet and signs it with key 1. The Key app
  co-signs it (below). The batch gives the counter its own contiguous index
  range. The counter checks both signatures against the two roots it pinned at
  pairing (§2.9) and uses the next address in its range. It cannot make a new
  one. A stolen counter leaks only the addresses in its batch, not the whole
  history. No private keys.
- **Every batch is checked by software other than the OpenBMT app.** Owner
  mode and Counter mode are the same app, so a bad update could sign a batch
  of the attacker's addresses. So the counter accepts a batch only with the
  Key app's co-signature: the Key app derives the same index range from its own
  copy of the wallet description and signs the batch's hash, in an approval
  session (§2.10).
  - **Hardware wallet as key 2 (the weaker path):** the owner picks which
    addresses to compare, and the hardware wallet shows them in its own
    address explorer. The counter does not pick. The key chooser says the Key
    app is the full check.
- **Batch sizing.** A batch covers about 8 weeks of observed sales, at least
  1,000 addresses. The owner phone asks for the next batch 4 weeks ahead,
  inside a session. Honest cost: a stolen counter leaks more future addresses
  than a small batch would. Ranges are contiguous and the Key app keeps a
  monotonic high-water mark of every index it has signed (§5.4, §5.5).
- **Address use.** An open sale keeps its address across price refreshes; only
  the amount changes. New addresses are rate-limited per hour. **An empty
  range means "do not offer Bitcoin"**, never reuse.
- **Reset or restored counter.** It gets a brand-new batch. The old tail is
  retired and stays watched (§5.5).
- Builds a BIP321 URI for each sale: `bitcoin:<address>?amount=<btc>&label=<shop>`.
  The `label` is free text. It proves nothing, so wallets must not treat it as
  proof of who is being paid.
- Shows the URI as a QR code and as an emulated NFC tag on Android (§7). The
  tag is empty when no sale is open.
- Checks payments itself. The merchant chooses how (settled
  2026-11-09):
  - **Default: light check.** The `bip157` light client talks directly to
    several random Bitcoin peers. It watches for payments to our addresses and
    for other transactions that spend the same coins (conflict alerts). It
    confirms with compact block filters. The check asks Bitcoin peers, not a
    company server. A light client cannot see the amounts of a payment's
    inputs and cannot confirm that its inputs are confirmed (§6).
  - **Option: your own node.** The merchant's own full node (on their OpenBMT
    Box, Start9, Umbrel, or a home node). The device then checks against it.
  - The screen always says which check was done. Labels, from weakest to
    strongest:
    - "Seen by 3 peers (not verified by this device)"
    - "Paid (your Box reports it)"
    - "Confirmed (light check)"
    - "Confirmed (your node)"
- **Peers come from distinct network groups.** The light client will not count
  two peers in the same network group (for example the same IP range) as
  separate peers. This makes it harder for one attacker to pose as several.
- **Header chain, always.** In every mode, including your-own-node mode, the
  counter keeps its own chain of block headers from random peers. It accepts
  "Confirmed" from the Box's node only with a merkle proof that the payment is
  in a block with real proof-of-work on that header chain. The proof check
  rejects 64-byte transactions and checks that the proof depth is consistent
  (the CVE-2012-2459 family). The Box can hold back information, but it cannot
  make up a confirmation.
- **Own-node unconfirmed payments.** The own-node label and caps apply to an
  unconfirmed payment only when the counter's own light client also saw the
  transaction from independent peers. Otherwise the label is "Paid (your Box
  reports it)" and the light-check caps apply (§6).
- **Stall and unhealthy state.** If the light client has few filter-serving
  peers, stops getting new blocks, or sees headers fall behind, the counter
  shows "Check unhealthy" and **turns instant acceptance off** until the check
  recovers. Bitcoin Core does not serve compact filters by default, so the pool
  of filter peers is small **[verify the current count, E3]**.
- **Watcher heartbeat.** Instant needs a heartbeat from the owner phone,
  signed with a pinned key, within N minutes (default 10 **[to tune]**). The
  Box cannot supply it. Without one the counter shows "Instant off: no
  independent watcher" (§6).
- **Clock check.** The counter compares its clock with recent block times. A
  large mismatch turns on a warning and stops the price lock and expiry from
  trusting the local clock.
- **Network.** If customers can join the Wi-Fi the counter is on, they could
  feed it fake data. So setup asks "Can customers join this Wi-Fi?" (§2.1). If
  yes or unsure, use cellular or turn the counter's Tor switch on. Light-check
  instant on shared Wi-Fi runs only with Tor on **[verify latency, E3]**.
- **Connectivity pre-check.** Before the counter offers Bitcoin on a sale, it
  checks that it has enough healthy peers and a current header chain. If not,
  it does not show a QR and tells the cashier to use another payment type.
- **Outage ladder.** If the connection drops mid-sale, the counter keeps the
  QR up, records the payment as **"unverified"** if one arrives, and shows the
  red "Do not hand over goods" screen. When the connection returns it checks
  again. The sale never closes in the POS on an unverified record.
- Tor: off by default for speed (except as above), one switch to turn it on.
- Runs as a foreground service. The recommended deployment is a device that
  stays plugged in.

### 5.2 Owner mode

- Holds key 1 (§5.4). Android only in v1.
- Queues refunds, cash-outs, batches, settings and destinations. Every one is
  approved and signed with key 2 in the approval session (§2.10). It builds its
  own transactions from its own wallet view. The server is never needed to
  spend.
- Signs the shop's settings and pairings (§2.9). Loosening also needs the Key
  app.
- Runs **its own watcher** on the shop's addresses (light check or the Box's
  node), so a hacked counter cannot hide a cancel or a stuck payment. It sends
  the signed heartbeat the counter needs (§5.1). This is a promise of the
  software itself: a bad main-app build could fake it. The monthly outside
  check (§10) is the check that does not depend on this software.
- Polls the POS directly with its own read-only sign-in and matches every
  bitcoin-tender order to received funds (§5.3).
- Holds the head of the hash chain of records (§2.9) and an owner-held copy of
  the books (§10).
- Broadcasts refunds and cash-outs over Tor by default (§11).

### 5.3 OpenBMT Box: the merchant's own server

The Box is software for a small computer the merchant owns (a mini PC, or an
app on Start9 or Umbrel). It runs the POS connectors, record sync between the
merchant's devices and, if the merchant chooses, their own full node. When the
merchant runs the Box themselves, no one else is given the shop's wallet
description or addresses by OpenBMT. The Box is reached over a Tor onion
service or on the local network (§4.1).

- **POS connectors use polling where the POS allows it.** The Box asks Square
  or Clover for new orders. It does not need a public address for webhooks. If
  a POS only offers webhooks **[verify per POS, E5, E6]**, the Box uses the
  smallest exposure the POS allows and the setup screen says what is exposed.
- **Idempotent calls.** Every call to the POS carries a key so that a retry
  does not pay an order twice.
- **"Not yet in your POS" queue.** If a sale is paid but the POS call fails, the
  sale goes into a visible queue and is retried. It is never lost and never
  closed in the POS without payment.
- **The Box posts "paid" only after the counter signs it.** The counter signs
  the paid record. The Box cannot call PayOrder on its own. On a hosted Box,
  an order without a counter signature is never fulfilled automatically.
- **The owner phone polls the POS itself.** It has its own read-only POS
  sign-in, so its order list does not come from the Box. We say it plainly:
  **the Box (or a hosted host) holds the POS token and can mark an order
  paid. The owner phone catches it.** The phone matches every bitcoin-tender
  order, including voids and cash re-tenders, to a payment at an address it
  derived itself. Any gap raises an alert. This also catches a cashier who
  picks the manual "Bitcoin" tender and keeps cash.
- **Revocation.** The owner can revoke the Box's POS connection from the owner
  phone and from the POS vendor's own settings. The setup screen shows both
  paths.
- **Optional hosted Box (Q4, decided).** "Someone runs it for me" is kept and
  labeled honestly at the point of choice: "They can see your orders and
  revenue. They never touch your bitcoin." It is not the default. It is
  limited to the POS connectors. The host never receives addresses,
  transaction IDs or the wallet description, and it never verifies payments.
  It could mark an order paid that was not, but the owner phone's order
  matching would catch that. It is listed in §4.1. The Box's key is on the
  two-root-signed device list (§2.9). Its "paid" notice is never a heartbeat
  (§5.1).
- **Record sync.** Sale records synced between the merchant's devices are
  encrypted on the device. Any relay between devices sees only ciphertext, and
  its size and timing **[design detail, R5]**.
- **Push.** Wake-ups carry no content: "something happened, open the app". The
  app fetches the details itself. The default is UnifiedPush (or ntfy) running
  on the merchant's own Box. If a merchant has no Box, FCM or APNs is a
  labeled fallback. Message timing is still visible to Google or Apple in that
  case, and the setup screen says so.
- **Updates and migrations.** Updates are installed outside trading hours.
  Database migrations are atomic and roll back on failure, so a failed update
  leaves the old version working.
- It holds no key, cannot move funds, cannot pick addresses, and cannot close a
  sale in the POS without the counter's signature. Its "paid" notice is only a
  hint until the counter signs it.
- Legal line, from FinCEN FIN-2019-G001 §4.2.2 and California DFAL §3102(c):
  the operator never holds a key, never receives and forwards funds, and never
  sweeps funds for the merchant. Crossing any of those changes the legal
  picture. New York needs a lawyer's confirmation before a managed server is
  offered there.

### 5.4 Keys: the merchant chooses, every choice is safe

**Rule 1: no single device may hold or drive two keys.** "Drive" means making
a key sign without that key's own device showing and checking the transaction
on its own screen. The owner phone builds transactions that key 2 then checks
and signs on the Key app's screen. That is not driving. A key card tapped on
the owner's own phone would be driving, which is why it is not offered. The app
enforces the rule and never offers a setup that breaks it. Malware on one phone
plus one routine tap must not be able to spend everything, because that quietly
turns 2-of-3 into 1-of-1.

**Rule 2: keep key 2 on different software than key 1.** If key 1 and key 2 ran
the same app build, one bad update would sign anything on both. So the default
key 2 is the separate **OpenBMT Key app** (below). The key chooser says "keep
key 2 on different software than key 1" and offers a hardware wallet, or
Sparrow or Nunchuk on another device, as other ways to do that. **Until the
Key app's release key is held by a second person (below), one publisher signs
both apps.** The doc and the key chooser say so plainly and recommend a
hardware wallet or a different publisher's signer (Sparrow, Nunchuk) for key
2. When key 2 is a hardware wallet, the Key app is still recommended on a spare
device as an **approver**: it holds an identity key only, no wallet key, and
co-signs batches, settings and pairings. Without it, the screen says those
rest on the owner phone plus spot checks.

**Rule 3: every signing device shows what it signs, and checks it itself.**
- It checks the change output against its **own registered copy** of the wallet
  description. It never trusts wallet or key information supplied inside the
  transaction file (PSBT).
- It requires `non_witness_utxo` (the full previous transaction) for **every
  input**. Without it, a signer can be lied to about input amounts and sign a
  huge hidden fee (CVE-2020-14199, which applies to the P2WSH script we use).
- It enforces a **fee ceiling**.
- Its screen shows four things **separately**: the destination, the amount, the
  fee, and "change back to your wallet". The change line is shown only if the
  signer verified it.

**The OpenBMT Key app.** It is the second authority in every approval session
(§2.10).
- A small, separate code base with its own release key (Q3, below).
- **Honest wording on the network.** The Key app asks for no network permission
  and has no networking crates. That does not make leaks impossible. A
  compromised OS, or another app talking to it through intents, could still
  move data. So the controls below limit what a bad build can leak or sign.
- **What the Key app cannot know.** It has no sale records. So the sale
  context, and "refund no more than received", are owner-phone assertions: the
  Key app shows them, labeled so, and cannot check them. Its own guarantees are
  the satoshi ceilings and the allowlist. The wipe of a key after too many
  wrong tries, and the checks for root, overlays and similar, are promises of
  the Key app's own software. A bad build would not keep them.
- Its own registered copy of the wallet description, and key 3's xpub, which
  came by QR from the other publisher's app (§2.1).
- Its own records, which no other device can edit:
  - **Destination allowlist.** Cash-out addresses. A new destination is
    enrolled only by scanning the exchange's deposit QR with the Key app's own
    camera, with the label typed on the Key app. It becomes active after 24
    hours on the Key app's own clock. It is not queued from the owner phone.
  - **Satoshi ceilings.** All ceilings are in satoshis and set on the Key app:
    per transaction, per day and cumulative per week. They apply to unknown
    destinations (refunds) and also to allowlisted destinations. A dollar
    figure on the Key app is labeled "from the owner phone", because the Key
    app has no rate of its own. Changed only on this device.
  - **High-water mark.** The highest address index it has signed, never
    lowered.
  - **Change-index bound.** A change output must lie within a set distance of
    the high-water mark. An outlier needs an explicit override on this device.
  - **Fee ceiling.** Measured on the total input value, not only the value
    sent, with a hard bound that cannot be raised from the owner phone. It is
    adjustable on this device, so an emergency spend is possible.
- **Transport.** QR in the BC-UR format (compatible with Sparrow and hardware
  wallets) is the default. A file by USB-C or SD card is optional. Neither
  needs a network permission. Large transactions need many frames **[verify
  frame counts, E8]**.
- **Input parsers.** QR, BC-UR, PSBT and file parsers are memory-safe code with
  strict size limits. Fuzzing them is a launch condition.
- **Signing hygiene.** RFC 6979 deterministic nonces. Anti-exfil signing needs
  host-supplied entropy that the host verifies, so the Key app uses it only
  where the host does that check **[verify protocol]**. The Key app refuses to
  sign under root, USB debugging, accessibility services or screen overlays
  (a promise of the software itself, above). Recommended spare-phone hygiene:
  airplane mode and no accounts.
- **Restore-and-sign mode.** For recovery, the Key app can restore a wallet
  from words and sign without the usual allowlist history. The satoshi and fee
  ceilings still apply in this mode. It is used when moving to a new wallet or
  onto a new phone.
- Never auto-updates, and an update is never installed without the owner's
  action. Android enforces only certificate continuity on update, so no
  installed app can block a bad update of itself. Distribution is our own-signed APK and F-Droid only,
  **never Google Play**. A signed release manifest carries an
  `earliest_install_date`, and Owner mode shows it ("Key app 1.4 is 9 days
  old; wait until X"). An **urgent-fix flag**, signed by the release key, lets
  the merchant choose to install sooner. The install needs a matching
  reproducible-build hash.
- **"Built by Avila Labs only"** is shown on the Key app's about screen until
  two independent parties publish matching reproducible-build hashes. **One
  independent rebuilder is a launch condition.**
- Minimal crate set (§13), vendored and reviewed by someone other than the
  main app's author. On a spare phone, the cost is one manual update per
  release.

**Release key (Q3, decided; tightened in round 3).** The Key app is signed
with its own release key, separate from the main app's key. **Launch
condition: that key is held by a second, independent person**, not the main
app's signer, and kept offline on a device that never builds or publishes. Key
app releases are reproducible and published with their hash, with an
independent rebuilder before release. Until the second holder exists, one
publisher signs both apps, and the doc and the key chooser say so (Rule 2).
The second holder has not been named yet; until one is, this launch
condition is not met (§16).

| Key | Choices (★ = recommended) | Why each is safe |
|---|---|---|
| Key 1 (owner) | ★ Owner's phone, in Owner mode · A hardware wallet | Phone key rules below. A hardware wallet is the stronger choice. |
| Key 2 (approver) | ★ A spare phone or tablet you own, running the OpenBMT Key app · A manager's phone running the Key app · A hardware wallet (Coldcard, Jade, Passport, Keystone, others that sign PSBTs, **[verify each, E8]**) · A key card tapped on a different person's phone | A thief has to get past the owner phone **and** the key 2 device. The Key app and hardware wallets show the transaction on their own screens. A **key card has no screen**: it is as safe as the phone it is tapped on, and the app says so. A hardware wallet cannot hold our allowlist or co-sign settings, so loosening uses the block-count fallback (§2.9) and batches the weaker spot check (§5.1). The Key app on a spare device as an approver (identity key only) restores the co-signature for batches, settings and pairings; without it those rest on the owner phone plus spot checks, and the screen says so. Until the Key app's release key has a second holder, a hardware wallet or a different publisher's signer is the recommended choice (Rule 2). |
| Key 3 (recovery) | ★ Seed words made with a different publisher's wallet app (for example Nunchuk **[verify, E8]**) on the counter tablet before it becomes the counter, in airplane mode, then written on paper or steel and stored away from the shop; the tablet is then factory reset · A hardware wallet in a safe (stronger) · A key card in a safe · A trusted person's device | Used only to recover a lost key. It is made by software that OpenBMT did not publish, on a device that holds no other key, offline. Only its xpub reaches the Key app. |

Notes:

- **Same seed in two slots.** The app compares the fingerprints of all three
  keys. If two match, it blocks the setup. "The app checks what it can." It
  cannot see inside a hardware wallet, so two hardware wallets loaded with the
  same seed would show matching fingerprints only if the owner reads them out.
  The checksum words and the pairs screen (§2.1) ask the owner to confirm they
  are different.
- **Phone keys** (key 1, or key 2 in the Key app) are hot keys. Purists prefer
  hardware, and the app says so in one line, with a "move to a hardware
  wallet" option at any time. All keys are BIP39. Phone key rules:
  - **The file key comes from the PIN or passphrase.** A memory-hard KDF turns
    the PIN or passphrase into the key, together with a Keystore key. A PIN
    that is only a screen gate is not allowed. Delays escalate with wrong
    tries, and the key is wiped after N attempts (default 10 **[to tune]**). A
    long-passphrase option is offered.
  - **The wrapping key is not bound to biometrics or the lock screen.** On
    Android, a key that needs user authentication is permanently invalidated
    when the lock screen is removed, and by default when a new fingerprint is
    enrolled. That would silently kill the key. Instead, the app has its own
    unlock before it will sign.
  - **Key files are excluded from backup and from device transfer.** Android's
    Auto Backup restores app files but never the Keystore key, so a restore
    would bring back a file that looks fine and cannot be opened. The app
    excludes it.
  - **Dead-key detection.** If the key file is present but its wrapping key is
    gone, the app says so right away: "This phone's key no longer works."
  - **Health check.** Each key device signs a harmless challenge locally. The
    owner phone queues the challenge and the Key app signs it in the approval
    session (§2.10), so the check works for an offline Key app. The key 1
    check includes PIN entry. The owner is told "Key 1: works. Key 2: works."
    If a key fails, the owner is told the same day.
  - **Key 2 on a new phone.** Restore from its words onto the new phone. No
    sweep is needed.
  - **Key 1 replacement is a move to a new wallet** (below).
  - **A phone key with an unknown PIN and no words is a lost key.** The kit
    says so.
  - **Key 1 has no seed words by default.** If the owner turns this on, the
    app shows them once. They must be stored apart from key 3's and key 2's
    words.
- **Losing the wallet description.** Funds need two keys and the wallet
  description. The recovery kit holds the description, so it is not lost with a
  phone.
- **"Move to a new wallet".** Used when a key is lost, a keyholder leaves, or
  key 1's phone is replaced. There is no on-chain "revoke". The flow uses the
  other two keys to send everything to a new wallet with a new key in that
  slot. The button says **"Move to a new wallet"** and the screen says it
  sends all your funds, costs network fees, and takes about an hour. The flow
  then:
  - **re-pairs every counter** with both roots;
  - **discards old batches** and issues new ones;
  - **reruns the checksum comparison** and the pairs screen;
  - prints a new kit and keeps the old one marked "retired".
  The default advice is to keep key 2 at home, so no one at the shop can reach
  two keys.
- **Script type and descriptor.** The wallet is P2WSH `sortedmulti` 2-of-3, for
  the widest hardware support. Taproot is a later option once hardware signer
  support is broad **[verify]**, and would remove the need for
  `non_witness_utxo`. The stored form is pinned, with one hardened marker `h`
  and a checksum:
  `wsh(sortedmulti(2,[fp1/48h/0h/0h/2h]xpub1/<0;1>/*,[fp2/48h/0h/0h/2h]xpub2/<0;1>/*,[fp3/48h/0h/0h/2h]xpub3/<0;1>/*))#checksum`.
  That is BIP48 script type 2' (P2WSH) paths, BIP389 `<0;1>/*` multipath for
  receive and change, and full key origins. The kit also carries two
  single-path descriptors (receive and change) as a fallback for software that
  lacks `<0;1>` **[verify per wallet, E9]**.
- **The recovery kit** (one page, reprinted after any key change, with a
  version and date) holds hints, not secrets:
  - the full wallet description with checksum, all fingerprints and paths, as
    text and QR, and the 4 checksum words;
  - the **birthday block height**;
  - **every wallet generation with its status** (current, retired). Old kits
    are kept;
  - an **explicit import range** up to the highest index issued plus a margin
    (§5.5);
  - the words "any two of three";
  - for each key: where it is, how to unlock it, who knows (hints, never the
    PIN or the words);
  - the statement that a phone key with an unknown PIN and no words is a lost
    key;
  - **restore steps.** The named path is Sparrow with key 2's words, key 3's
    words and the kit, which an heir can realistically follow. The alternative
    is an unpruned Bitcoin Core with a block filter index. These steps go in
    the kit only after experiment E9 has tested them.
- **Key 3 inspection.** Yearly, a physical checklist: the words are there, the
  paper or steel is readable, and the place is safe. This is the default
  because a drill re-exposes the words. A full drill, optional, runs in the
  other publisher's app or a hardware wallet in airplane mode, never in the Key
  app. A hardware wallet uses its own seed-verify feature.
- **Setup check:** the first receive address is compared on a hardware wallet
  screen or a second device, and the checksum words on every device (§2.1).
- **Update check.** After an app update, before signing anything, the owner
  phone and the Key app compare the 4 checksum words of the whole wallet
  description. The Key app is the independent channel, on a different release
  schedule. The check never locks the shop out: the counter keeps using its
  already-checked batch. If the words differ, the owner phone stops signing new
  batches, settings and spends until the owner resolves it with key 2.

### 5.5 Addresses

- **A shown address is never reused.** Every sale gets a new address from the
  counter's own range, except that one open sale keeps its address across price
  refreshes (§5.1). A cancelled or expired sale's address stays watched. This
  stops a late payment from customer A paying customer B's sale.
- **Contiguous per-counter ranges.** Owner mode gives each counter its own
  non-overlapping, contiguous range, and the Key app's high-water mark covers
  every index it has signed. Two counters never issue the same address, and no
  gap hides a payment.
- **Every address ever shown stays watched,** so a late payment is found and
  recorded.
- **Payment first seen before the sale opened is flagged, not Paid.** If an old
  payment arrives for an address before its sale starts, it is not shown as
  paying the new sale. The cashier sees the flag.
- **Block new sales after a restore.** After a restore or a reset, the counter
  does not open a sale until a rescan finishes, and it takes a brand-new batch.
  The old tail is retired and watched.
- **Wide lookahead.** The wallet watches far ahead of the highest index used.
- **Recovery scans use an explicit import range.** The kit records the highest
  index issued plus a margin, and says to import that whole range rather than
  rely on a gap limit.
- **Refills.** A counter asks for the next batch 4 weeks ahead (§5.1). It
  tells the cashier if it cannot get one. Sales continue until the range is
  used, then Bitcoin is not offered.

### 5.6 POS connectors

| POS | How | Button in the POS | Sale closes |
|---|---|---|---|
| Clover | Custom tender app (TenderConnector) | Yes | Yes |
| Lightspeed-style gateway | "Other payment method" opens the counter's own flow and returns "paid". This is the pattern BTCPay's Lightspeed plugin uses (TChukwuleta, v1.1.0, 2026-08-19). | Yes | Yes |
| Square | Counter device beside Square; the Box posts an external CRYPTO payment and calls PayOrder after the counter signs "paid" | Manual tender selection | Yes |
| Any POS, or no Box | Manual "Bitcoin" payment type; counter device holds the record | Manual | Manual |
| Cannabis POS (Treez, Cova, Flowhub, Dutchie) | Partner programs with certification | — | — |

The gateway flow must not serve a payment page from a shared server that could
change the address. The address comes from the counter's signed batch (§5.1).
**[verify Lightspeed's flow, R6]**

Cannabis POS integration needs partner agreements; that outreach is out of scope
for the software. Square bans cannabis sellers and runs its own custodial bitcoin.

## 6. Acceptance policy

Full-RBF is unconditional since Bitcoin Core 29, so any unconfirmed payment can
be replaced. The shop chooses, and the screen always says which rule applied.

| Tier | Rule | Trust | Release |
|---|---|---|---|
| T0 Confirmed | Wait for confirmation, at the depth the shop sets (1 block by default; more for large sales) | None beyond depth | v1 |
| T1 Instant | Accept on the counter if all hold: the limits below; the checks that the device can do (below); a live watcher; no conflicting spend seen for 8 s | The shop's limit is what it risks | v1 |

**T1 limits (Q2, decided).** Instant stays available under both checks. The
defaults are lower under the light check, and the label says so.

| Check | Per sale | Outstanding total (each counter's own slice of the budget) | Label |
|---|---|---|---|
| Light check (default) | $10 | $50 | "Paid (light check)" |
| Merchant's own node, also seen by the counter's light client | $25 | $100 | "Paid (your node checked it)" |
| Box reports it only | $10 | $50 | "Paid (your Box reports it)" |

All amounts are shop-adjustable and **[to tune with data, E7]**. Setting zero
turns instant off. Raising any of them is loosening (§2.9).

**T1 rules, all of them:**

- **The limit is judged on `max(sale, received)`.** A $10 sale paid with $5,000
  counts as $5,000 and is not instant.
- **Exposure is the outstanding unconfirmed instant total, against a budget
  the owner sets for the whole shop.** It is not a per-hour count. Each counter
  gets its own signed slice of that budget, so no counter needs to know what
  another has taken. The slices add up to the budget; a shared running total
  cannot be enforced without a coordinator. Overrides at the counter (§2.9)
  count in the counter's slice, and an override is blocked when the slice is
  full. **A payment leaves the slice only at its first confirmation or by the
  owner's action, never on a timeout.** After the first confirmation its risk
  is a reorg, which §2.7 and the refund depth handle. So a busy shop's cap does
  not fill up with payments that are already in a block.
- **The owner sees the worst case.** "If every instant sale failed you would
  lose at most $X" is shown at setup and in the settings, where X is the
  exposure budget plus the override ceilings (the cashier daily caps and the
  manager ceilings).
- **Fee bump versus cancel.** A replacement that still pays the same address at
  least the same amount is a fee bump, not a cancel. Anything else is a cancel.
- **Pause after a cancel: 1 hour.** Instant is off for 1 hour. A second cancel
  within 24 hours pauses instant until the owner resets it on the owner phone.
  Pauses appear in the weekly line. This replaces the old separate 24-hour
  count.
- **No instant for remote or online orders.** Instant is for customers at the
  counter only. Gateway and online orders wait for confirmation.
- **Light check needs the right network.** If customers can join the Wi-Fi,
  instant runs only on cellular or with Tor on **[verify latency, E3]**, and
  peers must come from distinct network groups (§5.1). Otherwise instant is
  off.
- **An unhealthy check turns instant off** (§5.1). So does a missing watcher
  heartbeat: "Instant off: no independent watcher".
- **Hosted-Box orders without a counter signature are never fulfilled
  automatically** (§5.3).

**What a light client can actually check for T1.** A light client sees
unconfirmed transactions only as peers relay them. It cannot see the amounts of
a payment's inputs, cannot confirm that its inputs are confirmed, and cannot
compute its fee rate. So the light-check T1 does these, and only these:

- watch for conflicting spends of the same coins;
- require that the peers reporting the payment come from distinct network
  groups.

It does not claim to have checked the fee or the inputs. (Asking peers whether
the parent transactions are in their mempool was dropped: a confirmed parent
and an invalid one both answer "not found", so it tells us nothing.) With the
merchant's own node, the node does check them, and the counter accepts the
node's answer only with a merkle proof for confirmation (§5.1). The own-node
label for an unconfirmed payment needs the counter's own light client to have
seen it too.

A fake payment and a cancelled payment cost the shop the same capped amount. So
the cap is the real protection, and the label keeps it honest.

No public measurement of double-spend rates since full-RBF exists. The best
older numbers are GAP600's 2022 sample (about 0.26% of 4.3M transactions
double-spent, mostly ones that signaled replaceability) and Bitrefill's
pre-full-RBF experience. So T1 ships with low defaults that the shop can raise.
We should publish our own measurements (E7).

### 6.1 Dropped

- **Prepaid tabs:** rejected 2026-11-09.
- **Co-signed instant payments (Linus protocol, threshold co-signers,
  bonds):** dropped 2026-11-09 under the purist veto. They need merchants to
  trust a signer set ("reinvented PayPal"), give the co-signer freeze power,
  and leak every spend to it. The research note
  (docs/research/instant-cosigner-2026-11-09.md) is kept as a record only. No
  slot for co-signer attestations is built into the terminal.

Without the customer's cooperation, nobody can make an unconfirmed on-chain
payment uncancellable. OpenBMT's answer is honest: instant only within the
shop's own limit, an explained wait above it, and catch-and-proof (§6.2).

### 6.2 Catch and proof

If a payment is replaced or cancelled **in a way that is relayed over the
network**, the counter device and the owner phone see the conflicting
transaction and alert. A conflict sent **privately to a miner** is not relayed.
It is seen only when the block containing it arrives, which can be minutes
later. The design does not claim to detect those within seconds.

The device saves a signed record: the original payment, the replacement, times
and amounts. That proves the payer's coins paid and were then pulled back. It
turns a silent loss into a documented one, the way cards rely on traceability
rather than on making fraud impossible.

## 7. Tap to pay: the OpenBMT Tap spec

- The counter device emulates an NFC Forum Type 4 tag (Android host card
  emulation, AID D2760000850101, category `other`), read-only. Numo's `ndef/`
  package is the reference for the mechanics.
- **Android customer:** record 1 is a URI record holding the BIP321
  `bitcoin:` URI. Android dispatches on the first record, so a wallet that
  registered for `bitcoin:` opens.
  - Android 16 adds a per-app NFC allowlist. Android 17 requires the
    `DISPATCH_NFC_MESSAGE` permission. Both can silently stop a wallet from
    opening **[verify, E1]**.
- **iPhone customer: scan the QR.** iOS background reading ignores custom
  schemes, so `bitcoin:` cannot launch a wallet from a tag. Draft 1 used an
  https page to bridge this. That page is **removed**. Whoever serves it could
  change the address for every iPhone sale, and it needs a third-party server.
  iPhone customers scan the QR (1 scan + 1 confirm), which is fewer steps than
  the old path and needs no page.
- **Empty when idle.** The tag holds no payment when no sale is open.
- **Shop identity:** BIP 353 names (`pay@joescoffee.com`, DNSSEC-signed) can
  later let wallets show who is being paid. Per-sale signed payment requests are
  a later extension (§15); no standard exists.
- **Fake sticker over the device.** A fake sticker can show a different QR or
  tag. The counter device shows "Not paid yet", so the shop is not fooled, but
  the customer loses money. The defense is optional protection: the customer
  checks the amount, the first characters after `bc1q` and the last characters
  of the address on the counter screen against their wallet (§2.3); the device
  screen sits behind a tamper-evident cover **[to design]**; staff check the
  counter for stickers at open and close.
- **Customer-side emulation.** A phone in the queue can emulate a payment tag
  toward other customers. The Android customer's wallet shows the address and
  amount before it signs, and the customer checks them. Experiment E10 tests
  whether a hidden or emulated tag can be told from the real one by the
  counter's reader mode.

## 8. Small sales and Lightning-only customers

The on-chain fee is flat: about $0.11–0.42 at 1–3 sat/vB for a typical
one-input payment **[verify]**. That is fine for a $20+ sale and heavy on a $5
coffee. Customers with Lightning-only wallets cannot pay an on-chain address
at all.

OpenBMT is on-chain by default and says so on the screen: "This shop takes
on-chain bitcoin." Shops can offer a bitcoin price that absorbs the network
fee on small tickets (§9).

**Option: connect your own Lightning node** (decided 2026-11-09).
A merchant who already runs Lightning (Core Lightning, LND or Eclair on their
own Box, Start9 or Umbrel) can connect it. The counter then offers a BIP321
code with both on-chain and Lightning, and Lightning payments are final
instantly at any amount. OpenBMT does not manage the node; the setup screen
says plainly that running Lightning means watching liquidity, updates and
backups. The self-managing node was researched and rejected: it cannot be
hands-off, because receiving consumes inbound liquidity that must keep being
bought (docs/research/merchant-lightning-node-2026-11-09.md).

**Design rule:** keep the payment-method layer pluggable, so a covenant-based
layer 2 can be added if one ever activates.

Dropped 2026-11-09 under the purist veto: Lightning-in via a third-party swap
provider (for example Boltz). It makes every such sale depend on a central
service that sees amounts, timing and both sides. Boltz paused its service
after a wave of attacks, which showed how fragile that is.

## 9. Fees and the merchant pitch

- Cards cost a merchant 1.5–3.5% plus chargebacks, at $15–30 each in processor
  fees alone **[verify against primary sources before quoting; not on
  merchant screens until sourced]**.
- OpenBMT charges **no processor fee on receiving**. The customer pays the
  network fee to send. That is not the whole cost of ownership: **spending
  each receipt later costs the merchant network fees.** A 2-of-3 input is about
  104 vB, against about 68 vB for a single-key input, so each receipt adds about
  104 vB when it is spent. In a fee spike a small receipt can cost more to spend
  than it is worth.
- **The app shows the projected spend cost.** In Owner mode, the books and
  cash-out screens show "Spending your current receipts would cost about $X at
  today's fees".
- **Consolidate when cheap.** When fees are low, the app suggests merging small
  receipts into fewer outputs. This is a suggestion, never automatic. It
  merges receipts, so it also has a privacy cost (§11).
- **Size cap by PSBT bytes.** One transaction is capped by the size of its
  PSBT in bytes, not by input count, because each input carries a full parent
  transaction (`non_witness_utxo`) and the signers read it by QR or file. The
  byte cap is set from E8 **[to measure]**. Coin selection prefers inputs with
  small parents. A larger cash-out is split into several transactions that stay
  under the standard size limit of 100,000 vB. Taproot would later remove the
  parent-transaction need.
- **Speed up.** If a refund or cash-out is stuck, one tap on "Speed up" builds
  a higher-fee replacement. It needs the same signers as the original, so if
  key 2 is away it waits for the next approval session (§2.10).
- A shop can offer a bitcoin price a few percent below the card price and
  still come out ahead.
- Funds have no chargebacks or reserves, and they arrive in minutes instead of
  1–2 days. Instant sales carry the cancel risk of §6.

## 10. Books and tax

Per IRS Notice 2014-21, bitcoin received for goods is income at its dollar
value on receipt, and that value is the merchant's basis. Sales tax is charged
on the dollar price as usual. A merchant receiving directly gets no 1099
forms for those sales, so the books are entirely theirs, and the export is a
core feature. **US rules; other countries are not covered.**

- **Owner-held copy.** The owner phone holds a full copy of the books. The books
  do not live only on the counter and the Box.
- **Monthly outside check (recommended).** Once a month, the owner opens the
  kit's wallet description as watch-only in Sparrow or Bitcoin Core and
  compares the receipts with the POS bitcoin-tender total. It takes about 5
  minutes **[to measure]**. It is the only check that does not depend on Avila
  Labs software, and so the only one that catches a bad counter or main-app
  build that shows its own address and says Paid. The app reminds the owner and
  shows the steps.
- **Scheduled encrypted export.** On a schedule, the app writes an encrypted
  export to a place the merchant picks (a file folder, a USB drive, the
  Box). The merchant, not OpenBMT, holds the key. Each export carries the
  current address **high-water mark**, so a rebuild knows the range to import.
- **Rebuild-from-chain tool.** If every device is lost, the wallet description
  from the recovery kit can rebuild the receipts from the blockchain. Sale
  detail (order IDs, tax) can come back only from an export or the POS.
- **Per sale:** UTC time, opaque sale ID, dollar amount, sales tax, sats
  received, rate and rate source, dollar value at first-seen time.
- **Opaque sale IDs only in accounting exports and POS fields.** Transaction
  IDs, addresses and amounts in satoshis are never written into the CSV for
  an accountant or into any field of the POS. Anyone who has them could link
  the shop's payments on the blockchain. The only place a customer sees a
  transaction ID is the receipt they ask for (§2.3).
- **Export 1:** a QuickBooks/Xero-ready CSV (Date, Description, Amount), split
  by month. It uses the opaque sale ID.
- **Export 2:** the full ledger CSV, with transaction IDs and addresses. It is
  behind a warning: "This file reveals the shop's payments on the blockchain.
  Share it only with people you trust with that."
- **Refunds and spend fees are disposals.** Sending bitcoin out (a refund, a
  cash-out, or a network fee paid in bitcoin) is a disposal for tax. The books
  record each with its basis and a dollar value at the time. The export has
  lines for them. A cash refund from the till is recorded as a return.
- **Form 1099-DA.** Brokers began reporting digital asset sales on Form 1099-DA
  for 2025 **[verify the scope and dates]**. A merchant who sells bitcoin on an
  exchange may get that form. The exchange reports, not OpenBMT. Our export is
  not tax advice.

## 11. Cash-out

Nothing converts a share of receipts automatically while keys stay cold, and
we will not have any server do it (legal line, §5.3). Instead:

1. Owner mode prepares a weekly transaction sending the share the owner chose
   to a saved destination and queues it.
2. The owner approves it in the approval session (§2.10): key 1 on the owner
   phone, then key 2 on the Key app, which checks the destination against its
   own allowlist and its satoshi ceilings.

- **Default screen:** "Cash out $1,850 to Kraken (saved). Fee about $X.
  Approve." Nothing else is shown by default.
- **Destinations.** Cash-out addresses are enrolled on the Key app's own
  screen with a label the Key app shows itself, and mirrored in a signed
  address book on the owner phone. The signing screens show the label in plain
  words, plus the raw address for those who check. A new destination is
  loosening (§2.9). It is enrolled only on the Key app, by scanning the
  exchange's deposit QR with its camera, and is active after 24 hours on the
  Key app's clock (§5.4). Allowlisted destinations also have satoshi
  ceilings there.
- **Coin control is under Advanced.** The owner can choose which receipts to
  spend. The default selection minimizes the number of linked receipts. A
  privacy line ("links N receipts") appears only past a threshold, because
  spending many receipts at once links them on the blockchain. Anyone who paid
  once, and the exchange, can see the cluster and estimate the shop's revenue.
- **Exchanges can freeze deposits.** Some exchanges freeze or question deposits
  based on where the coins came from. OpenBMT cannot check this and shows no
  warning for it. The exchange account and its rules are the merchant's.
- **Broadcast over Tor by default.** Refunds and cash-outs are sent through Tor
  so the owner's network address is not linked to the wallet. This is a
  **separate setting** from the counter's Tor switch. It has no speed cost,
  because spending is not time-critical. The counter's Tor switch stays off by
  default. If Tor is blocked, the app broadcasts through the merchant's own
  node, or asks the owner what to do. It never falls back silently.
- **Fees:** see §9 for the size cap, the projected cost and the speed-up.

The exchange account, its compliance and the bank withdrawal are the
merchant's. The cash-out is recorded in the books with its basis (§10).

## 12. Threat model

Each row maps to a finding in the round-1 review (S1–S17), the round-2
review (A, W, U, P) or the round-3 check (R3-1 to R3-10), or to an existing
row, corrected.

| Threat | Review | Effect | Mitigation | Section |
|---|---|---|---|---|
| Setup plants or keeps a key; both devices compare addresses from the same planted description | S1, A2, W1 | Funds go to an attacker | Each key created on its own device; the Key app checks key 2 and key 3 slots byte for byte; same 4-word checksum and three fingerprints on every device; pairs screen; kit prints the checksum | §2.1, §5.4 |
| Key 3 made on an online, ordinary phone; words typed through the system keyboard | A3, W5, U1 | Seed stolen | Key 3 made with another publisher's app on the counter tablet, offline, then factory reset (or a hardware wallet); key 2's in-app word picker; FLAG_SECURE; environment checks, stated as promises of the software; honest note that the phone could keep the words | §2.1 |
| One bad update signs on both key 1 and key 2; owner phone is the only authority | S2, A1, A4, A5 | Both screens show the same lie | Key app is the second authority for descriptions, pairings, batches, destinations and loosening; separate code and release key; no auto-update; checksum compare after updates | §2.10, §5.4, §13 |
| Counter learns its roots from Owner mode | A1 | Fake roots pinned | Counter shows its own QR, the Key app scans it; counter pins both roots and shows them as 4-word fingerprints; two-root-signed device list; re-pairing needs both | §2.9 |
| Malicious PSBT builder: fake change, false input amounts, blind signing, huge change index | S3, W11 | Funds or fees stolen | Own registered description, `non_witness_utxo`, fee ceiling, change-index bound against high-water mark, separate display | §5.4 |
| Malicious Key app build leaks data; "no internet" overclaim | P8, A9 | Secret leaked | Honest wording; RFC 6979 and anti-exfil signing; refuses root, USB debugging, accessibility, overlays | §5.4 |
| Phone event silently kills a key; Auto Backup restores a dead file; PIN only a screen gate | S4, W15 | Two of three keys lost; or key stolen | Memory-hard KDF plus Keystore; escalating delays and wipe; not bound to biometrics; excluded from backup; dead-key detection; health check in sessions | §5.4 |
| Restore without OpenBMT fails; heir has one key; kit stale or missing birthday | W2, W3, W13, W14 | Funds unrecoverable | BIP39 keys; key 2 words by default; kit with birthday, generations, import range, checksum, fallback descriptors; Sparrow path; E9 | §5.4, §5.5 |
| Yearly drill re-exposes key 3; kit is a map to the keys | W6, A15 | Seed exposure | Physical inspection default; optional drill in airplane mode; kit holds hints only; offline print | §5.4, §2.1 |
| Two keyholders collude; someone who left keeps a key; robbery reaches two keys | S5 | Funds emptied | Pairs screen; "move to a new wallet" flow with re-pairing; key 2 at home as default advice; counter never shows balances; time-locked vault researched | §2.1, §5.4, §15 |
| Refund then cancel; overpay into an instant sale | S6 | Goods or cash lost beyond the cap | Separate refund depth; `max(sale, received)`; refund ≤ received | §2.4, §6 |
| Refund destination swapped; habit approvals; warning fatigue; refund counts instead of dollars | S9, A4, U3, A10 | Funds to attacker | Plain-address refund QR; Key app satoshi ceilings for unknown destinations; daily dollar cap per cashier; manager PIN threshold; anomaly-only warnings; batched refunds in a session; cash-from-till option | §2.4, §2.10 |
| Cash-out destination swapped | S9, A4 | Funds to attacker | Key app allowlist enrolled on its own screen with its own labels | §11, §5.4 |
| Fake payment fed to a light client on shared Wi-Fi; peers all one attacker; private-to-miner conflict | S7, U8 | False "Paid" | Lower cap and label; cellular or Tor on shared Wi-Fi; distinct network groups; honest limits of a light client; conflict at next block is stated | §5.1, §6, §6.2 |
| "Paid (your node checked it)" unverifiable for unconfirmed payments; useless parent-mempool check | A7, W12, P4, P6 | False higher cap | Own-node label only if the counter's light client also saw it; else "Paid (your Box reports it)" at light caps; mempool check dropped | §5.1, §6 |
| Sustained instant fraud; colluding cashier; online instant | S8, U7, A13 | Repeated losses | Shared exposure including overrides; pause 1 h with escalation; alarm not dismissable at counter; no instant for remote orders; override ceilings; check-ins | §2.9, §2.7, §6 |
| Instant cap fills at a busy counter | U2 | Instant unusable | Exposure leaves the total at first confirmation; worst-case line shown | §6 |
| Cheap shop-wide pause by a cancel; fee bump looks like a cancel | A11 | Denial of service | Same-address same-amount replacement is a fee bump; 1-hour pause then owner reset on repeats | §6 |
| Dead owner-phone watcher looks like safety | A14 | Cancels missed | Heartbeat from the owner phone, signed with a pinned key, required for instant | §5.1, §6 |
| POS "paid" with no money; hacked Box or host marks orders paid; voids | S10, A6 | Goods released without payment | Owner phone polls the POS itself with a read-only sign-in and matches every order; Box posts "paid" only after counter signs; hosted option labeled; connector revocation | §5.3 |
| Settings changed at the counter; forged sync; edited records; delays with no trustworthy clock | S11, A5, U6 | Limits raised, destinations changed | Roles; loosening needs the Key app co-signature; block-count fallback; two-root pairing; signed hash-chained records; check-ins | §2.9 |
| Hacked Box fakes "Confirmed (your node)" | S12 | False confirmation at any size | Counter keeps its own header chain; merkle proof; 64-byte transaction and depth checks | §5.1 |
| Address reuse; reset counter reissues; batch cliff; address burning | S13, U5, A12 | Wrong sale paid; sales stop | Never reuse; contiguous ranges; high-water mark; about 8-week batches; same address per open sale; rate limit; empty range means no Bitcoin; new batch after reset | §5.1, §5.5 |
| Receipts linked at cash-out; txids in exports; broadcast IP; push metadata; rate-fetch timing; telemetry; contacts unlisted | S14, P1, P7 | Privacy loss | Minimized linked receipts; opaque IDs; Tor broadcast; batches; content-free pushes; scheduled rate fetches; no analytics; "Who sees what" lists every contact | §4.1, §10, §11, §13 |
| Veto wording bans the hosted Box and FCM fallback | P3 | Design contradiction | Precise veto wording; optional metadata-only services allowed if not default, labeled and listed | §0, §4.1 |
| Single publisher; 14 days unenforceable offline; Play as single point | P2 | Malicious release | Own APK and F-Droid only; `earliest_install_date` shown; urgent-fix flag; "built by Avila Labs only" until two rebuilders match; one independent rebuilder before launch | §5.4 |
| Taint warning needs third-party data; unsourced claims on screens | P5, P6 | Leak or false claim | Taint warning removed, honest statement; figures marked [verify] and kept off screens | §11, §3, §9 |
| Spending costs; dust; no fee bump; PSBT too big for QR | S15, A8, W10 | Merchant loses margin; spend impossible | Projected cost; consolidate suggestion; byte cap; small-parent selection; USB-C/SD file; BC-UR; speed-up in sessions | §9 |
| Offline counter; webhook exposure; stall; clock skew; stuck payment; books lost; reorg; non-idempotent retries; failed migration | S16 | Stranded sale or lost record | Pre-check; outage ladder; polling; unhealthy state; clock check; stuck and reorg states; owner-held books; idempotent calls and queue; updates off-hours, atomic migrations | §2.6, §5.1, §5.3, §10 |
| Customer cannot prove a payment | S17 | Dispute | Counter-signed receipt with txid on request | §2.3 |
| Stale tag; fake tag; no tolerance; fake "Sent" screens; price option; refund arbitrage; dust; wording confusion | S17, U7, U12 | Customer or cashier misled | Empty tag; first and last characters as optional check; tolerance; red screen with plain wording; 5-minute lock; return window; ignore sub-threshold; disposal lines | §2.2, §2.3, §2.4, §7, §10 |
| Counter device stolen | — | Only the batch's addresses leak; no funds at risk | Encrypted at rest; holds no key and no wallet description; owner removes it in Owner mode (the next address batch goes to the new device) | §5.1, §2.9 |
| Owner phone stolen | — | One key of three; cannot spend alone | "Move to a new wallet" with the other two keys; recovery kit steps | §5.4 |
| Malicious app update | — | Address swap | Reproducible builds, signed releases, batches co-signed by the Key app, checksum comparison after updates | §5.4, §13 |
| Customer double-spend | — | Goods lost | Tiers and limits | §6 |
| Network isolation | — | False "seen", missed conflict | Distinct network groups; own node; unhealthy state turns instant off | §5.1 |
| Rate source manipulated | — | Wrong price | Two or more sources, deviation check, locked per sale, fetched on a fixed schedule | §2.3 |
| POS sign-in token stolen | — | Orders altered, no funds | Least-privilege scopes; polling; the owner phone matches orders to funds | §5.3 |
| Lost recovery kit and one key | — | Two keys needed to spend: one key alone can never spend, but nothing can be spent if two are lost | Kit reminders; yearly key 3 inspection; health check; prompt to move to a new wallet when any key is lost | §5.4 |
| Key 2 and key 3 born in the same app on the same phone; a bystander sees both sets of words | R3-1 | One component or person holds two keys | Key 3 made with another publisher's wallet app on the counter tablet before it becomes the counter, in airplane mode, or on a hardware wallet; tablet factory reset; only its xpub reaches the Key app; separate sittings; "do this step alone". Launch condition 1 | §0, §2.1, §5.4 |
| One publisher holds both release keys; Android cannot block a bad update of an installed app | R3-2 | Malicious pair of releases defeats the two-app separation | Key app release key held by a second, independent person (launch condition 2, second holder not yet named); until then the doc and chooser say one publisher signs both and recommend a hardware wallet or another publisher's signer for key 2; vendored crates reviewed by someone else; independent rebuilder; no automatic updates | §0, §5.4, §16 |
| Counter or main-app build shows its own address and says Paid; owner-phone matching runs the same app | R3-3 | Sales proceeds lost until noticed | Monthly outside check in Sparrow or Bitcoin Core against the POS bitcoin-tender total; claim restated as protection up to the caps and until reconciliation | §0, §10 |
| Allowlist poisoning; no ceiling on allowlisted destinations | R3-4 | Funds to attacker | Enrolled only by scanning the deposit QR with the Key app's camera, label typed there, active after 24 hours on the Key app's clock; satoshi ceilings also on allowlisted destinations | §5.4, §11 |
| Dollar ceilings on an offline Key app; restore mode; fee measured on value sent | R3-5 | Ceilings meaningless or bypassed | All ceilings in satoshis set on the Key app, per transaction, per day and per week; dollars labeled "from the owner phone"; restore-and-sign enforces ceilings; fee ceiling on total input with a hard bound | §5.4 |
| Key 2 is a hardware wallet: batches, settings and pairings need only key 1 | R3-6 | Owner phone alone loosens limits | Key app as an approver on a spare device (identity key only); without it, the screen says these rest on the owner phone plus spot checks | §5.4, §2.10 |
| Counter named by the owner phone at pairing; Box has no pinned identity | R3-7 | Fake counter or Box accepted | Counter shows its own pairing QR, the Key app scans it; 4-word fingerprints of both roots; Box key on the two-root-signed device list | §2.9, §5.1 |
| Key app claims it cannot back: sale records, wipe, environment checks, anti-exfil | R3-7 | Overclaim | Stated as owner-phone assertions and promises of the software itself; anti-exfil needs host-verified entropy [verify protocol] | §5.4 |
| Box or host: faked heartbeat; exposure release unspecified; shared total with no coordinator | R3-8 | Instant exposure unbounded or hidden | Heartbeat only from the owner phone with a pinned key; release only at first confirmation or by owner action; per-counter signed slice of the budget; hosted-Box orders without a counter signature never fulfilled automatically | §5.1, §5.3, §6 |
| Cashier overrides | R3-9 | Exposure grows past the budget | Overrides blocked when the exposure total is full; daily override cap per cashier | §2.6, §2.9, §6 |
| Key app input parsers | R3-10 | Memory-safety bug in a signer | Memory-safe parsers with strict size limits; fuzzing as a launch condition; QR default, file transport optional | §5.4, §13 |

## 13. Stack

Pinned as of 2026-11-09:

- Rust core:
  - `bdk_wallet = "=3.2.0"`
  - `bip157 = "=0.8.1"`. Kyoto, renamed from `kyoto-cbf`. Young, with
    breaking changes every minor release, so it is pinned exactly and gets
    fuzz and soak tests. We wrap its gossip monitor ourselves.
  - `bitcoin 0.32.x`
  - `uniffi = "=0.31.2"`
- Android: Kotlin UI, `HostApduService` for the tag, foreground service for
  the watch.
- OpenBMT Box: Rust, on Bitcoin Core v30 (pruned) via RPC/ZMQ; packaged for a
  mini PC image, Start9 and Umbrel. UnifiedPush (or ntfy) runs on it.
- OpenBMT Key app: Kotlin shell plus a minimal Rust crate set: key handling,
  PSBT decode and check, BC-UR and QR. No network crates and no network
  permission. The crate list is short, vendored, and reviewed by hand by
  someone other than the main app's author. Parsers are fuzzed (§5.4).
- Batch-signing keys use hardened derivation or a separate key, so a leaked
  batch-signing key never exposes the wallet.
- Not reused: Avila Node's P2P layer. It drags in a full node's dependencies,
  and `bip157` already has the mempool watch.

**Supply chain.**
- `Cargo.lock` is committed. Builds are reproducible.
- `cargo-vet` and `cargo-deny` run in CI. A new dependency needs a review.
- Gradle dependency verification is on.
- Docker images (the Box) are pinned by digest.
- The Key app has the smallest crate set we can manage, and each crate is read
  before it is added.

**Distribution.**
- The OpenBMT app: our own APK, signed with our release key and published with
  its hash, and F-Droid, alongside Google Play.
- The Key app: our own-signed APK and F-Droid only, never Play (§5.4).
- Google's developer verification and Play App Signing could make the release
  channel a single point of failure. The dates and rules are not yet checked
  **[verify dates; verify that Play App Signing is mandatory for new apps]**.
  Because we publish our own signed APK and F-Droid builds, a merchant does not
  have to rely on Play.
- The Key app has its own release key, held by a second person (§5.4, Q3).

**No analytics and no crash upload.** The apps send no analytics or crash data
to anyone. Every contact they make is listed in §4.1. Update checks are manual
or against the published hash the merchant downloads. A merchant who wants
crash reports exports a local log and sends it by hand.

## 14. Experiments

Each gets a written protocol before it runs.

| ID | Question | Needs |
|---|---|---|
| E1 | Which Android wallets open from an emulated `bitcoin:` tag, on Android 15, 16 and 17? | Two Android phones |
| E2 | Retired: iPhone is now QR-only (§7). Kept as a note that the tap path for iPhone was ruled out by design. | — |
| E3 | Time to "Seen", bandwidth and battery for the light client on a phone, and via the merchant's own node; with and without Tor. **Also:** latency of light-check instant on shared Wi-Fi with Tor on; how Kyoto relays unconfirmed transactions, what a light client sees of conflicts in peers' mempools, and how many filter-serving peers exist | One Android phone; a node |
| E4 | Can an app on a PAX or Sunmi terminal drive the contactless reader directly? | Hardware purchase |
| E5 | Square sandbox: external CRYPTO payment closes an order with receipt and inventory. **Also:** can a payment be undone after PayOrder (un-pay)? Polling versus webhooks; OAuth scopes; a read-only sign-in for the owner phone | Square developer account |
| E6 | Clover sandbox: custom tender button, order close, refund. **Also:** can a payment be undone after PayOrder? Polling versus webhooks; a read-only sign-in | Clover developer account |
| E7 | Double-spend attempts seen by our mempool watch over weeks, to tune T1 limits | Our node |
| E8 | Hardware-wallet descriptor registration and signing: does each (Coldcard, Jade, Passport, Keystone) accept our P2WSH 2-of-3 wallet, check change against it, and require `non_witness_utxo`? **Also:** BIP389 `<0;1>` support and the single-path fallback; BC-UR support; PSBT frame counts and the byte cap for QR; the address explorer for owner-picked indexes; USB-C and SD file transport on the Key app | The four wallets |
| E9 | Restore drill: can a wallet be restored from the recovery kit alone in Sparrow and in Bitcoin Core with a block filter index? Does `<0;1>` import work, or does the fallback? Does the explicit import range find every address? | A laptop, the kit |
| E10 | Hidden NFC sticker versus emulated tag: can a hidden tag near the counter be detected by the counter's reader mode? What does an Android wallet show when two tags answer? | Two Android phones, sticker tags |

## 15. Research tracks

- R2: US cash-out fees, verified on official pages.
- R3: state money-transmission law for a keyless operator, with New York first.
- R4: OpenBMT Box packaging: mini PC image, Start9 and Umbrel apps, pruned-node sync time.
- R5: encrypted record sync design. Includes a **no-Box sync transport**: a
  relay that sees only ciphertext, so a merchant without a Box can still sync.
  Also the Box reached over an onion service or the local network.
- R6: Clover App Market and Lightspeed gateway approval rules for a bitcoin
  tender.
- R7: phone key storage. How a secp256k1 key is protected when Android and
  iOS secure hardware support only other curves; prior art in mobile
  multisig wallets (Nunchuk, Bitkey, Blue).
- R8: **time-locked vault** (miniscript, a Liana-style path): a recovery path
  that opens after a long delay. Stops a thief who reaches two keys from
  emptying the wallet at once. Researched before it is offered.
- R9: **iPhone owner app.** Owner mode is Android-only in v1. An iPhone version
  needs a plan for key storage and background watching.
- R10: **per-sale signed payment requests**, so a wallet can show who is being
  paid. No standard exists today.

## 16. Decisions and open questions

- **D1. Lightning-only customers and small sales.** Decided
  2026-11-09: v1 is on-chain only and says so on the screen. Swaps were later
  dropped under the purist veto (§8).
- **D2. Keys.** Decided 2026-11-09: the merchant chooses, every
  option is secure and purist-acceptable, and the recommended second key is a
  device the merchant already owns (§5.4). A key card on the owner's own phone
  and a single-key mode are never offered.
- **D3. Instant limit defaults.** Decided under Q2 (below):
  $10 per sale and $50 outstanding on the light check; $25 and $100 with the
  merchant's own node. Shop-adjustable, until our own data (E7) says otherwise.
- **D4. Small-refund allowance.** Dropped 2026-11-09 as
  unfeasible.
- **D5. Hardware and accounts.** Not needed for v1. A PAX or Sunmi terminal
  (E4) is only for a later "runs on your card terminal" option. Square and
  Clover sandboxes (E5, E6) are needed only when those connectors are built.
  The gateway and manual connectors need neither. Phones for E1–E3 are
  needed now. Hardware wallets for E8 are needed before the key ceremony is
  built.

**Security-review decisions (Q1–Q4).** Decided on 2026-11-09.

- **Q1. iPhone: scan, not tap.** Tap is Android-only. iPhone customers scan the
  QR. The https payment page is removed (§7).
- **Q2. Instant on the light check: kept, tighter and labeled.** $10 per sale
  and $50 outstanding with the light check ("Paid (light check)"); $25 and $100
  with the merchant's own node, only when the counter's light client also saw
  the payment (§6).
- **Q3. Release keys.** The Key app has its own release key, kept offline on a
  device that never builds or publishes. Round 3 adds that it is held by a
  second, independent person (a launch condition, below). Releases are
  reproducible, published with their hash, never auto-update, and carry an
  `earliest_install_date` that Owner mode shows (§5.4). This replaces the fixed
  14-day delay.
- **Q4. "Someone runs it for me": kept,** labeled honestly ("they can see your
  orders and revenue; they never touch your bitcoin"), not the default, with the
  owner phone's order matching catching any false "paid" (§5.3).

**Round-2 decisions (2026-11-09).**

- The Key app is the second authority for everything in one approval session
  (§2.10). The 24-hour delays are removed; loosening needs Key app co-signing,
  with a block-count delay only when key 2 is a hardware wallet (§2.9).
- Setup order: key 3 outside OpenBMT software, then key 2 on the spare phone in
  the Key app, offline; key 1 on the owner phone; 7 steps (§2.1). Round 3
  replaced the first part (key 3 in the Key app on the same phone).
- Key 2's backup words are written by default; all keys are BIP39 (§5.4).
- Batches cover about 8 weeks; instant exposure leaves the total at first
  confirmation (§5.1, §6).
- Refunds are queued and paid in sessions, with a cash-from-till option (§2.4).
- Pause after a cancel is 1 hour, then an owner reset on a repeat within 24
  hours (§6). The second-cancel rule is a first setting, to be tuned.
- The Key app ships only as an own-signed APK and on F-Droid; one independent
  rebuilder is a launch condition (§5.4).
- Defaults marked **[to tune]** (block delay 144, PIN length 6, check-in
  interval 15 minutes, heartbeat 10 minutes, wipe after 10 tries) are first
  guesses, to be tuned with trials.
- **The §0 veto wording.** It is now precise: metadata-only optional services
  are allowed if not default, labeled and listed. This matches how the veto
  was applied on 2026-11-09 (the hosted connectors-only Box survived the
  purist audit).

**Further technical decisions (2026-11-09).**
- The Key app is the default key 2 software.
- Key 3 is made offline on a device that holds no other key, with software
  that OpenBMT did not publish (round 3, §2.1).
- The counter gets signed address batches, not the wallet description.
- Phone-key rules (§5.4).
- Refunds and cash-outs go out over Tor by default.
- Owner mode is Android-only in v1.

**Round-3 decisions (2026-11-09).** They fix findings R3-1 to R3-10
(docs/research/security-review-round3-2026-11-09.md).

- Key 3 is made with a different publisher's wallet app on the counter tablet
  before it becomes the counter (or on a hardware wallet), then the tablet is
  factory reset; only its xpub reaches the Key app (§2.1). This corrects the
  round-2 draft, which put keys 2 and 3 in one app on one phone and broke Rule 1.
- Key app ceilings are in satoshis, set on the Key app, and include a weekly
  ceiling and allowlisted destinations. New destinations are enrolled by direct
  scan on the Key app and wait 24 hours on its clock (§5.4).
- The Key app is an approver when key 2 is a hardware wallet (§5.4).
- Pairing is scanned by the Key app from the counter; the Box key is on the
  signed device list; the heartbeat comes only from the owner phone (§2.9,
  §5.1).
- Exposure is released at first confirmation or by the owner; each counter has
  a signed slice; hosted-Box orders without a counter signature are not
  fulfilled automatically; cashiers have a daily override cap (§6).
- A monthly outside check is recommended (§10).
- Parser fuzzing is a launch condition (§5.4).

**The two launch conditions** (§0). The claim that no single component or
person can spend the wallet holds only when both are met:

1. Key 3 is made outside OpenBMT software and away from the key 2 device
   (built into setup, §2.1).
2. The Key app's release key is held by a second, independent person.
   **The second holder has not been named yet.** Until then the doc and the key
   chooser say that one publisher signs both apps.

## 17. Sources

- FinCEN FIN-2019-G001, §4.2.2 (multisig), §4.5.1(b) (software suppliers):
  https://www.fincen.gov/sites/default/files/2019-05/FinCEN%20Guidance%20CVC%20FINAL%20508.pdf
- California DFAL, Fin. Code §3102(c) (control), §3103(b)(7) (exemptions)
- IRS Notice 2014-21: https://www.irs.gov/pub/irs-drop/n-14-21.pdf
- BIP 321: https://bips.dev/321/ · BIP 353: https://bips.dev/353/
- BIP 48 (multisig derivation, script type 2'), BIP 389 (multipath
  descriptors), BIP 158 (compact block filters): https://bips.dev/
- Security review of draft 1 (this repo): docs/research/security-review-2026-11-09.md
  (Android Keystore invalidation and backup behavior; BIP158 limits; Bitcoin
  Core 29.0 and 30.0 release notes; CVE-2020-14199 as cited there)
- Security review of draft 2, round 2 (this repo):
  docs/research/security-review-round2-2026-11-09.md
- Single-component check of draft 3, round 3 (this repo):
  docs/research/security-review-round3-2026-11-09.md
- Apple, background tag reading:
  https://developer.apple.com/documentation/corenfc/adding-support-for-background-tag-reading
- Android NFC dispatch: https://developer.android.com/develop/connectivity/nfc/nfc
- Android HCE: https://developer.android.com/develop/connectivity/nfc/hce
- Numo: https://github.com/cashubtc/Numo (spec: docs/NDEF_Payer_Side_Spec.md)
- Kyoto / bip157: https://github.com/2140-dev/kyoto
- bdk_wallet: https://github.com/bitcoindevkit/bdk_wallet/releases ·
  bdk-ffi v3.1.1: https://github.com/bitcoindevkit/bdk-ffi/releases/tag/v3.1.1
- Clover custom tenders: https://docs.clover.com/docs/creating-custom-tenders-apps
- Square external payments:
  https://developer.squareup.com/docs/payments-api/take-payments/external-payments
- BTCPay Lightspeed plugin: https://github.com/TChukwuleta/BTCPayServerPlugins
- BTCPay Shopify V2: https://docs.btcpayserver.org/ShopifyV2/
- Robin Linus, zero-conf protocol:
  https://gist.github.com/RobinLinus/b9fcbb0e5de19ec6b9a80aa848a94253
- Ruffing, Kate, Schröder, CCS 2015:
  https://www.cs.purdue.edu/homes/akate/publications/Non-equivocationWithBitcoinPenalties.pdf
- Lark equivocation bonds (Delving, 2026-08-18):
  https://mirror.b10c.me/delvingbitcoin-org/2026-08-18-improving-the-security-of-lark-oor-channels-with-equivocation-bonds-id2816.html
- Boltz reverse-swap lifecycle:
  https://github.com/BoltzExchange/boltz-backend/blob/master/docs/lifecycle.md ·
  pause:
  https://cointelegraph.com/news/boltz-pauses-service-after-wave-of-ai-assisted-hacking-attempts
- Blockstream Swaps: https://blog.blockstream.com/announcing-blockstream-swaps/
- Stacker News items 112663, 408359, 1586239, 1336584, 93588 (purist critiques)
- GAP600 2022 double-spend sample:
  https://gnusha.org/pi/bitcoindev/CACkWPs867CH+haR8ZEjnX7kzsruX=TbrhnRX+-XFJpnv=EBHnw@mail.gmail.com
