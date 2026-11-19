<a id="setting-up-a-shop"></a>

# Setting up a shop

This page follows the owner app from the first time it opens. Each part
says whether it is built. The design behind it is in design section 2.1.

## Try it first

The app opens on a welcome screen with two choices: try it first, or set
up the shop.

Trying it first opens a practice shop on the phone. It runs one sale from
start to finish: the amount, the code a customer scans, Paid, the books
and a cash refund. Every screen is marked Practice. The code in the
practice shop says it is not a payment, so no wallet can pay it. Nothing
is saved, and one tap deletes it. It needs no wallet, no second device
and no internet.

Built in this version.

## The shop

The app asks whether this is a shop or a place that takes gifts, then the
shop's name and its money, guessed from the phone's settings.

OpenBMT is where the shop rings up its sales and takes payment, on the
counter. It does not sit beside another register. Ringing up items, tax,
discounts, cash and cards on the counter are the next work; today the
counter takes an amount and a bitcoin payment.

Built in this version.

## Where the money goes

OpenBMT never holds the shop's money. The app offers three ways forward.

**Sell today, then move to a hardware wallet.** The owner phone makes a
small starter wallet now, with a limit the owner sets, and the shop moves
to a hardware wallet when it arrives.
[Start simple](#start-simple-a-starter-wallet-on-the-owner-phone) below
explains it.

**I already have a wallet.** For anyone who already holds their own keys:
a hardware wallet, or a wallet such as Sparrow or Electrum on an offline
computer. OpenBMT takes only the wallet's public description, which lets
it see payments and make addresses but cannot spend. The first receive
address is compared on the wallet's own screen before the shop takes a
payment. A description that holds a private key or recovery words is
refused, and the app says that key is now exposed. Under this choice,
"Other ways to make one" explains a signer built from parts and free
software.

**Set up the rest now, add the wallet later.** Everything else is set up,
and the counter says "Not taking bitcoin yet" until a wallet is added.

## Connecting the wallet you have

Every wallet can be described by a line of public text, its output
descriptor. It names the wallet's public keys and how addresses are made
from them. With it, OpenBMT can make a new address for every sale and see
what has been paid. It cannot spend: spending needs the private keys,
which stay in the wallet.

The owner app asks for that description on "Connect your wallet", by
scanning a code from the wallet's screen or by pasting the text.

| Wallet | Where the description is | Read by OpenBMT |
|---|---|---|
| Sparrow | The wallet's Settings tab, "Show Descriptor QR"; or File, Export Wallet, Output Descriptor | Pasted text today. Its animated code once the new scan reader is in the app. |
| Coldcard | Export wallet, Descriptor, as a file, NFC or a code | Pasted text and its plain code today. |
| Bitcoin Keeper | The wallet's settings, Wallet Configuration | Pasted text and its plain code today. Its animated code once the new scan reader is in the app. |
| Jade, Keystone, Passport | The device's export of its account key as an animated code | Once the new scan reader is in the app. |
| BlueWallet, Blockstream Green, Bitcoin Core | BlueWallet's "Show XPUB"; Green's two descriptors; Core's `listdescriptors` | Not yet: each writes the description in a form OpenBMT does not read yet. |
| Electrum | Electrum shows no output descriptor | Not yet. |

A wallet that shows only an xpub or a zpub, with no description around
it, cannot be used: an xpub alone does not say which master key it came
from, which hardware signers need in order to sign the shop's refunds.

### The first address

Before anything else, the app shows the wallet's first receive address in
large type and asks the owner to find the same address in the wallet: in
its list of addresses, the first receive address. They must be the same,
character for character. "Same address" goes on; "Different" stops, and
nothing is kept.

The check matters because it is the one moment the owner sees, with their
own eyes, that the description the phone holds is the wallet whose keys
they hold. A description from the wrong wallet, or one changed on its way
to the phone, would send every sale somewhere else.

### How far the wallet looks ahead

A wallet does not watch every address its keys could make. It watches
the addresses it has used, plus a number of addresses after the last
used one, its gap limit, usually 20. OpenBMT hands each counter a block
of 1,000 addresses, and a sale that is never paid leaves its address
unused. After enough unpaid sales in a row, a payment can land beyond
what the wallet watches: the money is there, but the wallet does not
show it until it looks further.

The owner app keeps track of this. On the wallet's page it shows how far
ahead the wallet must look to see every payment, and asks the owner to
raise the wallet's setting when the usual 20 is not enough. Sparrow
allows up to 1,000,000 (Settings, Advanced, Gap limit); Bitcoin Core
looks 1,000 ahead by default; Blockstream Green allows up to 1,000;
BlueWallet and Bitcoin Keeper are fixed at 20.

### What is built

Reading and checking a wallet's description, the first-address check and
adding a counter to the shop's own wallet are built in the owner app and
the counter, and tested against Bitcoin Core on a test network. The
library prepares a refund from a wallet with one key in the form Bitcoin
Core signs, checked against Bitcoin Core in a test; hardware signers have
not been tried yet. None of it has been run on a phone.

## Start simple: a starter wallet on the owner phone

A shop that wants its first bitcoin sale today, before its hardware wallet
arrives, can let the owner phone make a small wallet: the starter wallet.
It is for the first weeks, not for savings.

**The key is on an everyday phone.** Malware that controls the phone at the
moment the owner unlocks a payment can take what the wallet holds, and
nothing on a phone prevents that. The starter wallet is built around that
fact: it has a limit, and every rule below makes the key harder to take,
harder to use without the owner, or worth less.

**The limit and the date.** The owner sets the most the starter wallet may
hold and the date to move to the shop's own wallet. The app suggests a date
30 days away and never suggests an amount. The owner phone's home screen
shows what the wallet holds against both, every day. At the limit or the
date, the app opens on the upgrade screen every time; "Not yet" needs a
sentence typed exactly and lasts at most seven days. Sales never stop: the
counters keep taking payments, unless the owner chose at setup to stop
bitcoin sales above the limit, which is off unless chosen.

**How the key is made and kept.**

- The randomness comes straight from the phone's Linux kernel and is
  checked before use. The owner may add die rolls, mixed in so that
  either source alone, honest, makes the key unguessable.
- The 12 words are shown once, four at a time, to write on a card in pen,
  and three of them are asked back. The app never shows them again. Those
  screens block screenshots and screen recording and hide other apps
  drawn on top of them. No app or person will ever ask for the words; any
  that does is a thief.
- The key is sealed under a key that the phone's key hardware holds and
  opens only when the owner proves they are there, by fingerprint, face or
  the phone's PIN. It is never in the phone's cloud backup. Removing the
  phone's screen lock makes Android delete that key; the card of words then
  restores the wallet.
- The key only pays out: refunds, and the final move to the new wallet.
  Receiving never uses it. Every payment shows each address in full and
  each amount before the owner unlocks it. Before making the key and before
  each payment, the app looks for signs that another app could watch the
  screen or act for the owner, names each one it finds, and will not make
  the key or show the words while an unknown accessibility service is on.

**Moving to the shop's own wallet.** Once the shop's hardware wallet is
set up, each counter is switched to the new wallet, once. The counter shows the new wallet's first address, and the
manager compares it with the new wallet's own screen, never with the owner
phone, then confirms with the manager PIN. Once every counter has switched,
the owner phone moves everything in the starter wallet to the new wallet
in one payment. When that payment has one confirmation, the phone deletes
the starter key.

Keep the card of starter words after that. A payment that reaches the
starter wallet later can only be opened with it. The owner phone keeps
watching the starter wallet's addresses, shows such a payment on its home
screen, and offers to restore the key from the card and move the payment.

The new wallet's first sale is not at its address number 0: each counter's
addresses go on past the ones it gave out for the starter wallet. A wallet
program that opens the new wallet from its words needs to look further
ahead than usual, as [How far the wallet looks ahead](#how-far-the-wallet-looks-ahead)
explains.

### What is built

The starter wallet, its limit and date, the switch of each counter and the
move to the new wallet are built in the owner phone, the counter and their
apps, and tested against Bitcoin Core on a test network. The setup screens
offer it as "Sell today, then move to a hardware wallet". None of it has
been run on a phone. Its rules are in `docs/START-SIMPLE-RULES.md`.

> **OpenBMT has had no independent security audit. Do not use it with real
> funds.**
