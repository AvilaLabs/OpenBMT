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

## The shop and its till

The app asks for the shop's name and its money, then how the shop takes
payments today. The list has seven tills with written steps: Square,
Toast, Shopify POS, Loyverse, Odoo, Hike and StoreHub. It also offers "No
till" and "Another till".

Bitcoin joins the till as one more way to pay. In most tills that means
adding a payment type called Bitcoin, which takes about two minutes. At
the till, the cashier rings up the sale as usual, takes the bitcoin on the
counter, then picks Bitcoin as the payment in the till. The till company
does not move the money, so it charges nothing for that payment type.

The steps for each till were read on its own help pages in November 2026.
Menus change; if a till looks different, look for "payment types".

Built in this version. The steps appear after the first counter is added,
and again under Settings.

## Where the money goes

OpenBMT never holds the shop's money and never makes its keys. The app
offers three ways forward.

**Use my own wallet.** For anyone who already holds their own keys in a
wallet such as Sparrow or Electrum on an offline computer, a hardware
signer or a do-it-yourself signer. OpenBMT takes only the wallet's public
description, which lets it see payments and make addresses but cannot
spend. The first receive address is compared on the wallet's own screen
before the shop takes a payment. A description that holds a private key or
recovery words is refused, and the app says that key is now exposed.

**Help me make a cold wallet.** The owner makes a wallet from their own
coin flips or die rolls, on the computer they already have, with networking
off. The next section explains how.

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

Reading and checking a pasted description, and the first-address check,
are built in the owner app. The counter's side of pairing with the shop's
own wallet is built: it checks the first address, takes the owner phone's
code and makes its own addresses from the wallet. The library prepares a
refund from a wallet with one key in the form Bitcoin Core signs, checked
against Bitcoin Core in a test; hardware signers have not been tried yet.
The owner phone's side of adding a counter this way, the new scan reader
and the other wallets' forms are being built. Until they are, all
three choices continue into the older setup with two phones, described in
[Who controls the money](safety.md).

## Making a cold wallet with a coin or a die

Nothing is bought for this. It needs a computer, two USB sticks, a coin or
one six-sided die, a pen, a card and about an hour.

1. Put Tails on the larger stick, from tails.net, following its own
   guide. Tails starts the computer from the stick, never touches the
   computer's own disk, and forgets everything at shutdown.
2. Copy the OpenBMT key maker page onto the small stick.
3. Start the computer from Tails with Offline Mode on. Offline Mode turns
   the network off before anything else runs.
4. Open the key maker page in Tails' browser. It may ask for one reload.
5. Rehearse once with made-up results. Then choose the coin or the die and
   press what it shows each time: about 512 coin flips, or about 270 die
   rolls. Use the same coin or the same die from start to finish.
6. Write the 12 words on the card by hand, in order. Never photograph,
   type or print them.
7. The page checks three of the words against the card.
8. Scan the page's code with the owner app. The page and the phone must
   show the same fingerprint and first address.
9. Restore the same words in Electrum, which is already in Tails. Electrum
   must show the same fingerprint and first address. Electrum is made by
   other people, so a fault in the key maker page shows up here.
10. Clear the page and shut down Tails. Put the words on steel as soon as
    possible.

Why a coin or a die: wallets have made weak random numbers before, and a
coin or a die has no chip and no update to get wrong. Why offline: any app
can be hacked, including this one, so the words never touch a connected
device.

### Why so many flips and rolls

A real coin or die can favour one side slightly. The page removes that
bias with the standard methods, so 128 fair bits come out even from an
uneven coin or die:

- **Coin.** Flips are read in pairs. Heads then tails gives 0, tails then
  heads gives 1, and two the same are dropped. A pair is as likely to be
  heads-tails as tails-heads, whatever the coin's bias. About half the
  pairs are dropped, so 128 bits take about 512 flips.
- **Die.** Rolls are read in threes. Three different numbers give one
  value, 1 to 6, from the order of low, middle and high in which they came;
  three with a number repeated are dropped. Each of the six orders is as
  likely as the others, whatever the die's bias. About 45 in 100 threes are
  dropped, so 50 values take about 270 rolls. The 128 bits are the first
  128 bits of the SHA-256 of the 50 values written as one line of digits.

The orders for the die:

| Order of the three rolls | Value |
|---|---|
| low, middle, high | 1 |
| low, high, middle | 2 |
| middle, high, low | 3 |
| high, middle, low | 4 |
| high, low, middle | 5 |
| middle, low, high | 6 |

### Checking the page by hand

The page's "Check it by hand" screen shows the bits, how they make each
word, and a command to type in the Tails Console (open Applications, then
System Tools, then Console). For the die, it is

```
echo -n "<the 50 digits>" | sha256sum
```

and the first 32 characters of the answer must match the page. For the
coin, the command checks the 4 check bits of the last word:

```
printf '%s' '<the 128 bits>' | basenc --base2msbf --decode | sha256sum
```

Its first character, written in binary, is the check bits. The groups of
11 bits can be looked up by hand in the BIP39 word list, counting from 0.
These checks do not depend on the key maker page, so they catch a page that
makes the wrong words from the bits or values it shows.

The page is one file with no connections allowed. It uses well-known
audited libraries for the cryptography and a build anyone can repeat to
get the same file. Its rules and tests are in `docs/KEYMAKER-RULES.md`.

Built and tested in a Tails 7.17 virtual machine with networking off: the
words, fingerprint, first address and the code on screen matched Electrum
and the project's own checks. The coin and die methods give the same words
as worked examples published by others. Not yet released, and not yet tried
by a person on a real computer. The phone's side of these steps is built
and has been tried on an emulator, but not yet with a real scan.

<a id="start-simple"></a>

## Start simple: a starter wallet on the owner phone

A shop that wants its first bitcoin sale today, with no signer and no
computer to make a key on, can let the owner phone make a small wallet: the
starter wallet. It is for the first weeks, not for savings.

**The key is on an everyday phone.** Malware that controls the phone at the
moment the owner unlocks a payment can take what the wallet holds, and
nothing on a phone prevents that. The starter wallet is built around that
fact: it has a limit, and every rule below makes the key harder to take,
harder to use without the owner, or worth less.

**The limit and the date.** The owner sets the most the starter wallet may
hold and the date to move to a key made offline. The app suggests a date
30 days away and never suggests an amount. The owner phone's home screen
shows what the wallet holds against both, every day. At the limit or the
date, the app opens on the upgrade screen every time; "Not yet" needs a
sentence typed exactly and lasts at most seven days. Sales never stop: the
counters keep taking payments, unless the owner chose at setup to stop
bitcoin sales above the limit, which is off unless chosen.

**How the key is made and kept.**

- The randomness comes straight from the phone's Linux kernel and is
  checked before use. The owner may add die rolls, mixed the same way as
  on the stick; either source alone, honest, makes the key unguessable.
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

**Moving to a key made offline.** Once the owner has made the shop's key
offline, on the stick or a signer, each counter is switched to the new
wallet, once. The counter shows the new wallet's first address, and the
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
do not offer the starter wallet yet, and none of it has been run on a
phone. Its rules are in `docs/START-SIMPLE-RULES.md`.

> **OpenBMT has had no independent security audit. Do not use it with real
> funds.**
