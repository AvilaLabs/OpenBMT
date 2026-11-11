# What OpenBMT changes

The appealing part of accepting bitcoin directly is simple: the customer
pays the shop, and the shop receives the money. There is no OpenBMT account
in between and no OpenBMT payout arriving later.

Making that sentence true at a working counter is the difficult part. The
merchant needs the payment to remain direct without expecting every cashier
to manage a wallet or every owner to improvise a key-and-approval system.
OpenBMT is designed to turn that problem into a defined set of roles, screens
and procedures.

> **OpenBMT is still pre-release. This chapter explains the intended value of
> its design, not a claim that the current apps are ready for real funds.**

## The shop receives money, not a promise of a payout

With a conventional card or custodial payment processor, the checkout result
and the merchant's money are separate. The processor records the sale in an
account, subtracts its fees and later sends a payout to the merchant's bank.
Access to that payout depends on the account and the processor.

With OpenBMT, the payment request contains an address from the shop's own
Bitcoin wallet. The customer broadcasts a transaction paying that address.
The shop's policy determines when the cashier treats the sale as paid. If
that happens before a block, it is a limited-risk business decision; the
payment is not yet confirmed or final. The decision does not change the
destination: spending the received outputs requires the shop's wallet keys.
OpenBMT does not hold a matching balance or decide when the shop may withdraw
it.

The difference is not primarily a fee claim. It changes who controls the
proceeds of a sale.

That control has consequences. The shop receives bitcoin rather than dollars,
so its value can move. The shop decides what to retain and what to send to an
exchange. There is no automatic bank payout. If the shop loses enough keys,
OpenBMT cannot restore its wallet access. Direct receipt replaces a provider's
permission with the owner's responsibility.

## The restrictions protect the point

OpenBMT does not present every easier arrangement as another setup choice. It
does not accept altcoins or tokens. It does not offer a wallet key stored on a
payment server. It does not connect the shop to a custodial Lightning balance,
and it does not require an OpenBMT account or payment backend.

This makes OpenBMT less flexible than a general cryptocurrency processor. The
restriction is deliberate. A merchant should not have to work out which
setting quietly changed the money being accepted, gave a server spending
authority or made an outside provider necessary for a sale.

Useful choices remain. The owner can choose confirmation requirements, small
payment limits, a shop-run full node, on-chain payments or a shop-run
Lightning node, and whether to keep or sell the bitcoin. Those decisions
change the shop's work and risk without changing the asset or putting OpenBMT
in control of the proceeds. [Why Bitcoin only](never.md) explains the monetary
boundary; [Design choices](principles.md) explains the service boundary.

## The till can receive without being able to spend

Putting an ordinary wallet on the counter gives the checkout device two very
different powers: it can show a receiving address, and it can spend the
wallet. The cashier needs the first power. The till does not need the second.

OpenBMT separates them. The counter receives batches of addresses that have
already been approved by the owner's devices. It can assign one to a sale and
watch for payment, but it holds no wallet spending key. Stealing the counter
does not provide the keys needed to transfer the wallet's existing balance.

That boundary is useful even in a shop where every employee is trusted. A
checkout device is exposed to customers, networks, charging cables, loss and
the pressure of a busy shift. It should not become the easiest route to the
treasury merely because it needs to display payment codes.

The separation is not magic. Approved address batches limit the addresses an
honest counter is allowed to assign. They cannot stop a fully compromised
screen from showing an attacker's address or falsely claiming that a customer
paid. An independent comparison with the shop's wallet is meant to detect
that substitution. The owner still has to perform the check. The benefit is
a smaller and more understandable failure, not an invulnerable till.

## The cashier gets an answer instead of a Bitcoin lesson

A cashier should not have to inspect a transaction, estimate replacement risk
or debate confirmation policy while a customer waits. Those are owner
decisions that should already be reflected in the checkout rules.

OpenBMT lets the owner set limits for accepting some smaller on-chain payments
before confirmation. The rules consider the size of the sale and the shop's
total outstanding exposure. A sale that does not fit those limits waits for a
block. The cashier applies the result instead of inventing policy at the
counter.

The screen also identifies what the software actually observed. A transaction
seen on the network, a transaction included in a block and a matching report
from the shop's node are not presented as if they were the same fact. If a
payment is late, short, replaced or otherwise unusual, the counter gives it a
separate state rather than quietly treating it as a normal sale.

This matters because "paid" can conceal several different claims. OpenBMT's
convenience comes partly from making those claims precise before translating
them into a simple cashier instruction. [A sale at the
counter](getting-paid.md) follows that workflow in detail.

## Important actions get a second pair of eyes

The default wallet requires two of three keys to spend. The owner phone holds
one wallet key, the separate Key app normally holds another, and the recovery
key is generated outside OpenBMT. No one key is enough.

The Key app does more than contribute a signature. It shows what a proposed
transaction will do before approving it. The same separate review applies to
new receiving-address batches and changes that increase the shop's risk.

This creates a useful division of work:

| Role | Ordinary authority |
|---|---|
| Cashier and counter | Create sales, show approved addresses and report payment results. |
| Owner | Review the business reason, prepare refunds or transfers and choose shop policy. |
| Separate signer | Check the proposed action and provide the second approval. |
| Recovery key holder | Provide key 3 with the surviving key if key 1 or key 2 is lost. |

The arrangement is not a promise that two different people always participate.
One person could control two keys, and the Bitcoin script cannot tell where
the devices are kept. The protection depends on the merchant actually
separating the devices and recovery material. OpenBMT makes that separation
the normal workflow instead of leaving it as an unwritten suggestion.

The software releases are not independently separated yet. A second holder
of the Key app's release-signing key, reproducible builds and an independent
rebuilder are still missing. Until that work is complete, using two devices
does not establish that they received independently checked software. That is
a current weakness, not a property the wallet script can fix.

## The owner can choose stronger payment evidence

OpenBMT does not require the shop to run a full Bitcoin node before taking its
first test payment. A full node is software the shop runs to check Bitcoin's
rules and transaction history itself. The apps include a light payment check,
and the owner can add a shop-run node as another source of evidence.

This keeps the starting arrangement smaller while preserving a route toward
more independent verification. When the shop selects the own-node mode, the
counter requires both its light check and the shop's node to agree before it
uses the own-node result. If the node is unavailable, the counter falls back
to its light-check rules rather than displaying the own-node result. It does
not replace one unqualified green light with another.

A similar ownership decision applies to Lightning. It is optional, and the
supported route connects to a node operated for the shop. That node reports a
settled Lightning payment through a separate path; the on-chain light-and-node
agreement does not apply to it. OpenBMT does not place a hosted Lightning
balance between the customer and merchant. The cost is that the shop must take
responsibility for channels, receiving liquidity, updates and backups if it
chooses Lightning.

OpenBMT therefore does not define convenience as hiding every technical
dependency behind an OpenBMT service. It tries to make the dependencies
visible and give the owner a workable choice among the supported arrangements.

## It works beside the business the shop already has

Most merchants considering Bitcoin already have a POS, a catalog, accounting
records and a bank relationship. Replacing all of that would make a new
payment method much harder to adopt.

OpenBMT is intended to handle the Bitcoin side of the sale while the existing
POS remains the business record. Its Square and Clover work reads orders for
matching; it does not take control of those systems. The cashier still closes
the order using the shop's Bitcoin payment type. The records library keeps the
transaction with the counter sale. The matching code associates a final sale
with its POS order and tender, using a reference where available or defined
amount-and-time rules. OpenBMT does not put the address or transaction ID in
the POS fields. The records library has no books screen in the apps yet.

That integration is limited today. It has been checked against vendor
documentation rather than real seller accounts, and closing the order remains
manual. The useful idea is the boundary: OpenBMT should add direct Bitcoin
acceptance without demanding that the merchant migrate the rest of the shop
into an OpenBMT platform.

## Convenience without giving the money back to a middleman

OpenBMT's design can be summarized by the questions it answers for the shop:

| Merchant question | OpenBMT's answer |
|---|---|
| Where does the payment go? | To an address from the shop's wallet. |
| Can the counter spend it? | No. The counter has no wallet spending key. |
| Who approves money leaving? | Normally two of the shop's three wallet keys. |
| Who decides when a sale is acceptable? | The owner sets supported limits; the counter applies them. |
| What supports the payment result? | The screen names the transaction, block or own-node evidence it used. |
| Must the shop use an OpenBMT server? | No. There is no OpenBMT payment account or required OpenBMT backend. |
| Who chooses whether to keep or sell the bitcoin? | The shop does. |
| Who recovers the wallet? | The shop, using its separately kept keys and recovery information. |

None of these answers eliminates work. Together, they make the work
predictable. The software carries the payment rules into checkout, prepares
reviewable actions, separates everyday roles and keeps a record the owner can
compare independently.

That is the convenience OpenBMT is pursuing: not one-tap custody, but a direct
payment system a merchant can understand and operate without rebuilding its
key-handling rules for every sale.

## Why the combination matters

Bitcoin gives a merchant the unusual ability to receive a digital payment
without asking a payment company to hold the proceeds. OpenBMT's contribution
is not any one feature in isolation. It is making direct receipt, constrained
counter authority, separate spending approval, explicit payment evidence and
business records parts of one required arrangement.

Existing tools can provide several of those pieces. OpenBMT's position is that
the combination should be the normal design, not a custom configuration a
merchant has to invent.

The current software has not proved the result yet.
Physical-device testing, release independence, real POS and Lightning trials,
reproducible builds and an independent audit are still outstanding. Those
limits are collected in [What is built and tested](status.md).

The project succeeds only if both sides work: a cashier can use it during a
busy shift, and the owner can independently check where the money went and
who can spend it. OpenBMT is built around both requirements.
