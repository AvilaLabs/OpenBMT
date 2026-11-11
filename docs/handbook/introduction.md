<a id="introduction"></a>

# Start here

This guide assumes you know how to run a shop and take card payments. It
explains Bitcoin from the beginning, so you can decide whether this way of
being paid would suit your business.

OpenBMT is a project for taking bitcoin payments at a shop's counter. A
customer pays from a Bitcoin wallet, an app that lets them send and receive
bitcoin. The payment goes to a wallet controlled by the shop. OpenBMT does
not hold the proceeds in an account or send you a later bank payout.

That gives you control of the money, along with responsibility for keeping
access to it. You also receive bitcoin rather than dollars. If you want
dollars in your bank account, you arrange the conversion through an
exchange you choose.

> **OpenBMT is pre-release. It has had internal review only and no independent
> security audit. Do not use it with real funds.**

You can use this handbook to evaluate the design. OpenBMT is not ready to
put into service with paying customers. Its Android apps have started on an
emulator, a computer that simulates a phone, but have not been tried on a
physical phone. Setup instructions are still being written. The
[status page](status.md) distinguishes working code, tests and unfinished
parts.

<a id="what-openbmt-is"></a>
<a id="the-shops-devices"></a>
<a id="payments-and-records"></a>
<a id="what-openbmt-is-not"></a>

## What would change at the shop

A cashier would enter the sale amount, show a payment code and wait for the
counter to report the payment. The customer needs a wallet containing
bitcoin. The cashier then closes the order in the shop's existing
point-of-sale system, or POS, using a Bitcoin payment type.

You would also have owner work outside checkout: looking after the wallet,
approving refunds and transfers, checking the records, and deciding how much
bitcoin to keep or sell. There is no company that can reset the wallet if
you lose access to it.

The next chapters explain those tasks before the detailed software rules.
They use shop examples and explain terms when they first matter. The fee
examples and all starting limits are examples or dated figures, not
promises about future costs or losses.

<a id="who-it-is-for"></a>

## What to read first

| What you want to understand | Chapter |
|---|---|
| What you receive and how a payment becomes confirmed | [What a bitcoin payment is](bitcoin-basics.md) |
| What the cashier and customer do | [A sale at the counter](getting-paid.md) |
| Fees, selling the bitcoin and keeping records | [Costs and cashing out](why-bitcoin.md) |
| Keys, approvals and losing a phone | [Who controls the money](safety.md) |
| Equipment, owner workload and whether it suits the shop | [Equipment, daily work and fit](fit.md) |

<a id="what-is-built-today"></a>

## The decision today

If you need a finished payment system now, OpenBMT is not ready for that
job. If you are curious about a shop-controlled Bitcoin checkout, the
handbook explains what the project offers and what it asks of the owner.

<a id="how-to-read-a-safety-claim"></a>
<a id="checking-the-implementation"></a>

The merchant guide also covers privacy, risks and the backup routine.
The project-choice chapters explain the reasons for the design.

## For readers who already know Bitcoin

The separate [technical design section](technical-overview.md) assumes
familiarity with transactions, descriptors, multisig and full nodes. It
sets out the trust boundaries, format choices, exact checks, alternatives
and current test gaps, with links to the rules and source code. You can
start there without reading the introductory lessons.

The status and source chapters record implementation progress and dated
fee and tax material. References such as `SR-34` and `CR-15` identify rules in
the
repository's `docs/` folder and corresponding tests in the code. You do not
need to follow them to read the merchant guide. "Not verified" marks
something that has not been checked; the surrounding text says what is
known.
