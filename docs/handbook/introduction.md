# Introduction

OpenBMT (Bitcoin Merchant Terminal) lets a shop take bitcoin at the counter
the way it takes a card. The customer pays from their own wallet. The money
goes straight into a wallet that the shop owns. The sale then closes in the
point-of-sale system (POS) the shop already uses.

> **Status: pre-release. Internal review only. No independent security audit
> has been done yet. Do not use OpenBMT with real funds.**

That line is not a formality. OpenBMT is about real money, and the book
says how far the protection goes and where it stops. Every claim in it says
what it rests on.

## What OpenBMT is

The wallet is the shop's. It needs two of three keys to spend, and the shop
holds all three. No company holds one, and no company can move or freeze the
money.

The counter screen shows the sale and a QR code, and says what it has checked:
"Seen by 3 peers", "Paid (light check)" or "Confirmed (your node)". These are
different things, and the screen keeps them apart.

A small separate app, the Key app, holds the second key. It checks every
spend, every address batch and every loosening of a safety limit on its own
screen.

The Box is an optional small server of the shop's own. It links the shop's
devices and reads the POS. It holds no key.

The wallet is a standard output descriptor, so it can be restored in other
wallet software without anything from this project.

## Who it is for

The book is written for shop owners and managers first. It uses plain words
and says "you" for the reader. A technical reader who wants to check the
claims will find the rule IDs, such as `SR-34` or `CR-15`, next to them.
These point into the rules documents in the repository's `docs/` folder. A
rule ID names a rule that a test in the code is named after.

## What OpenBMT is not

OpenBMT is not a payment processor: no company sits between the customer and
the shop's wallet. It is not a custodian or a hosted wallet, and nobody holds
a balance for the shop. It is not an exchange. Turning bitcoin into dollars is
the shop's own business with an exchange the shop picks. OpenBMT prepares the
transaction and the shop approves it.

It is not tax advice. The books export follows United States rules only and
writes down what the records say. It is also not finished, as the next
section shows.

## What is built today

The book uses four status words. They are never softened.

| Word | Meaning |
|---|---|
| **Built** | The code exists and its tests have run, on a desktop or against Bitcoin Core on a test network. |
| **Not run on a phone** | Built into an Android app that has not yet been run on a phone or an emulator. |
| **Planned** | Written in the design only. |
| **Not built yet** | Named so that a reader does not expect it. |

Where things stand:

| Part | Status |
|---|---|
| The rules every device shares: wallet, batches, settings, signed records, instant acceptance | Built |
| The Key app's signing rules (what it refuses to sign) | Built |
| The counter's judgment of a sale, and its Bitcoin peer client | Built, checked against Bitcoin Core on a test network |
| The owner's coin view, refund queue, spending and approval session | Built, checked against Bitcoin Core on a test network |
| Key app, Counter app and Owner app for Android | First build. Not run on a phone |
| The Box and its packaging (container, Umbrel, StartOS) | Built. The container image is built and smoke-tested; the Umbrel and StartOS packages have not been run on a device |
| Tap to pay by NFC | Not built yet |
| Square and Clover order matching (read-only) | Built. Written from vendor documentation, not from a real account |
| Lightning with the shop's own node | Built at the counter. Never run against a real node |
| Books and exports | Built as a library. The apps have no books screen yet |
| Recovery kit printing | Not built yet. The Key app checks a scanned kit against its own wallet |
| Push wake-ups, a full node on the Box, a hosted Box | Not built yet |
| Reproducible builds, a second holder of the Key app's release key | Not built yet |

The rest of the book says which of these a statement depends on. The
chapters on using OpenBMT day to day (setup, the Box, refunds, books,
cash-out) come in the next part of the book.

## How to read a safety claim

Every claim about safety here has two parts: **Rests on**, which names what
makes it true, and **Stops at**, which names where it ends. If a claim holds
only under a condition, the condition is written next to it. If something
could not be checked, the book says "not verified" and says what was checked
instead.
