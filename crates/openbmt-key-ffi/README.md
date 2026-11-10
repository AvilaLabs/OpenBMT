# openbmt-key-ffi

The OpenBMT Key app's core as a library an Android app calls through
[UniFFI](https://mozilla.github.io/uniffi-rs/). One object, `KeyApp`, holds
the one `Approver` of `openbmt-approver` (with the signer of
`openbmt-signer`) behind a lock. The app implements one callback interface,
`KeyStore`, for storage, randomness and the clock.

**This crate has had internal review only. No independent security audit
has been done yet. Not for real funds.** Kotlin has not been compiled or
run against these bindings.

This crate holds no rule of its own. Every check and every refusal is in
the existing crates (`docs/SESSION-RULES.md` section 2,
`docs/SIGNER-RULES.md`); the layer converts types, keeps the lock and
stores state before anything is returned. The rule IDs pass through in
`KeyError.rule`, and the rule crates' own text in `KeyError.text`.

## The object

```text
KeyApp.open(store: KeyStore)                  restore from store.load()

setup (docs/DESIGN.md section 2.1)
  state() -> SetupState        Empty | HasIdentity | Ready
  createFromEntropy(source, network)   identity + key 2; stored; words wait
  createApproverOnly()         identity only (key 2 on a hardware wallet)
  keyWords() -> [String]       the words, once, never stored (nothing for
                               a key made from typed words)
  key2Public() -> Key2Public?  master fingerprint, origin path, xpub
  deviceRoot(), deviceRootWords()
  acceptWallet(WalletSetup) -> WalletView   pins wallet + owner root;
                               with key 2 here, the signer checks the slots
  walletView()                 checksum words, three keys, first address
  pairOfferBytes()             the OBMO pair offer for the owner phone
  checkKit(scanned) -> KitCheck  the scanned kit description against the
                               Key app's own, byte for byte (Ready only)

session (docs/SESSION-RULES.md section 2)
  openSession(request) -> SessionHandle     one at a time
    lines() -> [LineView]      flat records, one per item
    decide(item, Decision)     Approve | Decline(reason)
    finish() -> bytes          stores (AS-34), then returns the response
    cancel()                   releases the slot; dropping does the same

own screens (the signer's public calls, one to one)
  signerStatus(), receiveMark()
  enrollDestination(address, label), removeDestination(address)
  setCeilings(other, allowlisted), setFeeCeilings(fee)
  raiseChangeMark(n), raiseReceiveMark(n), correctClock()

free functions: coinBits, diceBits, diceFaceCounts (progress of the
entropy screens, from openbmt_core::entropy); wordSuggestions(prefix) and
wordIsValid(word) for the word picker; parseDepositCode(text, network) ->
DepositCode (a bare address or a BIP21 URI; enrols nothing)
```

Entropy sources follow `docs/DESIGN.md` section 5.4: `Platform` (32 bytes
from `KeyStore.random`, 24 words), `DiceHashed`, `CoinFlips`,
`DiceSkewCorrected` and `TypedWords`. With the dice and coin sources nothing
else is mixed in, so the same throws give the same words in another tool.
The identity secret always comes from `KeyStore.random`. Key 2 is the BIP39
seed (empty passphrase) at `m/48'/coin'/0'/2'`.

### Words the owner already has

`TypedWords` takes 12 or 24 BIP39 English words, each trimmed and
lower-cased, and derives key 2 as for every other source (empty passphrase).
A wrong count, a word that is not on the list and a wrong checksum are
refused with the rules `words-count`, `words-unknown` and `words-checksum`;
the second names the 1-based position of the word, and no error repeats what
was typed. The words are not stored and `keyWords` returns nothing for this
source (`words_waiting` is false). This is the same fresh-signer path as the
other sources: the signer starts with empty history and no rule is relaxed.
Restore-and-sign mode (`docs/DESIGN.md` section 5.4) is not built.

### The kit check

`checkKit` compares the text scanned from the printed kit with the Key
app's own canonical wallet description, byte for byte, after trimming ASCII
white space at both ends. `Different` carries the position of the first
difference, whether the text parses as a wallet description, and whether
its four checksum words equal the Key app's own. The last case is R4-2 (a
description made to share the words people compare) and is the most serious
result. Nothing is stored and the scanned text is not kept; text over 8,192
bytes is refused. Only the description is checked here: no kit format
exists in the repository yet, so the kit's other fields (birthday,
generations, import range, fallback descriptors) are not checked.

### Deposit codes

`parseDepositCode` reads a bare address or a BIP21 URI (`bitcoin:` in any
case, `amount` in bitcoin with at most 8 places, converted in integer
arithmetic, `label`, `message`, percent escapes). A `req-` parameter that is
not understood refuses the whole code; other unknown parameters, `lightning`
and `pj` included, are listed by name in `ignored` and never acted on. The
address must be for the given network and of a kind the signer's allowlist
takes (P2WPKH, P2WSH, P2TR, P2PKH, P2SH; SR-22, SR-30). The text is at most
2,048 bytes. The function enrols nothing: the app shows the result, the
person types the label, and `enrollDestination` applies the signer's rules.

### What the app must do

- **`save` must be durable before it returns.** The Key app returns a
  response, or the answer of an own-screen call, only after `save` returned
  without error (AS-34). If `save` throws, nothing the Key app keeps has
  changed and the same call can be made again. A session whose `finish`
  failed on a store error can be finished again.
- **Protect the bytes at rest.** What `save` receives is the `OBM7`
  envelope. It holds the identity secret and, when this device holds key 2,
  key 2's account secret, in the clear. Wrapping it with a key from the
  Android Keystore, keeping it out of backups and device transfer, and
  never writing it anywhere else is the app's job. This crate does none of
  it.
- **Show the words once.** `keyWords` returns them one time. They are not in
  the envelope, so they cannot be shown again after the call or after a
  restart. Making a key again is allowed until the wallet is accepted.
- **Release sessions.** A `SessionHandle` that is dropped without `finish`
  changes nothing, but in Kotlin the object is released when it is closed or
  collected. Call `cancel()` (or `close()`) when the person leaves the
  screen. Until then a second `openSession` and every setup or own-screen
  change is refused with rule `session-open`.
- **After `KeyError.Internal`** the object is poisoned: every call returns
  the same error. Open a new `KeyApp` from the stored bytes.
- The checks the Key app promises of its own software (no root, no
  overlays, no accessibility services, no screenshots, wipe after wrong
  tries) are the app's, not this library's.

### The `OBM7` envelope

`OBM7`, version 1, strict decode (`src/envelope.rs`): the identity secret;
key 2's network, master fingerprint and account key if this device holds
key 2; and, once the wallet is accepted, the network, the canonical wallet
description, the owner root, the batch minimum and the approver snapshot
(`OBM3`). It only parses; the wallet, owner and snapshot are checked by the
crates that own them when the Key app is restored.

### Secrets in memory

Overwritten when dropped: the identity secret, key 2's account key (and the
approver's copy of it, by the approver), the words after they are taken or
replaced, and the typed dice rolls and words. Not overwritten, because the
types do not allow it: the approver's copy of the identity
(`openbmt_core::Identity` has no erase), the `bip39` mnemonic, the byte
vectors UniFFI copies across the boundary (the envelope handed to `save`,
the words returned to Kotlin), and everything on the Kotlin side. The
compiler and the operating system may keep other copies of any of it.

### Signing seam

The signer is called in one function, `sign_spend` in
`crates/openbmt-approver/src/evaluate.rs`. Signatures are RFC 6979. A nonce
protocol (anti-exfil) would change only that function.

## Errors

`KeyError` has four variants, each with `rule` and `text`:
`Refused` (a rule crate refused; `rule` is its ID such as `AS-20` or
`SR-30`, or the name of the check), `Store` (the callback failed, or the
stored bytes cannot be read), `State` (the call is not possible now, with
what to do instead) and `Internal` (a panic was caught). The field is
called `text`, not `message`, because a Kotlin exception already has a
`message`.

## Proof

Everything runs under the memory guard with a separate target directory:

```sh
# 1. host tests: the exported API driven as Kotlin would, through an
#    in-memory KeyStore, with the owner crate on the other side
~/.cache/openbmt/guard.sh env CARGO_TARGET_DIR=$HOME/.cache/openbmt/target/keyffi \
    cargo test -p openbmt-key-ffi

# 2 and 3. bindings and Android libraries (one script, prints PASS, FAIL
#    or NOT RUN for each step)
scripts/key-ffi-build.sh
```

The script runs the bindings generator that is part of this crate
(`cargo build -p openbmt-key-ffi --features bindgen --bin uniffi-bindgen`,
then `uniffi-bindgen generate --library target/debug/libopenbmt_key_ffi.so
--config crates/openbmt-key-ffi/uniffi.toml --language kotlin --no-format`),
twice, and writes `bindings/kotlin/` only if both runs give the same bytes.
The libraries are built with `rustup target add aarch64-linux-android
x86_64-linux-android`, then `cargo build --release -p openbmt-key-ffi
--target <target>` with `CARGO_TARGET_<TRIPLE>_LINKER`, `CC_<triple>` and
`AR_<triple>` set to the NDK's clang (API level 26) and `llvm-ar`. The
`.so` files stay in the target directory. `llvm-readelf --dyn-syms` lists
the exported `uniffi_openbmt_key_ffi_*` symbols.

The generated Kotlin calls the library through JNA, so the Android app
needs the `net.java.dev.jna:jna` AAR as a dependency and the `.so` files in
`jniLibs`.

Not built: restore-and-sign mode (a key from words together with a
signer that starts without allowlist history, `docs/DESIGN.md` section
5.4), the kit's other fields in `checkKit`, and any use of a code's amount
or `message` beyond reading them.

Not covered: compiling or running the Kotlin, loading the library on a
device or emulator, and the Android Keystore wrapping.

## Dependencies

`uniffi` 0.32.2 (exact pin, MPL-2.0), without its default features (the
`cargo-metadata` feature pulls in crates that need a newer Rust than 1.89).
The `bindgen` feature, off by default, adds the generator. `unsafe` is
allowed in this crate only, for the code UniFFI generates; no hand-written
line uses it, which a test checks.
