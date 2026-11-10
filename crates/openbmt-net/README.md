# openbmt-net

The OpenBMT counter's own Bitcoin peer client, first slice: the wire framing
and the session with one peer, as a Rust library with no sockets, no clock,
no threads, no async and no randomness. The caller passes in the bytes
received, the time in seconds and a nonce, and takes out the bytes to send
and the events. The library decides what a peer may send and nothing more.

The specification is [docs/NETWORK-RULES.md](../../docs/NETWORK-RULES.md).
Each rule has an ID (NR-01 and on). Every drop names its rule
(`Drop::rule()`, and its text starts with the ID), and every test is named
for the rule it proves.

**This crate has had internal review only. No independent security audit has
been done yet.** Do not use it with real funds.

This crate implements NR-01 to NR-05 and NR-11 to NR-14, and the address
book: NR-22 to NR-24 and the 24 hours of NR-25. The manager of several
peers and the driver that owns the sockets (NR-10, NR-20, NR-21, NR-26,
and the policy of NR-30 to NR-54) are later slices. Of NR-55
this slice does its part: no request sends a transaction, and a request from
the peer is not answered.

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
| `Session::send` | A `Request` after the handshake: `GetHeaders`, `GetTx`, `GetBlock`, `GetFilter`, `GetAddr`. | NR-13, NR-55 |
| `Event` | `Ready`, `Headers`, `Announced`, `Tx`, `Block`, `Filter`, `NotFound`, `Addresses`. | NR-04, NR-12 |
| `Drop` | Why a session ends. | all of the above |
| `AddressBook::add` | Keeps an address heard of from a seed, a peer or the owner. Refuses what is not publicly routable, not IPv4, IPv6 or Tor v3, or has port 0. At most 2,000 in all, 8 per network group, 64 from the peers of one group, 256 from seeds. A full book drops only entries that failed 3 times. | NR-22 |
| `AddressBook::pick` | Draws the next address to dial from a caller-supplied random number: not banned, not in a connected group, not tried in 600 seconds; anchors first; compact-filter peers first when asked; groups drawn with equal chance. | NR-23, NR-24, NR-21 |
| `AddressBook::connected_ok`, `failed` | A finished handshake clears the failures; the third failure in a row drops the entry. | NR-22 |
| `AddressBook::misbehaved` | Bans an address for 24 hours (kept in the book) and ends its time as an anchor. | NR-24, NR-25 |
| `AddressBook::set_anchors`, `anchors` | At most 2 stored peers that are dialed first. | NR-24 |
| `AddressBook::to_bytes`, `from_bytes` | The stored form (`OBMA`, version 1). Reading applies every rule of `add` again and refuses the whole file on any failure. | NR-22 |

A drop means the connection must be closed. Once a session has returned a
drop, every later call returns the same drop.

## How the rules map to the code

| Rules | Where |
|---|---|
| NR-01 to NR-04 | `src/frame.rs`: `FrameReader`, `Frame::parse` |
| NR-05 | `src/frame.rs` (what is received), `src/session.rs` (what is queued) |
| NR-11 to NR-14 | `src/session.rs` |
| NR-22 to NR-25 (book) | `src/book.rs`: `AddressBook` |
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
- It does not check a header, a block, a filter or a transaction against the
  chain. The watcher does that.
- The session keeps no address book and knows of no other peer. The book
  (`AddressBook`) is separate and has no sockets or clock either.
- It does not relay. It sends no transaction, and answers no `getdata`,
  `getheaders`, `getaddr` or `mempool`.
- It does not encrypt (NR-62).

## Tests

`cargo test -p openbmt-net`. There is at least one test for every rule this
slice implements (a test checks this against the rules file), named
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
