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

**Help me make a savings wallet.** The owner makes a wallet from their own
coin flips or die rolls, on the computer they already have, with networking
off. The next section explains how.

**Set up the rest now, add the wallet later.** Everything else is set up,
and the counter says "Not taking bitcoin yet" until a wallet is added.

Not built yet. Reading a public description and its checks are built; the
app does not keep the wallet yet, and all three choices still continue
into the older setup with two phones, described in [Who controls the
money](safety.md).

## Making a savings wallet with a coin or a die

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

> **OpenBMT has had no independent security audit. Do not use it with real
> funds.**
