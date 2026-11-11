# Introduction

OpenBMT (Bitcoin Merchant Terminal) lets a shop take bitcoin at the counter
the way it takes a card. The customer pays from their own wallet. The money
goes straight into a wallet that the shop owns. The sale then closes in the
point-of-sale system (POS) the shop already uses.

> **Status: pre-release. Internal review only. No independent security audit
> has been done yet. Do not use OpenBMT with real funds.**

## What OpenBMT is

The wallet is the shop's. It needs two of three keys to spend, and the shop
holds all three. No company holds one, and no company can move or freeze the
money.

The counter screen shows the sale and a QR code, and a line that says what
checked the payment: "Light check", "Your Box reports it", "Your Box and light
check" or "Your node and light check". Each names a different source, and the
screen never merges them.

A small separate app, the Key app, holds the second key. It checks every
spend, every address batch and every loosening of a safety limit on its own
screen.

The Box is an optional small server of the shop's own. It links the shop's
devices and reads the POS. It holds no key.

The wallet is a standard output descriptor, so it can be restored in other
wallet software without anything from this project.

## Who it is for

This handbook is for shop owners and managers. Technical readers can check
claims against rule IDs
such as `SR-34` and `CR-15`, which point to rules in the repository's `docs/`
folder. Tests in the code use those rule IDs as names.

## What OpenBMT is not

No company sits between the customer and the shop's wallet, and nobody holds
a balance for the shop. OpenBMT is not an exchange. The shop chooses and
works with its own exchange to turn bitcoin into dollars. OpenBMT prepares
the transaction; the shop approves it.

The accounting export follows United States rules only and reports what is
in the records. It does not provide tax advice.

## What is built today

The handbook uses four status words with fixed meanings.

| Word | Meaning |
|---|---|
| **Built** | The code exists and its tests have run, on a desktop or against Bitcoin Core on a test network. |
| **Not run on a phone** | Built into an Android app that has been started on an Android emulator but not yet tried on a phone. |
| **Planned** | Written in the design only. |
| **Not built yet** | Named so that a reader does not expect it. |

Where things stand:

| Part | Status |
|---|---|
| The rules every device shares: wallet, batches, settings, signed records, instant acceptance | Built |
| The Key app's signing rules (what it refuses to sign) | Built |
| The counter's judgment of a sale, and its Bitcoin peer client | Built, checked against Bitcoin Core on a test network |
| The owner's coin view, refund queue, spending and approval session | Built, checked against Bitcoin Core on a test network |
| Key app, Counter app and Owner app for Android | First build. Started on an emulator; not run on a phone |
| The Box and its packaging (container, Umbrel, StartOS) | Built. The container image is built and smoke-tested; the Umbrel and StartOS packages have not been run on a device |
| Tap to pay by NFC | Not built yet |
| Square and Clover order matching (read-only) | Built. Written from vendor documentation, not from a real account |
| Lightning with the shop's own node | Built at the counter. Never run against a real node |
| Books and exports | Built as a library. The apps have no books screen yet |
| Recovery kit printing | Not built yet. The Key app checks a scanned kit against its own wallet |
| Push wake-ups, a full node on the Box, a hosted Box | Not built yet |
| Reproducible builds, a second holder of the Key app's release key | Not built yet |

The chapters on setup, the Box, refunds, books and cash-out are still to
come. Their entries appear under "Using OpenBMT" in the contents.

## How to read a safety claim

Each safety claim names the checks and conditions it depends on, and
explains what can still go wrong. "Not verified" means
a claim has not been checked; the text says what was checked instead.
