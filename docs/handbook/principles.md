# Principles

OpenBMT is intended to give a shop control of its Bitcoin payments without
making checkout harder for the cashier or customer. Its design has three
requirements, all of which must be met.

<a id="the-standard"></a>

## Design requirements

A new merchant, cashier or customer should be able to use it correctly on
the first attempt. Confusing behavior is a defect in the software.

The performance target is to match or beat the best available checkout
products, including custodial ones, on setup time, taps, time to "paid",
fees, refunds and bookkeeping. These are targets. Setup time and most
speeds have not been measured; the design still marks them "to measure".

OpenBMT also follows a bitcoin purist position: the shop keeps custody,
payments do not depend on a trusted server, privacy is the default, and the
wallet uses open standards. Claims about a payment must stay within what
the software actually checked.

### Why a purist objection is a veto

A proposal is rejected if it would let a third party hold, move, freeze or
redirect the shop's funds, or give that party the shop's keys, wallet
description or addresses. Rejected proposals cannot be retained as options
or deferred to a later version. An exception could become the default and
leave the shop dependent on a processor again.

This rule excluded co-signed instant payments: the shop would have to trust
a group of signers that could freeze funds and see every spend. It also
excluded Lightning through a third-party swap provider, which would see
each sale's amount, timing and both sides of the swap.

An optional service may see metadata alone if it is never the default, its
use is disclosed where the shop chooses it, and its exposure is listed in
[Privacy](privacy.md). OpenBMT has no such service today.

<a id="choice-without-confusion"></a>

### Configuration choices

Shops can choose their keys, node and waiting periods. Every offered
combination must meet the safety and purist requirements on its own;
unsafe combinations are excluded. OpenBMT marks one option as recommended
so that a new user does not have to research every choice before starting.

## Bitcoin only

OpenBMT accepts bitcoin only. Other coins, stablecoins and tokens are
excluded. [What OpenBMT will never do](never.md) sets out this policy and
the other permanent restrictions.

<a id="your-keys"></a>

## Key ownership

The shop holds all three wallet keys. Any two can spend, so the keys must
be kept on separate devices. No device may hold two keys or cause a second
key to sign without an independent review on that key's device. The
[wallet chapter](safety.md) describes this separation and its current limits.

## No company in the payment path

A payment travels from the customer's wallet to the shop's wallet over the
Bitcoin network. The shop's devices check it through Bitcoin peers or the
shop's own node. There is no OpenBMT payment server or account.

Some activity is still visible outside the shop. The POS vendor sees its
orders, price sources see price requests, and an exchange sees deposits the
shop sends for cash-out. Bitcoin peers see the counter's network address
unless it uses Tor. [Privacy](privacy.md) describes these contacts.

<a id="status-words"></a>

## Payment evidence and status

The counter must identify the check behind a payment report and distinguish
its sources of evidence (`CR-105`, `docs/screens/counter.md`). The actual
labels are explained in [Getting
paid](getting-paid.md#which-check-saw-the-payment).
No screen says "fully verified". "Settled" and "final" refer to the refund
depth chosen by the shop. More blocks reduce the chance of reversal without
making it impossible.

The same precision is required in documentation. Conditions belong beside
the claims that depend on them, and an unverified claim must be labeled
"not verified". The status terms in the [Introduction](introduction.md)
distinguish working code from plans and untested features.

Four internal design reviews are complete. Device testing, an outside
code review and an independent security audit are still needed. OpenBMT
makes no claim to be proof against attack.
