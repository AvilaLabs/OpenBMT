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
