# openbmt-net

The OpenBMT Bitcoin peer client: wire framing, single-peer sessions, an
address book and a manager of several peers. It is a Rust library with no
sockets, clock, threads, async or randomness of its own. The caller supplies
received bytes, time and entropy, and takes out bytes to send and events.
The manager uses `openbmt-watch` to check headers, filters and blocks, tracks
the merchant's own node, and can broadcast transactions supplied by the app.

The specification is [docs/NETWORK-RULES.md](../../docs/NETWORK-RULES.md).
Each rule has an ID (NR-01 and on). Every drop names its rule
(`Drop::rule()`, and its text starts with the ID), and every test is named
for the rule it proves.

**This crate has had internal review only. No independent security audit has
been done yet.** Do not use it with real funds.

The implemented modules cover framing and sessions, the address book,
multi-peer scheduling and checked chain data, BIP324 encrypted transport,
app-requested broadcasting, and a separate own-node connection. The socket
driver is implemented in [`openbmt-net-io`](../openbmt-net-io/), which also
supplies the clock, entropy, storage and address-book integration.

## The calls

| Call | What it does | Rules |
|---|---|---|
| `FrameReader::new`, `push`, `next` | Cuts a byte stream into messages. Decides from the 24-byte header alone: magic, command, length against the limit. Skips commands it does not read. | NR-01 to NR-03, NR-05 |
| `FrameReader::set_handshake_done` | From now on the commands that belong after the handshake are read; before, one that arrives is a drop at its header. | NR-13 |
| `Frame::parse` | Reads a payload as its command and refuses one that leaves bytes over, or holds too many items. Returns a `Message`. | NR-04 |
| `encode` | Writes a message with the network's magic, length and checksum. | NR-01 |
| `Session::new` | Starts a session and queues the counter's `version`. | NR-11 |
| `Session::receive` | Bytes and the time in; events out. Runs the handshake and answers pings. | NR-04, NR-12 to NR-14 |
| `Session::take_outgoing` | The bytes queued for the peer. | NR-05 |
| `Session::tick` | The time passes: ends a slow handshake, sends the next ping. | NR-14 |
| `Session::send` | A `Request` after the handshake: `GetHeaders`, `GetTx`, `GetBlock`, `GetFilter`, `GetAddr`, `Announce`, `SendTx`. | NR-13, NR-55, NR-86 |
| `Event` | `Ready`, `Headers`, `Announced`, `Tx`, `Block`, `Filter`, `NotFound`, `Addresses`. | NR-04, NR-12 |
| `Drop` | Why a session ends. | all of the above |
| `AddressBook::add` | Keeps an address heard of from a seed, a peer or the owner. Refuses what is not publicly routable, not IPv4, IPv6 or Tor v3, or has port 0. At most 2,000 in all, 8 per network group, 64 from the peers of one group, 256 from seeds. A full book drops only entries that failed 3 times. | NR-22 |
| `AddressBook::pick` | Draws the next address to dial from a caller-supplied random number: not banned, not in a connected group, not tried in 600 seconds; anchors first; compact-filter peers first when asked; groups drawn with equal chance. | NR-23, NR-24, NR-21 |
| `AddressBook::connected_ok`, `failed` | A finished handshake clears the failures; the third failure in a row drops the entry. | NR-22 |
| `AddressBook::misbehaved` | Bans an address for 24 hours (kept in the book) and ends its time as an anchor. | NR-24, NR-25 |
| `AddressBook::set_anchors`, `anchors` | At most 2 stored peers that are dialed first. | NR-24 |
| `AddressBook::to_bytes`, `from_bytes` | The stored form (`OBMA`, version 1). Reading applies every rule of `add` again and refuses the whole file on any failure. | NR-22 |
| `Manager::new`, `wanted`, `connected`, `received`, `tick`, `outgoing`, `closed` | Runs peer sessions, requests headers, filters and blocks, and reports checked data and peer events. | NR-20, NR-21, NR-26, NR-30 to NR-54 |
| `Manager::watch_script`, `unwatch_scripts`, `rescan_from` | Changes the scripts watched and the range to check. | NR-40, NR-41, NR-97 |
| `Transport`, `Manager::connected_with` | Uses the old framing or BIP324 encrypted transport with caller-supplied entropy. | NR-80 to NR-85 |
| `Manager::broadcast`, `forget`, `relay_state` | Holds only transactions the app supplied, announces them and reports what peers did with them. | NR-86 to NR-89 |
| `OwnNode`, `Manager::connected_own_node`, `own_node`, `ask_own_node` | Keeps the merchant's own-node connection and its evidence separate from random peers. | NR-98 to NR-102 |

A drop means the connection must be closed. Once a session has returned a
drop, every later call returns the same drop.

## How the rules map to the code

| Rules | Where |
|---|---|
| NR-01 to NR-04 | `src/frame.rs`: `FrameReader`, `Frame::parse` |
| NR-05 | `src/frame.rs` (what is received), `src/session.rs` (what is queued) |
| NR-11 to NR-14 | `src/session.rs` |
| NR-22 to NR-25 (book) | `src/book.rs`: `AddressBook` |
| Multi-peer scheduling, headers, filters and blocks | `src/manager.rs`, `src/manager/` |
| NR-80 to NR-85 | `src/v2.rs`: `Transport` |
| NR-86 to NR-89 | `src/manager/broadcast.rs` |
| NR-98 to NR-102 | `src/own_node.rs`, `src/manager/own.rs` |
| Drops and their rule IDs | `src/reason.rs` |
| Limits and times | `pub const`s in `src/lib.rs`, named for their rule |

Some choices worth knowing:

- **The header decides.** The reader checks magic, command and length from
  the 24 bytes of the header, and gives up before a payload byte is kept. It
  never holds more than the message in progress and one read of at most
  65,536 bytes. The caller reads the socket in pieces of that size, and calls
  `next` until it returns `None` before the next `push`. `Session::receive`
  does this itself for a long slice.
- **Before the handshake.** The commands that belong after it (`ping`,
  `pong`, `headers`, `inv`, `notfound`, `tx`, `block`, `cfilter`, `addr`,
  `addrv2`) are dropped at their header under NR-13. A first message that is
  one of the handshake's own but not `version` is a drop under NR-12.
- **Commands the counter reads and ignores.** `wtxidrelay`, `sendcmpct` and
  `feefilter` are limited to 100 bytes, checked for their checksum, and
  otherwise ignored. They are accepted before the peer's `verack`. Every
  other command is skipped under NR-03, at any time.
- **Messages with no payload** (`verack`, `sendaddrv2`, `sendheaders`) must
  have none.
- **Headers.** Each header in a `headers` message is followed by a zero
  count of transactions, and anything else is a drop. At most 2,000.
- **Inventory.** The 36-byte entries are read by hand, because the bitcoin
  crate reads the error type (0) without its hash. `inv` takes only plain
  transaction and block entries. `notfound` also takes the witness types,
  because the counter asks with them.
- **`tx` and `block`.** A `tx` is checked to be one transaction with every
  byte used, and its bytes are handed on. A `block` is handed on as bytes
  without being parsed here.
- **Asking for data.** `GetTx` and `GetBlock` ask for the witness form, so
  that a peer sends the whole transaction or block. `GetHeaders` states
  protocol 70016 and no stop hash. `GetFilter` is for type 0.
- **Addresses.** `addr` entries become `AddrV2` addresses; an old-form Tor
  address is left out. At most 1,000 entries.
- **Time.** The handshake must end less than 30 seconds after `new`; a call
  at 30 seconds or later is a drop. The first ping is due 120 seconds after
  the handshake, the next 120 seconds after each ping. The nonce of the
  n-th ping is the session nonce plus n.
- **A request before the handshake** is a drop (NR-13), not a refusal that
  leaves the session open.
- **Queued bytes.** More than 2,000,000 bytes waiting to be sent is a drop
  (NR-05). The caller empties the queue with `take_outgoing`.

## What it deliberately does not do

- It does not open a socket, read a clock or make a nonce.
- It does not implement chain validation independently: the manager uses
  the header, block and filter checks of `openbmt-watch`.
- The session keeps no address book and knows of no other peer. The book
  (`AddressBook`) is separate and has no sockets or clock either.
- It does not forward arbitrary peer transactions. Broadcasting requires an
  explicit `Manager::broadcast` call from the app; the manager serves those
  held transactions when peers request them.
- Socket handling and Tor proxy connections belong to the driver.

## Tests

`cargo test -p openbmt-net`. There is at least one test for every rule this
crate implements (a test checks this against the rules file), named
`nr_NN_...`, and every drop is checked to name an implemented rule of the
file. The tests play the peer with messages written by the bitcoin crate:
every limit at the limit and one over, the header-only drop, messages cut at
every byte, a handshake fed one byte at a time, each refusal of a bad
`version`, every ordering case, the timers at their edges, the queue cap, and
every kind of event. Fuzz targets `net_frame` and `net_session` are in
`fuzz/` at the repository root.

The address book is tested for every refused kind and range, every limit at
the limit and one over, a flood of made-up addresses from one peer, the
draw (never a connected, banned or recently tried address; equal chance for
a group of eight and a group of one; same inputs, same result), the ban at
86,399 and 86,400 seconds and under a clock that goes back, and the stored
form (every truncation, and each rule of `add` applied to a file). The fuzz
target `net_book_decode` is in `fuzz/`.

Manager tests exercise scheduling, checked chain data, broadcasting and the
separate own-node path. BIP324 tests exercise handshakes and encrypted
packets. The `net_manager` and `net_v2_bytes` fuzz targets cover those entry
points.
