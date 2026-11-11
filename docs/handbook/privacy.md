# Privacy

OpenBMT sends no analytics or crash data to anyone. These are the contacts it
does make, what each party sees, and how to reduce it.

> **Status: the network behavior below is Built and was checked against
> Bitcoin Core and, for observation, on test and live networks. The apps have
> not been run on a phone.**

## Who sees what

| Party | What it sees | How to reduce it |
|---|---|---|
| Bitcoin peers (the light check) | The counter's network address, that it follows the chain, and which blocks it asks for | Tor; the shop's own node |
| The shop's internet provider or carrier | That the device speaks Bitcoin, and when. With Tor, only that it uses Tor | Tor |
| DNS seeds (finding peers) | The address of a device looking for peers | Tor looks the seeds up itself, so no name reaches the phone's resolver |
| Price sources | That a device fetched the price, and when. No sale, no address | A fixed 60-second schedule, never triggered by a sale; fetch over Tor |
| The POS vendor | Every order, amount and the "Bitcoin" tender. Never an address or a transaction ID | None. The POS sees its own orders |
| The Box's host, if someone else runs it | Orders, amounts and timing: how much the shop sells. Never addresses, transaction IDs or the wallet description | Run the Box yourself. A hosted Box is **Not built yet** |
| Push services | Nothing, today: push wake-ups are **Not built yet** | |
| Where the apps are downloaded | The downloader's address and the version | Download over Tor |
| The customer's phone | The address and amount in the QR code for that sale | A fresh address each sale |
| An exchange used for cash-out | The deposit and, through it, what the owner chooses to send | Coin control (see below) |

The counter sends no message that holds an address or a script of the shop's
(`NR-60`). It finds payments by asking peers for compact block filters and, when
a filter matches, whole blocks. A peer that is asked for a block learns the
counter was interested in that block. The counter also downloads every new
block in full while a payment is waiting for its first block, because a
cancelling spend is in no filter it can match (`NR-44`). So the peers learn
the counter's interest in those blocks, and the rules do not hide that
interest completely (`NR-61`).

## Tor

The counter's **Tor switch** routes every connection, peers and price fetches
through Orbot, a separate Android app that must be running. With Tor on and
Orbot not running, nothing is sent around it and the counter says so
(`CR-87`).

The Owner app has **no switch**: all of its traffic goes through Tor, and it
will not start if Orbot is not answering (`OA-05`). Spends and refunds are
broadcast over Tor. That is a separate matter from the counter's switch.

The counter's Tor switch is off by default for speed. On a network that
customers can join, instant runs only with Tor on, because the customers could
feed the counter false data.

Peers also speak an encrypted transport (BIP324) when they offer it, which
hides what the counter asks from someone who only listens. It does not say who
the peer is (`NR-62`).

**Stops at.** Tor hides the device's address from peers. It does not hide that
the traffic belongs to a Bitcoin wallet (`OA` section 9). Onion peers are
grouped coarsely, so two onion peers are not evidence of two operators
(`docs/NETWORK-RULES.md` section 8).

## The shop's own node

The merchant may run a Bitcoin Core node (on a Box, Umbrel, StartOS or at home)
and let the counter check payments against it as well as against the light
client. It adds a check and never replaces the light client.

In the built counter the own node must be reached at a **Tor v3 address**. The
address is what authenticates it, because the name of a version-3 address is the
node's public key. A node on the shop's own network or on a plain internet
address is not accepted as the own node, because an address says nothing about
who answers (`NR-98`, `NR-99`, `CR-100` to `CR-102`). The counter also keeps its
own header chain from random peers, so a Box cannot invent a confirmation.

## The Box

The Box is a small server the shop owns. It relays the shop's sealed messages
between its devices, reads the POS, and matches orders to sales. It sees order
amounts and timing in the shop's money, the way the POS vendor does, but never
an address or a transaction ID, and it holds no key (`BX-04`, `BX-40`). Messages
it relays are sealed so that only their recipient can read them. It is reached
by an onion address or on the local network.

Whoever runs a Box can estimate payments from order amounts and times and a
public price. Only running the Box yourself keeps that inside the shop
(`docs/BOX-RULES.md`, section 6).

## The books and the export

The accounting export carries opaque sale IDs only. Transaction IDs, addresses
and satoshi amounts never go into the accountant's file or into any POS field,
because anyone who has them could link the shop's payments on the blockchain.
The full ledger, which does carry them, sits behind a warning.

Spending many receipts at once links them on the blockchain: anyone who paid
once, and the exchange, can see that they belong to one wallet. Merging receipts
has the same effect. The app says so when it offers a merge (design section 9).
