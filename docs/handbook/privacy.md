<a id="privacy"></a>

# What others can see

Paying with bitcoin does not require the customer to send a name or bank
account number with the payment. It still creates a public transaction.
Anyone who knows the payment address or transaction ID, its identifying
number, can look up the movement of bitcoin and follow links to other
transactions.

A customer knows the address used for their purchase. An exchange knows
the deposit you send it. A person who connects an address with your shop
may be able to learn more than the amount of one sale. OpenBMT limits
what it shares, but does not make the shop's activity invisible.

<a id="who-sees-what"></a>

## The main contacts

| Who | What it can learn |
|---|---|
| Customer | The address and amount for that purchase. |
| POS vendor | Its orders, prices and the fact that a Bitcoin payment type was used. OpenBMT does not put wallet addresses or transaction IDs in those fields. |
| Bitcoin peers | Computers used for payment checking can see the counter's network address and which blocks it requests. |
| Price sources | That a device requested a price and when. They receive no sale or wallet address. |
| Someone hosting a Box for you | Order amounts and timing, enough to estimate activity using the public bitcoin price. |
| Exchange | The deposit and the incoming payments linked when you spend them together. |

The [network reference](network-reference.md#who-sees-what) lists every
contact and the exact data exposed. OpenBMT sends no analytics or crash
uploads to its publisher. Push wake-ups and a hosted Box service are not
built yet.

## A fresh address helps, but is not a private account

OpenBMT uses a new address for each sale. That prevents every customer
from seeing all receipts at one reused shop address. The transaction
record remains public, and later spending can link the receipts.

Suppose you receive several purchases, then send their bitcoin together
to an exchange. That one transaction links the incoming payments. A
customer following their own payment, or the exchange receiving the
transfer, can see the association. Consolidating small incoming payments
has the same effect. The app warns when it offers a merge.

Wallet software can also leave recognizable patterns in the transactions
it builds. OpenBMT does not claim to hide which software created its
transactions. The [fingerprint
reference](network-reference.md#wallet-fingerprints)
links a detector for readers who want to inspect that issue.

<a id="tor"></a>

## What Tor changes

Tor routes connections through other computers so the destination does
not see the phone's ordinary network address. It does not hide the public
Bitcoin transaction. It also does not turn an unconfirmed payment into a
reliable one.

The Counter app's Tor switch covers its Bitcoin and price connections.
It is off by default for speed. If customers can join the counter's
network, instant acceptance requires Tor, because a customer on that
network could feed the counter false information. The Owner app sends its
Bitcoin peer traffic and broadcasts of spends
and refunds through Tor, with no switch to turn that off. An explicitly
configured local link between shop devices can run directly over the
local network.

Both apps need Orbot, a separate Android app, to use Tor. If Orbot stops
answering, the Owner app will not start and a counter with Tor enabled
will not send around it. This is a real operating dependency: turning Tor
on requires keeping Orbot running.

<a id="the-shops-own-node"></a>

## Checking through your own node

A full node checks the Bitcoin rules on a computer you run. OpenBMT can
add its report to the counter's own light check. It does not replace that
check or remove the need to wait for confirmations under the shop's
policy.

The current own-node connection requires a Tor v3 address. This address
also helps establish that the counter has reached the intended node.
The [network reference](network-reference.md#the-shops-own-node) explains
the connection restriction and why the counter retains its own chain
checks.

<a id="the-box"></a>

## Who runs the Box matters

The Box relays messages and reads POS orders. Device messages are
encrypted so only their intended recipient can read the contents. The
Box has no wallet key, wallet description,
address or transaction ID. It can still see order values and timing.
Someone else running it can estimate how much the shop sells.

Running it yourself keeps that information inside the shop. The Box can
be reached on the local network or through a Tor address. A hosted Box
service is not built yet.

<a id="the-books-and-the-export"></a>

## Give the accountant the accounting export

The ordinary accounting export uses sale IDs that do not reveal a
blockchain transaction by themselves. It excludes wallet addresses,
transaction IDs and bitcoin amounts in satoshis. The full ledger does
contain those details and is placed behind a warning. Choose the file
that fits the recipient's job rather than sending every wallet detail
with the accounts.

<a id="wallet-fingerprints"></a>

The network reference also explains block-request patterns, encrypted
peer connections and transaction fingerprints. These are limits on what
observers can learn, rather than a promise of anonymity.
