# Equipment, daily work and fit

Receiving bitcoin directly changes the owner's job as well as checkout.
The decision depends on whether customers want to use it, the amounts you
sell, and whether you want to manage the money yourself.

> **Today OpenBMT is pre-release and unsuitable for real-funds use. The
> considerations below help evaluate the design; they do not make it ready
> for a live shop.**

## The equipment

[Getting started](getting-started.md) shows these parts in pictures and
puts the setup in order.

| Part | Purpose | Work it adds |
|---|---|---|
| Your wallet | A hardware signer or a computer kept offline, with its recovery words on steel. Every payment lands here, and only it can spend. | Keep the signer and the steel plate safe and apart from each other; bring the signer out to sign refunds and cash-outs. |
| Owner phone | Shows payments and records; prepares refunds, transfers and settings | Lock it with a PIN, review the records and keep it charged. |
| Counter phone or tablet | Shows sales and watches for payment; cannot spend | Keep it open, powered and connected; train staff to follow its payment result. |
| Optional Box | Lets the till send amounts to the counter and reads POS orders | Run and maintain the small computer yourself if you want its information to stay inside the shop. |

Until connecting a wallet you already hold is built, the apps use an
earlier arrangement: a two-of-three wallet whose second key sits in a
separate Key app on a spare phone, described in
[Who controls the money](safety.md). The design is moving from that to a
wallet the owner holds.

A full Bitcoin node is optional. It checks the Bitcoin rules on a computer
you run and can give the counter another source of payment evidence. The
counter keeps its light check as well. Its own-node connection currently
requires a Tor v3 address, a kind of network address reached through Tor.
The technical setup is in the [network reference](network-reference.md).

Lightning is optional and has two routes. The first is a Lightning node
the shop runs, LND or Core Lightning; you maintain its receiving capacity,
updates and backups. The second is a swap service the owner names in the
settings: the customer pays over Lightning, the service pays the same sum
on-chain to the sale's own address in your wallet, and the counter checks
that payment itself. The service charges a fee for each swap. It can
refuse or fail a swap, and the counter then says the Lightning payment is
unfinished.

## At the start of a day

The counter needs to stay open, connected and powered. The Owner app also
needs to be open and linked for instant on-chain acceptance in the current
apps. Its regular signed message tells the counter that the owner's
payment watcher is still running. If that check stops, instant acceptance
stops too.

Tor sends network connections through a separate routing system to hide
the device's network address from its Bitcoin peers. The Owner app always
uses Tor. The counter has a switch, but it must use Tor for instant
acceptance on a network customers can join. Both apps run Tor themselves,
or go through Orbot, a separate Android app, if the shop chooses. Plan for
Tor's start-up time rather than expecting a phone on guest Wi-Fi to work
unattended.

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
Lightspeed, Hike and every other till use the manual Bitcoin payment
type in the same way.

The counter quotes prices in the shop's currency, chosen at setup. The
accounting export follows United States rules only.

## Outside checkout

The owner reviews queued refunds, cash-outs, address batches and changes
that increase risk. These are taken to the separate Key app for review
and approval. You can group the work into approval sessions, but the
software does not yet remind you to do so.

You also choose whether to keep bitcoin or send it to an exchange. There
is no automatic bank payout. The owner app's Books page shows sales,
refunds and totals by day and saves the files for a bookkeeper. Detailed
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
