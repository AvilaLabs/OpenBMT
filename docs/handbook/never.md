# What OpenBMT will never do

These restrictions apply to future versions as well as the current design.
Some depend on release safeguards that are still unfinished; those limits
are stated here. The [design choices](principles.md) explain how they shape
the checkout system.

## Never anything but bitcoin

OpenBMT accepts BTC on the Bitcoin network, and bitcoin over the shop's own
Lightning node. Bitcoin Cash, Litecoin and other currencies are excluded,
along with stablecoins and tokens on any layer.

### Why Bitcoin

OpenBMT's starting point is money whose supply the holder can verify and
whose rules do not depend on an issuer's discretion. Bitcoin's issuance
schedule approaches a limit of 21 million. A full node checks that schedule
itself and rejects a block that creates too much, even if miners produced
it. Developers can publish different software, but cannot make a holder's
node accept different rules.[^validation]

A supply limit is useful only while people enforce it. The currency also
needs miners doing the work that protects settlement and people willing to
hold and accept it. OpenBMT regards Bitcoin's record of enforcing its
issuance rules, independent validation and established use as the strongest
basis for money. That judgment is why the project builds on this network;
a supply cap or proof-of-work algorithm alone is not enough.[^network]

### Why that excludes Bitcoin Cash and Litecoin

Bitcoin Cash and Litecoin also use proof of work and public issuance rules.
They are separate currencies with their own networks. The objection to
issuer-controlled tokens does not explain their exclusion.[^other-networks]

Copying Bitcoin's code or specifying another supply cap does not by itself
reproduce the people choosing to use Bitcoin, the mining work protecting
its current payments, or their continued agreement on its rules. A chain split
can
share the earlier ledger, as Bitcoin Cash does, but the networks settle
different currencies after the split. Creating those currencies does not
create more BTC on the Bitcoin ledger.[^forks]

Independent verification also depends on what it costs to run a node.
More transaction data has to be downloaded and checked; increasing
base-layer capacity can make that work more demanding. OpenBMT favors
keeping verification practical for the shop.[^verification-cost]

OpenBMT gives greater weight to preserving Bitcoin's monetary rules and
independent verification than to a different chain's block interval or
advertised transaction fee. Lower fees alone are not a reason to ask the
shop to receive another currency. For small purchases, the supported
alternative is bitcoin through the shop's own Lightning node. For on-chain
payments, the shop can set a minimum sale and choose how many confirmations
to wait for.

### Tokens, sidechains and mints

An issuer-controlled token introduces another objection: its issuer may
retain powers over issuance, redemption or transfers. Those powers create
a dependence on that issuer, in addition to the reasons for choosing BTC
over another currency.

The exclusion includes tokens on Lightning, Taproot Assets and RGB. A token
carried over Bitcoin-related infrastructure is still a different asset.
Federated sidechains such as Liquid require a federation to hold the coins,
and ecash mints such as Cashu or Fedimint hold money while issuing claims
against it. OpenBMT does not accept those claims as payment.

The wallet is native segwit, two of three (`SR-60`). The Key app rejects
data outputs, non-standard scripts and inputs containing Taproot fields
(`SR-22`, `SR-06`), and refuses transactions carrying tokens. Lightning
accepts bitcoin invoices from the shop's own node only (`LN-01`). The
checks follow the currency choice described above.

Bitcoin still has price risk and fees. Choosing it does not establish a
stable purchasing power or remove the payment risks described in
[Getting paid](getting-paid.md). The exclusions are not trials or features
waiting for a later release.

## Never custody

Neither OpenBMT nor Avila Labs holds a shop's wallet key or balance, and
there is no hosted wallet. A processor's failure, sanction or account
freeze therefore cannot lock up money held in the shop's own wallet.

The Owner app holds key 1 and no other secret key (`OA-08`); the Key app
holds key 2. The counter cannot sign a spend. Its refund request is a
record, not a payment (`CR-60`). The Box holds no wallet description,
address, transaction ID or key that signs for the shop (`BX-04`).

The shop takes responsibility for lost and stolen devices; see
[The wallet and its keys](safety.md). Avila Labs will hold app release
keys, which are separate from wallet keys. A malicious app release remains
a risk: the Key app is separate, but its independent release-key holder
has not been named.

## Never an account or an identity check

OpenBMT requires no sign-up or KYC. An account could be closed, frozen,
subpoenaed or breached, and software that holds no funds for a shop should
not need its identity.

A device identifies itself with a key it generates. Staff names on the
counter are labels entered by the shop, without identity verification
(`CR-156`). There is no OpenBMT account or payment server.

The shop still deals with accounts at its POS vendor and chosen exchange.
Those companies have their own sign-up and checks. OpenBMT neither asks
for those accounts nor sees them.

## Never a third party deciding where money goes

Only the shop's devices can choose its payment addresses. A counter
accepts address batches signed by two separate devices, checked against
the roots pinned at pairing (`CR-02`, design section 5.1). It cannot make
an address of its own, and the Box cannot choose or alter one (`BX-04`).
An address-selection server would have the power to substitute a thief's
address.

OpenBMT also excludes public Electrum and Esplora indexers. Giving one the
shop's addresses would reveal its sales and let the server link them.
The counter instead requests compact block filters and blocks without
sending an address or script (`NR-60`).

Peers can still observe which blocks it requests or withhold information.
Several peers from different network groups make interference harder,
but cannot eliminate it. The counter reports when it lacks enough
information (`NR-61`; `docs/NETWORK-RULES.md`, section 8).

## Never telemetry, analytics or crash upload

OpenBMT does not send the publisher the shop's sales activity. The Key
app has camera permission only, with no network permission; a build check
fails if its manifest changes. The Counter and Owner apps have network
permission only for the contacts listed in the design: Bitcoin peers, price
sources, POS reads, and the shop's own Box and node. The Owner app itself
opens no connection beyond the proxy greeting and till read (`OA-15`).
[Privacy](privacy.md) lists all contacts.

These rules and manifest checks have not had outside review. They cannot
control a compromised device or an operating system that leaks data.
To share crash information, the shop must export a local log and send it
manually.

## Never automatic updates, and never a remote switch

The owner must choose to install an update. Automatic installation would
let the publisher, or someone taking over its releases, change the shop's
devices without that choice. The apps have no update check and no
background service, receiver or provider to run one; each app's manifest
check verifies this.

There is no OpenBMT service for a remote switch. Counter commands require
the owner phone's signature and sequence numbers that prevent replay
(`CR-14`).

The Box ships as a container without an updater. Its host, such as a mini
PC, Umbrel or StartOS, controls its own update behavior. The shop must
manage that there.

The Key app's proposed signed release manifest, install-date delay,
F-Droid and signed-file distribution, and reproducible builds are planned.
Google Play distribution is excluded.

## Never a Lightning service provider holding the shop's channels

Lightning connects to the shop's existing LND or Core Lightning node.
OpenBMT does not run, rent or select one (design section 8). A provider
holding the channels would hold the money and see every payment. The
same reason excluded third-party swaps.

The node's public key is in the signed settings. Naming or changing it
needs the Key app's co-signature, and each invoice must be signed by that
key (`LN-03`, `LN-17`). Setup checks that the counter's credential permits
invoices only (`LN-04` to `LN-20`).

OpenBMT cannot establish who actually operates the named node. If the shop
chooses a hosting company's node, that company holds its channels.
OpenBMT offers no such provider. Setup explains that the shop must manage
liquidity, updates and backups. The counter integration is built but has
never been run against a real node.

## Never calling an unconfirmed payment final

An unconfirmed payment can be replaced by its sender. The fixed counter
states (`CR-10`) therefore show "Paid" with "it could still be undone"
when accepting before a block. A payment is confirmed at the shop's
confirmation depth; "final" refers only to its refund depth. The books
count it as money at that refund depth (`CR-31`). More blocks reduce the
chance of reversal without making it impossible.

Each screen identifies its check (`CR-105`). The own-node label requires
both the shop's node and the counter's light client to see the payment
(`CR-104`). OpenBMT will not say "fully verified" without a report from
the shop's own node.

## Never closed source

The code, rules and design are published under the MIT licence in
`LICENSE`. A shop should be able to read the source, build it and check
what it is being asked to install. The dependency policy accepts only
licences on its published list (`docs/DEPENDENCIES.md`, rule 6).

Dependencies retain their own licences. OpenBMT does not publish the
phone's operating system or Orbot, the separate app needed for Tor.
Reproducible builds, which would let someone check a released app against
the source, are not built yet.

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

[^other-networks]: [Bitcoin Cash proof of work](https://documentation.cash/protocol/blockchain/proof-of-work.html)
    and [Litecoin's explanation](https://litecoin.com/what-is-litecoin)
    describe their respective networks. See also
    [Litecoin's proof-of-work reference](https://litecoin.info/docs/key-concepts/proof-of-work).
    Read 2026-11-11.

[^forks]: [Bitcoin Cash's history](https://bitcoincash.org/)
    records its 2017 chain split. The
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
