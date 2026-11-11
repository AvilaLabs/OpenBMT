# openbmt-key-ffi

The OpenBMT Key app's core as a library an Android app calls. One object,
`KeyApp`, holds the one `Approver` of `openbmt-approver` (with the signer of
`openbmt-signer`) behind a lock. The app implements one interface,
`KeyStore`, for storage, randomness and the clock. The library has one entry
point, `dispatch`, which takes a framed request and returns a framed
response; on Android the one exported symbol is a JNI function that calls it.

**This crate has had internal review only. No independent security audit
has been done yet. Not for real funds.** The Kotlin binding and its OBM9
decoder harness have been compiled and run on the JVM. This does not compile
the Android JNI library or exercise an Android device. See "What is checked"
below.

This crate holds no rule of its own. Every check and every refusal is in
the existing crates (`docs/SESSION-RULES.md` section 2,
`docs/SIGNER-RULES.md`); the layer converts types, keeps the lock and
stores state before anything is returned. The rule IDs pass through in
`KeyError.rule`, and the rule crates' own text in `KeyError.text`.

## Shape

- The Rust API is plain Rust: `KeyApp`, `SessionHandle`, the view types,
  `KeyError`, the `KeyStore` trait (`Send + Sync`) and the free functions.
  The tests drive it directly and again through `dispatch`.
- `dispatch(registry, store, request) -> Vec<u8>` (`src/call.rs`) is safe
  code. A `Registry` holds the open apps and sessions by `u64` handle
  (starting at 1, never reused in a process; at most 4 apps, and one session
  handle for each app).
- `src/jni.rs` is the only file with `unsafe`: 14 blocks (each one JNI call
  or one read of the function table, with a `SAFETY` line) and 2 impls. It
  exports `Java_org_openbmt_key_Native_call(env, class, store, request)`,
  copies the request out, wraps the Java store object, calls `dispatch` and
  returns the response as a new byte array. The store object is used only
  during the call and on the calling thread: no global references, no cached
  environment, no threads. A Java exception in a store method is cleared and
  becomes a store error. The crate root denies `unsafe`; a test pins the
  count in `jni.rs` and refuses it anywhere else.
- `bindings/kotlin/org/openbmt/key/` holds `Native.kt` (the `external`
  declaration and the `KeyStore` interface) and `KeyApp.kt` (a typed wrapper,
  one method for each op, the key ceremony's ops 80 to 85 included). No JNA,
  no generated code, no Kotlin library.

The Cargo feature `jni` (on by default) compiles `jni.rs` on every target so
that the host can test it against a fake `JNIEnv`; on Android it is always
compiled.

## The call protocol

A request is `"OBM8" | 1 | op u16 | handle u64 | fields...`; a response is
`"OBM9" | 1 | status u8 | ...`. Status 0 is ok and carries the op's result;
1 refused, 2 store, 3 state and 4 internal (a panic was caught) carry `rule`
and `text`, each a u16 length and UTF-8, and then the words a person is
shown: tone u8, who u8, heading, what, next (each a u16 length and UTF-8),
code (a flag byte and a string), at (a flag byte and a u64), quoted (a flag
byte, then from and text) and detail (a flag byte and a string). Integers
are little-endian, a bool
is one byte, bytes and strings have a u32 length, an option a flag byte, a
list a u32 count, an enum a tag byte. A request is at most 1 MiB, is read
completely before anything is done, and is refused when bytes are left over
or a length, flag or tag is not defined; an op number that is not in the
table is refused with the rule `op-unknown`. The handle is the app (from
op 1), the session (from op 30), or 0 for the free functions.

`bindings/PROTOCOL.md` has the layout of every op and type. It is generated
from the same tables the encoder uses, and a test fails if the committed file
differs (`OPENBMT_BLESS_PROTOCOL=1 cargo test -p openbmt-key-ffi protocol`
writes it again). Op numbers are never reused.

| op | name | handle | what it does |
| --- | --- | --- | --- |
| 1 | `open` | none | restore the Key app from `store.load()`; returns the app handle |
| 2 | `close_app` | app | close it and its session |
| 3 | `close_session` | session | cancel and close the handle |
| 10 | `state` | app | `Empty`, `HasIdentity`, `WalletPending`, `Ready`, or `Locked` |
| 11 | `create_from_entropy` | app | identity and key 2 from a source; stored; the words wait |
| 12 | `create_approver_only` | app | identity only (key 2 on a hardware wallet) |
| 13 | `key_words` | app | the words, once, never stored |
| 14 | `key2_public` | app | fingerprint, origin path, xpub, or none |
| 15 | `device_root` | app | the Key app's root, 32 bytes |
| 16 | `device_root_words` | app | the same as four words |
| 17 | `accept_wallet` | app | pin wallet and owner root; returns the wallet view |
| 18 | `wallet_view` | app | checksum words, three keys, first address |
| 19 | `pair_offer_bytes` | app | the `OBMO` pair offer for the counter to scan |
| 20 | `check_kit` | app | the scanned description against the Key app's own |
| 30 | `open_session` | app | check a request and open the session; returns its handle |
| 31 | `session_id` | session | the session's ID |
| 32 | `lines` | session | one record for each item |
| 33 | `decide` | session | approve or decline an item |
| 34 | `finish` | session | store the new state, then return the response |
| 35 | `cancel` | session | release the slot |
| 40 | `signer_status` | app | ceilings, marks, totals, allowlist |
| 41 | `receive_mark` | app | the receive mark |
| 42 | `enroll_destination` | app | enrol a bare address or a `bitcoin:` URI with a label |
| 43 | `remove_destination` | app | remove one |
| 44 | `set_ceilings` | app | the two sets of ceilings |
| 45 | `set_fee_ceilings` | app | the fee ceilings |
| 46 | `raise_change_mark` | app | raise the change mark |
| 47 | `raise_receive_mark` | app | raise the receive mark |
| 48 | `correct_clock` | app | correct a clock that is behind |
| 50 | `make_invitation` | app | make and store an invitation for a Box |
| 51 | `open_invitation` | app | the open invitation, or none |
| 52 | `cancel_invitation` | app | forget the open invitation |
| 60 | `coin_bits` | none | progress of the coin flips |
| 61 | `dice_bits` | none | progress of the dice rolls |
| 62 | `dice_face_counts` | none | how often each face came up |
| 63 | `word_suggestions` | none | up to 8 words for a prefix |
| 64 | `word_is_valid` | none | whether a word is on the list |
| 65 | `parse_deposit_code` | none | read an address or BIP21 text; enrols nothing |
| 80 | `take_key3` | app | read and keep key 3 from text; returns what to compare |
| 81 | `key3_view` | app | key 3 as kept, or none |
| 82 | `public_answer` | app | the public answer (`OBMv`) for the owner phone |
| 83 | `accept_wallet_setup` | app | check the owner phone's setup (`OBMw`) and keep it as pending; returns the wallet view |
| 84 | `confirm_wallet` | app | "they match": the pending wallet becomes the registered wallet |
| 85 | `discard_wallet` | app | "they differ": drop the pending wallet, back to the state before op 83 |
| 86 | `take_key2` | app | no key 2 held: read and keep key 2 from the text its hardware signer shows; returns what to compare |
| 87 | `key2_taken_view` | app | key 2 as taken from its signer, or none |
| 90 | `lock_status` | app | `NoPin`, `Legacy` (an older state without a PIN), `Locked` or `Open` |
| 91 | `set_pin` | app | set the PIN before a key is made, or wrap an older state behind one |
| 92 | `unlock` | app | open the stored state with the PIN; a wrong PIN is `pin-wrong` and changes nothing |
| 93 | `change_pin` | app | the current PIN and a new one; the stored state is written once |

The ceremony's wallet is accepted in two steps (`docs/SIGNER-RULES.md`
SR-67). Op 83 runs every check and stores the setup as pending, in an `OBM7`
envelope of version 3 (the `OBMw` bytes, and no wallet), so that a restart
reopens on the comparison: `state` says `WalletPending`, `wallet_view` shows
it, and every op that signs, pairs or changes the setup is refused with the
state rule `wallet-pending`. Op 84 registers it (SR-63 applies from then
on); op 85 drops it and leaves the Key app as it was before op 83, with key
2 and key 3 kept. Both are refused with `nothing-pending` when nothing is
pending. The three refusals of SR-67 have the rule strings `key3-missing`,
`key3-differs` and `key3-not-in-wallet`. The older `accept_wallet` (op 17)
registers at once, except on a Key app with no key 2, where it is refused
with `key2-missing`.

With key 2 on a hardware signer the Key app holds no key 2, and op 86 takes
its public part straight from the signer, after key 3 and before the answer
(`docs/SIGNER-RULES.md` SR-84 to SR-90). It is kept in an `OBM7` envelope of
version 4; a state of an older version reads as "no key 2 taken". Op 83 then
needs it: without it the setup is refused with `key2-missing`, and a wallet
whose slot 2 is not exactly that key is refused with `key2-not-in-wallet`.
Op 86 also has the refusals `key2-secret` and `key2-is-key3`. The answer
(`OBMv`) never carries it.

Opening a session ends the handle of the app's previous session. One call
runs at a time: a call made while another runs, or from inside a store
method, is refused with the rule `busy` (the Kotlin wrapper serialises its
calls).

## Behaviour

Setup follows `docs/DESIGN.md` section 2.1. Entropy sources follow section
5.4: `Platform` (32 bytes from `KeyStore.random`, 24 words), `DiceHashed`,
`CoinFlips`, `DiceSkewCorrected` and `TypedWords`. With the dice and coin
sources nothing else is mixed in, so the same throws give the same words in
another tool. The identity secret always comes from `KeyStore.random`. Key 2
is the BIP39 seed (empty passphrase) at `m/48'/coin'/0'/2'`.

`TypedWords` takes 12 or 24 BIP39 English words, each trimmed and
lower-cased. A wrong count, a word that is not on the list and a wrong
checksum are refused with the rules `words-count`, `words-unknown` and
`words-checksum`; the second names the 1-based position of the word, and no
error repeats what was typed. The words are not stored.

`check_kit` compares the scanned text with the Key app's own canonical wallet
description, byte for byte, after trimming ASCII white space at both ends.
`Different` carries the position of the first difference, whether the text
parses as a wallet description, and whether its four checksum words equal the
Key app's own (the R4-2 case, the most serious result). Nothing is stored.
Text over 8,192 bytes is refused.

`parse_deposit_code` reads a bare address or a BIP21 URI. A `req-` parameter
that is not understood refuses the whole code; other unknown parameters are
listed by name in `ignored` and never acted on. The address must be for the
given network and of a kind the signer's allowlist takes. The text is at most
2,048 bytes.

## The store contract

- **`save` must be durable before it returns.** The Key app returns a
  response, or the answer of an own-screen call, only after `save` returned
  without error (AS-34). If `save` throws, nothing the Key app keeps has
  changed and the same call can be made again.
- **Protect the bytes at rest.** What `save` receives is the `OBM7`
  envelope. It holds the identity secret and, when this device holds key 2,
  key 2's account secret, in the clear. Wrapping it with a key from the
  Android Keystore, keeping it out of backups and device transfer, and never
  writing it anywhere else is the app's job. This crate does none of it.
- **Show the words once.** `key_words` returns them one time.
- **Release sessions.** Call `cancel` or `close_session` when the person
  leaves the screen. Until then a second session and every setup or
  own-screen change is refused with the rule `session-open`.
- **After status 4** the app is poisoned: every call returns the same error.
  Close it and open a new one from the stored bytes.
- `now()` cannot fail: a store method that throws, or a negative time, reads
  as 0.
- The checks the Key app promises of its own software (no root, no overlays,
  no accessibility services, no screenshots, wipe after wrong tries) are the
  app's, not this library's.

### The `OBM7` envelope

`OBM7`, version 1 (2 with key 3, 3 with a pending wallet setup), strict
decode (`src/envelope.rs`): the identity secret;
key 2's network, master fingerprint and account key if this device holds
key 2; and, once the wallet is accepted, the network, the canonical wallet
description, the owner root, the batch minimum and the approver snapshot
(`OBM3`).

### Secrets in memory

Overwritten when dropped: the identity secret, key 2's account key (and the
approver's copy of it, by the approver), the words after they are taken or
replaced, the typed dice rolls and words, and the JNI glue's copy of each
request. Not overwritten, because the types do not allow it: the approver's
copy of the identity, the `bip39` mnemonic, the byte arrays on the Java side
(the envelope handed to `save`, the request and response arrays) and
everything else on the Kotlin side.

### Signing seam

The signer is called in one function, `sign_spend` in
`crates/openbmt-approver/src/evaluate.rs`. Signatures are RFC 6979. A nonce
protocol (anti-exfil) would change only that function.

## Errors

`KeyError` has four variants, each with `rule` and `text`: `Refused` (a rule
crate refused; `rule` is its ID such as `AS-20` or `SR-30`, or the name of the
check), `Store` (the store failed, or the stored bytes cannot be read),
`State` (the call is not possible now, with what to do instead) and
`Internal` (a panic was caught). In Kotlin they are one `KeyException` with
`rule`, `text`, `status` and `words`.

`rule` and `text` are for logs and tests. A person is shown the words: every
error response carries them, made by `KeyError::words` (and `words_for`, from
the kind, the rule string and the text) under `docs/SCREEN-RULES.md`. The
heading, `what` and `next` hold no rule number, no key and no raw number; the
rule goes in `code`, a time in `at` and what the system said in `detail`, and
the app shows `code` and `detail` only under "Details". Text that came from
another device or a scanned code would go in `quoted`, never in the
sentences. A rule string the crate has no words for gets the words of its
kind, which say that the cause is not known and send the person to the owner;
a test fails when a rule string passed to `refused_by`, `wrong_state` or
`internal` has no words of its own. The words of the setup state, of a check
and of the kit check are `SetupState::words`, `CheckStatus::words` and
`KitCheck::words`; the words of every refusal of the signer, the session and
the approver are in those crates, and `docs/screens/key.md` lists them all
(`OPENBMT_WRITE_SCREENS=1 cargo test -p openbmt-key-ffi screens_list` writes
it again).

Not changed here: the line `failure` text of a session is still the rule
crates' own text, and `Decision::Decline` reasons are the key holder's own.

## What is checked

```sh
~/.cache/openbmt/guard.sh env CARGO_TARGET_DIR=$HOME/.cache/openbmt/target/keyjni \
    cargo test -p openbmt-key-ffi
scripts/key-ffi-build.sh
```

- The Rust API tests (setup from each source, a whole session, own screens,
  words, kit check, deposit codes, failing stores, a panic) with the owner
  crate on the other side; the main flows run again through `dispatch`.
- Strictness: every cut and every flipped bit of a set of valid requests is
  refused or handled without a panic and without an internal error; unknown
  ops, oversized lengths, handles that do not exist and a session handle used
  after it was closed. Every error response of that corpus carries words that
  pass `check` (SC-12).
- The words: every state and refusal has words, none breaks the rules of
  `docs/SCREEN-RULES.md`, no two read the same, and every rule string the
  source passes to `refused_by`, `wrong_state` and `internal` has words of
  its own.
- The JNI glue against a fake `JNIEnv`: the happy path, a Java exception in
  each store method, null returns, a wrong array length from `random`, missing
  methods, no array for the response, use after the call and from another
  thread.
- The fuzz target `key_dispatch` (arbitrary request bytes against a Key app
  that has its wallet and an open session; never panics, the stored bytes
  always restore).
- `scripts/key-ffi-build.sh` builds the library for `aarch64-linux-android`
  and `x86_64-linux-android` (NDK, API 26) and checks with `llvm-readelf`
  that the only exported `Java_` symbol is the entry point above and that no
  `uniffi` symbol remains.
- The JVM binding check compiles `Native.kt`, `KeyApp.kt` and
  `bindings/tests/ProtocolDecoderTest.kt`, then runs the harness. It covers a
  valid success frame, a valid error frame and rejection of undefined status,
  tone and person numbers, malformed UTF-8, truncation and trailing bytes.
  Run it with `bindings/tests/run-jvm.sh` after setting the compiler classpath
  variables described in `bindings/tests/README.md`.
- The same harness decodes the success frames of ops 80 to 83 (key 3 as
  kept, key 3 or none, the public answer, the wallet view) with every cut and
  a changed flag refused, and compares the request frames of `takeKey3` and
  `acceptWalletSetup` byte for byte.
- Rust source checks still compare Kotlin `OP_*`, `TAG_*`, `TONE_*` and
  `WHO_*` constants with the Rust tables and keep the response field order in
  sync. `PROTOCOL.md` remains generated from the Rust encoder tables.

There is no restore mode to build: a Key app made from typed words is a new
Key app under every signer rule (`docs/SIGNER-RULES.md` SR-39). Not built: the
kit's other fields in `check_kit` (birthday, generations, import range, fallback
descriptors), and anti-exfil signing.

Not covered by these library checks: loading the library on a device or
emulator, and the Android Keystore wrapping. Cross-compilation is checked
by `scripts/key-ffi-build.sh` as described above.

## Dependencies

Besides the workspace crates (`openbmt-core`, `openbmt-approver`,
`openbmt-session`, `openbmt-signer`, `openbmt-lock`), the library directly
depends on `bitcoin` 0.32.8, `bip39` 2.2.2 and `jni-sys` 0.3.0. The shared
lock crate brings in Argon2id and ChaCha20-Poly1305. `jni-sys` 0.3.0 has no
dependencies of its own; its later releases depend on a macro crate and
`syn`. The complete external package list is
[`deps/openbmt-key-ffi.txt`](../../deps/openbmt-key-ffi.txt), checked by
`scripts/deps-check.sh`.
