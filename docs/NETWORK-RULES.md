# Network rules: how the counter talks to Bitcoin peers

Status: specification for `crates/openbmt-net`, written before the code.
Each rule has an ID so that a test can name the rule it proves. The rules
implement the network side of `docs/DESIGN.md` §5.1 and feed the checks of
`docs/WATCHER-RULES.md`.

This crate is transport. It fetches headers, filters, blocks and
unconfirmed transactions from Bitcoin peers and says which peer sent what.
It decides nothing about a payment. Every header goes through the watcher's
header rules (WR-04 to WR-08), every block through WR-20 to WR-22, every
sighting through WR-40 to WR-46. A peer can waste the counter's time and
can stay silent. These rules keep it from doing more than that, and keep
the counter from telling peers more than it has to.

The counter speaks the Bitcoin peer protocol itself and does not use a
general light-client library. The watcher needs to know which peer
reported each transaction and what height each peer claims, and it checks
every header itself. A small client that does exactly this, and nothing
the watcher does not use, is less code to trust than a general one.

The crate has two layers. The inner layer has no sockets, no clock and no
threads: bytes and the time go in, bytes and events come out, so every
rule can be tested and fuzzed without a network. The outer layer is a
thin driver that owns the sockets.

## 1. Messages on the wire

- **NR-01.** A message is the 24-byte header of the Bitcoin peer protocol
  and a payload. The network magic must be the counter's network. The
  checksum must match. A message that fails either ends the connection.
- **NR-02.** The payload length in the header is checked before any of the
  payload is kept, against a limit for that command: `block` 4,000,000
  bytes; `tx` 400,000; `cfilter` 1,000,000; `headers` 162,003; `inv` and
  `notfound` 1,800,003; `addr` and `addrv2` 50,003; `version` 400;
  every other command the counter reads, 100. A longer payload ends the
  connection.
- **NR-03.** A command the counter does not read is skipped without being
  parsed if its payload is at most 100,000 bytes. A longer one ends the
  connection.
- **NR-04.** A payload that does not parse as its command, or leaves bytes
  over, ends the connection.
- **NR-05.** The counter keeps at most one message being received and at
  most 2,000,000 bytes waiting to be sent per peer **[to tune]**. A peer
  that does not read is disconnected.

## 2. Starting a connection

- **NR-10.** The counter only dials out. It never listens.
- **NR-11.** The counter's `version` message says as little as it can:
  protocol 70016, no services, the caller's time, both address fields
  zero, a fresh random nonce, an empty user agent, its own tip height and
  relay on. It does not ask for compact blocks or address relay to
  itself.
- **NR-12.** The peer's `version` must come first, be protocol 70016 or
  higher, offer the `WITNESS` service and either `NETWORK` or
  `NETWORK_LIMITED`, and not carry the counter's own nonce. Its services
  and start height are kept as the peer's claims. They are claims:
  nothing is believed because of them.
- **NR-13.** The counter sends `sendaddrv2` before its `verack`, and
  `sendheaders` as soon as the peer's `verack` is in: a peer does not
  read it earlier. Nothing but the handshake is sent or read until both
  sides have sent `verack`. A message that belongs after the handshake and
  arrives before it ends the connection.
- **NR-14.** A handshake that is not finished in 30 seconds ends the
  connection. After the handshake the counter sends a `ping` every 2
  minutes; a peer that has not answered the last ping when the next is
  due is disconnected. A peer's `ping` is answered.

## 3. Which peers

- **NR-20.** The counter keeps 8 connections **[to tune, E3]**. No two
  connected peers are in the same network group (WR-41). The Box's node
  is never one of them.
- **NR-21.** At least 2 of the connections go to peers that offer the
  compact-filter service, when the address book has them.
- **NR-22.** Addresses come from the network's DNS seeds, from `addr` and
  `addrv2` messages, and from the counter's own stored address book. The
  book holds at most 2,000 addresses and at most 8 per network group, so
  that one peer cannot fill it with its own. Only publicly routable
  addresses are kept.
- **NR-23.** The next address to dial is drawn at random, with randomness
  the caller supplies, from groups that have no connection now.
- **NR-24.** The two peers that have been connected longest without
  fault are stored and dialed first after a restart, so a restart is not
  a chance to surround the counter with new peers.
- **NR-25.** A peer is dropped and not dialed again for 24 hours when it
  breaks a rule of sections 1 or 2, sends a header that WR-05 refuses
  (other than for an unknown parent), sends a block that WR-20 or WR-21
  refuses, or sends a filter that is proven false (NR-43).
- **NR-26.** With Tor on, every connection goes through the SOCKS5 proxy
  the caller names, and no DNS seed is asked directly. Peers reached by
  IP address through Tor are grouped by that address like any other.

## 4. Headers

- **NR-30.** After the handshake the counter asks every peer for headers
  from its own chain (`getheaders` with the watcher's block locator), not
  one chosen peer. It asks again while a peer answers with a full message
  of 2,000 headers.
- **NR-31.** Every header received goes to the watcher's header store
  (WR-05 to WR-07) with the caller's clock. The counter's chain is the
  store's chain; this crate keeps none of its own.
- **NR-32.** A header with an unknown parent makes the counter ask that
  peer again from its locator, at most once per minute per peer.
- **NR-33.** For each peer the counter records the highest block the peer
  has shown: the start height it claimed, or the height of the highest
  header it sent that the store accepted, whichever is greater. This is
  what WR-52 reads. A peer that claims a height more than 2 blocks above
  the tip and does not deliver the headers within 10 minutes is counted
  as ahead for that rule; it is not dropped for it.

## 5. Blocks and filters

- **NR-40.** For every block that joins the best chain after the counter
  started watching, the counter asks for the block's basic compact filter
  from two peers in different network groups.
- **NR-41.** A filter is matched against every script the counter is
  watching: every address it has shown for a sale, and those not yet
  used in its current batch.
- **NR-42.** The counter downloads the full block when either filter
  matches, when the two filters are not the same bytes, or when fewer
  than two peers answered within 60 seconds **[to tune]**.
- **NR-43.** A downloaded block is handed to the watcher (WR-20 to
  WR-22). If the block pays a watched script and a peer's filter for it
  did not match, that peer sent a false filter (NR-25).
- **NR-44.** While any sale has a payment that is seen and not yet
  confirmed, the counter downloads every new block in full, whatever the
  filters say. A transaction that spends the payment's coins and pays the
  shop nothing is in no filter the counter can match, because the counter
  does not know the scripts of the customer's coins. Reading the whole
  block is the only way to see it (WR-42).
- **NR-45.** A block is asked of one peer at a time, with 60 seconds to
  answer **[to tune]**, then of another. A block is identified by its
  hash on the header chain; a peer cannot substitute another.
- **NR-46.** When the best chain changes, the blocks that joined are
  treated as new under NR-40 to NR-44.

## 6. Unconfirmed transactions

- **NR-50.** Every peer's transaction announcements (`inv`) are recorded
  as: this peer says it has this transaction.
- **NR-51.** The counter asks for every announced transaction it does not
  have, from the first peer that announced it, with 30 seconds to answer
  and then from the next. It does this all the time by default, not only
  while a sale is open. Asking only during sales would tell every peer
  when the shop makes a sale.
- **NR-52.** A shop can choose "less data": transactions are then fetched
  only from the opening of a sale until its payment confirms or the sale
  ends, and for 10 minutes after. The settings screen says what this
  tells peers.
- **NR-53.** A fetched transaction is handed to the watcher as a sighting
  from every peer that announced it (WR-40). A later announcement of the
  same transaction by another peer is a further sighting.
- **NR-54.** Transactions the watcher ignores are forgotten at once. The
  counter remembers the IDs of the last 100,000 announced transactions
  **[to tune]** so that it does not fetch one twice; it keeps no mempool.
- **NR-55.** The counter never announces or sends a transaction. It does
  not answer `getdata`, `getheaders`, `getaddr` or `mempool`.

## 7. What peers and the network learn

- **NR-60.** No message the counter sends holds an address or a script
  of the shop's. The only transaction IDs it sends are in `getdata` for
  transactions a peer announced, and under NR-51 it asks for all of them
  alike.
- **NR-61.** A peer that is asked for a full block under NR-42 learns
  that the counter was interested in that block. NR-44 and the
  two-filter rule mean many downloaded blocks hold nothing for the shop;
  the rule does not hide the interest completely.
- **NR-62.** Connections are not encrypted in this version (the BIP324
  transport is not built yet). Someone who can read the counter's
  traffic sees that it is a Bitcoin client and which blocks it
  downloads. With Tor on, Tor encrypts the path to the exit.

## 8. What these rules do not do

- **They do not stop a counter from being surrounded.** If every peer the
  counter can reach belongs to one attacker, the attacker chooses what
  the counter sees. Distinct network groups, stored long-lived peers and
  Tor make that harder. The watcher's rules then limit the damage: a
  confirmation still needs real proof of work, and the health rules turn
  instant acceptance off when the chain stops moving.
- **They do not check that a filter is complete.** A false filter can
  hide a payment until a second peer or a full block shows it (WR-23).
- **They do not relay.** The counter gives nothing back to the network.
  A shop that wants to support the network runs the Box's full node.
