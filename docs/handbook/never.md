# What OpenBMT will never do

These restrictions apply to future versions as well as the current design.
They are enforced through the veto described in [Principles](principles.md).
Some depend on release safeguards that are still unfinished; those limits
are stated here.

## Never anything but bitcoin

OpenBMT will not support other coins, stablecoins or tokens on any layer.
That includes tokens on Lightning, Taproot Assets and RGB, federated
sidechains such as Liquid, and ecash mints such as Cashu or Fedimint. They
will not be added as trials, optional features or later phases.

The objection is control. Other coins place their rules or supply under an
issuer, foundation or small group; a token issuer can freeze tokens or
rewrite their rules. A federation holds the coins behind its sidechain,
and a mint holds money while issuing claims against it. Bitcoin's rules
change when node operators choose to run new rules. OpenBMT is intended
for a shop that wants to hold its own money without those intermediaries.

The wallet is native segwit, two of three (`SR-60`). The Key app rejects
data outputs, non-standard scripts and inputs containing Taproot fields
(`SR-22`, `SR-06`), and refuses transactions carrying tokens. Lightning
accepts bitcoin invoices from the shop's own node only (`LN-01`).

The design's section 0 excludes these protocols through the veto rather
than naming each one. New protocols would face the same restriction.

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
