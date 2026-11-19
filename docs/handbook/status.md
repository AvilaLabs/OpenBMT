# What is built and tested

> **OpenBMT is pre-release. Internal review only. No independent security
> audit has been completed. Do not use it with real funds.**

The handbook describes a design and the code built so far. A successful
software test does not establish that a shop can rely on the whole system.
The Android apps have started on an emulator but have not been tried on a
physical phone.

## How the status terms are used

| Term | Meaning |
|---|---|
| Built | Code exists and has passed tests on a desktop or against Bitcoin Core on a test network. |
| Not run on a phone | An Android build has started on an emulator, but has not been tried on a physical phone. |
| Planned | A feature is described in the design. |
| Not built yet | The named feature is absent. |
| Not verified | The stated claim or behavior has not been checked; the text identifies the evidence that is available. |

A test network uses test bitcoin rather than money for live purchases.
Bitcoin Core is a full-node implementation used in the project's
engineering tests. The current results are not an independent audit.

## Current components

| Part | Status |
|---|---|
| Shared wallet, address batches, settings, signed records and instant acceptance | Built |
| Key app signing rules (the earlier setup with three keys, two needed to spend; no longer the main path) | Built |
| Counter sale handling and Bitcoin peer client | Built; tested against Bitcoin Core on a test network |
| Owner coin view, refund queue, spending and approval session | Built; tested against Bitcoin Core on a test network |
| Refund float (a small, capped wallet on the owner phone for paying refunds at once) | Built in the owner library; tested against Bitcoin Core on a test network; the owner app sets it up and keeps its cap, and paying from it and turning it off in the app are still being joined; not run on a phone |
| Start simple (a capped starter wallet the owner phone makes, and the later move to a key made offline) | Built, with the owner app's screens and each counter's switch to the new wallet; tested against Bitcoin Core on a test network; not run on a phone |
| The stick (the shop's key made, and a payment from savings signed, on an ordinary PC started from a USB stick) | Built; making the key and signing run in a virtual machine; signing through the PC's camera not yet tried on a real PC |
| Payment requests (an amount asked of a customer by link or code, and money returned) | Built; a counter makes a request as a link or a code, and the owner phone matches its payments and keeps it in the books; tested against Bitcoin Core on a test network; not run on a phone |
| Ringing up the whole sale: items, tax, discounts, tips, cash, cards, receipts, the drawer, staff, stock and reports ([the terminal rules](https://github.com/AvilaLabs/OpenBMT/blob/main/docs/TERMINAL-RULES.md)) | Not built yet; this is the next work |
| Counter, Owner and Key Android apps | First builds; started on an emulator, not run on a phone |
| Box (optional: passes messages between the shop's devices and keeps Shop Lightning's second copy) | Built; runs from source; not packaged for a device |
| NFC tap to pay | Built at the counter: the payment request as a read-only tag, given only on a sale's payment screen, the same text as its code; not yet tried with a wallet on a phone |
| Shop Lightning (the shop's own Lightning node on the counter, on the LDK library) | Built, and started by the counter app; channels from a test provider, payments, restarts and the sweep to the shop's wallet tested against Bitcoin Core on a test network, with chain data from Bitcoin Core or from peers' block filters; the provider reached over Tor; the second copy is kept on the Box, and keeping it on another counter or the owner phone is not built yet; test networks only; not run on a phone |
| Books and exports | Built; the owner app has a Books page that saves the files |
| Connecting a wallet the owner already holds | Built: the owner phone keeps it, pairs counters to it, and prepares refunds and cash-outs for that wallet to sign; tested against Bitcoin Core on a test network; not run on a phone |
| Recovery kit printing | Not built yet; the Key app can check a scanned wallet description against its own |
| Push wake-ups, a full node on the Box and a hosted Box | Not built yet |
| Reproducible builds and a second holder of the Key app's release key | Not in place yet |

Internal design and code reviews continue as the code is written. Device
testing, outside code review and an independent security audit remain
necessary.

## Guides still to be written

The step-by-step guides for choosing a wallet, counter setup, the Box,
daily use, refunds, end-of-day books,
cash-out, hardware signers, troubleshooting and development are unfinished.
The merchant chapters explain what the work involves so you can assess
it; they are not a substitute for those tested setup instructions.
