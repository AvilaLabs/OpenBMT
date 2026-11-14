# Shop Lightning rules: the shop's own Lightning node, built in

Status: specification, written before the code, for `crates/openbmt-lightning-node`.
Each rule has an ID so that a test can name the rule it proves (`sl_NN_...`).
It implements the choice of 2026-11-12: a Lightning node inside OpenBMT, on
the LDK library (`lightning` 0.2.8), used as a library and never forked.

Shop Lightning receives payments at the counter and nothing else. It holds a
small amount, capped by the owner, and moves anything above the cap to the
shop's own wallet. Its key comes from the shop's 12 words, made offline, so
there is nothing new to write down. No server holds a key, and no company
takes a cut.

**First build. No independent security audit has been done. Not for real
funds.** Until the purist check of section 10 is done, the node runs only on
the test networks (regtest and signet).

**What the facts rest on.** The library calls named here were read in the
source of `lightning` 0.2.8, `lightning-liquidity` 0.2.8,
`lightning-block-sync` 0.2.8 and `lightning-net-tokio` 0.2.0, and the
behaviour in the prototype of 2026-11-12 (regtest, Bitcoin Core 32.1, LND
0.21.8, `ldk-node` 0.7.1 over `lightning` 0.2.8). Each rule names what it rests
on. The prototype used `ldk-node`; this crate does not (SL-02).

## 1. What it is

- **SL-01. Receive only.** The node takes payments for the counter's sales.
  It never pays an invoice, never forwards a payment and never opens a
  channel itself. Money leaves it only by the sweep (SL-33) and the close
  (SL-36). A refund is never paid over Lightning: it is paid on-chain, as
  `docs/SPEND-RULES.md` says.
- **SL-02. On the library, not on ldk-node.** The crate uses `lightning`
  directly, with its own persister, chain watcher and event handling.
  `ldk-node` 0.7.1 can wait for a second device only by blocking a write,
  which stalls the peer until the provider drops it (prototype E4, 38
  seconds), and it panics on a store error (`lightning` 0.2.8
  `chain/chainmonitor.rs:1394` and `:1500`). Neither is acceptable at a
  counter.
- **SL-03. Where it runs.** On one counter the owner picks, called the
  Lightning counter. Every other counter asks it for invoices over the shop's
  own link (`docs/LINK-RULES.md`) and never holds the key. A shop with a Box
  may run it there instead. The owner phone never runs it. In the first
  build only the Lightning counter takes Lightning (SL-70).
- **SL-04. Off until turned on.** A shop without Shop Lightning takes
  on-chain payments, or Lightning through its own node
  (`docs/LIGHTNING-RULES.md`) or a swap (`docs/SWAP-RULES.md`). Turning it on
  needs the owner phone and the Lightning key (SL-10).

## 2. The key

- **SL-10. A child of the shop's words.** The node's seed is the 64-byte
  BIP-85 HEX child at `m/83696968'/128169'/64'/0'` of the shop's 12 words.
  The child cannot sign for the savings, and the words remake it.
  Prototype E1: the spec's vector gives the spec's child byte for byte, and
  the same child gives the same node key on two fresh rebuilds.
- **SL-11. Made offline.** The child is made where the words are: the stick
  (`docs/STICK-RULES.md`, a step after the public result) or a signer that
  supports BIP-85. It is shown once as a QR code and scanned by the
  Lightning counter. OpenBMT never asks for the 12 words on a counter or a
  phone.
- **SL-12. The one exception.** "No key on any phone" has three exceptions:
  this key, only while Shop Lightning is on, the refund float
  (`docs/FLOAT-RULES.md`) and the starter wallet
  (`docs/START-SIMPLE-RULES.md`). Each holds a capped amount. Turning Shop Lightning off (SL-36) deletes the key from the
  counter after the close.
- **SL-13. Another counter.** Moving Shop Lightning to another counter is a
  close (SL-36) and a fresh start on the new one. The key is never copied
  from one counter to another.

## 3. The state, kept on two devices

- **SL-20. Every change stored twice.** A channel's state is written to the
  Lightning counter and to a second shop device the owner picks (another
  counter, the Box, or the owner phone). The second device holds only the
  encrypted state, never the key. In the first build the second device is
  the shop's Box (SL-73).
- **SL-21. Paid only when both have it.** The persister answers
  `ChannelMonitorUpdateStatus::InProgress` (`chain/mod.rs:271`) and reports
  completion with `ChainMonitor::channel_monitor_updated`
  (`chain/chainmonitor.rs:778`) only once both copies are written, through
  `MonitorUpdatingPersisterAsync` (`util/persist.rs:782`) on
  `ChainMonitor::new_async_beta` (`chain/chainmonitor.rs:438`). A sale is
  marked paid only on `Event::PaymentClaimed` after that. The node claims a
  payment only for an invoice it made and recorded (SL-29), not expired, and
  fails back any other. Each paid event is stored before the counter hears
  of it and is given again at every start until the counter says it has
  recorded the sale; it is then marked, so the library giving it again after
  a crash does not pay the sale twice, and the mark is dropped after 30
  days.
- **SL-22. The beta, said plainly.** The library marks `InProgress` and the
  async persister as beta, and its documents say this path "may lead to funds
  loss" in rare cases. This is why section 10 holds real funds back, and why
  the cap (SL-34) is small.
- **SL-23. When the second device is away.** A write that cannot reach it
  stays in progress; nothing panics. The node logs it, the counter's Lightning
  stops offering invoices (SL-41), and the manager sees which device is
  missing. Payments already in flight finish when it returns.
- **SL-24. Order of writes.** Each channel's monitor goes to the second
  device first, then to the counter's own copy, so the second device never
  holds an older monitor. The channel manager, the provider's state and the
  spendable outputs go to the counter's copy first and then to the second
  device, newest first, so that a second device that is away does not stop
  the node. A start from the second device's copy alone can therefore find a
  manager older than its monitors; the library then closes those channels on
  the chain, and their money comes back to the shop's wallet after the
  channel's wait (SL-30). The expectation of each invoice (SL-29) is written
  as a monitor is, second device first, before the counter shows the
  invoice; an invoice whose expectation could not be stored is not shown.
  The paid events (SL-21) and the sweep address (SL-33) go to the counter's
  copy first, as the manager does.
- **SL-25. No old copy, ever.** On a start, the node reads the update
  numbers of both copies and starts only from the newer one. A copy behind
  the other is never used. Prototype E5b: an old copy makes the library stop
  with "you are running with an old backup"; the provider did not close the
  channel, but a broadcast of that old state would lose the channel's money.

## 4. The chain

- **SL-26. Own chain data only.** The node learns of blocks and its
  transactions from the shop's own Bitcoin Core (`lightning-block-sync`
  `RpcClient`) or from the compact-filter watcher (`docs/WATCHER-RULES.md`),
  through `chain::Confirm` and `chain::Filter`. Never Electrum, Esplora or any
  server that would learn the shop's addresses. On compact filters the newest
  block reaches the library once its header is stored, which can be about a
  minute after the block arrives; the library keeps its deadlines in blocks,
  so a minute is far inside every one. After a reorganization the newest
  block reaches the library only once the watcher has checked the new branch
  from the fork, so every transaction of the new branch is given before the
  library hears that the chain moved; the watcher reports its checked height
  again from the fork even where it does not pass the height it reported
  before. At start and at every change of the chain, each transaction the
  library holds as confirmed is read against the chain the node knows: one
  whose block is not the chain's block at that height, or that lies above
  the newest block known, is given back as unconfirmed.
- **SL-26a. Fee rates without Core.** On compact filters the fee rates the
  library asks for come from samples of blocks, taken as the owner's device
  takes them (`docs/FEE-RULES.md` FE-20, with the node's own `fee-history.bin`):
  the rates for a channel, a close and a spend are the recent rate (FE-13);
  the urgent rate, and the highest the node proposes or counts for dust, are
  twice the higher of the recent and the usual rate (FE-17). The library sets
  no upper limit on the rate the provider sets; it refuses only one below the
  floor. No rate is ever below the fixed floor the
  node uses with Core, and until the first sample the fixed rates are used.
- **SL-27. What it watches.** The funding output of each channel and every
  output the library registers with `chain::Filter`. When a block that holds a
  channel's funding leaves the chain, the library closes that channel by
  force, since it uses a channel only on a confirmed funding, and the shop's
  money comes back as from any close (SL-33, SL-36); a later payment opens a
  new channel (SL-29).

## 5. The provider

- **SL-28. Any provider, the shop's choice.** The first channel and every
  later one come from a liquidity provider the shop picks, through LSPS2
  (`lightning-liquidity` 0.2.8: `request_opening_params`,
  `select_opening_params`) with `client_trusts_lsp` false. The provider can
  be changed at any time. OpenBMT runs none and takes nothing.
- **SL-29. The fee before the payment.** A payment that opens a channel
  shows the provider's fee to the customer's amount before the invoice is
  made, and the owner sets the most the shop will pay for one opening. Above
  it, the sale is on-chain.
- **SL-30. The window to be back online.** The node asks for
  `our_to_self_delay` of 2016 blocks (about two weeks), or the provider's
  `max_client_to_self_delay` if lower. A provider offering under 1008 blocks
  (about a week) is refused, so a shop closed for a long weekend stays safe.
  The library has no watchtower client; this window is the protection.
- **SL-31. Private channels.** Channels are unannounced
  (`announce_for_forwarding` false), inbound channels are accepted by hand
  (`manually_accept_inbound_channels`) and only from the chosen provider.
- **SL-32. Tor.** With Tor on, the node reaches the provider through Tor,
  handing the SOCKS stream to `lightning_net_tokio::setup_outbound`, and the
  compact-filter watcher reaches its peers through the same proxy. A proxy
  that does not answer at start stops the node before anything is made on
  disk; there is never a connection outside Tor. The provider is given as an
  IP address and port: a provider at an onion address is not built yet.

## 6. The cap, the sweep and the close

- **SL-33. The sweep.** Once the node's balance passes the cap, it moves
  everything above half the cap to the shop's wallet by a splice out
  (`splice_channel`, `ln/channelmanager.rs:4828`, beta), in one
  transaction. The address is one the counter derives itself from the pinned
  wallet description, never one sent by another device. The provider must
  accept splices (`reject_inbound_splices` false on its side); one that does
  not is swept by a close (SL-36). What a close leaves to the shop that the
  library must spend itself (`SpendableOutputDescriptor`) is spent by the
  library's `OutputSweeper` to an address derived the same way. The last
  address the counter set is kept and used after a restart.
- **SL-34. The cap.** Set by the owner, with a suggestion of one day's
  Lightning sales. The suggestion is never more than the counter's instant
  limit.
- **SL-35. Room to receive.** A sweep does not give back room to receive: the
  channel shrinks (prototype E6). Room comes back only when the provider adds
  coins, by a new channel at the next payment too big for the room, at its
  fee. The owner phone says so plainly, and shows the room left.
- **SL-36. Closing for a while.** Before the shop closes for longer than half
  the window of SL-30, and whenever Shop Lightning is turned off, the node
  closes every channel cooperatively to the shop's wallet. A provider that
  will not close is closed on, and the money arrives after the window.

## 7. At the counter

- **SL-40. Lightning only below the room.** Before showing an invoice, the
  counter sums `inbound_capacity_msat` over `list_usable_channels()`. A sale
  above that sum less a margin is on-chain only, or opens a new channel
  (SL-29). A payment bigger than the room is held by the payer for about two
  minutes and then fails (prototype E7, `MPP_TIMEOUT_TICKS` = 3), which a
  customer at a counter must never see.
- **SL-41. When Lightning is not offered.** With the second device away, the
  chain data behind, no usable channel and no provider answering, the
  counter shows the on-chain code alone and the manager sees why.
- **SL-42. One code.** When both are offered, the counter shows one BIP 321
  code with the address and the invoice, as `docs/LIGHTNING-RULES.md` LN-18
  does.

## 8. What the owner sees

- **SL-45.** The owner phone shows the Lightning balance and the cap, the
  room to receive, the provider and its fees paid so far, the second device
  and when it last stored the state, and the date of the last sweep.

## 9. Tests

- **SL-50. Regtest, end to end.** Against Bitcoin Core 32.1 and an LSPS2
  provider: a first payment opens a channel; a second uses it; a sweep pays
  the counter's own address; a payment too big is never offered; the second
  device away holds a payment without a panic and finishes it on return; a
  restore from the newer copy works and from the older is refused; 1,000
  blocks offline then back is safe.
- **SL-51. The key.** BIP-85 vectors (SL-10), and a stick or signer child
  equals the core's.

## 10. Before real funds

- **SL-60.** Real funds need, in order: the library's async persistence out
  of beta or a review of its funds-loss cases against SL-21 to SL-25; the
  purist check against `docs/DESIGN.md`'s standard; and a run on signet with
  the provider the handbook will name.

## 11. The first build on a counter

Sections 1 to 8 describe Shop Lightning in full. This section is what the
first build on a counter does. Where it is narrower, it holds until a later
section widens it.

- **SL-70. Only the Lightning counter.** Only the Lightning counter named in
  the settings (SL-71) offers Lightning. Every other counter takes on-chain
  payments only, as in a shop without Shop Lightning. Asking the Lightning
  counter for invoices over the link (SL-03), and running the node on a Box,
  come later.
- **SL-71. In the signed settings.** The settings field `shop_lightning`
  names the Lightning counter, which is a counter of the settings; the
  node's ID, 33 bytes (SL-72); the second device, which is a Box of the
  settings (SL-73); the provider, by its node ID and its IP address and port
  (SL-28, SL-32); the cap, in satoshis (SL-34); and the most the shop pays
  for one opening, in satoshis (SL-29). With the field unset, Shop Lightning
  is off (SL-04). It is never set together with `lightning_node` or `swap`.
  Settings that break any of this are refused when they are read, as any
  other settings that do not check are. Settings without the field encode
  and hash exactly as they did before it existed.
- **SL-72. The node's ID, pinned.** The node's ID is the public key the
  library makes from the seed of SL-10. The seed's BIP-32 master key gives
  the 32 bytes handed to `KeysManager::new`, and the node key is the
  hardened child `0'` of the master key of those 32 bytes. `openbmt-core`
  makes the same ID without the library. A test of `openbmt-lightning-node`
  checks that the two agree for the vector of SL-10 and for two other seeds.
  The Lightning counter refuses a scanned seed whose ID is not the pinned
  one, before it stores anything. A node whose seed does not give the pinned
  ID does not start.
- **SL-73. The Box keeps the second copy.** The second device of SL-20 is
  the shop's Box, which is also the shop's relay (`docs/BOX-RULES.md`
  section 5d). Another counter or the owner phone as the second device comes
  later. A shop without a Box cannot turn Shop Lightning on, and the owner
  phone says why.
- **SL-74. Over the shop's own link.** The Lightning counter writes, reads,
  lists and removes its stored state with messages of the kinds Store and
  Stored (`docs/LINK-RULES.md` LK-80 to LK-83), sent straight to the Box on
  the counter's relay connection. Each message is sealed for the other
  device and signed by the sender's identity key, as every link message is.
  A write counts only when the Box's Stored answer, opened and checked,
  names the same request and the SHA-256 of the same bytes. A device on the
  shop's network that is not the Box cannot make that answer, so it cannot
  make the counter count a payment whose state is kept only once (SL-21).
  The node crate's plain TCP device is for tests and is never built into an
  app.
- **SL-75. What the Box can see.** The state is sealed with the node's key
  before it leaves the counter (SL-20), and the link seals it again. The Box
  sees the names the state is stored under, their sizes and when they
  change. It holds no key that opens the state.
- **SL-76. Turning it on.** The owner phone turns Shop Lightning on by a
  change of the settings (`docs/APPROVAL-RULES.md` AP-13). It takes the
  node's ID from the stick's code (`docs/STICK-RULES.md` SK-35), never from
  a counter. The Lightning counter then scans the stick's seed code (SK-32)
  and checks it against the pinned ID (SL-72).
- **SL-77. Turning it off or moving it.** Settings without
  `shop_lightning`, or naming another Lightning counter or another node's
  ID, make the counter close every channel of the node it runs (SL-36). It
  offers no invoice from then on. It keeps the seed, and keeps storing
  twice, until every channel is closed and its money is in the shop's
  wallet; then it deletes the seed (SL-12, SL-13).
- **SL-78. What the owner sees.** With each check-in, the Lightning counter
  sends the owner a message of kind Lightning report (`docs/LINK-RULES.md`
  LK-84) with the facts of SL-45: the balance, the room to receive, the
  opening fees paid so far, when the Box last stored the state, the time of
  the last sweep, and why Lightning is not offered when it is not (SL-41).
  The owner phone shows the newest report and when it came. The cap and the
  provider it shows are those of the settings.
- **SL-79. Tested.** The core's ID equals the library's (SL-72); settings
  with `shop_lightning` round-trip, and each broken case of SL-71 is refused;
  a counter given a seed of another ID stores nothing; a write the Box did
  not answer, or answered for another request or other bytes, does not
  count; a device that is not the Box cannot answer for it; on regtest a
  payment at the Lightning counter is stored on the Box before the sale is
  paid, and with the Box away the counter shows the on-chain code alone;
  turning Shop Lightning off closes the channels and then deletes the seed.
