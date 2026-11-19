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
| Your wallet | A hardware wallet, or a wallet you already keep offline, with its recovery words on steel. Every payment lands here, and only it can spend. | Keep the signer and the steel plate safe and apart from each other; bring the signer out to sign refunds and cash-outs. |
| Owner phone | Shows payments and records; prepares refunds, transfers and settings | Lock it with a PIN, review the records and keep it charged. |
| Counter phone or tablet | Rings up sales, shows the payment code and watches for payment; cannot spend your savings | Keep it open, powered and connected; train staff to follow its payment result. |
| Optional Box | Passes messages between the shop's devices; today it also keeps Shop Lightning's second copy | Run and maintain the small computer yourself. |

A full Bitcoin node is optional. It checks the Bitcoin rules on a computer
you run and can give the counter another source of payment evidence. The
counter keeps its light check as well. Its own-node connection currently
requires a Tor v3 address, a kind of network address reached through Tor.
The technical setup is in the [network reference](network-reference.md).

Lightning is optional. The shop's own Lightning node runs on the counter;
a channel provider opens the channel and charges for it as its terms say,
but never holds the shop's money. Each change to a channel is kept on a
second shop device as well; today that is the Box.

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

The counter makes its addresses from ranges of 1,000 that the owner phone
gives it. If a range runs out, it stops offering Bitcoin instead of
reusing an address, until the owner phone gives the next one.

## During the day

Cashiers ring up sales, read the payment result and hand over the goods.
A manager can handle some exceptions within the shop's limits. Refunds
become requests for the owner rather than immediate cashier payments.
An owner must be available to deal with a payment-replacement alarm.

Today the cashier enters the sale's amount; ringing up items, tax and
discounts, and taking cash and cards on the counter, are the next work.

The counter quotes prices in the shop's currency, chosen at setup. The
accounting export follows United States rules only.

## Outside checkout

The owner reviews queued refunds and cash-outs on the owner phone and
signs them with the shop's wallet. A change of settings that increases
risk does not take effect until someone confirms it on a counter.

You also choose whether to keep bitcoin or send it to an exchange. There
is no automatic bank payout. The owner app's Books page shows sales,
refunds and totals by day and saves the files for a bookkeeper. Detailed
end-of-day instructions are still being written.

Once a month, the design calls for an independent comparison: open a copy
of the wallet's public description in other wallet software and compare
its receipts with the Bitcoin total in the owner app's Books. This helps detect a counter or
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
