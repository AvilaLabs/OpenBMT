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
  compact-filter service, when the address book has them. What one peer
  says about another's services earns no preference: only what a seed or
  the peer itself has said.
- **NR-22.** Addresses come from the network's DNS seeds, from `addr` and
  `addrv2` messages, and from the counter's own stored address book. The
  book holds at most 2,000 addresses and at most 8 per network group, so
  that one peer cannot fill it with its own, and at most 64 learned from
  the peers of any one network group, so that one peer cannot fill the
  book with addresses it made up. Only publicly routable addresses are
  kept.
- **NR-23.** The next address to dial is drawn at random, with randomness
  the caller supplies, from groups that have no connection now.
- **NR-24.** The two peers that have been connected longest without
  fault are stored and dialed first after a restart, so a restart is not
  a chance to surround the counter with new peers.
- **NR-25.** A peer is dropped and not dialed again for 24 hours when it
  breaks a rule of sections 1 or 2, sends a header that WR-05 refuses
  (other than for an unknown parent), sends a block that WR-20 or WR-21
  refuses, sends a filter that is proven false (NR-43), sends a block,
  transaction or filter that was not asked of it, or claims a height it
  sends no headers for (NR-33).
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
  header it sent that the store accepted, or the height of a block on
  the counter's best chain that it announced with `inv`, whichever is
  greater. An announced block that the store does not hold yet is
  remembered (the last 8 for each peer) and counts once its header has
  joined the best chain, whichever peer the header came from. This is
  what WR-52 reads. The counter also notes when the
  height last rose (NR-40). A peer that claims a height more than 2
  blocks above the tip and sends no headers for it within 10 minutes is
  dropped (NR-25): the claim told the counter nothing, and one such peer
  must not be able to stop the shop. A peer that did send headers which
  the store could not take for a reason that is not the peer's doing
  (WR-06, WR-07) is kept and counts as ahead for WR-52. That is the case
  the rule is for: the counter's clock or chain may be the one that is
  wrong.

## 5. Blocks and filters

- **NR-40.** For every block that joins the best chain after the counter
  started watching, the counter asks for the block's basic compact filter
  from two peers in different network groups. Only a peer that has
  itself shown the block (NR-33) is asked: a node drops a peer that asks
  about a block it does not know, and a node whose filter index has not
  caught up sends nothing. A peer whose shown height is above the block
  is asked at once; a peer whose newest block it is is asked 2 seconds
  after it showed it **[to tune]**. The filter peers that may be asked
  take turns by block height, so that no two peers are asked about every
  block.
- **NR-41.** A filter is matched against every script the counter is
  watching: every address it has shown for a sale, and those not yet
  used in its current batch.
- **NR-42.** The counter downloads the full block when either filter
  matches, when the two filters are not the same bytes, or when fewer
  than two peers answered within 60 seconds **[to tune]**, counted from
  when the block entered the filter stage. A peer that was never asked
  is not counted as one that did not answer. A filter that
  cannot be read counts as a match. A peer that lets three requests in a
  row run out (filters, blocks or transactions) is dropped, without the
  24 hours of NR-25. An answer that comes after the counter gave up on
  it is ignored and is not "unasked" data.
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
  answer **[to tune]**, then of another. A peer that has shown the block
  is preferred to one that has not. When every connected peer has
  failed to give a block, the counter waits 60 seconds and starts over.
  A block is identified by its hash on the header chain; a peer cannot
  substitute another. At most 8 blocks are in work at once.
- **NR-46.** When the best chain changes, the blocks that joined are
  treated as new under NR-40 to NR-44.

## 6. Unconfirmed transactions

- **NR-50.** Every peer's transaction announcements (`inv`) are recorded
  as: this peer says it has this transaction.
- **NR-51.** The counter asks for every announced transaction it does not
  have, from the first peer that announced it, with 30 seconds to answer
  and then from the next. It does this all the time by default, not only
  while a sale is open. Asking only during sales would tell every peer
  when the shop makes a sale. At most 5,000 requests are out at once, and
  at most 1,000 of them with one peer, so that a peer which announces
  much and answers nothing cannot hold up the others.
- **NR-52.** A shop can choose "less data": transactions are then fetched
  only from the opening of a sale until its payment confirms or the sale
  ends, and for 10 minutes after. The settings screen says what this
  tells peers.
- **NR-53.** A fetched transaction is handed to the watcher as a sighting
  from every peer that announced it (WR-40), at most 8. A later
  announcement of the same transaction by another peer is a further
  sighting, reported by itself.
- **NR-54.** This crate keeps no transaction: a fetched one is handed on
  and forgotten. The counter remembers the IDs of the last 100,000
  announced transactions **[to tune]** so that it does not fetch one
  twice; it keeps no mempool.
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

## 9. What the driver decides

The driver (`crates/openbmt-net-io`) owns the sockets and the threads. These
rules are what it decides where the rules above are silent.

- **NR-70.** An address that closed, failed or was refused is not dialed
  again for 30 seconds. A fixed peer (one the caller names) is dialed again
  after the pause, for as long as the driver runs. A fixed peer or seed
  name that breaks a rule (NR-25) is not dialed for 24 hours; the driver
  keeps that in memory, because the address book does not hold addresses on
  this machine.
- **NR-71.** The next address to dial is, in this order: a stored anchor
  (NR-24), a fixed peer, an address drawn from the book (NR-23), and
  last the seeds (NR-72). At most 4 connections are being opened at once
  and at most the target number of peers plus 4 sockets exist. The groups
  of connections being opened count as connected for the draw. A
  connection that the manager refuses (NR-20) is closed at once.
- **NR-72.** The seeds are asked only when the book has nothing to dial.
  Without Tor, each seed name is looked up in a thread of its own; an
  answer that takes more than 10 seconds is ignored, and a round is
  started at most every 5 minutes. What comes back enters the book as
  learned from a seed. Each seed is also asked for nodes that state the
  compact-filter service (the `x49.` prefix); those enter the book with
  that service stated by a seed (NR-21). Without two filter peers every
  block is downloaded in full (NR-42). The default names are the ones
  Bitcoin Core 32.1 ships. With Tor on (NR-26), no name is looked up: one seed
  name at a time is dialed through the proxy as an ordinary peer.
- **NR-73.** The header store is saved when the tip has moved and 60
  seconds have passed since the last save, and always before a block is
  handed to the app if the tip has moved since the last save: what the
  app stores about a block must never be ahead of the stored headers,
  or a crash would make a confirmed payment look like one whose block
  left the chain. The anchors are set to the two
  ready peers connected longest, and the book is saved, every 10 minutes.
  Both are saved, and the anchors set, when the driver stops. A stored
  file that does not read back is replaced by an empty one and reported to
  the app. Storage that cannot be read at all stops the start.
- **NR-74.** The channel from the peers to the driver and the channel of
  events to the app are bounded. When the app does not read its events,
  the driver stops reading from the peers, and the peers wait on their
  sockets, instead of the driver holding more in memory.
- **NR-75.** The app is told the ready peers, whether a peer is ahead
  and the tip whenever one of them changes and at least every 10 seconds,
  so that it can run the health rules (WR-50 to WR-52). It is told the
  height up to which every block has been checked whenever that rises;
  it stores the height and starts from it next time, so that a restart
  does not check the same blocks again. It is told of every peer that
  goes, with the reason.
- **NR-76.** A Tor v3 address in the book is kept but not dialed in this
  version: the driver does not yet turn it into a host name for the
  proxy.
- **NR-77.** Stopping saves the data, closes every socket and ends the
  driver's threads within 5 seconds. A thread that is still in a connect or
  a lookup at that time holds no data and is left to end by itself.
- **NR-78.** With Tor on, each connection gives the proxy a fresh random
  name and password. Tor puts connections with different ones on
  different circuits, so two peers are not reached through one exit. A
  proxy that asks for no name and password is used as it is.
- **NR-79.** One process uses a data directory at a time. Whatever owns
  the directory (the counter's node, the owner's watcher, an example that
  runs the driver alone) takes an exclusive lock on the file `lock` in it
  before it reads anything, and keeps it until it stops. A second start on
  the same directory does not wait: it fails with "another OpenBMT process
  is using" the directory, and touches nothing else in it. The driver's
  storage shares the directory with its owner and does not lock it again.
  The system lets go of the lock when the process ends, however it ends,
  so a crash leaves no stale lock and the file holds no process number.
