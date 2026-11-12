# Network checks, risks and policy details

For the information a merchant needs before choosing a setup, start with
[Privacy](privacy.md) and [Risks](limits.md). This reference describes
network behavior and the detailed threat model.

## Sealed messages and identity authentication

Application messages carry their own signatures and policy checks. The
link adds confidentiality, sender authentication and replay handling; it
does not turn the contents into trusted payment evidence.

For each message, the sender generates a fresh one-time secp256k1 key and
uses ECDH with the recipient identity. HKDF-SHA256 derives a new
ChaCha20-Poly1305 key, binding the one-time public key and both identities.
The nonce is zero because the key is one-use. That construction requires
fresh, unpredictable ephemeral secrets; nonce reuse under a repeated key
would violate the design's premise (LK-02, LK-03).

A BIP340 tagged signature authenticates the header and ciphertext hash.
The header binds recipient, sender, message number, kind and one-time key.
A recipient checks the frame, identity authorization, signature, message
number, decryption and padding before returning contents. It persists
the new last-seen message number before delivery. Gaps are allowed, but
old numbers and jumps more than 2^32 above the last are refused.

Padding buckets are 256, 1,024, 4,096, 16,384 and 65,536 bytes. A relay can
see sender and recipient identities, message kind and padded size, even
without plaintext. Those device identities are separate from wallet keys
and payment addresses. The Box's order-reader role additionally reveals
order amounts and timing.

The full wire forms and message authorization matrix are in
[Link rules](https://github.com/AvilaLabs/OpenBMT/blob/main/docs/LINK-RULES.md),
with [sealing code](https://github.com/AvilaLabs/OpenBMT/blob/main/crates/openbmt-link/src/seal.rs).

## Network privacy

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
and price sources, go through Tor. The counter runs Tor itself, or uses
Orbot, a separate Android app, if the manager chooses it under Tor in the
counter's settings. If Tor cannot connect, the counter reports the problem
and sends nothing around it (`CR-87`, `CR-201` to `CR-204`).

The Owner app sends its Bitcoin peer traffic and spend or refund
broadcasts through Tor. It runs Tor itself, or uses Orbot if the owner
chooses it in Settings, and does not start its node until Tor is
connected (`OA-05`, `OA-97` to `OA-100`). Its behavior does not depend on
the counter's switch. An
explicit `direct` local link route is not sent through Tor.

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


## Limits and risks

> **OpenBMT has had internal review only and no independent security audit.
> None of the Android apps has been run on a phone. Do not use it with real
> funds.**

### What the protection claims, and when it does not hold

The wallet design aims to withstand one compromised OpenBMT component
without allowing a spend. The script itself enforces two keys, not two
distinct people: a person controlling two usable keys can spend. Device
and holder separation are operational conditions. The design also requires
both of these launch conditions:

1. Key 3 is generated outside OpenBMT software and away from the key 2
   device.
2. An independent second person holds the Key app's release key.

The first is part of the setup design. The second is unmet: no second
holder has been named. Reproducible builds and an independent rebuilder
are also launch requirements and are not in place. The single-component
protection claim does not hold today; the documents and screens say so.

Even with those conditions met, protecting the wallet does not prevent
every loss at the counter. Limits bound the exposure of sales proceeds
and goods, and the owner must reconcile records to catch a bad counter,
app build or Box.

### The threat model in plain words

| Incident | Potential loss | Protection and its limit |
|---|---|---|
| One phone stolen | One of the three keys; insufficient to spend alone | PIN, wipe and two remaining keys |
| Two keys stolen | The wallet | Keep the owner phone, spare phone and off-site steel words apart |
| Malicious main-app build | Wrong addresses or false "Paid" screens | Key app co-signing, separate release key and manual updates; release separation is incomplete |
| Counter displays its own address and reports payment | Sales proceeds until detected | Owner matching of orders to funds and the monthly outside check |
| Customer cancels an instant payment | Up to the instant limit | Per-sale and exposure limits, alarm, signed cancel record and pause |
| Hostile cashier | Up to the cashier's caps | Per-sale and daily caps, exposure total and names in signed records |
| Compromised or hosted Box | False payment notices or withheld messages | It cannot spend, select addresses or close an order without the counter's signature; owner independently matches orders |
| Refund sent to the wrong destination | Up to the Key app's ceilings | Satoshi ceilings, allowlist enrollment on the Key app and review on both devices |
| Recovery kit lost along with one key | Wallet cannot be spent | Two keys are still required; reminders and inspection remain design-only |
| All reachable peers controlled by an attacker | False sightings or missed conflicts | Distinct network groups, own-node checks and disabling instant when checks are unhealthy |

### What the light check cannot see

The light check asks Bitcoin peers directly. For an unconfirmed payment,
it cannot inspect input amounts, establish whether the inputs are
confirmed, or calculate the fee rate. It monitors conflicting spends and
requires different peer network groups. It does not execute scripts or
check each transaction's coins within a block (`docs/WATCHER-RULES.md`,
section 8). Confirmation depth is therefore a choice about how much work
must accumulate above the payment.

An attacker controlling every reachable peer controls what the counter
sees. Distinct network groups, stored long-lived peers and Tor make this
harder without ruling it out (`docs/NETWORK-RULES.md`, section 8).

On signet, the header check verifies work only. Test-network results do
not establish protection for real funds.

### What the PIN does not stop

The phone's key file is locked behind the PIN, and guessing is slowed.
That does not protect against someone using an unlocked phone, restoring
an older stored file, or obtaining both the file and the Keystore material
needed for offline guessing.

A rooted phone, hostile keyboard or accessibility service can read input
or displayed data. The Key app refuses to run with debugging or an
accessibility service enabled, but that refusal is enforced by the app
itself. A cashier PIN on the counter deters casual access only.

### Phone and hardware risks

The Counter app runs only while open, and the counter must stay open and
plugged in. The Owner app follows the chain while closed only with Watch in
the background turned on, and Android may stop that while the phone sleeps.
Both apps run Tor themselves, unless the owner or the manager chooses
Orbot, a separate Android app, instead.
Some phones offer a software-only Keystore; the app reports this on screen.
The design assumes the phone is not already
controlled by an attacker.

Android versions may handle wallet opening from an NFC tag differently.
Tap to pay is not built and this behavior is untested.

### Unverified items

| Area | What remains unknown |
|---|---|
| Hardware signers, experiment E8 | Coldcard, Jade, Passport and Keystone have not been tested for wallet acceptance, change checks or full previous-transaction requirements. Large QR transfer frame counts are unmeasured. Bitcoin Core was tested as a second signer on a test network; Sparrow was not. |
| POS readers, E5 and E6 | No real Square or Clover seller account has been used. Square's custom-tender order fee is unknown from its pages. Clover's virtual-currency app rules could not be read. |
| Lightning | Request and response formats came from source and documentation. No real node has been run. |
| Sparrow recovery | No restore has been run. |
| NFC tap, E1 and E10 | Not built. |
| Timings | Setup time, time to "Seen", battery use and bandwidth have not been measured. |
| Competitor figures in the design | Marked "to verify" and excluded from merchant screens until sourced. The processor fees in [Why bitcoin](why-bitcoin.md) came from their own pages, read 2026-11-11, and may change. |

### The monthly outside check

Once a month, open the recovery kit's wallet description as watch-only in
Sparrow or Bitcoin Core. Compare the receipts with the POS Bitcoin-tender
total. This is the only check independent of OpenBMT's software, and the
only one that catches a counter or main-app build substituting its own
address while claiming payment.

The estimate is five minutes; it has not been measured. In-app reminders
and instructions are planned.

### The audit status

The four internal design reviews examined remote theft, insiders, hostile
customers, wallet engineering, privacy, adversarial behavior and recovery.
They also checked single-component compromise and compared the first
implementation with the design. They were not an independent audit.

Device testing, outside code review and an independent security audit
remain necessary before OpenBMT can be considered for real funds.


## What OpenBMT will never do

These restrictions apply to future versions as well as the current design.
Some depend on release safeguards that are still unfinished; those limits
are stated here. The [design choices](principles.md) explain how they shape
the checkout system.

### Never anything but bitcoin

OpenBMT accepts BTC on the Bitcoin network, and bitcoin over the shop's own
Lightning node. Other currencies, stablecoins and tokens on any layer are
excluded.

#### Why Bitcoin

OpenBMT's starting point is money whose supply the holder can verify and
whose rules do not depend on an issuer's discretion. Bitcoin's issuance
schedule approaches a limit of 21 million. A full node checks that schedule
itself and rejects a block that creates too much, even if miners produced
it. Developers can publish different software, but cannot make a holder's
node accept different rules.[^policy-validation]

A supply limit is useful only while people enforce it. The currency also
needs miners doing the work that protects settlement and people willing to
hold and accept it. OpenBMT regards Bitcoin's record of enforcing its
issuance rules, independent validation and established use as the strongest
basis for money. That judgment is why the project builds on this network;
a supply cap or proof-of-work algorithm alone is not enough.[^policy-network]

<a id="why-that-excludes-bitcoin-cash-and-litecoin"></a>

#### Other monetary networks

Other currencies can also use proof of work, cap their supply and let
anyone verify issuance without a central issuer. Those properties do not
make the currencies interchangeable. Each network settles its own asset
under its own rules.[^policy-other-networks]

Copying Bitcoin's code or specifying another supply cap does not by itself
reproduce the people choosing to use Bitcoin, the mining work protecting
its current payments, or their continued agreement on its rules. A chain
split can share the earlier ledger, but the networks follow separate rules
and settle different currencies after the split. Creating those currencies
does not create more BTC on the Bitcoin ledger.[^policy-forks]

Independent verification also depends on what it costs to run a node.
More transaction data has to be downloaded and checked; increasing
base-layer capacity can make that work more demanding. OpenBMT favors
keeping verification practical for the shop.[^policy-verification-cost]

OpenBMT gives greater weight to preserving Bitcoin's monetary rules and
independent verification than to a different chain's block interval or
advertised transaction fee. Lower fees alone are not a reason to ask the
shop to receive another currency. For small purchases, the supported
alternative is bitcoin through the shop's own Lightning node. For on-chain
payments, the shop can set a minimum sale and choose how many confirmations
to wait for.

#### Tokens, sidechains and mints

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
(`SR-22`, `SR-06`). These format checks implement the supported wallet;
they are not a general detector of all token protocols. Lightning
accepts bitcoin invoices from the shop's own node only (`LN-01`). The
checks follow the currency choice described above.

Bitcoin still has price risk and fees. Choosing it does not establish a
stable purchasing power or remove the payment risks described in
[A sale at the counter](getting-paid.md). The exclusions are not trials or
features
waiting for a later release.

### Never custody

Neither OpenBMT nor Avila Labs holds a shop's wallet key or balance, and
there is no hosted wallet. A processor's failure, sanction or account
freeze therefore cannot lock up money held in the shop's own wallet.

The Owner app holds wallet key 1 and its separate owner identity secret;
it holds no other wallet key (`OA-01`, `OA-08`). The default Key app holds
wallet key 2 as well as its own identity. The counter cannot sign a spend. Its
refund request is a
record, not a payment (`CR-60`). The Box holds no wallet description,
address, transaction ID or key that signs for the shop (`BX-04`).

The shop takes responsibility for lost and stolen devices; see
[Who controls the money](safety.md). Avila Labs will hold app release
keys, which are separate from wallet keys. A malicious app release remains
a risk: the Key app is separate, but its independent release-key holder
has not been named.

### Never an account or an identity check

OpenBMT requires no sign-up or KYC. An account could be closed, frozen,
subpoenaed or breached, and software that holds no funds for a shop should
not need its identity.

A device identifies itself with a key it generates. Staff names on the
counter are labels entered by the shop, without identity verification
(`CR-156`). There is no OpenBMT account or payment server.

The shop still deals with accounts at its POS vendor and chosen exchange.
Those companies have their own sign-up and checks. OpenBMT neither asks
for those accounts nor sees them.

### Never a third party deciding where money goes

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

### Never telemetry, analytics or crash upload

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

### Never automatic updates, and never a remote switch

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

### Never a Lightning service provider holding the shop's channels

Lightning connects to the shop's existing LND or Core Lightning node.
OpenBMT does not run, rent or select one (design section 8). A provider
holding the channels would hold the money and see every payment. A swap
provider never holds the shop's money, so Lightning through one the shop
picks is planned as an option, off by default and reached over Tor only.

The node's public key is in the signed settings. Naming or changing it
needs the Key app's co-signature, and each invoice must be signed by that
key (`LN-03`, `LN-17`). Setup checks that the counter's credential permits
invoices only (`LN-04` to `LN-20`).

OpenBMT cannot establish who actually operates the named node. If the shop
chooses a hosting company's node, that company holds its channels.
OpenBMT offers no such provider. Setup explains that the shop must manage
liquidity, updates and backups. The counter integration is built but has
never been run against a real node.

### Never calling an unconfirmed payment final

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

### Never closed source

The code, rules and design are published under the MIT licence in
`LICENSE`. A shop should be able to read the source, build it and check
what it is being asked to install. The dependency policy accepts only
licences on its published list (`docs/DEPENDENCIES.md`, rule 6).

Dependencies retain their own licences. OpenBMT does not publish the
phone's operating system or Orbot. The Owner and Counter apps include
Tor itself, as built by the Guardian Project from the Tor Project's code
(`docs/DEPENDENCIES.md`).
Reproducible builds, which would let someone check a released app against
the source, are not built yet.

[^policy-validation]: [Bitcoin Core validation](https://bitcoin.org/en/bitcoin-core/features/validation)
    explains full-node enforcement of the supply limit. The
    [Bitcoin FAQ](https://bitcoin.org/en/faq)
    describes voluntary adoption of protocol rules. Read 2026-11-11.

[^policy-network]: The
    [Bitcoin developer guide](https://developer.bitcoin.org/devguide/block_chain.html)
    describes independent validation and proof of work. The
    [Bitcoin FAQ on value](https://bitcoin.org/en/faq)
    explains the role of acceptance and adoption. The judgment about which
    network OpenBMT should use is the project's. Read 2026-11-11.

[^policy-other-networks]: Examples of independently mined currencies are
described
    in the [Bitcoin Cash protocol documentation](https://documentation.cash/protocol/blockchain/proof-of-work.html)
    and [Litecoin proof-of-work reference](https://litecoin.info/docs/key-concepts/proof-of-work).
    Read 2026-11-11.

[^policy-forks]: The
    [Bitcoin developer guide on consensus changes](https://developer.bitcoin.org/devguide/block_chain.html#consensus-rule-changes)
    explains how incompatible rules can split a chain. The distinction
    between copying software and reproducing a monetary network follows
    from those separate rules and participants. Read 2026-11-11.

[^policy-verification-cost]: Bitcoin Core's
    [capacity discussion](https://bitcoincore.org/en/2015/12/23/capacity-increases-faq/)
    explains the validation costs involved in increasing capacity and the
    use of payment channels to reduce on-chain data. Published 2015-12-23;
    read 2026-11-11. This is a design tradeoff, not a measurement of current
    node costs.
