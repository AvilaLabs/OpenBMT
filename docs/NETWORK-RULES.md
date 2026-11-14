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
  zero, a fresh random nonce, an empty user agent, the local tip height and
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
  the caller supplies, from groups that have no connection now. The driver
  reads that randomness, and the nonces and keys of NR-11 and NR-80, from the
  kernel. When the kernel gives nothing, the step that needed it is not
  taken: no dial, no connection, no lookup. The driver says so in a note,
  and never makes up a number from the clock.
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
  the caller names, and no name reaches the system's resolver. The seeds'
  filter names are looked up through Tor itself (NR-72). Peers reached by
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
  While NR-44 holds, NR-104 shortens the 60 seconds.
  A block is identified by its hash on the header chain; a peer cannot
  substitute another. At most 8 blocks are in work at once.
- **NR-46.** When the best chain changes, the blocks that joined are
  treated as new under NR-40 to NR-44. The checked height is kept with the
  hashes of the blocks at that height and up to 99 below it. At start, if
  the block at the checked height is no longer
  on the stored chain (the chain changed while the device was stopped, or
  after the headers were stored and before the change was handled), the
  checked height goes down to the highest of the kept blocks that is still
  on the chain, or below all of them when none is, so the blocks that took
  its place are checked as new.
- **NR-47.** The app may ask for one block of the best chain by its hash,
  whatever the filters say: no filter is asked for it, and it is fetched as
  NR-45 fetches any block (one peer at a time, 60 seconds each, a peer that
  has shown the block first). It is handed out apart from the blocks of
  NR-43, as a block the app asked for, after the same checks of the block
  on its own (WR-20 to WR-22) and only while the chain still holds it
  (WR-01). It is not part of the checks of NR-40 to NR-46: it moves
  neither the checked height nor any work item, and a payment in it is not
  matched against the watched scripts. The owner's device uses it to take a
  fee sample from a block that matched no filter (`docs/FEE-RULES.md`,
  FE-20). At most one such block is in work: asking for another hash gives
  up the first, whose late answer is tolerated like any request given up
  (the module note on stale requests), and asking for the same hash again
  changes nothing. A hash that is not on the best chain is refused and told
  to the app. A block asked for that leaves the chain is given up. The peer
  that is asked learns that the app was interested in that block, as under
  NR-61.

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
  and forgotten. The counter remembers each announced transaction ID for
  30 minutes from its first announcement, and at most 20,000 IDs, so that
  it does not fetch one twice while peers are still passing it on; it keeps
  no mempool. An ID whose request is still out stays until the request
  ends. An ID announced again after it is forgotten is fetched again and
  handed on as a new sighting.
- **NR-55.** The counter never announces or sends a transaction, except
  one the app asked it to broadcast (NR-86). It does not answer
  `getheaders`, `getaddr` or `mempool`, and it answers `getdata` only for
  a transaction it holds for broadcast.

## 7. What peers and the network learn

- **NR-60.** No message the counter sends holds an address or a script
  of the shop's. The only transaction IDs it sends are in `getdata` for
  transactions a peer announced, and under NR-51 it asks for all of them
  alike.
- **NR-61.** A peer that is asked for a full block under NR-42 learns
  that the counter was interested in that block. NR-44 and the
  two-filter rule mean many downloaded blocks hold nothing for the shop;
  the rule does not hide the interest completely.
- **NR-62.** A connection that uses the old transport (v1) is not
  encrypted. Someone who can read the counter's traffic on it sees that
  it is a Bitcoin client and which blocks it downloads. A connection
  that uses the encrypted transport (§10) hides that from someone who
  only listens. With Tor on, Tor encrypts the path to the exit.

## 8. What these rules do not do

- **They do not stop a counter from being surrounded.** If every peer the
  counter can reach belongs to one attacker, the attacker chooses what
  the counter sees. Distinct network groups, stored long-lived peers and
  Tor make that harder. The watcher's rules then limit the damage: a
  confirmation still needs real proof of work, and the health rules turn
  instant acceptance off when the chain stops moving.
- **They do not check that a filter is complete.** A false filter can
  hide a payment until a second peer or a full block shows it (WR-23).
- **They do not trust the exit that answers a lookup.** With Tor on, the
  exit that answers a seed lookup can lie. That costs a dial and nothing
  else: what a peer says about filters is compared across two network
  groups (NR-42), and a peer that lies is dropped (NR-25).
- **They do not make onion groups as strong as IP groups.** The 16 groups
  of NR-93 are cut from the first 4 bits of a key that its operator
  chooses. One operator can run onion peers in every one of the 16 groups
  at no cost, where an IP address in a group costs one address in it. Two
  onion peers in two groups are not evidence of two operators.
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
- **NR-72.** The seeds are asked when the book has nothing to dial, and
  also while filters are wanted (fewer than two ready filter peers in
  different groups) and the book holds no address that can be dialed now
  whose filter service was stated by a seed, the owner or the peer itself
  (NR-21). In that second case the book's other addresses go on being
  dialed meanwhile: the round waits for them no more than they wait for it.
  Without Tor, each seed name is looked up in a thread of its own; an
  answer that takes more than 10 seconds is ignored, and a round is
  started at most every 5 minutes. What comes back enters the book as
  learned from a seed. Each seed is also asked for nodes that state the
  compact-filter service (the `x49.` prefix); those enter the book with
  that service stated by a seed (NR-21). Without two filter peers every
  block is downloaded in full (NR-42). The default names are the ones
  Bitcoin Core 32.1 ships. With Tor on (NR-26), no name reaches the
  system's resolver. A round, at the same spacing, asks each seed's
  filter name through the proxy's own lookup command (Tor's `RESOLVE`):
  three lookups a seed, each on its own circuit (NR-78), each giving one
  address, at most 4 at a time. What a lookup returns enters the book as
  stated by a seed to offer filters, as without Tor, unless it is not
  publicly routable (NR-22). A proxy that cannot do this, or a round that
  gave no address, is noted once, and one seed name at a time is dialed
  through the proxy as an ordinary peer, as before. A proxy that answers
  "command not supported" is not asked again while the driver runs.
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
  events to the app are bounded: the second by the number of events (64)
  and by the bytes of the blocks and transactions in them (32 MiB). The
  driver counts those bytes when it hands an event over, and the count
  goes down when the app takes the event. When the app does not read its
  events, the driver stops reading from the peers, and the peers wait on
  their sockets, instead of the driver holding more in memory. An event
  larger than the byte limit is still handed over once the app has taken
  the others, so that one large block never stops the driver for good.
- **NR-75.** The app is told the ready peers, whether a peer is ahead
  and the tip whenever one of them changes and at least every 10 seconds,
  so that it can run the health rules (WR-50 to WR-52). It is told the
  height up to which every block has been checked whenever that rises;
  it stores the height and starts from it next time, so that a restart
  does not check the same blocks again. It is told of every peer that
  goes, with the reason.
- **NR-76.** A Tor v3 address in the book is dialed only as NR-92 says.
  Before NR-92 it was kept and never dialed.
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
  The file is made empty if it is missing and is never removed. The lock
  does not cover a directory on a network file system, where locks may not
  work, or a directory copied from another: a copy has its own lock file
  and is not seen as the same directory. A directory is copied only after
  the program that uses it has stopped.

## 10. Encrypted transport

The counter speaks the encrypted transport of BIP324 (v2) to peers that
offer it, so that someone on the shop's network or on the path cannot read
what the counter asks its peers for, and cannot change it without the peer
noticing. It speaks the old transport (v1, section 1) to the others. The
message-level rules above are the same on both: a session hands a message
to the transport and gets bytes, and feeds it bytes and gets messages.

This is protection against someone who listens. It is not protection
against someone who changes what is sent, and it does not tell the counter
who the peer is: the key exchange is not authenticated, as in BIP324
itself. The counter's cryptography is the `bip324` crate's; the counter
writes none of it.

- **NR-80.** The counter tries v2 first on every outbound connection: to
  a peer whose services, from the address book or a seed, include
  `NODE_P2P_V2` (bit 11), to a fixed peer, and to a peer whose services
  are not known. It goes straight to v1 only to a peer that stated its
  own services in its `version` without that bit, and to a peer that
  NR-81 names. What a seed or another peer says about a peer's services
  does not keep the counter from trying v2: someone could use that to
  hold the counter to v1. The counter sends 64 bytes of key and no
  garbage; the key and the 32 bytes that randomize its encoding come from
  the caller, as the nonce does (NR-11), and nothing in the crate draws
  random numbers. Its `version` follows the peer's version packet.
- **NR-81.** If the v2 handshake fails before any encrypted packet of the
  peer was read, because the peer closed the connection or answered with
  the start of a v1 message (the network magic and `version`), the counter
  connects to that peer once more at once with v1, and the address book
  remembers for that peer that v2 failed, for 24 hours **[to tune]**
  (kept in memory: a restart forgets it). After an encrypted packet was
  read there is no way back to v1: a failure then is a dropped peer, with
  no ban and no mark against it (NR-25 does not apply), just closed. An
  attacker on the path can force v1 this way, as BIP324 itself says:
  v2 here protects against a passive listener, not against an active one.
  Which transport each peer uses is shown to the app (NR-82), so that the
  shop's screen can say so.
- **NR-82.** The list of peers the app is given (`health_peers` and the
  `Peers` event) names the transport of each peer, v1 or v2. No health
  rule reads it.
- **NR-83.** Limits. The peer's garbage is at most 4,095 bytes, then the
  terminator, or the peer is dropped. Before its version packet a peer may
  send at most 8 decoy packets of at most 4,096 bytes of contents each.
  After the handshake a packet holds at most 4,000,013 bytes of contents,
  which is the largest message of NR-02 and the 13 bytes that name its
  command; the length is checked from the 3 length bytes, before any of
  the packet is kept. The limits of NR-02 and NR-03 apply to the payload
  of the message in a packet. Decoy packets are read and ignored, and
  their bytes are counted: more than 1,000,000 in all drops the peer
  **[to tune]**. A packet that fails its authentication drops the peer at
  once.
- **NR-84.** The session ID of v2, which both ends derive and nobody
  else can, is available from the session once the peer's version packet
  has been read (`Session::v2_session_id`), so that it can later be
  compared out of band. Nothing uses it yet.
- **NR-85.** With Tor on (NR-26) the same rules apply: v2 inside the
  connection through the proxy is allowed and is not required.

## 11. Broadcasting a transaction

The owner's device sends the transactions the Key app approved. It is
the one place where this crate gives a transaction to the network.

- **NR-86.** The app hands a finished transaction to the manager with
  `broadcast`. The manager holds it until the app says `forget`: at most
  64 at a time, each at most 400,000 weight units (the signer's own
  limit). Giving one that is held already changes nothing. A held
  transaction is announced with `inv` by its txid to half of the ready
  peers, at least one, by lowest connection number. The counter does not
  ask for wtxid relay, so a peer announces and asks by txid too. The
  manager answers `getdata` for a held transaction with the whole
  transaction, witness included, to any peer that asks. A peer that
  becomes ready while a transaction is held is announced to at once.
  Nothing else is announced or sent.
- **NR-87.** A held transaction counts as seen back when peers that were
  not sent its announcement announce it to the counter, from at least 2
  network groups. The same group twice counts once, and a peer that was
  sent the announcement counts for nothing. A transaction that is
  announced back is never asked for. The manager says `Sent` until then,
  and `SeenBack` with the number of groups after. This is evidence that
  the network relayed it. It is not evidence that anyone will mine it:
  a peer can announce a transaction and drop it, and peers that look
  alike can be one operator. A block that holds the transaction is the
  only proof that it was mined, and the app reads that from its own
  checked blocks, not from this crate.
- **NR-88.** A held transaction that has not been seen back 10 minutes
  after it was announced is reported as `NotSeen` and announced to every
  ready peer again. The same happens every 10 minutes after, until it is
  seen back or the app lets it go. The manager never drops a held
  transaction by itself. Once every ready peer has been sent the
  announcement, there is no peer left to see it back from; a block is
  then the only evidence.
- **NR-89.** Peers send no `reject` message that the counter reads, and
  the counter invents none: a peer that fetched a transaction and
  discarded it, or never fetched it, looks the same as a peer that is
  slow. The counts the manager reports (peers asked, peers that fetched)
  are what happened on the wire and nothing more.
- **NR-90.** The driver passes `broadcast` and `forget` to the manager,
  and tells the app the counts and the state as events. It also takes a
  `rescan` of a height: the manager gives up the blocks in work and
  checks every block above that height again against all the scripts
  watched then (NR-40, NR-41). After applying the command, it sends
  `RescanStarted(height)` in the same ordered event stream, before progress
  from the restarted work. An app uses it after it has added scripts whose
  payments may be in blocks it has checked.
- **NR-91.** The owner's device sends over a driver of its own, apart from
  the one that watches the wallet, with its own directory, lock and
  peers. With a proxy set (NR-26), that driver reaches every peer
  through it. With no proxy, the app refuses to send, says so, and holds
  the transaction, unless the owner has turned on sending without Tor;
  every send then says that the wallet's own connections were used. It
  never falls back to the wallet's connections without saying so.
- **NR-97.** The driver takes `unwatch_scripts` with a list of scripts: the
  manager stops watching them, so that they no longer count towards the
  limit of NR-41. A script that is not watched is passed over. Filters
  checked after the command are matched against the scripts that are left;
  work in progress keeps what it matched. An app that wants such a script
  checked again for the blocks it missed adds it with `watch_script` and
  asks for a `rescan` (NR-90). The owner's device uses it to keep its watch
  set within the limit (`docs/OWNER-COINS-RULES.md`, OC-22 to OC-27).

## 12. Onion peers

With Tor on, the counter also dials Bitcoin nodes that are Tor v3 hidden
services. A path to one never leaves the Tor network, so no exit sees it.
This is a path to peers that does not depend on the clear-net exits.

- **NR-92.** A Tor v3 address of the book is dialed only through the
  proxy of NR-26, never directly, by asking the proxy to connect to the
  host name of the address. The name is the lower-case base32, without
  padding, of the 32-byte public key, a 2-byte checksum and the version
  byte 3, followed by `.onion`: 56 characters and the suffix. The checksum
  is the first 2 bytes of SHA3-256 of `.onion checksum`, the key and the
  byte 3. Without a proxy a Tor address is never drawn from the book (the
  draw of NR-23 leaves it out) and never dialed: it stays in the book. The
  name goes to the proxy as a host name, as NR-26 allows for a name, and
  is not looked up by anything else. The counter does not check that a
  name it has dialed is the key the peer proves: Tor does that. The driver
  tells the app, without the address, each time it starts an onion dial
  and each time one fails, with the proxy's reason.
- **NR-93.** An onion peer's network group (WR-41) is the Tor network and
  the first 4 bits of its public key: 16 groups, apart from every IP group
  and from the single group of the peers reached by name (NR-26). A peer
  reached through the proxy by an IPv4 or IPv6 address keeps its group by
  that address. Two onion peers whose keys start with different 4 bits are
  in two groups, so they can be the two peers in different groups of NR-20
  and NR-42. Two in one group count once. This is how Bitcoin Core groups
  Tor addresses. The groups are weaker than the IP groups against one
  operator (see section 8): the key of an onion address is chosen by the
  one who runs it, and an operator can make as many keys as it likes and
  keep the ones that start as it wants.
- **NR-94.** With the proxy set, when the book holds an onion address that
  may be dialed, at most half of the connections (the target number
  divided by 2, rounded down, and at least 1) go to onion peers, those
  open and those being opened together. While fewer do, and the book holds
  both kinds of address that may be dialed, the draw of NR-23 chooses
  between an onion address and another with equal chance, and then
  as NR-23 says. So the clear-net exits are not the only path to peers,
  and the onion path is not the only one either. The cap and the equal
  chance are **[to tune]**. The seeds give no onion addresses (NR-72); they
  come from `addrv2` (NR-95).
- **NR-95.** An onion address comes from an `addrv2` message (BIP155), the
  only form that can carry one. The counter sends `sendaddrv2` before its
  `verack` (NR-13) and reads `addrv2`: an entry of the network kind Tor v3
  with a 32-byte key is an address like any other (NR-22), and the book
  keeps it with the services the peer claimed for it. An entry of a kind the
  book does not keep, a Tor v3 entry with a key of another length, and an
  `addr` entry with an old Tor address are not kept.
- **NR-96.** Each dial to an onion peer gives the proxy a fresh random name
  and password, as every other dial does (NR-78): two onion peers, or an
  onion peer and a clear-net peer, are not reached through one circuit.

## 13. Your own node

The merchant may run a Bitcoin Core node of their own (on a Box, Start9,
Umbrel or at home) and let the counter check payments against it as well
(`docs/DESIGN.md` §5.1, §6). The counter reaches it over the peer protocol
like any peer, with the differences below. It is one more check next to the
light client, never instead of it: the random peers of sections 3 to 6 stay,
and what the node reports is kept apart from what they report
(`docs/WATCHER-RULES.md`, WR-60 to WR-65).

- **NR-98.** The own node is one Bitcoin node reached at a Tor v3 address
  that the owner set: the host name of NR-92 and a port. The address is what
  authenticates it, because the name of a v3 address is the node's public key
  and the proxy reaches that key or nothing. The driver's settings take at
  most one own node, as that name and port. A name that NR-92 does not
  accept (wrong length, a character outside the alphabet, upper case, a
  wrong checksum or version), a text that is an IP address or any other
  kind of name, and port 0 are refused when the settings are made, and the
  start is refused when the own node is a Tor address and no proxy is set
  (NR-26). Nothing is looked up: the name goes to the proxy as a name
  (NR-92). Only one own node is connected at a time.
- **NR-99.** A LAN, loopback or clear-net address is never the own node, and
  nothing about a connection to one is labelled or trusted as the own node:
  an address on the owner's network says nothing about who answers, so a
  node there is, at most, a fixed peer (NR-70), and is then one of the
  random peers in every respect. The own node's address is the other way
  round: it is never dialed as a peer, from the anchors, the fixed peers or
  the book, so the own node is never also one of the random peers. The label
  "own node" is given by one thing:
  an `OwnNode` made from a Tor v3 name. There is one other way to make one,
  for the driver's check against Bitcoin Core on regtest: a constructor that
  takes a socket address on this machine. It is compiled only with the cargo
  feature `clear-own-node-for-tests` of `openbmt-net`, which is off by
  default, is turned on by no program, and is turned on in the workspace
  only by the dev-dependency of the driver, so no build of a program holds
  the constructor and no setting can reach it. A test of the driver reads
  every `Cargo.toml` and every source file of the workspace and fails if the
  feature or the constructor is named anywhere else.
- **NR-100.** The driver dials the own node at the start, before the anchors
  and the fixed peers, and keeps it connected for as long as it runs. After a
  failed dial or a closed connection it waits 5 seconds, then 10, doubling up
  to 5 minutes between two dials, and starts again from 5 seconds when the
  connection that ended had been ready for a minute. The own node takes no
  place of the target number of peers (NR-20), no network group, no filter or
  block work (NR-40 to NR-47) and no pause of NR-70; the address book and the
  anchors never hold it. It is not dropped for what drops a peer: a request
  that runs out (NR-42, NR-51), a claim of a height without the headers
  (NR-33), or a group it shares with a peer (NR-20). It is dropped for the
  timers of NR-14 and for a protocol violation, and a violation (NR-25: a
  message that breaks sections 1 and 2, a block, a filter or a transaction
  that was not asked for) is not retried by itself: the driver reports it
  with the reason and does not dial the node again until the app calls
  `retry_own_node`. The app tells the owner (WR-64).
- **NR-101.** What the own node reports is whether it holds a transaction in
  its memory pool, and nothing else. It holds it when it announced it with
  `inv`, or when it sent it after the app asked about it with
  `ask_own_node` (a `getdata` by txid). The manager remembers the last
  50,000 transactions it announced and the last 256 the app asked about
  and has no answer for yet; it asks again at every connection and when the
  node announces one. A request that is not answered within 30 seconds is
  let go, and the node is not dropped for it. No answer is not a "no": a
  node does not serve a transaction it has not yet announced to the counter,
  so the counter takes only a positive report and says nothing about the
  rest. A transaction from the own node is never a sighting (NR-53): the
  manager does not fetch what it announces (NR-51), and the light client's
  sightings come from the random peers alone.
- **NR-102.** The own node's tip is the best height it has shown: its
  statement in `version`, and then every block it announced or sent a header
  for that the counter's chain holds (a block the chain does not hold yet is
  remembered, 8 at most, and placed when the chain holds it). The manager
  also keeps when anything last came from it. The driver passes these, with
  the state of the connection and when it ended, to the app (NR-103), and the
  watcher's WR-63 says what they mean. The counter does not ask the node for
  headers or blocks, and a header it sends is not taken into the counter's
  chain: the header chain comes from the random peers alone.
- **NR-103.** The driver tells the app about the own node on a channel of its
  own, never in the events of the peers: `Has` for a transaction it holds
  (NR-101), and its state whenever the connection or its best height changes
  and at least every 10 seconds (NR-75). The own node is not in the peers of
  that event, so the light client's health (WR-50 to WR-52) never reads it.
  The channel holds 256 events and the core thread never waits for it: when
  it is full a new event is dropped. A state comes again within 10 seconds,
  and a report that was dropped is asked for again; a dropped report is
  never taken for a "yes", and the counter falls back to the light check
  while it is missing (WR-64). Without an own node nothing is sent.
- **NR-104.** While NR-44 holds, a sale is waiting for the blocks, so a block
  that has not arrived 10 seconds after it was asked for is asked of another
  peer at once, without waiting for the 60 seconds of NR-45 **[to tune]**. The
  first request is not cancelled: the peer is counted as unanswered (NR-42)
  once the 60 seconds are over, as before, and a block that comes late is
  dropped as one that was given up. A peer that lets two requests in a row
  pass 10 seconds is asked last for blocks, until one of its blocks arrives.
  When every connected peer has failed to give the block, the counter waits 10
  seconds and starts over. Without NR-44 nothing changes: 60 seconds each.

## 14. Only your own node

A shop that runs its own Bitcoin Core node can choose to check every
payment against that node alone, with no random peers at all. The node
checks every rule of Bitcoin itself, and nobody else learns which addresses
the shop watches. Section 13's own node, a second check next to the light
client, stays as it is for shops that do not choose this. Bitcoin Core is
the node these rules are written and tested against.

- **NR-105.** Three fields of the shop's signed settings
  (`openbmt_core::settings::Settings`) say how the shop's devices reach the
  network: `own_node`, the node's Tor v3 name and port as NR-98 accepts
  them, or none; `node_only`, which may be on only when `own_node` is set;
  and `light_fallback`, which may be on only when `node_only` is. Settings
  that break either condition are not valid. Every device of the shop, the
  counters, the owner phone and its background watcher, takes the node and
  the mode from the settings in force; the counter's own setting of an own
  node (its app configuration) is not used once the settings name one.
- **NR-106.** With `node_only` on, the driver dials the own node and nothing
  else: no anchors, no fixed peers, no address book, no seeds and no onion
  peers. The own node is then the only peer, and it serves the headers,
  filters, blocks and transactions that sections 4 to 6 ask of peers. The
  checks of those sections still apply to what it sends: a header chain is
  checked for its proof of work and difficulty as from any peer, a block
  must match its header, a filter must be the one its header commits to
  where the counter can check it. A node can withhold; it cannot make work
  it did not do.
- **NR-107.** A node that does not advertise compact filters
  (`NODE_COMPACT_FILTERS`, BIP 157) and blocks (`NODE_NETWORK` or
  `NODE_NETWORK_LIMITED`) in its `version` cannot serve a device in this
  mode. The device says so, names the two Bitcoin Core settings it needs
  (`blockfilterindex=1`, `peerblockfilters=1`), and does not count the
  node as usable (WR-63).
- **NR-108.** A block older than the node serves (a node that keeps only
  recent blocks serves the last 288) is not fetched elsewhere. A device that
  needs one, after being off for days, says which block it lacks and that
  the node must keep blocks that far back, and waits; with
  `light_fallback` on, NR-110 applies.
- **NR-109.** Transactions go out only to the own node in this mode:
  refunds, cash-outs and every broadcast of section 11.
- **NR-110.** When the own node cannot be used (WR-63), a device in this
  mode without `light_fallback` dials no one else: sales wait, nothing is
  accepted at once, and the line says so (WR-67). With `light_fallback` on,
  the driver starts the random peers of sections 3 to 6 as if `node_only`
  were off, says so on the line, and stops them when the own node has been
  usable for 2 minutes **[to tune]**. Nothing falls back without the line
  saying it.
- **NR-111.** What the network learns in this mode: the own node learns
  what the shop's devices ask of it, as any peer would; no other node
  learns anything from them. The node's own connections to the network are
  the node's.
- **NR-112.** In this mode no other peer can announce a held transaction
  back (NR-87), so the own node is asked instead. At least 130 seconds
  after the node was sent the announcement, and every 60 seconds after
  until it answers, the manager sends the node `getdata` for the
  transaction by its txid. The node answers only from its memory pool, and
  only once the transaction has waited out its two-minute relay delay, so
  a reply that is the same transaction, witness included, means the node
  holds it in its pool. The manager then says `HeldByNode`; a `notfound`
  changes nothing and the asking goes on. NR-88 applies as before, with
  the announcement sent again to the node alone. While the light fallback
  runs (NR-110), NR-87 counts the random peers as well, and either state
  may come first. As in NR-87, this is evidence that the node took the
  transaction, not that anyone will mine it.
