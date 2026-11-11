# Getting paid

> **The counter's sale handling is built and has been tested against Bitcoin
> Core on a test network. The Android app is a first build and has not been
> run on a phone.**

## An on-chain sale, as built

1. Ring up the order in the POS.
2. Enter the amount on the counter's keypad in the shop's currency. The POS
   does not start the counter automatically.
3. Show the customer the payment code. The screen also displays the amount
   and the address's first and last characters in large type.
4. The customer scans the code and approves the payment in their wallet.
5. When the counter turns green, close the POS order with the shop's
   "Bitcoin" tender.

The code is a standard `bitcoin:` payment request, so any wallet that reads
that format can pay it. Each sale gets a new address, saved before it is
displayed (`CR-01` to `CR-09`). If the connection is down, the counter stops
offering Bitcoin and asks the cashier to take another payment type
(`CR-01`).

Customers must scan today. NFC tap to pay is not built; the design
proposes an NFC tag for Android customers. The manifest checks currently
fail the build if an NFC permission appears.

Square and Clover support read-only order matching. The POS closes the
order, and the owner phone matches each Bitcoin tender to a counter sale
under `docs/POS-RULES.md`. The amount still comes from the cashier's entry.
Matching can flag a discrepancy but cannot prove that goods were handed
over for a payment (section 10 of those rules). Both readers were written
from vendor documentation and have not been tried with real seller
accounts. A Clover custom tender button and Lightspeed gateway are planned.

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

A non-final state explains both the reason and the next step. Waiting is
blue; it does not use the red warning screen. The complete wording is in
`docs/screens/counter.md`, generated from the code.

The confirmation depth starts at 1 block and the refund depth at 6. You
can change either, but cannot set either below 1. The counter recalculates
the displayed state from the watcher's facts each time (`CR-10` to
`CR-12`).

### Which check saw the payment

The source line tells you who saw the payment, separately from its state:

| Check line | Evidence |
|---|---|
| "Light check" | The counter's own check through Bitcoin peers saw the payment. |
| "Your Box reports it" | Only the Box has reported it so far. |
| "Your Box and light check" | Both have seen it. |
| "Your node and light check" | Both the shop's own node and the light check have seen it. |

An own-node report alone is insufficient for the own-node label: the
counter's light check must also see the payment (`CR-104`, `CR-105`).

## What "instant" means, and the limit that bounds it

An unconfirmed payment can be replaced by its sender. Accepting one means
handing over goods before the first block, with a limit on the loss the
shop is willing to take.

The initial limits are:

| Check | Per sale | Outstanding total for the shop | Check line |
|---|---|---|---|
| Light check | $10 | $50 | "Light check" |
| Own node and light check | $25 | $100 | "Your node and light check" |
| Box only | $10 | $50 | "Your Box reports it" |

The light check is the default. These figures are guesses marked "to tune",
not measured risk estimates. Limits are enforced in satoshis and displayed
in dollars. You can change any of them, including to zero to disable
instant acceptance. Raising one needs approval as a loosening change.

Before accepting instantly, the counter checks all of the following
(`policy`, `CR-12`, `CR-50`):

- The amount at risk is below the per-sale limit. It uses the greater of
  the sale amount and the amount received: paying $5,000 for a $10 sale
  exposes $5,000.
- Its allocated share of the shop's outstanding budget has room. A payment
  leaves this total at its first confirmation or through the owner's
  action, never by timeout.
- No conflicting spend has been seen for 8 seconds.
- The light check has enough peers in different network groups, some
  serving compact filters, a current header chain and a sensible clock.
- The owner phone's signed heartbeat is recent; the initial interval is
  10 minutes.
- The order is being paid at the counter, rather than remotely.
- Instant acceptance is not paused. One cancel pauses it for 1 hour. A
  second within 24 hours disables it until the owner resets it.
- Tor is on if customers can join the shop's Wi-Fi.

A replaced payment turns the screen red and triggers a loud alarm. Only
the owner can clear it. The counter saves a signed record of both the
original payment and its replacement.

The light check cannot inspect the inputs or fee of an unconfirmed
payment. Different peer groups and conflict monitoring help, but the cap
is what limits the shop's loss from a fake or cancelled payment. A cancel
sent privately to a miner becomes visible only when its block arrives.
The screen names which check accepted the payment.

The heartbeat detects a dead or disconnected owner watcher. It comes from
the same software family and does not protect against a bad build
(`CR-56`). In the current apps it travels through a relay, normally the
Box. A newly created wallet has no relay configured, and neither app has
a background service. Instant acceptance therefore needs both the Owner
and Counter apps open and linked.

The initial settings assume customers can join the Wi-Fi and leave Tor
off. Instant stays off until the owner answers that question. There is no
public measurement of double-spend rates since full replace-by-fee on
which to base the defaults; choose limits the shop can afford to lose.

## Confirmations, the refund depth, and the red screen

At the confirmation depth, the screen says "Paid" and "The payment is in
a block." At the refund depth it says "Paid" and "The payment is settled,
so a refund can be paid out if the customer asks." The books count the
sale as money only at the refund depth (`CR-31`).

If the payment is replaced, dropped, or loses its block in a
reorganization, the counter shows "Do not hand over goods". It reports
what it observed: "A different payment replaced this one" or "A block
that confirmed this payment was replaced". It does not guess the cause.

A cashier may override a red screen within the per-sale instant limit
and daily cap. A manager may override within a dollar ceiling. Each
override records the person's name, time and amount, counts toward the
exposure total, and is refused when that total is full (`CR-20` to
`CR-22`).

## Late and wrong payments

| Event | What happens |
|---|---|
| Underpayment | The screen shows, for example, "Paid $8.00 of $10.00". Ask for the rest with a new code, or cancel. The tolerance starts at 0.5% and cannot exceed 5%. |
| Overpayment | The sale is paid and the surplus is recorded as owed to the customer (`CR-16`). |
| Late payment | A payment to an expired or cancelled sale is recorded and flagged to the owner. It never turns another sale green (`CR-19`). Shown addresses remain watched until their sales settle. |
| Dust | Less than 1,000 satoshis in total is logged without changing the sale. |
| No confirmation after 2 hours | The screen adds "stuck: not confirming" and the sale stays open (`CR-18`). |
| Nothing received after 15 minutes | The sale expires. |

The price is locked for 5 minutes at a time. It refreshes automatically
without changing the address. It comes from at least two public sources
that must agree; only United States dollars are supported
(`docs/RATE-RULES.md`).

Customers request refunds at the counter. Refunds are paid in an approval
session; the walkthrough belongs to the next part of the handbook.

## Small sales and the shop's own Lightning node

An on-chain fee does not scale with the sale amount, so it can be large
relative to a small purchase. A Lightning-only wallet also cannot pay an
on-chain address. The counter tells customers "This shop takes on-chain
bitcoin".

You can set a minimum on-chain sale. Below it, the cashier must take another
payment type (`CR-75`). There is no minimum by default. Each receipt also
costs the shop a fee when spent later, which is another reason to set one.

Alternatively, a shop already running LND or Core Lightning can let the
counter display a code with both an on-chain address and a Lightning
invoice. A Lightning payment is final immediately at any amount, without
the instant limit. This counter integration is built but has never been
run against a real node.

The shop must operate the node and manage liquidity, updates and backups.
OpenBMT does not do that work. Eclair is unsupported (`LN-02`), and
third-party swap providers are excluded under the purist veto.
