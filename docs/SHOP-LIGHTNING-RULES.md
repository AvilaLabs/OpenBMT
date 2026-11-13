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
  may run it there instead. The owner phone never runs it.
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
- **SL-12. The one exception.** "No key on any phone" has two exceptions: this
  key, only while Shop Lightning is on, and the refund float. Both hold a
  capped amount. Turning Shop Lightning off (SL-36) deletes the key from the
  counter after the close.
- **SL-13. Another counter.** Moving Shop Lightning to another counter is a
  close (SL-36) and a fresh start on the new one. The key is never copied
  from one counter to another.

## 3. The state, kept on two devices

- **SL-20. Every change stored twice.** A channel's state is written to the
  Lightning counter and to a second shop device the owner picks (another
  counter, the Box, or the owner phone). The second device holds only the
  encrypted state, never the key.
- **SL-21. Paid only when both have it.** The persister answers
  `ChannelMonitorUpdateStatus::InProgress` (`chain/mod.rs:271`) and reports
  completion with `ChainMonitor::channel_monitor_updated`
  (`chain/chainmonitor.rs:778`) only once both copies are written, through
  `MonitorUpdatingPersisterAsync` (`util/persist.rs:782`) on
  `ChainMonitor::new_async_beta` (`chain/chainmonitor.rs:438`). A sale is
  marked paid only on `Event::PaymentClaimed` after that.
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
  channel's wait (SL-30).
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
  server that would learn the shop's addresses.
- **SL-27. What it watches.** The funding output of each channel and every
  output the library registers with `chain::Filter`.

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
  handing the SOCKS stream to `lightning_net_tokio::setup_outbound`.

## 6. The cap, the sweep and the close

- **SL-33. The sweep.** Once the node's balance passes the cap, it moves
  everything above half the cap to the shop's wallet by a splice out
  (`splice_channel`, `ln/channelmanager.rs:4828`, beta), in one
  transaction. The address is one the counter derives itself from the pinned
  wallet description, never one sent by another device. The provider must
  accept splices (`reject_inbound_splices` false on its side); one that does
  not is swept by a close (SL-36).
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
