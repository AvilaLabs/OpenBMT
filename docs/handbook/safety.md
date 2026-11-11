<a id="how-your-money-is-kept-safe"></a>
<a id="the-wallet-and-its-keys"></a>

# Who controls the money

A wallet needs secret information to authorize spending. That information
is called a private key. Someone with the required keys can authorize a
payment; someone without them cannot ask a company to reset the wallet.

OpenBMT gives the shop three wallet keys and requires any two to spend.
This arrangement is called **two-of-three multisig**, meaning multiple
signatures. The three keys are not three pots of money. They are three
ways to authorize spending from the same wallet, with two needed together.

<a id="the-wallet-two-of-three"></a>

## What the second key does

Suppose the owner phone is stolen. It has one wallet key. That key alone
cannot spend the money. The owner can use the remaining two to move funds
to a new wallet.

Now suppose two keys are lost and no usable backups remain. Only one key
is left, so nothing can be spent. OpenBMT or Avila Labs cannot regenerate
the missing keys. Keeping the keys apart protects against one loss; keeping
usable recovery information protects against losing access permanently.

The separation also matters when approving a transaction. A compromised
owner phone could display one thing while preparing another. The second
device checks the transaction independently and shows it on its own
screen before signing.

<a id="the-three-keys"></a>
<a id="three-rules-about-the-keys"></a>
<a id="how-a-key-is-made"></a>

## Where the three keys are kept

| Key | Normal place | Purpose |
|---|---|---|
| Key 1 | Owner app on the owner's phone | The owner's signature on Bitcoin spends. |
| Key 2 | Key app on a separate phone | Independent review and the second signature. The owner or a manager can hold it. |
| Key 3 | Recovery words on steel, away from the shop | A separately generated key available when recovering from a loss. |

No device may hold two keys or make another key sign without that key's
own device reviewing the transaction. Keeping both signing devices in the
same place would also make a single theft more damaging.

Key 3 must be made outside OpenBMT software and away from key 2's device.
Only its public information is imported into OpenBMT. Recovery words are
secret words that can recreate a key; they are not the public payment
address. Keys 2 and 3 need their words recorded separately on steel.
Key 1 has no recovery words by default, a choice still marked for review in
the design. A locked phone without a known PIN or recovery words counts
as a lost key.

A hardware wallet, a dedicated signing device, or an offline computer is
an alternative for key 2 in the design. Hardware signing has not been
tried on real devices. The [wallet reference](wallet-reference.md)
contains the generation methods and exact validation checks; a tested
setup walkthrough has not been written yet.

## What is not protected yet

The apps are currently signed for release by one publisher. The independent
second person who must hold the Key app's release key has not been named.
Reproducible builds and an independent rebuilder, which would help check
released software against its source, are also not in place.

A malicious release could undermine the separation between the phones.
The design recommends a hardware or another publisher's signer for key 2
until release independence exists, but those alternatives remain
unverified. The claim that no single compromised component can spend the
wallet does not hold today. This is a launch condition, not a setting the
merchant can solve by choosing a stronger PIN.

> **OpenBMT has had no independent security audit. The apps have not been
> tested on a physical phone. Do not use it with real funds.**

<a id="roles-owner-manager-cashier"></a>

## What staff can do

| Person | Day-to-day authority |
|---|---|
| Cashier | Create and cancel sales, request refunds, see sales totals and accept some exceptions within assigned limits. Cannot spend from the wallet or see its balance. |
| Manager | Handle larger allowed exceptions and approve large refund requests at the counter by PIN. Cannot move the money alone or change money settings. |
| Owner | Review records and money, manage counters and settings, and prepare refunds and transfers. Needs the second signer for spending and changes that increase risk. |

An exception override accepts the sale on the shop's behalf and is logged.
It does not make the underlying payment more reliable. Only the owner can
clear a payment-cancel alarm. The counter has one manager PIN and a local
list of cashiers today; several independent manager accounts are not built.
The [reference](wallet-reference.md#roles-owner-manager-cashier) gives exact
permissions, PIN lengths and limits.

<a id="loosening-needs-the-key-app-tightening-does-not"></a>

## Increasing a risk limit needs another approval

Lowering the amount accepted before confirmation reduces risk. The owner
can do that immediately. Raising it increases risk and needs approval on
the Key app as well. OpenBMT calls these directions **tightening** and
**loosening**.

The same distinction applies to changes such as reducing the number of
confirmations, adding a counter or adding a Lightning node. The separate
approval shows each old and new setting. Some hard bounds cannot be
overridden by either signer. A proposed delay for hardware signers that
cannot sign settings is not built; without the spare Key app as approver,
the design discloses dependence on the owner phone alone.

<a id="the-approval-session"></a>

## Approving refunds and transfers

The owner phone collects work in a queue: refunds, cash-outs, more payment
addresses, sensitive settings and device pairings. Putting a request in
the queue does not move money.

When the second signer is available, the owner opens an approval session:

1. Transfer the queued requests from the owner phone to the Key app by
   animated QR code or file.
2. Read each request on the Key app. Approve or decline it there. An item
   left undecided is declined.
3. Transfer the signed reply back. The owner phone adds its signature and
   sends the approved result.

The Key app checks where the money goes, change returned to the wallet,
fees and its own spending limits. It cannot check the shop's sales record
or independently prove that a refund is owed. Dollar figures come from
the owner phone. Review the actual destination and amounts rather than
assuming a second signature makes a false refund claim true.

New cash-out destinations are enrolled by scanning on the Key app and
have a 24-hour wait before use. The app has per-transaction, daily and
weekly spending ceilings. The [approval
reference](wallet-reference.md#the-approval-session)
explains those ceilings and the clock and software limits behind them.

<a id="address-batches"></a>

## Why the owner approves payment addresses

The counter is not allowed to choose an arbitrary destination for sales.
It receives a batch of addresses approved by both signing devices, then
uses each once. If it runs out, it stops offering Bitcoin. This is why
preparing more addresses belongs in the owner's routine.

A stolen counter can reveal addresses in that batch, including future
ones, but it has no wallet signing key. The full wallet description,
which lets other software find all its addresses, stays off the counter.

<a id="the-instant-limit-and-the-exposure-total"></a>

The instant-sale limits protect a different thing: the value of goods
released before payment confirms. They cap that checkout exposure rather
than replace the two-key protection for spending the wallet.
[A sale at the counter](getting-paid.md) explains the difference.

<a id="when-a-phone-is-lost-or-stolen"></a>
<a id="the-recovery-kit"></a>

## Plan recovery before depending on the wallet

The owner and Key apps lock their keys behind a PIN or passphrase. The
tenth wrong attempt wipes the key from that phone. That is a reason to
have usable recovery arrangements, not a reason to keep two keys together.
A hostile or already compromised phone can defeat protections enforced
by its software.

After losing an owner phone, the designed response is to move money to
a new wallet and set up the counters again. The automatic "Move to a new
wallet" feature is not built. Restoring the Key app from key 2's words
starts a fresh app with its initial ceilings and no enrolled destinations.

The planned recovery kit is a printed page of public wallet information,
key locations and recovery steps, not a page of secret words. Other wallet
software can use its wallet description to locate the funds. Two keys are
still needed to spend. The Key app can check a scanned description, but
kit printing is not built and a Sparrow restore has not been tested.

[Backups and routine checks](best-practices.md) describes storage, access
for someone acting for the owner, and independent recovery checks. The
[wallet reference](wallet-reference.md#the-recovery-kit) preserves the
exact kit fields, scan behavior and test results.
