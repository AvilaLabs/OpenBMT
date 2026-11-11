# What OpenBMT will never do

These are the lines the project will not cross. Each section gives the line,
the reason, what makes it true today, and where it stops. A line is not a
promise about every possible future build. It is a rule the design applies to
every change, with the veto described in [Principles](principles.md).

Each line was checked against the design and the code. Where a line rests on a
status that is not yet true, the section says so.

## Never anything but bitcoin

No other coins. No stablecoins. No tokens of any kind on any layer, which
includes tokens carried on Lightning, Taproot Assets and RGB. No federated
sidechains such as Liquid. No ecash mints such as Cashu or Fedimint.

**Why.** Each of these puts someone else between the shop and its money. A
token has an issuer who can freeze it or rewrite its rules. A federated
sidechain has a federation that holds the coins. An ecash mint holds the
money and hands out claims on it. Under the veto, anything that lets a third
party hold, move or freeze the shop's funds is dropped. A shop that wants
money no company can touch does not want a claim on one.

**Rests on.**

- The veto in the design's section 0, which covers every item above.
- The wallet is an ordinary bitcoin wallet: native segwit, two of three
  (`SR-60`).
- The Key app refuses to sign any output that is a data output or a
  non-standard script, and any input that carries Taproot fields (`SR-22`,
  `SR-06`). A transaction that would carry a token is refused.
- The Lightning option is for bitcoin invoices from the shop's own node only
  (`LN-01`).

**Stops at.** The design does not name these protocols one by one. It rules
them out through the veto. The list in this section is how the veto applies
to them, and a new protocol would be judged against the same veto.

## Never custody

No OpenBMT key. No Avila Labs key. No hosted wallet. No balance held for
anyone.

**Why.** Custody is what makes a payment processor a processor. The shop's
protection from a processor failing, being sanctioned or deciding to freeze
the account is that there is no processor.

**Rests on.**

- The Box holds no wallet description, no address and no transaction ID, and
  no key that signs for the shop (`BX-04`).
- The counter holds no key that can move funds. A refund request is a record,
  not a payment (`CR-60`).
- The Owner app holds key 1 and no other secret key (`OA-08`). The Key app
  holds key 2.
- Avila Labs holds no key of any shop's wallet. It will hold release keys that
  sign the apps. That is a different thing, and it is covered below under
  updates.

**Stops at.** The shop's own devices hold the shop's keys, so losing them
matters. [How your money is kept safe](safety.md) covers loss and theft. A
publisher who signs the apps could in principle ship a bad build. The
separation of the Key app from the main app, and the second holder of the Key
app's release key, exist for that. The second holder has not been named yet.

## Never an account or an identity check

No sign-up. No KYC. Nothing tied to a person.

**Why.** An account is a handle: it can be closed, frozen, subpoenaed or
breached. A shop that takes bitcoin should not have to prove who it is to the
software that holds nothing of its.

**Rests on.** There is no OpenBMT server in the payment path and no OpenBMT
account anywhere in the apps. A device is identified by a key it makes itself,
not by a person. Staff names on the counter are labels the shop types, and no
check ties them to a real identity (`CR-156`).

**Stops at.** Accounts that other companies run still exist around the edges.
The POS vendor account is the shop's own account with that vendor. An exchange
used for cash-out has its own sign-up and checks. Those are the shop's
relationships. OpenBMT neither asks for them nor sees them.

## Never a third party deciding where money goes

Nobody outside the shop's own devices picks the address a customer pays, and
no public indexer (an Electrum or Esplora server) watches the shop's
addresses.

**Why.** A server that decides the address can swap it. A public indexer that
is given the shop's addresses learns every sale and can tie them together.

**Rests on.**

- Addresses come only in batches that two separate devices have signed. The
  counter accepts a batch only with both signatures, checked against two roots
  it pinned at pairing. It cannot make an address of its own (`CR-02`, design
  section 5.1).
- The Box cannot choose or change an address (`BX-04`).
- No message the counter sends to a Bitcoin peer holds an address or a script
  of the shop's (`NR-60`). Payments are found by asking peers for compact block
  filters and blocks, not by handing anyone the shop's addresses.

**Stops at.** A Bitcoin peer can still see that a device follows the chain and
which blocks it asks for, and a peer can stay silent about a payment. The
counter makes that harder by using several peers from different network
groups, and says so on screen when it could not check enough. It does not make
it impossible (`NR-61`, and section 8 of `docs/NETWORK-RULES.md`).

## Never telemetry, analytics or crash upload

**Why.** A payment terminal that reports home is a payment processor's
sensor. The shop's sales volume and timing belong to the shop.

**Rests on.**

- The Key app has no network permission at all. Its manifest lists the camera
  and nothing else, and a build check fails if that changes.
- The Counter app and the Owner app hold the network permission only for what
  the design lists: Bitcoin peers, the price sources, the POS read and the
  shop's own Box and node. The Owner app opens no connection of its own beyond
  the proxy greeting and the till read (`OA-15`).
- Every network contact is listed in [Privacy](privacy.md).

**Stops at.** This rests on the rules and on the manifest checks. It has not
been reviewed by anyone outside the project. A device that is compromised, or
an operating system that leaks, is outside what these rules cover. A shop that
wants crash information exports a local log and sends it by hand.

## Never automatic updates, and never a remote switch

**Why.** An update that installs itself is a way for the publisher, or someone
who takes over the publisher, to change what the shop's devices do while the
shop sleeps. A remote switch is the same power by another name.

**Rests on.**

- The apps contain no update check. They have no background service, receiver
  or provider that could run one (checked by the manifest check of each app).
- There is no OpenBMT service for a remote switch to come from. The commands a
  counter accepts are signed by the shop's own owner phone and are numbered so
  that an old one cannot be replayed (`CR-14`).
- The design requires that an update is never installed without the owner's
  action.

**Stops at.** The Box is a container. OpenBMT ships no updater in it. How the
host system that runs it (a mini PC, Umbrel or StartOS) treats updates is that
system's behavior, and the shop controls it there. The signed release
manifest, the install-date delay, the F-Droid and signed-file release (never
Google Play) and the reproducible builds the design describes for the Key app
are **Planned**, not built.

## Never a Lightning service provider holding the shop's channels

If a shop takes Lightning, it takes it on its own node, never through a
provider that holds the channels for it.

**Why.** A provider that holds the channels holds the money and sees every
payment. Taking Lightning through a swap provider was considered and dropped
under the veto for exactly that reason.

**Rests on.**

- Lightning is an option that connects the counter to a node the shop already
  runs (LND or Core Lightning). OpenBMT does not run, rent or choose a node
  (design section 8).
- The node's public key is part of the signed settings, and naming or changing
  it needs the Key app's co-signature. Every invoice must be signed by that key
  (`LN-03`, `LN-17`).
- The counter holds only a credential limited to invoices, and the setup
  checks it (`LN-04` to `LN-20`).

**Stops at.** The software cannot tell who runs the node whose key the
settings name. If a shop points OpenBMT at a node that a hosting company runs,
that company holds those channels. OpenBMT offers no such provider and says on
the setup screen that running Lightning means watching liquidity, updates and
backups. Lightning at the counter is **Built** and has never been run against
a real node.

## Never calling an unconfirmed payment final

And never "fully verified" unless the shop's own node said so.

**Why.** Any unconfirmed bitcoin payment can be replaced by its sender. A
screen that says "final" for one is a screen that lies.

**Rests on.**

- The counter's states are fixed (`CR-10`). A payment taken before a block is
  "Paid, could still be cancelled", a payment in a block at the shop's depth is
  "Confirmed", and "Final" is only the refund depth. The books count a sale as
  money only at "Final" (`CR-31`).
- Each screen carries the check that was done (`CR-105`). The own-node label
  applies only when the own node reported the payment and the counter's light
  client also saw it (`CR-104`).

**Stops at.** "Final" is depth-based. More blocks on top make a reversal
less likely and never impossible. The book never describes it as zero risk.

## Never closed source

OpenBMT is published under the MIT licence. The code, the rules and the design
documents are in the open.

**Why.** A shop that is asked to trust software with its money should be able
to read it, build it and check that a build matches. Closed software asks for
faith.

**Rests on.** The repository's `LICENSE`, and the dependency policy, which
allows only packages whose licences are on a published list
(`docs/DEPENDENCIES.md`, rule 6).

**Stops at.** The packages OpenBMT depends on carry their own licences. A phone
needs an operating system and, for Tor, a separate app, neither of which
OpenBMT publishes. Reproducible builds, which let anyone check that a published
app matches the source, are **Not built yet**.
