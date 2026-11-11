# Principles

OpenBMT checks each decision against three standards. A decision must meet
all three.

## The standard

1. **Easy.** A merchant, a cashier or a customer who has never seen it should
   get it right the first time. If something is confusing, that is a defect
   in OpenBMT, not in the person.
2. **Best in class.** For every measure a merchant or customer would compare
   (setup time, taps, time to "paid", fees, refunds, books), OpenBMT must
   match or beat the best product available, custodial ones included.
   This is a target, not a result. Setup time and most speeds have not been
   measured; the design marks them "to measure".
3. **Purist-approved.** No custody, no trusted server, no claim beyond what
   was checked, private by default, built on open standards.

### Why a purist objection is a veto

OpenBMT takes a bitcoin purist position. It rejects any proposal that lets a
third party hold, move, freeze or redirect the shop's funds, or gives that
party the shop's keys, wallet description or addresses. This veto also rules
out keeping such a proposal as an option or deferring it to a later phase.

A shop that takes bitcoin to avoid depending on a processor gains nothing
from a design that adds one back. An exception could become the default, so
OpenBMT does not make one.

This veto ruled out two ideas. Co-signed instant payments, where a
group of co-signers vouches for an unconfirmed payment, need merchants to
trust a signer set and give that set the power to freeze and to see every
spend. Lightning through a third-party swap provider makes every such sale
depend on a central service that sees amounts, timing and both sides.

The veto allows one narrow exception: an optional service that sees only
metadata, provided it is never the default, it is labeled where the shop
chooses it, and it is listed in [Privacy](privacy.md). OpenBMT has no such
service today.

### Choice without confusion

Merchants can choose which keys to use, how long to wait and which node to
connect. Each available option must be safe and acceptable under the purist
standard on its own. OpenBMT marks one option as recommended and does not
offer unsafe combinations.

## Bitcoin only

OpenBMT takes bitcoin only. It has no second coin, stablecoin or token.
[What OpenBMT will never do](never.md) says what that rules out and why.

## Your keys

The shop holds all three keys of its wallet. Any two can spend. The three
live on different devices, and no single device may hold or drive two of
them. [How your money is kept safe](safety.md) explains how.

## No company in the payment path

Payments travel between the customer's wallet and the shop's wallet over the
Bitcoin network. The shop's devices check them by connecting to Bitcoin peers
or to the shop's own node. No OpenBMT server handles a payment, and the apps
require no OpenBMT account.

Other companies may be involved in these ways:

- the POS vendor sees every order, because it is the shop's POS;
- price sources see that a device asked for the price;
- Bitcoin peers see the counter's network address unless Tor is on;
- an exchange sees the cash-out deposit, because the shop chose it.

None of them is in the payment path. [Privacy](privacy.md) lists each.

## Status words

OpenBMT reports what it checked on screen and in the documents.

On the counter, each payment carries a line naming what saw it:

- "Your Box reports it": only the Box has seen it so far.
- "Light check": the counter's own check against Bitcoin peers saw it.
- "Your Box and light check": both saw it.
- "Your node and light check": the shop's own node and the light check both
  saw it.

The rules for these lines are CR-105 and the counter's screen table
(`docs/screens/counter.md`). No screen says "fully verified".
"Settled" and "final" are reserved for the refund depth the shop sets, and even
then they mean "more blocks on top make a reversal less likely", never "zero
risk".

In the documents, every safety claim must be true as written. If it holds
only under a condition, the condition appears beside it. If something could
not be checked, the document says "not verified". This handbook follows the
same rule and uses the four status words from the
[Introduction](introduction.md).

OpenBMT does not claim to be proof against attack. It remains under internal
review; no independent security audit has been done. Four internal design
reviews are complete. The built software still needs device testing, an
outside code review and an audit.
