<a id="what-openbmt-will-never-do"></a>

# Why Bitcoin only

OpenBMT is intended for a shop that wants to receive bitcoin directly and
keep control of it. That is a choice of money and of the network used to
settle payments. It is not a promise that Bitcoin will have the lowest fee
or that its price will be stable.

<a id="never-anything-but-bitcoin"></a>
<a id="why-bitcoin"></a>

## Bitcoin's rules and the network enforcing them

Bitcoin's issuance schedule approaches a limit of 21 million. A full node
checks that rule itself and rejects a block creating too much, even if
miners produced it. Developers can release different software, but cannot
make a holder's existing node accept different rules.[^validation]

A limit written into software is useful only while people enforce it.
Payments also depend on miners doing the work that protects settlement
and people continuing to hold and accept the currency. OpenBMT regards
Bitcoin's record of enforcing its issuance rules, independent validation
and established use as the strongest basis for the money it supports.
That is the project's judgment about a monetary network.[^network]

<a id="why-that-excludes-bitcoin-cash-and-litecoin"></a>

## Other monetary networks

Other currencies can use proof of work, cap their supply and let anyone
verify issuance without a central issuer. Those properties do not make
the currencies interchangeable. Each network settles its own asset under
its own rules.[^other-networks]

Copying Bitcoin's code or choosing another supply cap does not by itself
reproduce the people using Bitcoin, the mining work protecting its current
payments or the continued agreement on its rules. A chain split can share
the earlier transaction record, then settle a different currency under
separate rules. It does not create more BTC on the Bitcoin record.[^forks]

Verification also has an operating cost. If more transaction data has to
be downloaded and checked, running a full node can become more demanding.
OpenBMT favors keeping that check practical for the shop.[^verification-cost]

The project gives more weight to Bitcoin's monetary rules and independent
verification than to another network's advertised fee or block interval.
For small purchases, its supported alternative is bitcoin through the
shop's own Lightning node. For on-chain purchases, the shop can set a
minimum sale and decide how many confirmations to wait for.

## Tokens, sidechains and mints

An issuer-controlled token adds a dependence on the issuer, which may
retain powers over creation, redemption or transfers. That is an
additional objection, separate from the choice between independently
mined currencies.

The exclusion covers stablecoins, other coins and tokens on every layer,
including tokens on Lightning, Taproot Assets and RGB. Using Bitcoin-related
infrastructure does not make a different asset into bitcoin.

A federated sidechain such as Liquid asks a group of operators to hold
the coins behind its system. An ecash mint such as Cashu or Fedimint holds
money while giving users claims against it. OpenBMT excludes those claims
as well. These are permanent restrictions, including optional features
and later versions.

The [technical policy
reference](network-reference.md#never-anything-but-bitcoin)
explains the transaction-format restrictions and their limits. Those
checks implement a Bitcoin wallet; they should not be mistaken for a
universal detector of every token protocol.

<a id="never-custody"></a>

## The shop keeps custody

OpenBMT and Avila Labs hold no shop wallet key or account balance. The
phones holding the wallet keys belong to the shop, and a payment does
not pass through an OpenBMT company account. That prevents a processor's
payout freeze from locking the wallet's funds.

It leaves recovery with the shop. Losing two usable keys can make the
money unspendable. A bad software release also remains a risk while the
Key app's independent release arrangements are unfinished. The
[wallet chapter](safety.md) explains both limits.

<a id="never-an-account-or-an-identity-check"></a>

## There is no OpenBMT sign-up

OpenBMT requires no account or identity verification. Devices identify
themselves using keys, and staff names are labels entered by the shop.
The shop still has its own POS and exchange relationships. Their account
and legal requirements remain outside OpenBMT.

<a id="never-a-third-party-deciding-where-money-goes"></a>

## Payment addresses come from the shop's devices

A remote address-selection service could substitute someone else's
address for the shop's. OpenBMT instead uses addresses approved by the
shop's own devices. The Box cannot select or change them.

It also avoids public address-indexing services, which could learn the
shop's history from its address queries. The counter requests Bitcoin
network data without handing peers its shop addresses. Peers can still
learn which blocks it asks for or withhold information. The
[privacy chapter](privacy.md) explains that exposure.

<a id="never-telemetry-analytics-or-crash-upload"></a>

## No automatic reporting to the publisher

OpenBMT does not upload analytics or crash data. The Key app has no network
permission. The other apps make the contacts described in
[Privacy](privacy.md). To share a crash log, the shop must export it and
send it manually.

The operating system or a compromised device can still leak information.
The app rules and permission checks have not had independent review.

<a id="never-automatic-updates-and-never-a-remote-switch"></a>

## The owner chooses updates

The apps do not check for and install their own updates, and there is no
OpenBMT service that can switch the shop off remotely. Counter commands
must come from the shop's owner and are protected against replaying old
commands.

A Box host such as Umbrel or StartOS controls its own updates, which the
shop must manage there. The proposed signed release process, installation
delay, F-Droid or signed-file distribution and reproducible builds are
still planned. Google Play distribution is excluded.

<a id="never-a-lightning-service-provider-holding-the-shops-channels"></a>

## Lightning belongs on the shop's node

OpenBMT connects only to the shop's LND or Core Lightning node. It does
not choose, run or rent one. A provider holding the channels would control
those funds and see payments.

A swap provider is different: it never holds the shop's money, because the
bitcoin it locks can be claimed only by the shop. Lightning through a swap
provider the shop picks is planned as an option, off by default (design
section 8). It is not built into the apps yet.

Naming or changing the node needs approval through the Key app. OpenBMT
cannot establish who actually operates the named node: pointing it at a
node run by a hosting company would leave those channels with that
operator. The shop must manage liquidity, updates and backups, and the
counter integration has not yet been tried against a real node.

<a id="never-calling-an-unconfirmed-payment-final"></a>

## The report cannot promise more than the check

An unconfirmed on-chain payment can be replaced. A payment in a block can
still be affected by a chain reversal. The counter identifies its evidence
and uses the shop's refund depth for "settled" or "final". The own-node
label requires both the shop's node and the light check to see the
payment. No screen says "fully verified".

<a id="never-closed-source"></a>

## The source remains available

The code, rules and design are published under the MIT licence. The
project also restricts dependency licences. A shop can inspect and build
the source, although reproducible releases that can be independently
checked against it are not built yet. Dependencies, the phone's operating
system and Orbot remain separate software with their own licences and
maintainers.

[^validation]: [Bitcoin Core validation](https://bitcoin.org/en/bitcoin-core/features/validation)
    explains full-node enforcement of the supply limit. The
    [Bitcoin FAQ](https://bitcoin.org/en/faq)
    describes voluntary adoption of protocol rules. Read 2026-11-11.

[^network]: The
    [Bitcoin developer guide](https://developer.bitcoin.org/devguide/block_chain.html)
    describes independent validation and proof of work. The
    [Bitcoin FAQ on value](https://bitcoin.org/en/faq)
    explains the role of acceptance and adoption. The judgment about which
    network OpenBMT should use is the project's. Read 2026-11-11.

[^other-networks]: Examples of independently mined currencies are described
    in the [Bitcoin Cash protocol documentation](https://documentation.cash/protocol/blockchain/proof-of-work.html)
    and [Litecoin proof-of-work reference](https://litecoin.info/docs/key-concepts/proof-of-work).
    Read 2026-11-11.

[^forks]: The
    [Bitcoin developer guide on consensus changes](https://developer.bitcoin.org/devguide/block_chain.html#consensus-rule-changes)
    explains how incompatible rules can split a chain. The distinction
    between copying software and reproducing a monetary network follows
    from those separate rules and participants. Read 2026-11-11.

[^verification-cost]: Bitcoin Core's
    [capacity discussion](https://bitcoincore.org/en/2015/12/23/capacity-increases-faq/)
    explains the validation costs involved in increasing capacity and the
    use of payment channels to reduce on-chain data. Published 2015-12-23;
    read 2026-11-11. This is a design tradeoff, not a measurement of current
    node costs.
