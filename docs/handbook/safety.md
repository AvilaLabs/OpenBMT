<a id="how-your-money-is-kept-safe"></a>
<a id="the-wallet-and-its-keys"></a>

# Who controls the money

A wallet needs secret information to authorize spending. That information
is called a private key, and the 12 or 24 recovery words are the key
written out. Whoever has the words can spend; nobody can ask a company to
reset a wallet whose words are lost.

OpenBMT never holds the shop's money and never sees the shop's savings
key. The shop's bitcoin goes straight to the shop's own wallet.

## The shop's own wallet

For most shops this is a hardware wallet: a small device that makes the
key, never connects to the internet and shows each payment on its own
screen before it signs. A shop may instead use a wallet it already keeps
offline. OpenBMT accepts a single-key native segwit wallet today
(BIP84); wallets that need several keys to spend are not accepted yet.

The owner phone and the counters receive only the wallet's **public
description**. It lets them make a new address for each sale and see
payments arrive. It cannot spend. When the wallet is connected, the owner
compares its first receiving address on the wallet's own screen with the
owner phone, and the shop takes no payment until they match. A description
that holds a private key or recovery words is refused, and the app says
that key is now exposed.

The recovery words are written by hand, never typed into a phone or a
connected computer, and later put on steel, away from the shop. With them,
any standard wallet program can restore the wallet, without OpenBMT.

## Starting before the hardware wallet arrives

A shop can start selling today on a **starter wallet** that the owner phone
makes. Its key is on an everyday phone, so it has a limit the owner sets:
the most they would be OK losing if the phone were stolen or taken over,
and a date to move to the shop's own wallet. The owner phone shows what
the starter wallet holds against both every day. Its 12 words are shown
once to write on a card, and the key is sealed by the phone's key hardware
and opened only by fingerprint, face or the phone's PIN. When the hardware
wallet is set up, each counter switches to it and the owner phone moves
everything in one payment, then deletes the starter key.
[Setting up a shop](setting-up.md#start-simple-a-starter-wallet-on-the-owner-phone)
has the details.

## What each device can do

| Device | What it holds | What it can do |
|---|---|---|
| Your wallet | The savings key | Signs every payment out, after showing it on its own screen. |
| Owner phone | The wallet's public description and the shop's owner key (and, on the starter plan, the capped starter key) | Sets up the shop, pairs counters, shows payments, keeps the books, prepares refunds and cash-outs for the wallet to sign, and signs the shop's settings. |
| Counter | The wallet's public description, ranges of address numbers and its own device key | Rings up sales, shows payment codes, checks payments and keeps the sale records. It cannot spend the shop's savings. |

A stolen counter can show the shop's payments and the addresses in its
ranges, including future ones; it cannot spend. A stolen owner phone
cannot spend the savings either, but it can sign settings, so the counters
guard the riskiest changes themselves, as below. On the starter plan the
owner phone holds the capped starter key, which is why that wallet has a
limit.

<a id="roles-owner-manager-cashier"></a>

## What staff can do

| Person | Day-to-day authority |
|---|---|
| Cashier | Create and cancel sales, request refunds, see sales totals and accept some exceptions within assigned limits. Cannot spend from the wallet or see its balance. |
| Manager | Handle larger allowed exceptions, approve large refund requests at the counter by PIN, and confirm a settings change that increases risk. Cannot move the money or change money settings. |
| Owner | Review records and money, manage counters and settings, and prepare refunds and transfers, which the wallet signs. |

An exception override accepts the sale on the shop's behalf and is logged.
It does not make the underlying payment more reliable. Only the owner can
clear a payment-cancel alarm. The counter has one manager PIN and a local
list of cashiers today; several independent manager accounts are not built.
The [reference](wallet-reference.md#roles-owner-manager-cashier) gives exact
permissions, PIN lengths and limits.

<a id="loosening-needs-the-key-app-tightening-does-not"></a>

## Changes that increase risk are confirmed at the counter

Lowering the amount accepted before confirmation reduces risk, and the
owner can do it at once. Raising it increases risk. OpenBMT calls these
directions **tightening** and **loosening**.

A loosening change, such as a higher instant limit, fewer confirmations or
a new counter, is signed by the owner phone and then waits on each counter
until a manager confirms it there, after reading each old and new value on
the counter's own screen. A change that decides where money goes, such as
turning on the shop's Lightning node, also carries a signature from the
shop's wallet, which each counter checks against the wallet it was paired
with. A phone that has been taken over cannot make either kind of change
alone.

<a id="the-approval-session"></a>

## Refunds and transfers

A refund in cash is paid at the counter. A refund in bitcoin, and moving
savings to an exchange, are prepared on the owner phone and signed on the
shop's wallet, which shows the destination, amount and fee on its own
screen. Review those there: the wallet cannot check the shop's sales
record or prove that a refund is owed.

<a id="address-batches"></a>

## How the counter gets its addresses

The counter makes each address itself from the wallet's public
description, from ranges of 1,000 address numbers that the owner phone
gives it. No two devices share a number, and no address is shown twice.
If a range runs out, the counter stops offering Bitcoin until the owner
phone gives the next one.

<a id="the-instant-limit-and-the-exposure-total"></a>

The instant-sale limits protect a different thing: the value of goods
released before payment confirms. They cap that checkout exposure rather
than protect the wallet. [A sale at the counter](getting-paid.md) explains
the difference.

<a id="when-a-phone-is-lost-or-stolen"></a>
<a id="the-recovery-kit"></a>

## When a device is lost

A lost counter or owner phone does not put the savings at risk: the key is
in the wallet, and the words on steel restore it. Lock both phones with a
PIN, because they can show the shop's payments. Remove a lost counter from
the shop on the owner phone; a counter that is removed refuses new sales.

On the starter plan, a lost owner phone takes the starter key with it.
The card of starter words restores that wallet in any standard wallet
program, which is why the starter wallet is capped.

The step-by-step guide for setting up a new owner phone for an existing
shop is not written yet. [Backups and routine checks](best-practices.md)
describes storage, access for someone acting for the owner, and
independent recovery checks.

## What is not protected yet

The apps are signed for release by one publisher, and reproducible builds,
which would let anyone check released software against its source, are not
in place. A malicious release of the owner app or the counter app could
show false screens or wrong addresses; it could not sign for the savings,
which stay in the wallet.

> **OpenBMT has had no independent security audit. The apps have not been
> used for real sales on a phone. Do not use it with real funds.**

## The earlier setup with three keys

Before 2026-11-12, OpenBMT's design gave the shop three keys, two needed to
spend, with the second on a separate Key app. That setup is still in the
code but is no longer offered at setup, and it is due to be removed.
