<a id="limits-and-risks"></a>

# Risks and current limits

There are several different ways a shop can lose money. A wallet that
requires two keys addresses one of them. It does not protect the value of
goods released for a bad payment, eliminate price changes or guarantee
that every app behaves correctly.

> **OpenBMT has had internal review only, with no independent security audit.
> The Android apps have not been tested on a physical phone. Do not use it
> with real funds.**

<a id="what-the-protection-claims-and-when-it-does-not-hold"></a>

## The release separation is unfinished

The design aims to prevent one compromised component, device or person
from spending the wallet. It depends on key 3 being generated outside
OpenBMT and away from the key 2 device, and on an independent second
person holding the Key app's release key.

The second release-key holder has not been named. Reproducible builds and
an independent rebuilder are also missing. A bad release could undermine
the independence of the signing devices. The single-component protection
claim does not hold today. A stronger PIN does not fix that gap. The wallet
rule counts keys rather
than people: a person with two usable keys can spend.

<a id="the-threat-model-in-plain-words"></a>

## A small unconfirmed sale can fail

If an $8 payment is accepted before a block and is replaced, the shop may
already have handed over $8 of goods. Instant acceptance limits the amount
at risk rather than making that payment irreversible.

The exposure budget limits the total currently accepted but waiting for
first confirmations. It is not a cap on every kind of loss or a guarantee
about a whole trading day. Confirmed payments can still be affected by a
chain reversal. Staff overrides also accept risk and are counted.

When a cancellation is detected, the alarm and pause reduce further
exposure. They cannot recover goods already handed over. The
[payment reference](payment-reference.md) gives the exact pause rules,
limits and conditions.

<a id="what-the-light-check-cannot-see"></a>

## A payment report can have incomplete evidence

The counter's light check asks other computers on the network for payment
information. It cannot check every detail that a full node checks, and it
cannot inspect all the input amounts or fee of an unconfirmed payment.
If all the computers it can reach belong to one attacker, that attacker
can shape what it sees.

Using different peers, Tor and the shop's own node makes interference
harder without making it impossible. This is why the counter names the
source of a report and why a block confirmation matters. The
[technical explanation](network-reference.md#what-the-light-check-cannot-see)
sets out the exact checks it omits.

## A bad counter can send sales to the wrong place

A counter can be compromised without stealing a key from the shop's
wallet. It could show an attacker's payment address and falsely report
that the shop was paid. That would steal new sales proceeds rather than
spend the existing wallet balance.

Matching POS orders to wallet receipts helps find discrepancies. The
design also requires the monthly outside check below, so the shop does
not rely entirely on OpenBMT to judge its own records.

<a id="what-the-pin-does-not-stop"></a>

## A locked phone and a hostile phone are different problems

The app lock slows guessing and can wipe a key after wrong attempts.
It does not protect a phone someone can already use unlocked. A rooted
phone, hostile keyboard or accessibility service may read typed or
displayed information. The Key app refuses some of these conditions, but
a malicious build could omit those refusals.

Someone with a locked file and the phone's Keystore material may also
attempt offline guessing or restore an older stored file. The detailed
limits are in the [wallet
reference](wallet-reference.md#when-a-phone-is-lost-or-stolen).
Keep recovery arrangements separate; a wipe removes a phone's key rather
than restoring the money automatically.

<a id="phone-and-hardware-risks"></a>

## Devices and supporting software must keep working

Today's Owner and Counter apps run only while open. Instant acceptance
needs them linked, and Tor needs Orbot running. A closed app or broken
connection can stop the intended payment flow. The counter must stay
open and powered. Some phones provide only software protection for stored
keys; the apps report that condition.

The shop also maintains any Box or Lightning node it chooses. These are
owner responsibilities rather than a service operated by OpenBMT.

<a id="the-monthly-outside-check"></a>

## Check the receipts independently

Once a month, the design calls for comparing the wallet's incoming
payments with the POS Bitcoin total using different wallet software.
It uses the wallet's public description in **watch-only** mode: it can
locate and display payments but has no spending keys.

This is the only check independent of OpenBMT's software, and the only
one that catches a counter or main-app build substituting its own address
while claiming payment. The estimate is five minutes, which has not been
measured. In-app reminders and instructions are planned.

[Backups and routine checks](best-practices.md) explains what must be kept
for this check and for recovery. A missing alert or clean-looking screen
is not a substitute for reconciliation.

<a id="unverified-items"></a>
<a id="the-audit-status"></a>

## What the tests do not establish

Test-network payment and restore checks show specific parts working with
test bitcoin. They do not establish readiness for real funds. Real-phone
use, hardware signers, real POS seller accounts, a real Lightning node
and Sparrow recovery remain unverified in the ways recorded on the
[status page](status.md) and in the
[detailed inventory](network-reference.md#unverified-items).

The project has four internal design reviews. Device testing, outside
code review and an independent audit are still required. The detailed
threat model also records losses, mitigations and the limits of each.
