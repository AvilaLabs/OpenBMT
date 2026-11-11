# Principles

Every decision in OpenBMT is checked against one standard with three parts.
All three must pass at once.

## The standard

1. **Easy.** A merchant, a cashier or a customer who has never seen it should
   get it right the first time. If something is confusing, that is a defect
   in OpenBMT, not in the person.
2. **Best in class.** On every number a merchant or customer would compare
   (setup time, taps, time to "paid", fees, refunds, books) OpenBMT has to
   match or beat the best product available, custodial ones included.
   Status: this is the target, not a result. Setup time and most speeds have
   not been measured yet, and the design marks them "to measure".
3. **Purist-approved.** No custody, no trusted server, honest about what is
   verified, private by default, built on open standards.

### Why a purist objection is a veto

OpenBMT is written from a bitcoin purist position. That is a stance, and
here it is, with its reasons.

A real purist objection is **a veto, not a trade-off**. The veto covers
anything that lets a third party hold, move, freeze or redirect the shop's
funds, or that gives a third party the shop's keys, its wallet description or
its addresses. An idea that fails the veto is dropped. It is not kept as an
option, a slot or a later phase.

The reason is simple. A shop that takes bitcoin to avoid depending on a
processor gains nothing from a design that quietly adds one back. A single
exception tends to become the default, so the project does not make one.

Two ideas were dropped this way. Co-signed instant payments, where a
group of co-signers vouches for an unconfirmed payment, need merchants to
trust a signer set and give that set the power to freeze and to see every
spend. Lightning through a third-party swap provider makes every such sale
depend on a central service that sees amounts, timing and both sides.

The veto does allow one narrow thing: an optional service that sees only
metadata, if all three hold. It is not the default. It is labeled at the
point of choice. It is listed in [Privacy](privacy.md). Nothing like that is
a default in OpenBMT today.

### Choice without confusion

Where merchants want different things (which keys, how long to wait, which
node), they choose. Every option offered must be safe and purist-acceptable
on its own, so there is no wrong answer. One option is marked as
recommended. Unsafe combinations are never offered.

## Bitcoin only

OpenBMT takes bitcoin. There is no second coin, no stablecoin and no token.
[What OpenBMT will never do](never.md) says what that rules out and why.

## Your keys

The shop holds all three keys of its wallet. Any two can spend. The three
live on different devices, and no single device may hold or drive two of
them. [How your money is kept safe](safety.md) explains how.

## No company in the payment path

Between the customer's wallet and the shop's wallet there is the Bitcoin
network and nothing else. The shop's devices check payments by talking to
Bitcoin peers directly, or to the shop's own node. There is no OpenBMT
server that a payment passes through, and no OpenBMT account.

Other companies do appear around the edges, and the book says where:

- the POS vendor sees every order, because it is the shop's POS;
- price sources see that a device asked for the price;
- Bitcoin peers see the counter's network address unless Tor is on;
- an exchange sees the cash-out deposit, because the shop chose it.

None of them is in the payment path. [Privacy](privacy.md) lists each.

## Honest status words

Two kinds of honesty run through the project.

**On the screens.** A payment is labeled with what was actually checked, from
weakest to strongest:

- "Seen by 3 peers (not verified by this device)"
- "Paid (your Box reports it)"
- "Confirmed (light check)"
- "Confirmed (your node)"

The screen never says "fully verified" unless the shop's own node said so.
"Final" is reserved for the depth the shop sets for refunds, and even that
word means "more blocks on top make a reversal less likely", never "zero
risk".

**In the documents.** Every safety claim must be literally true. If it holds
only under a condition, the condition is next to it. If something could not
be checked, the document says "not verified". This book follows the same
rule, and it uses the four status words from the
[Introduction](introduction.md).

OpenBMT makes no claim of being proof against attack. The status is:
internal review only, no independent security audit yet. Four internal design
reviews have been done.
They are internal, and the built software still needs tests on devices, an
outside code review and an audit.
