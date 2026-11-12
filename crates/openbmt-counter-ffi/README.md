# openbmt-counter-ffi

The OpenBMT counter node (`openbmt-counter-node`) as a library the counter
app for Android calls. One `Node` per data folder sits in a `Registry`
behind a lock. The library has one entry point, `dispatch`, which takes a
framed request and returns a framed response; on Android the one exported
symbol is a JNI function that calls it.

**This crate has had internal review only. No independent audit has been
done yet. Not for real funds.** The Kotlin binding and its OBMn decoder
harness have been compiled and run on the JVM, and the library has been
cross-built for `aarch64-linux-android` and `x86_64-linux-android`. No
Android device or emulator has run it.

This crate holds no rule of its own about sales. Every check and every
refusal is in the existing crates (`docs/COUNTER-RULES.md`,
`docs/WATCHER-RULES.md`); the layer converts types, keeps the lock and builds the payment code
(CR-83). The rule IDs pass through in `rule`, and the rule crates' own
text in `text`.

## Shape

- `dispatch(registry, request) -> Vec<u8>` (`src/call.rs`) is safe code.
  Requests start `OBMm`, version 1, op `u16`, handle `u64`; responses
  start `OBMn`, version 1, status `u8`. Everything is little-endian,
  lengths are `u32`, and decoding is strict: a cut, a byte left over, an
  unknown op, an unknown handle, an undefined tag or a request over 1 MiB
  is refused. `bindings/PROTOCOL.md` is generated from the encoder tables
  and checked by a test.
- A `Registry` holds at most 2 nodes by `u64` handle, starting at 1 and
  never reused.
- Every error response carries `rule`, `text` and the screen words (tone,
  who, heading, what, next, code, at, quoted, detail), and the words pass
  `docs/SCREEN-RULES.md` (SC-12). A panic is caught, becomes an internal
  error and poisons that node until it is stopped.
- `src/jni.rs` is the only file with `unsafe`: 8 blocks, each one JNI call
  or one read of the function table. It exports
  `Java_org_openbmt_counter_Native_call(env, class, request)`, copies the
  request out, calls `dispatch` and returns the response as a new byte
  array. No global references, no cached environment, no threads. The
  crate root denies `unsafe`; a test pins the count and refuses it
  anywhere else.
- `bindings/kotlin/org/openbmt/counter/` holds `Native.kt` (the `external`
  declaration) and `CounterNode.kt` (typed views, a strict decoder and one
  method for each op).

## Ops

| Op | Name | Fields | Result |
|---|---|---|---|
| 1 | start | folder, network, now, keys, settings, first batch, proxy, peers | handle |
| 2 | stop | none | last reports |
| 10 | pump | now, wait ms (cut to 250) | reports, notes, stopped |
| 11 | open | now, amount, currency, rate, source | sale, payment code |
| 12 | judge | now, index | screen, payment code |
| 13 | requote | now, index, rate, source | screen |
| 14 | cancel | now, index | screen |
| 15 | override | now, index, actor | screen |
| 16 | request refund | now, index, ask, actor, reason | rows |
| 17 | withdraw refund | now, index, request, actor | rows |
| 18 | refund in cash | now, index, actor, amount, reason | rows |
| 19 | refunds | index | rows |
| 20 | heartbeat | now, signed heartbeat | outcome |
| 21 | command | now, signed command | outcome |
| 22 | check in | now | outcome |
| 23 | add batch | now, signed batch | outcome |
| 30 | status | none | health, heights, sales, refused below minimum |
| 31 | notes | none | notes |
| 40 | rate sources | none | the table: id, URL, currency, initial |
| 41 | rate parse | source id, reply bytes, fetched at | a sample, or the refusal with its words |
| 42 | rate combine | samples, now | the rate, label and time for `open`, or the refusal with its words |
| 43 | pairing read | pairing record | network, then version, Tor on, customer Wi-Fi, smallest sale |
| 44 | pair begin | offer, name, now | request bytes, four words |
| 45 | pair match | now | nothing |
| 46 | pair differ | none | the words of the ended ceremony |
| 47 | pair cancel | none | nothing |
| 48 | pair finish | now, bundle | pairing record |
| 71 | pending change | now | the change that waits for a counter's confirmation (version, four words, each field loosened with its value now and after), and the words of one that was dropped or refused |
| 72 | confirm change | now, version | the words of the confirmation signed and written as a record (CR-241); the app asks after the manager's PIN |

## Threads

The app calls `pump` from one background thread and the other ops from its
UI thread. The node's lock is held only while a call works. A `pump` waits
for at most 250 ms whatever it is asked, and takes that wait in slices of
20 ms, letting go of the lock and sleeping a moment between them, so a
call from the UI thread waits for at most one slice and the work of the
call before it. The node's clock is the app's: every call that takes a
time raises it, and it never goes back. Tests in `src/tests/lock.rs` cover
the cap, the early return on a report and a sale paid under a live pump
loop.

## Price and settings

Ops 40 to 48 work on no node (handle 0). The app fetches the price itself
on the schedule of `docs/RATE-RULES.md` (RT-40 to RT-45) and hands the
bytes to `rate_parse`; `rate_combine` makes the samples into the rate that
`open` takes, so no JSON is read in the app. A refusal of either is an
error response whose words are those of `openbmt-rates`. `pairing_read`
reads the saved pairing record, and the settings file in the data folder it
is given (CR-163), before `start` so that the app can tell the network and
whether Tor is on now, for display only; it does not verify the record.
`start` verifies it and refuses to start when the settings have Tor on and
the start has no proxy (CR-87).

The pairing ceremony (`pair_begin`, `pair_match`, `pair_differ`,
`pair_cancel`, `pair_finish`) keeps one ceremony in the library, in memory
only (CR-91). `pair_finish` returns the pairing record, which holds the
counter's key: the app seals it and hands it to every `start`.

## Lightning

Ops 50 to 55 carry the shop's own Lightning node (CR-112 to CR-116). The
library never opens a connection: it hands the app one request at a time
(`ln_next`, or the first request of a test from `ln_setup`) and takes the
answer (`ln_reply`) or the news that there was none (`ln_failed`). The app
sends the request over HTTPS with no redirect, a body of at most 1,000,000
bytes and the host name unchecked, and asks `ln_pin` whether the certificate
the node showed is the pinned one before it sends a byte (CR-114). A test
(LN-20) is run against the node the owner named in the shop's settings; when
it passes, `ln_reply` gives the record, which the app seals and hands to
every `start` (`StartConfig.lightning`). `ln_remove` forgets the connection
at once. A sale's screen carries the Lightning line and, when the invoice
has passed, the code of LN-18 as its payment; `StateView::PaidLightning` is
a sale paid by Lightning, final at once.

Ops 66 to 68 carry the shop's swap provider (CR-219, CR-225): `swap_next`
hands out the next request for the provider's onion name (the number, the
`host:port`, and the bytes of an HTTP/1.1 request), `swap_reply` takes the
bytes that came back and `swap_failed` says there were none. The app sends
the bytes to port 80 of the onion name through its own Tor, one request at a
time, and never around it. A sale's Lightning line then also carries the
small line for a sale with no swap (`not_for_this_sale`), the words of a swap
that failed (`failed`), and, while the sale waits for one block, the sale's
words are those of CR-221. The status's Lightning line carries the words of a
claim that is stuck (CR-222) when no node connection is saved.

## Storage

The node stores in a data folder (the app passes `noBackupFilesDir`). The
folder's lock (NR-79) is taken by `start` and let go by `stop`.

## Payment code

`open` and `judge` return the BIP 21 request as text for the code
(`BITCOIN:` and the address in upper case, then `?amount=` in bitcoin with
at most 8 decimals), the same in lower case for "copy", and the code's
modules from `openbmt-qr` at level M, packed most significant bit first,
row by row.

## What is checked

- `cargo test -p openbmt-counter-ffi`: round trips for every op, strict
  decoding, errors with words, caught panics, the pin on `unsafe`, a fake
  JNI environment, a sale from open to paid and a restart, the code against
  `openbmt_qr::encode_text`, and the pump against the screen thread.
- `bindings/tests/run-jvm.sh`: the Kotlin binding and its decoder harness
  on a JDK (see `bindings/tests/README.md`).
- `scripts/counter-ffi-build.sh`: both Android targets, with a check that
  the only exported `Java_` symbol is the entry point.
- Not checked: running on a device, the app itself.
