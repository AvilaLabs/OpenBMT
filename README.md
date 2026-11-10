# OpenBMT: Bitcoin Merchant Terminal

OpenBMT lets a shop take bitcoin at the counter the way it takes a card. The
money goes straight into the merchant's own wallet, and the sale closes in the
point-of-sale system the shop already uses.

**Status: pre-release.** The design is written down and the shared rules
library is implemented and tested. No app is built yet, nothing has been tried
on a device, and **no independent security audit has been done**. Do not use
this with real funds.

## What it is

- **The merchant holds every key.** Payments arrive on-chain in a 2-of-3
  multisig wallet (`wsh(sortedmulti(2,...))`). There is no processor, no
  custodian and no company server in the payment path.
- **Tap or scan.** The counter shows a QR code and, on Android, acts as an NFC
  tag. Any wallet that reads a `bitcoin:` URI can pay.
- **The counter holds no keys.** It receives batches of addresses that two
  separate devices have signed, uses each address once, and checks payments
  itself: over the Bitcoin peer-to-peer network with a light client, or
  against the merchant's own node.
- **Honest labels.** The screen always says which check was done. "Seen by 3
  peers", "Paid (light check)" and "Confirmed (your node)" are different
  things and are never blurred together.
- **Closes the sale in the POS.** Connectors post the bitcoin tender back to
  the POS, so receipts, inventory and reports keep working.
- **Standard recovery.** The wallet is an ordinary output descriptor, so it
  can be restored in other wallet software without anything from this
  project. A restore drill against other wallets is part of the test plan.

## The security goal

Once two launch conditions hold, no single compromised OpenBMT component,
device or person should be able to spend the wallet:

1. the third key is made outside OpenBMT software, away from the device that
   holds the second key;
2. the signer app's release key is held by a second, independent person.

Until both hold, the claim does not hold, and the documents and screens say
so. Losses from accepting unconfirmed payments are bounded by limits the owner
sets. The full statement, its conditions and the reasoning are in
[docs/DESIGN.md](docs/DESIGN.md).

## What is in this repository

| Path | What |
|---|---|
| [docs/DESIGN.md](docs/DESIGN.md) | The design: flows, architecture, threat table, open questions |
| [docs/research/](docs/research/) | Research notes and three rounds of internal design review |
| [crates/openbmt-core/](crates/openbmt-core/) | The rules every device must agree on: wallet description, device identity, address batches, signed records, signed settings, instant-acceptance policy |

Planned next: the signer's refusal rules, the payment watcher, then the apps.

## Building

```
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Rust 1.85 or newer. Dependency versions are pinned exactly and `Cargo.lock`
is committed.

## Security

See [SECURITY.md](SECURITY.md) for how to report a problem. The reviews under
`docs/research/` are internal design reviews, not an audit.

## License

MIT. See [LICENSE](LICENSE). An Avila Labs project.
