# Introduction

OpenBMT is a Bitcoin checkout system for shops. The cashier enters a sale
amount, the customer scans a payment code, and the money goes to the shop's
wallet. The cashier then closes the order in the shop's existing
point-of-sale system (POS).

> **OpenBMT is pre-release. It has had internal review only and no independent
> security audit. Do not use it with real funds.**

<a id="what-openbmt-is"></a>

## The shop's devices

OpenBMT has three Android apps and an optional server:

| Part | Job |
|---|---|
| Counter app | Displays the sale and payment code, watches for payment, and tells the cashier what has been checked. It holds no wallet signing key. |
| Owner app | Tracks the shop's coins and prepares refunds, cash-outs and settings changes. |
| Key app | Holds a second wallet key on a separate device. It reviews spends, address batches and changes that loosen the shop's limits. |
| Box | Relays messages between the shop's devices and reads POS orders. The shop runs it; it holds no wallet key. |

The shop holds all three keys to its wallet, and any two are needed to
spend. OpenBMT and Avila Labs hold none of them. Keeping the keys on separate
devices prevents a single stolen phone from being enough to spend the
money. The [wallet chapter](safety.md) explains the other conditions this
protection depends on, including work that is still unfinished.

The wallet uses a standard output descriptor: a description that other
wallet software can read. Recovery therefore does not require OpenBMT to
remain available.

<a id="what-openbmt-is-not"></a>

## Payments and records

OpenBMT checks payments directly through the Bitcoin network and can add a
check from the shop's own node. The counter identifies whether information
came from its peer check, the Box or the node. [Getting paid](getting-paid.md)
explains these reports and the risk of handing over goods before a payment
confirms.

There is no OpenBMT account, hosted balance or payment processor between
the customer and the wallet. If the shop wants dollars, it chooses an
exchange and approves a transaction to it. OpenBMT prepares that
transaction; it does not exchange the money.

The accounting export reports what is in the records and follows United
States rules only. It does not provide tax advice.

## What is built today

"Built" means the code exists and has passed tests on a desktop or against
Bitcoin Core on a test network. The Android apps have started on an
emulator but have not been tried on a phone. "Planned" means a feature is in
the design; "Not built yet" marks something that is still absent.

| Part | Status |
|---|---|
| Shared wallet, address batches, settings, signed records and instant acceptance | Built |
| Key app signing rules | Built |
| Counter sale handling and Bitcoin peer client | Built; tested against Bitcoin Core on a test network |
| Owner coin view, refund queue, spending and approval session | Built; tested against Bitcoin Core on a test network |
| Counter, Owner and Key Android apps | First builds; started on an emulator, not run on a phone |
| Box and its container, Umbrel and StartOS packages | Built; container image smoke-tested, Umbrel and StartOS packages not run on a device |
| NFC tap to pay | Not built yet |
| Square and Clover order matching | Built, read-only; based on vendor documentation, not tested with a real seller account |
| Lightning through the shop's own node | Built at the counter; not run against a real node |
| Books and exports | Built as a library; no books screen in the apps |
| Recovery kit printing | Not built yet; the Key app can check a scanned wallet description against its own |
| Push wake-ups, a full node on the Box and a hosted Box | Not built yet |
| Reproducible builds and a second holder of the Key app's release key | Not in place yet |

The chapters under "Using OpenBMT", including setup, the Box, refunds,
books and cash-out, have not been written yet.

<a id="who-it-is-for"></a>
<a id="how-to-read-a-safety-claim"></a>

## Checking the implementation

This handbook is written for shop owners and managers. References such as
`SR-34` and `CR-15` are for readers who want to check the implementation:
they identify rules in the repository's `docs/` folder, with corresponding
test names in the code. When a claim is marked "not verified", the text
states what was checked and what remains unknown.
