# Privacy

A Bitcoin payment need not carry a customer's name, but its transaction is
public. An address or transaction ID can let someone connect that payment
to other activity. OpenBMT keeps these details out of the POS and ordinary
accounting exports, and does not upload analytics or crash data.

> **This network behavior is built. It has been checked against Bitcoin
> Core and observed on test and live networks. The apps have not been run
> on a phone.**

## Who sees what

| Party | What it sees | What you can do |
|---|---|---|
| Bitcoin peers used by the light check | The counter's network address, that it follows the chain, and the blocks it requests | Use Tor; add the shop's own node |
| Internet provider or carrier | That the device speaks Bitcoin and when; with Tor, that it uses Tor | Use Tor |
| DNS seeds used to find peers | The address of a device looking for peers | With Tor, seed lookups stay inside Tor rather than reaching the phone's resolver |
| Price sources | Price requests and their timing, without a sale or address | Fetch over Tor; requests already run every 60 seconds rather than in response to sales |
| POS vendor | Every order, amount and "Bitcoin" tender, without an address or transaction ID | The vendor necessarily sees its own orders |
| An outside Box host | Order amounts and timing, without addresses, transaction IDs or the wallet description | Run the Box yourself; a hosted Box is not built yet |
| Push services | Nothing; push wake-ups are not built yet | |
| App download host | The downloader's network address and requested version | Download over Tor |
| Customer's phone | The address and amount for that sale | OpenBMT uses a fresh address for each sale |
| Cash-out exchange | Its deposit and the coins the owner chooses to send | Use coin control; see the accounting section below |

The counter never sends a shop address or script to a Bitcoin peer
(`NR-60`). It requests compact block filters, then whole blocks when a
filter matches. This reveals interest in those blocks. While waiting for
a payment's first block, it also downloads every new block in full:
a cancelling spend is absent from the filters it could match (`NR-44`).
These requests do not completely conceal which blocks interest the shop
(`NR-61`).

## Tor

With the counter's Tor switch on, all connections, including Bitcoin peers
and price sources, go through Orbot. Orbot is a separate Android app and
must be running. If it is unavailable, the counter reports the problem
and sends nothing around it (`CR-87`).

The Owner app always uses Tor and will not start if Orbot is not answering
(`OA-05`). That includes broadcasting spends and refunds. Its behavior
does not depend on the counter's switch.

The counter switch is off by default for speed. If customers can join its
network, instant acceptance requires Tor, because a customer on the same
network could feed the counter false data.

Peers use encrypted BIP324 transport when they support it. This conceals
requests from a passive listener but does not establish the peer's
identity (`NR-62`). Tor hides the device's address from peers; it does
not hide that the traffic belongs to a Bitcoin wallet (`OA` section 9).
Onion peers are grouped coarsely, so two onion peers do not establish two
independent operators (`docs/NETWORK-RULES.md`, section 8).

## The shop's own node

You can run Bitcoin Core on a Box, Umbrel, StartOS or a computer at home,
and have the counter check payments against it. The node adds a check;
it does not replace the counter's light client.

The current counter accepts an own-node connection only at a Tor v3
address. The address authenticates the node through its public key. A
local-network address or plain internet address is not enough to establish
who answers, so it is not accepted as the own node (`NR-98`, `NR-99`,
`CR-100` to `CR-102`). The counter retains its own header chain from random
peers, preventing a Box from inventing a confirmation.

## The Box

The Box relays sealed messages between the shop's devices, reads the POS
and matches orders to sales. It can be reached on the local network or
through an onion address. Only the recipient can read a relayed message.

It sees order amounts in the shop's currency and their timing, as the POS
vendor does. It holds no wallet key and sees no address, transaction ID
or wallet description (`BX-04`, `BX-40`). A host can nevertheless estimate
payments by combining the orders with a public Bitcoin price. Run the
Box yourself to keep that information inside the shop
(`docs/BOX-RULES.md`, section 6).

## The books and the export

The accountant's export contains opaque sale IDs. It excludes transaction
IDs, addresses and satoshi amounts, as do all POS fields, because those
would let the recipient trace payments on the public ledger. The full
ledger contains those details and is shown behind a warning.

Spending several receipts in one transaction links them on the blockchain.
Someone who paid the shop, or an exchange receiving the spend, can then
see that the receipts belong to one wallet. Consolidating receipts has
the same effect. The app warns when offering a merge (design section 9).

## Wallet fingerprints

Transactions can also reveal which wallet software built them. OpenBMT
does not claim to hide that fingerprint. A wallet fingerprint detector
is listed at <https://btcmaxis.com/bitcoin-information/privacy.html>
(read 2026-11-11).
