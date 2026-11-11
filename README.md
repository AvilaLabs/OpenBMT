# OpenBMT

[![CI](https://github.com/AvilaLabs/OpenBMT/actions/workflows/ci.yml/badge.svg)](https://github.com/AvilaLabs/OpenBMT/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

**Bitcoin checkout for independent shops, paid directly to the merchant's
wallet.**

**Pre-release. Do not use with real funds.** The Android apps are early builds
that have not been tested on a phone or emulator. Release packaging and full
application integration remain in progress. OpenBMT has had internal design
reviews; no independent security audit has been completed.

## Get started

- [Try a local Box preview](#try-a-local-box-preview)
- [See how OpenBMT works](#about-openbmt)
- [Read the design and current limits](#design-and-scope)
- [Explore documentation](#documentation)

## About OpenBMT

OpenBMT is an open-source Bitcoin checkout project for shops. A cashier creates
a sale and shows a payment QR code. The customer pays from a Bitcoin wallet,
and the merchant controls the wallet keys. The counter reports what it has
checked about a payment, so an unconfirmed transaction seen by peers is distinct
from a confirmation checked against the merchant's own node.

The project includes on-chain payments using a 2-of-3 wallet, an owner app for
refunds and cash-outs, and a separate app for approving transactions. An
optional Box relays records between shop devices and reads POS orders. The owner
app can match Square and Clover orders with counter sales; completing a POS
checkout automatically is still in progress. Shops can also connect the counter
to their own LND or Core Lightning node to offer Lightning invoices.

### The four parts

| Part | What it does |
|---|---|
| [Counter app](apps/counter-android/) | Creates sales, displays payment codes, and reports the payment evidence it has checked. It holds no wallet signing key. |
| [Owner app](apps/owner-android/) | Tracks wallet coins, prepares refunds, cash-outs and settings, and reconciles POS orders with counter sales. |
| [Key app](apps/key-android/) | Reviews and approves signing requests on a separate device. |
| [Box](packaging/box/) | Optional merchant-run service for device messages and POS order reading. It holds no wallet key. |

## Try a local Box preview

This starts an unpaired Box on the local computer. It does not create a shop
wallet or connect a counter. Rust 1.89 or newer is required.

```bash
git clone https://github.com/AvilaLabs/OpenBMT.git
cd OpenBMT
cargo run --locked -p openbmt-box -- \
  target/box-preview 127.0.0.1:8986 \
  --status-page 127.0.0.1:8987
```

Open [http://127.0.0.1:8987](http://127.0.0.1:8987) in a browser on the same
computer to view the Box status page. The terminal shows the Box's current
state; type `quit` there to stop it. For container and node packaging options,
see [Box packaging](packaging/box/README.md).

The Android app sources have separate build instructions:
[Counter](apps/counter-android/README.md),
[Owner](apps/owner-android/README.md), and
[Key](apps/key-android/README.md).

## Payments and records

- **On-chain checkout:** the counter displays an address from an owner-approved
  batch. Payment tracking distinguishes peer sightings, light-client checks,
  and checks against the merchant's own node. See
  [payment checks](docs/WATCHER-RULES.md) and
  [counter behavior](docs/COUNTER-RULES.md).
- **Owner approval:** the owner prepares a spend or refund request; the separate
  Key app reviews it before the two signatures are joined. See
  [wallet spending](docs/SPEND-RULES.md), [signing rules](docs/SIGNER-RULES.md),
  and [approval sessions](docs/SESSION-RULES.md).
- **Optional Lightning:** the counter can request and monitor invoices through
  the merchant's own LND or Core Lightning node. Lightning balances follow
  that node's custody model. See
  [Lightning support](docs/LIGHTNING-RULES.md).
- **POS and books:** Square and Clover order readers support matching orders to
  counter sales and raising discrepancies. They do not yet provide a complete
  live checkout connector. The books component produces sales records,
  summaries, and exports. See [POS behavior](docs/POS-RULES.md) and
  [books and exports](docs/BOOKS-RULES.md).

## Current status

Rust components cover the core counter, payment watcher, owner coin view and
spending path, approval flow, records, and POS order readers. First Android APK
builds exist for the Counter, Owner, and Key apps; each is signed with a local
test key and has not been run on a phone or emulator. Full app integration,
release signing, device testing, and release packaging remain incomplete.

The Box can be run from source and has container packaging. Umbrel and StartOS
packages still need testing on their target devices.
Bitcoin Core regtest notes document specific engineering checks, including
restore and payment flows; those checks do not establish readiness for real
funds. See [current design and open conditions](docs/DESIGN.md) and the
[research notes](docs/research/).

## Design and scope

The on-chain wallet requires two of three keys to spend. The design goal is
that no single compromised device, component or person can spend from it.
That goal depends on two launch conditions: the third key must be created
outside OpenBMT software and away from the second key's device, and the Key
app's release key must be held by an independent person. Both conditions
remain requirements for launch. See the
[design and threat model](docs/DESIGN.md) for the conditions,
payment-acceptance limits, and remaining work.

## Documentation

- [Design and threat model](docs/DESIGN.md)
- [Payment checks](docs/WATCHER-RULES.md) · [Counter](docs/COUNTER-RULES.md) · [Owner app](docs/OWNER-APP-RULES.md)
- [Wallet spending](docs/SPEND-RULES.md) · [Signer](docs/SIGNER-RULES.md) · [Approval sessions](docs/SESSION-RULES.md)
- [Lightning](docs/LIGHTNING-RULES.md) · [POS matching](docs/POS-RULES.md) · [Books and exports](docs/BOOKS-RULES.md)
- [Box setup and packaging](packaging/box/README.md) · [Screen catalogs](docs/screens/)
- [Bitcoin Core engineering notes](docs/research/)

## Development

The Rust workspace uses a committed `Cargo.lock` and requires Rust 1.89 or
newer. Common checks are:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

See [dependency rules](docs/DEPENDENCIES.md) before changing dependencies.
Report vulnerabilities using [SECURITY.md](SECURITY.md).

## Licence

OpenBMT is licensed under the [MIT licence](LICENSE).
