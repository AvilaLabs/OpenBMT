# OpenBMT: Bitcoin Merchant Terminal

OpenBMT lets a shop take bitcoin at the counter the way it takes a card. The
money goes straight into the merchant's own wallet, and the sale closes in the
point-of-sale system the shop already uses.

**Status: pre-release.** Headless Rust pieces now cover the counter, owner
coin view and sending path, approval flow, and POS and books logic. The
user-facing screen word catalogs are written for the counter, owner, Key app,
and Box console. These are not finished apps: Android/Kotlin integration,
packaging, and device testing remain. The documented Bitcoin Core regtest
results are engineering checks, not a security review. **No independent
security audit has been done.** Do not use this with real funds.

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
  project. A restore drill against Bitcoin Core runs on regtest
  ([scripts/restore-drill-core.sh](scripts/restore-drill-core.sh)); drills
  against other wallets and hardware signers are part of the test plan.

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
| [docs/SIGNER-RULES.md](docs/SIGNER-RULES.md) | Every rule under which the Key app refuses to sign |
| [docs/SPEND-RULES.md](docs/SPEND-RULES.md) | How a transaction is built, joined and finished: coins, change, fee, the PSBT the signer accepts, and the checks on two signatures |
| [docs/SESSION-RULES.md](docs/SESSION-RULES.md) | The approval session: the file the owner phone and the Key app pass between them, what the Key app checks on every line, what is stored before an answer leaves, and the owner phone's refund queue |
| [docs/WATCHER-RULES.md](docs/WATCHER-RULES.md) | What the counter believes about a payment, and what each kind of evidence may prove |
| [docs/FEE-RULES.md](docs/FEE-RULES.md) | How the owner's device works out a fee rate from blocks it has checked itself, with no fee service, and what that number does not say |
| [docs/POS-RULES.md](docs/POS-RULES.md) | How the owner's device matches the POS orders to the counter's sales, and what raises an alert |
| [docs/BOOKS-RULES.md](docs/BOOKS-RULES.md) | How the counters' records become the books: per-sale fields and what stays unknown, the two CSV exports and the encrypted export file |
| [docs/OWNER-COINS-RULES.md](docs/OWNER-COINS-RULES.md) | How the owner's device learns wallet coins from checked blocks, handles reorganizations, and grows its watch set |
| [docs/screens/](docs/screens/) | Catalogs of the words shown for each counter, owner, Key app, and Box console screen state |
| [docs/BOX-RULES.md](docs/BOX-RULES.md) | What the merchant's own server holds, how a Box with no screen is paired, and its part in the relay and the POS side (implemented in `crates/openbmt-box`; no vendor's connector is in it) |
| [docs/SCREEN-RULES.md](docs/SCREEN-RULES.md) | What a person is told on every screen: one screen for every state, always a next step, no notation of the program, and the test each program runs over all of its words |
| [docs/DEPENDENCIES.md](docs/DEPENDENCIES.md) | Which outside packages each program is built from, the rules for adding or updating one, and the check that enforces them (`scripts/deps-check.sh`, lists in `deps/`) |
| [docs/research/](docs/research/) | Research notes, four rounds of internal design review, and the results of the checks against Bitcoin Core |
| [crates/openbmt-core/](crates/openbmt-core/) | The rules every device must agree on: wallet description, device identity, address batches, signed records, signed settings, instant-acceptance policy |
| [crates/openbmt-signer/](crates/openbmt-signer/) | The Key app's transaction signer: the rules under which key 2 signs |
| [crates/openbmt-spend/](crates/openbmt-spend/) | The spend path around the signer ([rules](docs/SPEND-RULES.md)): plan a transaction from the wallet's coins, write it as a PSBT the signer accepts, join two signers' signatures and finish it ([results against Core](docs/research/spend-e2e-core-2026-11-10.md)) |
| [crates/openbmt-box/](crates/openbmt-box/) | The Box, the merchant's own small server, ([rules](docs/BOX-RULES.md)): its key and storage, pairing by an invitation typed into it, the signed device list it follows, a relay for the shop's sealed messages that it also reads, and the POS side that matches the counters' paid notices to orders. No vendor's connector is in it |
| [crates/openbmt-books/](crates/openbmt-books/) | The merchant's books ([rules](docs/BOOKS-RULES.md)): the counters' verified records turned into sales lines and end-of-day totals, an accounting CSV that holds only opaque sale IDs, a full ledger behind a warning, and an encrypted export file that reads back. No network and no clock; not tax advice, US rules only |
| [crates/openbmt-session/](crates/openbmt-session/) | The approval session file ([rules](docs/SESSION-RULES.md)): the request the owner phone hands to the Key app and the response it hands back, in one strict encoding |
| [crates/openbmt-approver/](crates/openbmt-approver/) | The Key app's side of the approval session: opens a request, shows one line per item, decides each, and stores before it releases |
| [crates/openbmt-owner/](crates/openbmt-owner/) | The owner phone's side of the approval session: the refund queue, cash-out, batches, settings and pairings, the session it builds and the Key app's answer it takes |
| [crates/openbmt-key-ffi/](crates/openbmt-key-ffi/) | The Key app's core for Android: one object, a storage trait, flat records for each line of a session, one `dispatch` function over a framed request and a JNI entry point as the only `unsafe`; every rule stays in the crates above. The Kotlin binding and OBM9 decoder harness compile and run on the JVM; Android JNI/device integration remains unchecked |
| [apps/key-android/](apps/key-android/) | The Key app for Android, first build: plain `android.widget` screens over the library's words (first start, the words once, wallet and kit check, the approval session, limits and status), built into a signed test APK with the SDK's own tools and no Gradle, AndroidX or Compose; the Kotlin standard library is its one runtime library. Not run on a device |
| [crates/openbmt-watch/](crates/openbmt-watch/) | The counter's payment watcher: merkle proofs, full blocks, peer groups, conflicts, health |
| [crates/openbmt-net/](crates/openbmt-net/) | The counter's Bitcoin peer client: wire framing, the single-peer session and the manager that runs several peers, checks blocks by filters and fetches announced transactions, with no sockets and no clock |
| [crates/openbmt-net-io/](crates/openbmt-net-io/) | The counter's network driver: owns the sockets and threads, runs the peer manager and the address book over them, keeps the headers and the address book on disk, and speaks SOCKS5 for Tor |
| [crates/openbmt-counter/](crates/openbmt-counter/) | The counter's judgment of a sale ([rules](docs/COUNTER-RULES.md)): opening, quotes, the cashier's screens, instant acceptance, overrides, owner commands, and the order of writes to storage |
| [crates/openbmt-counter-node/](crates/openbmt-counter-node/) | The counter without a screen: the judgment of a sale joined to the network driver and to a directory of files, so that a payment is seen, confirmed, reorganized away and survives a restart ([results against Core](docs/research/counter-e2e-core-2026-11-10.md)) |
| [crates/openbmt-owner-node/](crates/openbmt-owner-node/) | The owner's headless watcher and coin view: learns coins from checked blocks, tracks owner spends and reorganizations, and sends approved transactions through the network driver; also provides the signed heartbeat used by instant acceptance ([coin rules](docs/OWNER-COINS-RULES.md); [regtest script](scripts/owner-spend-e2e-core.sh); [documented Core results](docs/research/owner-spend-e2e-core-2026-11-10.md); [heartbeat rules](docs/COUNTER-RULES.md), CR-50 to CR-56) |
| [crates/openbmt-link/](crates/openbmt-link/) | Messages between the shop's own devices ([rules](docs/LINK-RULES.md)): sealed to one device and signed by the sender, the relay that holds them (mailboxes, challenges, limits, files), and a blocking client and server over TCP or a Tor onion service |
| [crates/openbmt-pos/](crates/openbmt-pos/) | The vendor-neutral POS side ([rules](docs/POS-RULES.md)): the model of POS orders, the reference a cashier types, the matching of every Bitcoin tender to a counter sale, the alerts for what does not match, and the boundary a POS connector fills. No network and no vendor code |
| [scripts/](scripts/) | Regtest checks against Bitcoin Core, including restore, signer, peer and network-driver flows, counter payment tracking, owner heartbeat, and owner coin view and refund sending ([owner check](scripts/owner-spend-e2e-core.sh); its documented results are in [the research note](docs/research/owner-spend-e2e-core-2026-11-10.md)) |

Headless components and screen word catalogs are available. Further
hardening and screen-state coverage precede integration into the Android
and Box applications and testing on devices. Full application flows still
need to be completed and checked.

## Building

```
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Rust 1.89 or newer. Dependency versions are pinned exactly and `Cargo.lock`
is committed.

## Security

See [SECURITY.md](SECURITY.md) for how to report a problem. The reviews under
`docs/research/` are internal design reviews, not an audit.

## License

MIT. See [LICENSE](LICENSE). An Avila Labs project.
