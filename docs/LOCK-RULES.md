# Lock rules: the PIN around a phone's key file

Status: specification for `crates/openbmt-lock`, moved out of
`crates/openbmt-key-ffi` so that the Key app and the owner app keep the key
they hold in the same frame, with the same cost and the same PIN rules.
Each rule has an ID so that a test can name the rule it proves
(`lc_NN_*`, in `crates/openbmt-lock/src/tests/mod.rs`). The rules serve
`docs/DESIGN.md` §5.4 ("Phone key rules": the file key comes from the PIN or
passphrase through a memory-hard function together with a Keystore key; a
PIN that is only a screen gate is not allowed).

The crate locks bytes. It has no storage, no clock and no random source:
the app draws the salt and the nonce, stores what comes back, counts wrong
tries and wipes (`docs/SIGNER-RULES.md` SR-74 to SR-77 for the Key app,
`docs/OWNER-APP-RULES.md` OA-60 to OA-65 for the owner app). The Android
Keystore wraps the locked frame outside, so that the device and the PIN are
both needed.

**First build. No independent security audit has been done. Not for real
funds.**

## 1. The frame

- **LC-01. The locked frame.** The frame is `OBMx`, version 1 or 2: the
  Argon2id parameters (memory in KiB, passes and lanes, each 4 bytes), a
  16-byte salt, a 12-byte nonce, the length of the ciphertext, and the
  ChaCha20-Poly1305 ciphertext of the bytes locked, with the whole header as
  associated data. In version 2 the first 8 sealed bytes are the generation
  of the stored state, a little-endian `u64` that every save of the Key app
  raises by one (`docs/SIGNER-RULES.md` SR-92); it is read only with the
  key. A frame of version 1 reads as generation 0. The owner app writes
  version 1; the Key app writes version 2, and a version 1 frame it opens
  is written in version 2 at its next save. Decoding is strict: version 1 or
  2, the exact length, a ciphertext of at least the 16 bytes of the tag (24
  in version 2, with the generation) and at most 80 MiB, and only bytes
  that writing the frame again would give. A frame that is cut, lengthened
  or of another form is refused as unreadable. The bytes the Key app wrote
  before this crate existed open unchanged.
- **LC-02. The cost.** The parameters are 64 MiB, 3 passes and 1 lane. A
  frame that asks for more than 256 MiB, 10 passes or 4 lanes, or for no
  passes, no lanes or fewer than 8 blocks per lane, is refused when it is
  read, before Argon2 runs, and is never opened with a PIN.

## 2. The PIN

- **LC-03. The shape of a PIN.** A PIN is at least 6 digits, or a passphrase
  of at least 8 characters, and at most 128 characters in either case
  (`pin-weak`, `pin-long`). Characters are counted as characters, not
  bytes. A PIN that is refused locks nothing.
- **LC-04. A nonce is not repeated.** Every write has a new nonce, and a
  write whose nonce would repeat the last one under the same key is
  refused. The nonce is remembered before the frame is returned, so that a
  write that fails after it still never repeats it.
- **LC-05. A wrong PIN.** A wrong PIN and a frame whose ciphertext, salt,
  nonce or parameters were changed give the same answer: the tag does not
  verify, and nothing is returned. The key made from a PIN is overwritten
  when it is dropped, and so are the bytes opened inside a call that does
  not hand them on.
- **LC-06. Changing the PIN.** The current PIN is needed. The new PIN is
  checked against LC-03, a new salt and a new nonce are used, and the same
  bytes are locked again in one step: nothing is returned unless the
  current PIN opened the frame and the new PIN was taken. A salt or a nonce
  that is the old frame's own is refused. A frame of version 1 stays version
  1; a frame of version 2 keeps its contents, with the generation raised by
  one, and the Key app stores it only when the stored generation is still
  the one it opened (SR-92).

## 3. The cost under test

- **LC-07. The cheap cost exists only under test and fuzzing.** The macro
  `cost!` gives the standard parameters, and small ones only where
  `cfg(test)` or `cfg(fuzzing)` is set in the crate that uses it. A release
  build of the crate and of each app library asks for the standard cost, and
  a test reads the source to check it.

## 4. What these rules do not do

- **They do not count tries or wipe.** A PIN of six digits can be tried a
  million times by anyone who holds the locked bytes and a computer, at the
  cost of Argon2 each time; what stops a thief with the phone is the app's
  count of wrong tries and its wipe, and the Keystore key that wraps the
  frame. Neither is in this crate.
- **They do not hold the key anywhere safe.** The key lives in the memory of
  the app while a file is open. It is overwritten when dropped where the
  types allow it; the PIN text the app passes in is the app's to clear.
- **They do not audit the cipher.** Argon2id and ChaCha20-Poly1305 are the
  RustCrypto crates, at the versions of `docs/DEPENDENCIES.md`.
