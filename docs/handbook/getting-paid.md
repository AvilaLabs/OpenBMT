# Getting paid

This chapter follows one sale, from the counter to the books, and says what
each screen means and what bounds the risk of each.

> **Status: the counter's judgment of a sale is Built and checked against
> Bitcoin Core on a test network. The Counter app is a first build and has
> not been run on a phone.**

## An on-chain sale, as built

1. The cashier rings up the order in the POS as usual.
2. The cashier enters the amount on the counter's keypad, in the shop's
   currency. The POS does not yet start the counter by itself.
3. The counter shows the amount, a QR code and the first and last
   characters of the address in large type. The code is a standard
   `bitcoin:` payment request. Any wallet that reads one can pay.
4. The customer scans the code and confirms in their wallet.
5. The counter shows one of the states below. When it is green, the cashier
   closes the order in the POS with the shop's "Bitcoin" tender.

**Tap to pay is Not built yet.** The design describes an NFC tag for Android
customers, but no app has it, and the apps' manifest checks fail the build if
an NFC permission appears. Today every customer scans.

**Rests on.** `CR-01` to `CR-09` (opening a sale, one new address per sale,
stored before shown), and the vendor-neutral POS matching in
`docs/POS-RULES.md`. Square and Clover are read-only: the shop's POS closes
the order, and the owner phone matches every Bitcoin tender to a counter sale.
Clover's custom tender button and a Lightspeed gateway are **Planned**.

**Stops at.** The amount is the cashier's entry. The matching alerts when the
POS order and the counter disagree, but it cannot prove goods were handed over
for a payment (`docs/POS-RULES.md`, section 10). The Square and Clover readers
were written from vendor documentation and have not been tried against a real
seller account.

If the connection is down, the counter does not offer Bitcoin. The cashier is
told to take another payment type (`CR-01`).

## The states a cashier sees

| Heading on the counter | Color | Meaning |
|---|---|---|
| Not paid yet | Blue | Nothing has arrived. |
| Part paid | Blue | Less than needed has arrived. Ask for the rest or cancel. |
| Waiting (with the reason, such as "Waiting: over the limit") | Blue | A payment arrived and waits for a block. The screen says why. |
| Paid (before a block) | Green | Accepted at once within the instant limit. The screen says it could still be undone and which check accepted it. |
| Accepted by a person | Green | A cashier or manager accepted it by hand (an override). |
| Paid (in a block) | Green | The payment is in a block at the shop's depth. |
| Paid (settled) | Green | At the shop's refund depth: a refund can be paid out if asked. |
| Paid over Lightning | Green | The shop's own node has the money. |
| Do not hand over | Red | The payment was replaced, its block was dropped, or the counter cannot check. |
| Needs a person | Red | The sale holds more than the counter can judge. |
| Expired, Cancelled | Neutral | Ended with nothing received. |

The full list, with every reason and next step, is the counter's screen table
(`docs/screens/counter.md`), generated from the code.

Every state that is not final shows two texts: why it is in that state, and
what happens next. Waiting is blue, never alarming. The shop sets two depths:
the confirmation depth (1 block at first) and the refund depth (6 at first).
The shop can change them but never below 1 block.

**Rests on.** `CR-10` to `CR-12`. The state is worked out again from the
watcher's facts every time it is shown.

## What "instant" means, and the limit that bounds it

Any unconfirmed bitcoin payment can be replaced by its sender. Instant means
the counter lets the cashier hand over goods before the first block, within a
limit the shop can afford to lose.

The first limits, per sale and as the outstanding total for the shop:

| Check | Per sale | Outstanding total | Check line on screen |
|---|---|---|---|
| Light check (default) | $10 | $50 | "Light check" |
| Own node, also seen by the light check | $25 | $100 | "Your node and light check" |
| Only the Box reports it | $10 | $50 | "Your Box reports it" |

These are first values, marked "to tune", and the shop may set any of them,
zero included, which turns instant off. They are enforced in satoshis and
shown in dollars. Raising any of them is loosening.

Instant is allowed only when all of these hold (`policy`, `CR-12`, `CR-50`):

- the amount at risk, which is the larger of the sale and what was received,
  is under the per-sale limit. A $10 sale paid with $5,000 counts as $5,000;
- the outstanding total, in this counter's own slice of the shop's budget, has
  room. A payment leaves the total at its first confirmation or by the owner's
  action, never on a timeout;
- no conflicting spend of the same coins was seen for 8 seconds;
- the light check is healthy: enough peers in different network groups, some
  serving compact filters, a current header chain, and a sensible clock;
- the owner phone's signed heartbeat is recent (10 minutes at first);
- the order is at the counter, not remote;
- no pause is on. After a cancel, instant is off for 1 hour. A second cancel
  within 24 hours turns it off until the owner resets it;
- if customers can join the shop's Wi-Fi, instant runs only with Tor on.

If a payment is replaced, the counter raises a loud alarm that only the owner
can clear, saves a signed record of the original payment and its replacement,
and shows red.

**Stops at.**

- The light check cannot see a payment's inputs or fee. It watches for
  conflicting spends and requires distinct network groups. A fake payment
  and a cancelled one cost the shop the same capped amount, so the cap is the
  protection and the check line says which check was done.
- A cancel sent privately to a miner is not relayed. It is seen only when the
  block that holds it arrives.
- The heartbeat is a claim by the same software family on another device. It
  guards against a dead or cut-off owner watcher, not a bad build (`CR-56`).
- In the built apps, **the heartbeat reaches the counter over a relay link**,
  normally the Box. A wallet made fresh on the owner phone has no relay yet,
  and the Owner app and the Counter app run only while open, with no
  background service. In practice instant needs both apps open and linked.
  Defaults start with "customers can join the Wi-Fi" and Tor off, so instant
  is off until the owner answers.
- No public measurement of double-spend rates since full replace-by-fee
  exists. The limits are first guesses that the shop should size to what it
  can lose.

## Confirmations, the refund depth, and the red screen

A payment in a block at the shop's confirmation depth shows "Paid" with "The
payment is in a block." At the refund depth it shows "Paid" with "The payment
is settled, so a refund can be paid out if the customer asks." The books count
a sale as money only at the refund depth (`CR-31`).

The red **"Do not hand over goods"** screen shows when a payment was replaced,
dropped, or its block was replaced by a reorganization. The wording says what
the counter saw ("A different payment replaced this one", "A block that
confirmed this payment was replaced") and never guesses a cause it cannot
know. A person can override it: a cashier only up to the per-sale instant
limit and a daily cap, a manager up to a dollar ceiling. An override is logged
with the name, time and amount, counts toward the exposure total, and is
blocked when the total is full (`CR-20` to `CR-22`).

## Late and wrong payments

- **Short.** "Paid $8.00 of $10.00". The cashier can ask for the rest (a new
  code) or cancel. A tolerance (0.5% at first, at most 5%) counts a tiny
  shortfall as paid.
- **Over.** The sale is paid in full and the surplus is recorded as owed to the
  customer (`CR-16`).
- **Late.** A payment to an expired or cancelled sale is still detected,
  recorded and flagged to the owner. It never turns another sale green
  (`CR-19`). Every address ever shown stays watched until its sale is settled.
- **Dust.** Under 1,000 satoshis in total is logged and changes nothing.
- **Stuck.** A payment seen but not confirmed for 2 hours adds "stuck: not
  confirming". The sale stays open (`CR-18`).
- **Expiry.** A sale with nothing received ends after 15 minutes. The price
  is locked for 5 minutes at a time, and refreshes on its own without changing
  the address.
- **Price.** The price comes from two or more public sources that must agree,
  and only United States dollars are supported (`docs/RATE-RULES.md`).

Refunds are asked for at the counter and paid in an approval session. They are
covered in the next part of the book.

## Small sales and the shop's own Lightning node

An on-chain payment costs the customer a network fee that does not depend on
the sale's size. That is heavy on a small ticket, and a Lightning-only wallet
cannot pay an on-chain address at all. The counter says "This shop takes
on-chain bitcoin".

Two answers exist, both optional:

- **A smallest sale.** The shop can set a smallest bitcoin sale. A smaller
  sale is not opened, and the cashier takes another payment type (`CR-75`).
  The first value is no floor. Each receipt also costs the shop a network fee
  when it is spent later, which is why a floor exists.
- **The shop's own Lightning node.** The counter can show a code that holds both
  an on-chain address and a Lightning invoice made by LND or Core Lightning
  that the shop already runs. A Lightning payment is final at once, at any
  amount, with no instant limit. This is **Built at the counter and never run
  against a real node**. The shop runs and maintains the node. OpenBMT does not
  manage liquidity, updates or backups, and Eclair is not supported
  (`LN-02`).

Lightning-in through a third-party swap provider was dropped under the veto.
