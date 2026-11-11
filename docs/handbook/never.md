# What OpenBMT will never do

These commitments apply to every version of OpenBMT. Design and code changes
must meet them under the veto in [Principles](principles.md).

Each has been checked against the design and code. Where a commitment depends
on unfinished work, that is stated below.

## Never anything but bitcoin

OpenBMT accepts bitcoin only. It will not add other coins, stablecoins, or
tokens on any layer, including tokens carried on Lightning, Taproot Assets and
RGB. It also excludes federated sidechains such as Liquid and ecash mints such
as Cashu or Fedimint. These will not be offered as trials, optional features
or later additions.

Every other coin has an issuer, a foundation or a small group that can change
its rules or its supply. Bitcoin's rules change only when the people running
nodes choose to run new rules. Each of the others also puts someone between
the shop and its money. A token has an issuer who can freeze it or rewrite its
rules. A federated sidechain has a federation that holds the coins. An ecash
mint holds the money and hands out claims on it. Under the veto, anything that
lets a third party hold, move or freeze the shop's funds is dropped. These
arrangements would make the shop depend on a claim against someone else rather
than hold its own money.

The design's section 0 veto covers every item above. The wallet is an ordinary
bitcoin wallet: native segwit, two of three (`SR-60`). The Key app refuses to
sign data outputs, non-standard scripts or inputs with Taproot fields
(`SR-22`, `SR-06`), so it refuses a transaction that would carry a token. The
Lightning option accepts bitcoin invoices from the shop's own node only
(`LN-01`).

The design rules these protocols out through the veto rather than naming each
one. The same veto would apply to a new protocol.

## Never custody

OpenBMT and Avila Labs hold no wallet keys. OpenBMT offers no hosted wallet
and holds no balance for anyone.

Keeping the money in the shop's wallet protects it from a processor failing,
being sanctioned or freezing the account.

The Box holds no wallet description, address or transaction ID, and no key
that signs for the shop (`BX-04`). The counter holds no key that can move
funds. A refund request records a request; it does not send a payment
(`CR-60`). The Owner app holds key 1 and no other secret key (`OA-08`); the
Key app holds key 2.

Avila Labs will hold release keys that sign the apps. These are separate from
the shop's wallet keys and are covered below under updates.

Losing the shop's devices can mean losing its keys. [How your money is kept
safe](safety.md) covers loss and theft. A publisher who signs the apps could
in principle ship a bad build. The separation of the Key app from the main
app, and the second holder of the Key app's release key, exist for that. The
second holder has not been named yet.

## Never an account or an identity check

OpenBMT requires no sign-up or KYC, and ties nothing to a person.

An account can be closed, frozen, subpoenaed or breached. A shop should not
have to prove its identity to software that holds no funds for it.

There is no OpenBMT server in the payment path and no OpenBMT account anywhere
in the apps. A device is identified by a key it makes itself, not by a person.
Staff names on the counter are labels the shop types, and no check ties them
to a real identity (`CR-156`).

Accounts that other companies run still exist around the edges. The POS vendor
account is the shop's own account with that vendor. An exchange used for
cash-out has its own sign-up and checks. Those are the shop's relationships.
OpenBMT neither asks for them nor sees them.

## Never a third party deciding where money goes

Nobody outside the shop's own devices picks the address a customer pays, and
no public indexer (an Electrum or Esplora server) watches the shop's
addresses.

An address server could substitute a different address. A public indexer that
is given the shop's addresses learns every sale and can tie them together.

Addresses come only in batches signed by two separate devices. The counter
checks both signatures against the two roots it pinned at pairing. It cannot
make an address of its own (`CR-02`, design section 5.1), and the Box cannot
choose or change one (`BX-04`).

No message the counter sends to a Bitcoin peer contains a shop address or
script (`NR-60`). It finds payments by asking peers for compact block filters
and blocks.

A Bitcoin peer can still see that a device follows the chain and which blocks
it asks for, and a peer can stay silent about a payment. The counter makes
that harder by using several peers from different network groups, and says so
on screen when it could not check enough. It does not make it impossible
(`NR-61`, and section 8 of `docs/NETWORK-RULES.md`).

## Never telemetry, analytics or crash upload

Reporting sales volume and timing to the publisher would expose the shop's
business activity. OpenBMT keeps those records with the shop.

The Key app has no network permission. Its manifest lists the camera and
nothing else; a build check fails if that changes. The Counter app and Owner
app use their network permission for the contacts listed in the design:
Bitcoin peers, price sources, POS reads, and the shop's own Box and node. The
Owner app opens no connection of its own beyond the proxy greeting and the
till read (`OA-15`). [Privacy](privacy.md) lists every contact.

These rules and manifest checks have not been reviewed by anyone outside the
project. A device that is compromised, or an operating system that leaks, is
outside what these rules cover. A shop that wants crash information exports a
local log and sends it by hand.

## Never automatic updates, and never a remote switch

Automatic updates would let the publisher, or someone who takes over the
publisher, change what the shop's devices do without the owner's action. A
remote switch would give the same control.

The apps contain no update check. Each app's manifest check confirms there is
no background service, receiver or provider that could run one. The design
requires the owner's action before an update is installed.

There is no OpenBMT service for a remote switch. The counter accepts commands
signed by the shop's owner phone and numbered to prevent an old command being
replayed (`CR-14`).

The Box is a container, and OpenBMT ships no updater in it. Update behavior
belongs to the host system (a mini PC, Umbrel or StartOS), which the shop
controls. The signed release manifest, the install-date delay, the F-Droid and
signed-file release (never Google Play) and the reproducible builds the design
describes for the Key app are **Planned**, not built.

## Never a Lightning service provider holding the shop's channels

If a shop takes Lightning, it takes it on its own node, never through a
provider that holds the channels for it.

A provider that holds the channels holds the money and sees every payment.
Taking Lightning through a swap provider was considered and dropped under the
veto for exactly that reason.

Lightning connects the counter to a node the shop already runs (LND or Core
Lightning). OpenBMT does not run, rent or choose a node (design section 8).
The node's public key is part of the signed settings. Naming or changing it
needs the Key app's co-signature, and every invoice must be signed by that key
(`LN-03`, `LN-17`). Setup checks that the counter's credential is limited to
invoices (`LN-04` to `LN-20`).

The software cannot tell who runs the node named in the settings. If the shop
points OpenBMT at a node run by a hosting company, that company holds those
channels. OpenBMT offers no such provider and says on the setup screen that
running Lightning means watching liquidity, updates and backups. Lightning at
the counter is **Built** and has never been run against a real node.

## Never calling an unconfirmed payment final

OpenBMT will not call a payment "fully verified" unless the shop's own node
reported it.

Any unconfirmed bitcoin payment can be replaced by its sender, so calling one
"final" would misstate what the counter knows.

The counter's states are fixed (`CR-10`). A payment accepted before a block
shows "Paid" with the words "it could still be undone". A payment in a block
at the shop's depth is confirmed, and "final" is reserved for the refund
depth. The books count a sale as money only at the refund depth (`CR-31`).

Each screen names the check that was done (`CR-105`). The own-node label
applies only when the shop's node reported the payment and the counter's light
client also saw it (`CR-104`).

More blocks on top make a reversal less likely, but no number makes it
impossible. Reaching the refund depth does not mean zero risk.

## Never closed source

OpenBMT is published under the MIT licence. The code, the rules and the design
documents are in the open.

A shop using software to handle its money should be able to read the source,
build it and check that a published build matches.

The repository carries the MIT `LICENSE`. The dependency policy allows only
packages whose licences appear on a published list (`docs/DEPENDENCIES.md`,
rule 6).

OpenBMT's dependencies carry their own licences. A phone needs an operating
system and, for Tor, a separate app, neither of which OpenBMT publishes.
Reproducible builds, which let anyone check that a published app matches the
source, are **Not built yet**.
