# Equipment, daily work and fit

Receiving bitcoin directly changes the owner's job as well as checkout.
The decision depends on whether customers want to use it, the amounts you
sell, and whether you want to manage the money yourself.

> **Today OpenBMT is pre-release and unsuitable for real-funds use. The
> considerations below help evaluate the design; they do not make it ready
> for a live shop.**

## The equipment

| Part | Purpose | Work it adds |
|---|---|---|
| Counter phone or tablet | Shows sales and watches for payment | Keep it open, powered and connected; train staff to follow its payment result. |
| Owner phone | Tracks money and records; prepares refunds, transfers and settings | Review the records and queue, protect the phone and arrange approvals. |
| Separate Key phone | Reviews and provides the second signature for spending and sensitive changes | Keep it apart from the owner phone and bring it out when approving work. |
| Key 3 and its recovery words | Helps recover the wallet after losing another key | Generate it outside OpenBMT; record its secret recovery words on steel and keep them away from the shop and the other keys. |
| Optional Box | Connects shop devices and reads POS orders | Run and maintain the small server yourself if you want its information to stay inside the shop. |

The separate Key app on a spare phone is the default second signer.
Hardware wallets and an offline computer are alternatives in the design,
but hardware signing has not been tried on real devices. The current
release-signing weakness makes a hardware or different publisher's signer
preferable in the design; those alternatives are still unverified.
[Who controls the money](safety.md) explains that limitation.

A full Bitcoin node is optional. It checks the Bitcoin rules on a computer
you run and can give the counter another source of payment evidence. The
counter keeps its light check as well. Its own-node connection currently
requires a Tor v3 address, a kind of network address reached through Tor.
The technical setup is in the [network reference](network-reference.md).

A Lightning node is a separate, optional responsibility. OpenBMT connects
only to a shop-run LND or Core Lightning node, two implementations of
Lightning software. You maintain the receiving capacity, updates and
backups; OpenBMT does not do that for you. If you do not want to operate
one, evaluate the on-chain route and its fees first.

## At the start of a day

The counter needs to stay open, connected and powered. The Owner app also
needs to be open and linked for instant on-chain acceptance in the current
apps. Its regular signed message tells the counter that the owner's
payment watcher is still running. If that check stops, instant acceptance
stops too.

Tor sends network connections through a separate routing system to hide
the device's network address from its Bitcoin peers. On Android the apps
need a separate app called Orbot to use it. The Owner app always requires
Tor. The counter has a switch, but it must use Tor for instant acceptance
on a network customers can join. Plan for that dependency rather than
expecting a phone on guest Wi-Fi to work unattended.

The counter also needs a supply of addresses approved by both wallet
signers. If it uses them all, it stops offering Bitcoin instead of reusing
an address. Approving more addresses belongs in the owner's routine.

## During the day

Cashiers create sales, read the payment result and close the POS order.
A manager can handle some exceptions within the shop's limits. Refunds
become requests for the owner rather than immediate cashier payments.
An owner must be available to deal with a payment-replacement alarm.

Square and Clover support read-only order matching. The cashier still
enters the Bitcoin sale and closes the POS order manually. The readers
have not been tried with a real seller account, so a shop cannot assume
an existing POS will provide a finished automatic connector.

The counter currently quotes prices in United States dollars only. The
accounting export also follows United States rules only. A shop needing
another pricing currency cannot assume that route is supported.

## Outside checkout

The owner reviews queued refunds, cash-outs, address batches and changes
that increase risk. These are taken to the separate Key app for review
and approval. You can group the work into approval sessions, but the
software does not yet remind you to do so.

You also choose whether to keep bitcoin or send it to an exchange. There
is no automatic bank payout. The accounting library can produce records
and exports, but there is no books screen in the apps yet. Detailed
end-of-day instructions are still being written.

Once a month, the design calls for an independent comparison: open a copy
of the wallet's public description in other wallet software and compare
its receipts with the POS Bitcoin total. This helps detect a counter or
app falsely claiming payment. It does not require giving that other
software your spending keys. [Backups and routine checks](best-practices.md)
explains the task.

## What would make it a plausible fit after release

A shop with customers who already hold bitcoin has a practical reason to
consider the extra payment method. The case is stronger when the owner
wants direct control of the receipts and can keep separate signing devices,
maintain backups and review records.

Sale size matters. A shop selling occasional higher-value items may be
able to wait for confirmations. A busy shop selling low-priced goods may
find on-chain fees or waiting impractical; its own Lightning node is an
option only if it is willing to operate one. These are workflow choices,
not a prediction that either route will be cheaper on a particular day.

## What would make it a poor fit

OpenBMT is a poor fit if you need a finished system now, automatic
conversion and bank payouts, a single device with no separate approvals,
or a provider that can restore wallet access for you. It also adds little
checkout value if your customers do not want to pay with bitcoin.

If the owner cannot keep up with keys, approvals and reconciliation, the
absence of a payment company does not remove that work. It leaves the
work with the shop. The [status page](status.md) lists what must still be
finished and tested before a live-use decision is possible.
