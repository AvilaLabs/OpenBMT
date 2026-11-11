# openbmt-owner-ffi

The owner node (`openbmt-owner-node`) as a library for the owner app for
Android. The library holds no rule of its own. It converts types, keeps the
node on a thread of its own, and turns every state and every refusal into
the words a person is shown (`docs/SCREEN-RULES.md`), taken from the crates
that make them.

This crate has had internal review only. No independent security audit has
been done yet. Not for real funds.

## The call

`dispatch(&Registry, request) -> response` is the one entry point. A request
is `"OBMk" | version 1 | op u16 | handle u64 | fields`, and a response is
`"OBMl" | version 1 | status u8 | ...`. Status 0 is the result of the op.
Status 1 (refused), 2 (store), 3 (state) and 4 (internal) are followed by the
rule, the text and the words of the error. `bindings/PROTOCOL.md` lists every
op and type; a test writes it from the tables the encoder uses and fails
when the file differs.

A request is at most 9 MiB, is read completely before anything is done, and
is refused when bytes are left over. A [`Registry`] holds at most two nodes;
handles are never given out twice.

## Ops

| Group | Ops |
|---|---|
| Life | `start`, `stop`, `pump`, `link_step`, `deliver` |
| Lines | `status`, `receipts_line`, `merge_offer`, `send_states`, `settings`, `outbox` |
| Live watch | `old_addresses`, `request_deep_check` |
| Approval | `build_request`, `approve_merge`, `take`, `sessions`, `session_request`, `abandon_session` |
| Queue | `queue`, `queue_refund`, `cancel`, `requeue`, `cash_out`, `sign_batch`, `queue_settings`, `settings_preview` |
| Link | `link_state`, `clear_alert`, `refund_notes`, `dismiss_refund_note` |
| Upkeep | `notes`, `rescan`, `recover_key1`, `archive_paid` |
| Till | `pos_state`, `pos_setup`, `pos_remove`, `pos_request`, `pos_reply`, `pos_acknowledge` |
| Key ceremony | `ceremony_make_key1`, `ceremony_read_answer`, `ceremony_build_wallet`, `ceremony_finish`, `ceremony_open`, `ceremony_stage`, `ceremony_defaults` |
| Background watch | `watch_make_key`, `watch_named`, `watch_settings_preview`, `watch_export`, `watch_start`, `watch_step`, `watch_stop` |

The till ops (`docs/OWNER-COINS-RULES.md`, OC-30 to OC-37) match the shop's
orders against the counters' sales on the owner's phone. The app does the
HTTP: `pos_request` gives the request to send (without the sign-in, which is
the app's), `pos_reply` takes the status and the body of the answer, parses
it, stores the ledger and says whether another page is wanted. That is why
a request may be 9 MiB: a till page is at most 8 MiB.

`settings` is the signed settings the node holds as in force, decoded, for a
read-only view (none until the first settings are taken). `outbox` lists the
signed objects that wait, by kind; the node keeps no time for them, so only a
batch carries `signed_at`, read from the range the phone issued for it.

`settings_preview` (`docs/SESSION-RULES.md`, AS-74 to AS-77) takes an edit in
which every field is optional and gives back each changed field in the words
of a screen, whether the Key app must co-sign, and the bytes `queue_settings`
takes. It stores nothing. The app never builds settings bytes itself.

`queue_refund` takes no amount received from the app: what the sale
received and whether it is at the refund depth come from the wallet's own
checked view (OC-14). `cash_out` checks its address in Rust, for the
network of the node. `take` keeps Key 1's state in `key1.bin` of the data
folder in the same step as the node's journal.

## Threads

The node is not `Send`, so it lives on one thread that this library starts,
and every call is a job sent to it. The app calls `pump` from one
background thread in a loop (it waits at most 250 ms, in slices of 20 ms)
and every other op from its screen thread. A screen call waits for at most
one slice and the calls ahead of it. A panic in a job is caught, becomes an
internal error and poisons the node until it is stopped.

## Android

`src/jni.rs` is the only file with `unsafe`: one exported function,
`Java_org_openbmt_owner_Native_call`, that copies the request out of the
Java array, runs `dispatch` and returns the response as a new array. It
keeps no global reference and no `JNIEnv` between calls. A test counts the
`unsafe` blocks and fails when another file holds the word.

`bindings/kotlin/org/openbmt/owner/` holds the hand-written binding. Its
decoder is checked on the JVM by `bindings/tests/` (see its README).

## Checks

- `cargo test -p openbmt-owner-ffi`: round trips for every op, strict
  decoding (every cut, every extra byte, every flipped bit), words on every
  error and state, panics, the pinned `unsafe` count, a fake `JNIEnv`, the
  lock tests, the Kotlin constants, and one run from a payment received to
  a payment handed to the driver.
- `scripts/owner-ffi-build.sh`: both Android targets, with a check that the
  only exported `Java_` symbol is the entry point.
- `bindings/tests/run-jvm.sh`: the Kotlin decoder harness.

The data folder is the app's own (`noBackupFilesDir`). Keeping it out of
backups, and protecting the owner secret and Key 1's account key that the
app hands over at every start, are the app's job.
