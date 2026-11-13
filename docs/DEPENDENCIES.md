# Dependencies: what is allowed in, and how it changes

Every package OpenBMT is built from is code that runs with the same rights
as OpenBMT's own. This page is the policy for it. The check that enforces
the parts a machine can enforce is `scripts/deps-check.sh`, run in CI.

## Where things stand

The numbers are external packages, normal and build dependencies together,
for each program that ships. The lists themselves are in `deps/`.

| Program | List | Packages |
|---|---|---|
| Signer and core rules | `deps/openbmt-signer.txt` | 19 |
| Counter | `deps/openbmt-counter-node.txt` | 38 |
| Owner | `deps/openbmt-owner-node.txt` | 38 |
| Box | `deps/openbmt-box.txt` | 38 |
| Books export | `deps/openbmt-books.txt` | 42 |
| Key app library | `deps/openbmt-key-ffi.txt` | 43 |
| Counter app library | `deps/openbmt-counter-ffi.txt` | 39 |
| Owner app library | `deps/openbmt-owner-ffi.txt` | 47 |
| QR code encoder | `deps/openbmt-qr.txt` | 0 |
| Price sources | `deps/openbmt-rates.txt` | 19 |

Where they come from:

- **The base, 19.** The `bitcoin` crate family (`bitcoin`, `secp256k1`,
  `miniscript`, `bip39`, hashes, bech32, base58) and what builds the C
  code of `secp256k1` (`cc` and two helpers, build time only).
  The `secp-recovery` feature of `bitcoin` is on in `openbmt-core`
  (`docs/APPROVAL-RULES.md`, AP-07): the signature of a message made by a
  wallet gives the key that signed it. It is one more module of the C code of
  the same `secp256k1` package, and adds no package to any list.
- **The link between devices, about 16 more.** ChaCha20-Poly1305 from the
  RustCrypto project, used to seal messages (LK-02).
- WORDS_NEEDED_DEPS_ED25519
- **Encrypted peer connections, 3 more.** The `bip324` crate and its two
  helpers, from the rust-bitcoin project, with no further dependencies.
- **The books export, 4 more.** Argon2, for the passphrase (BK-24).
- **The Key app library, 24 more.** `jni-sys`, the declarations of the
  Java native interface, with no dependencies of its own, and the PIN lock
  of the stored state (SR-71): Argon2 and ChaCha20-Poly1305, the same
  versions and features as the books export, with 21 helpers between
  them, all of which are in the lock file already. They come in through
  `openbmt-lock` (`docs/LOCK-RULES.md`), the workspace crate that holds the
  lock and that the owner app library uses as well. Its binding to the
  app is written by hand, in place of a binding generator that brought
  in about 40 packages.
- **The counter app library, the same 1 more.** The counter's list and
  `jni-sys`, bound the same way by hand. It adds nothing else.
- **The owner app library, 8 more.** The owner's list and `jni-sys`, bound
  the same way by hand, and the PIN lock on key 1 (OA-60): `openbmt-lock`
  brings Argon2 and its 6 helpers that the Key app library already lists
  (`argon2`, `base64ct`, `blake2`, `block-buffer`, `digest`, `password-hash`,
  `rand_core`); ChaCha20-Poly1305 was in the list for the link. Every one
  of them is in the lock file already, at the same version: this adds no
  package to it.
- **The owner app library, the starter wallet's randomness, 1 more.**
  `libc`, the system call layer, for the one `getrandom` call that gives
  the starter wallet's key its 32 bytes (`docs/START-SIMPLE-RULES.md`,
  SS-01). The call is in the workspace crate `openbmt-kernel-random`, the only
  place that holds the `unsafe` block, in place of the `getrandom` crate,
  which is not in the lock file. `libc` is in the lock file already, at the
  same version as the stick's: this adds no package to it. The sealed seed
  (SS-20) is ChaCha20-Poly1305 from the link's list: no package.

- **The QR code encoder, none.** `openbmt-qr` has no dependencies of its
  own, not even for tests. The counter, the owner and the Key apps show
  codes, and a QR library would have been a new package in each list; the
  encoder is about a thousand lines in the repository, checked against
  ZXing (below) bit for bit.

- **The BC-UR transport, none.** `openbmt-ur` has no dependencies, not even
  a workspace crate. SHA-256 and CRC-32 are written in it (about 100 lines,
  checked against `sha256sum` and Python's `zlib.crc32`), so that the apps
  that show or read an animated QR add nothing to their lists
  (`docs/UR-RULES.md`). It is not yet in a shipped program's list.
  Sparrow's Hummingbird library (with its one dependency, `cbor`) is a test
  tool only, as ZXing is
  for `openbmt-qr`: `scripts/ur-crosscheck.sh` fetches the two jars against
  pinned SHA-256 values and runs a harness; nothing of it is built into a
  program or listed in `deps/`.

- **The price sources, the same 19 as the signer's.** `openbmt-rates` reads
  the replies of the price sources and combines them into the counter's rate
  (`docs/RATE-RULES.md`). It depends on `openbmt-core` and `openbmt-counter`
  by path and adds no package of its own: the JSON reader is about 250 lines
  in the crate, so that the one place that reads an untrusted reply has no
  parser from outside, and is fuzzed.

Two implementations of ChaCha20-Poly1305 are in the counter: RustCrypto's
for sealed messages and rust-bitcoin's inside `bip324`. Using the second
for both would remove about 13 packages. It was looked at and not done:
its tag comparison is not constant-time and it has had less review, and a
smaller list is not worth a weaker primitive.

## Packaging

The Start9 package of the Box (`packaging/box/startos/`) is written in
TypeScript against the `@start9labs/start-sdk` npm package, pinned by
`package-lock.json` (the SDK to an exact version, everything else by the
lockfile's versions and hashes). None of these packages is in the Box binary
or in the container image, which is built from the Rust workspace alone
(`packaging/box/Dockerfile`).

They are not build-time only, though. The SDK bundles the package's
TypeScript, with the parts of the SDK it imports, into the JavaScript of the
`.s9pk` file, and StartOS runs that JavaScript on the shop's server to start,
stop, configure and back up the Box. So the SDK and the parts of its lockfile
that end up in that bundle are code that runs at the shop. They are not
counted in `deps/` and are not checked by `scripts/deps-check.sh` or the Rust
advisory check; a change of the SDK version is reviewed as a change of
running code, and its advisories are checked by hand before a release.
`node_modules/` is not committed. CI installs them with `npm ci`, which
refuses a lockfile that does not match `package.json`.

## Rules

1. **A new package needs a reason that is written down.** The commit that
   adds a line to a list in `deps/` says what the package is for, who
   maintains it, what it pulls in, and what was considered instead
   (including writing the code here). CI fails when a program's packages
   are not exactly its list, so nothing arrives by accident, and nothing
   arrives through another package's update.
2. **Exact versions.** Every dependency is pinned with `=` and the lock
   files are committed. Builds use `--locked`.
3. **crates.io only.** No git dependencies and no other registries.
4. **Default features off** unless a default is needed, and the reason is
   a comment beside the dependency.
5. **Build-time code counts.** A build script or a procedural macro runs
   on the build machine with the builder's rights. It is listed and
   weighed like any other package.
6. **Licences** must be on `deps/licences.txt`.
7. **Known problems.** CI runs `cargo audit` against the RustSec advisory
   database on every push. An advisory fails the build until the package
   is updated or removed, or the advisory is shown not to apply and is
   listed with the reason in `.cargo/audit.toml`.
8. **Updates are deliberate.** Nothing updates on its own. A version
   change is its own commit; the upstream changes between the two
   versions are read first, and the commit says what was read. A release
   less than two weeks old is not taken unless it fixes a security
   problem.
9. **Tests and fuzzing may use more.** Development-only packages are not
   on the lists, are never compiled into a shipped program, and follow
   rules 2, 3, 6 and 7.

## The Android Key app

The Key app for Android (`apps/key-android/`) is built without Gradle,
without AndroidX and without Jetpack Compose. It has no Cargo package of its
own and is not in `deps/`; its Rust part is the `openbmt-key-ffi` list above.

- **One runtime library: the Kotlin standard library**, `kotlin-stdlib.jar`
  2.2.20, Apache License 2.0. `d8` puts the classes the app uses into the
  APK's dex. Nothing else is linked in: no AndroidX, no Compose, no HTTP or
  JSON library, no image or QR library. The build script checks the jar's
  SHA-256 against the value of the 2.2.20 release it was taken from.
- **Build tools, never shipped:** the Kotlin compiler 2.2.20 (not copied into
  the repository), JDK 21 with `keytool`, the Android SDK's `android.jar` of
  platform 36 and build-tools 36.0.0 (`aapt2`, `d8`, `zipalign`,
  `apksigner`), and the Android NDK 26.3.11579264 (clang, `llvm-strip`,
  `llvm-readelf`). They run on the builder's machine with the builder's
  rights and are trusted as the Rust toolchain is.
- **Signing key.** The build makes a throwaway test key with `keytool`. It is
  never committed and is not the release key.

## The built-in Tor of the Android Owner and Counter apps

The owner app (`apps/owner-android/`) and the counter app
(`apps/counter-android/`) run the Tor Project's C Tor themselves, so a shop
does not need Orbot (OA-97, CR-201). They take one prebuilt file from outside
the Rust lists: the native library of the Guardian Project's `tor-android`.

- **`tor-android` 0.4.9.14** (`info.guardianproject:tor-android`, AAR),
  licence **BSD 3-Clause** (the POM states it), which builds C Tor 0.4.9.14
  with OpenSSL, libevent, zlib and zstd into `libtor.so`. Published at
  `https://raw.githubusercontent.com/guardianproject/gpmaven/master/info/guardianproject/tor-android/0.4.9.14/tor-android-0.4.9.14.aar`
  (the Guardian Project's Maven repository) and the same bytes on Maven
  Central. SHA-256 of the AAR:
  `28ff2bb6d4d42eabe57183dedd7a97fce77e8ce4b6f5af6fa03993b3ca054edd`.
  `apps/android-common/tor/tor-library.sh`, which both builds call, refuses
  any other file and fails the build.
- **Only `libtor.so` is used**, for `arm64-v8a` and `x86_64`, stored in the
  APK like the app's own libraries. The AAR's Java code is not used: its
  `TorService` needs `jtorctl` and AndroidX, which the build does not have
  and the apps do not take. The app has its own class of the same
  name in `apps/android-common/tor/src/android/TorService.kt`, because the
  library's native functions are bound by that name; it follows the order
  of native calls of the library's `TorService.java` and says so, with its
  licence, in its header. Its control client is the strict reader in
  `apps/android-common/tor/src/model/TorControl.kt`, which sends five
  commands and no others. Both apps compile these files from the one place;
  the Key app does not.
- **Why.** The app is a client that must reach Bitcoin peers and a relay
  through Tor; with Orbot, a second app has to be installed, kept running
  and not stopped by Android. A setting keeps Orbot for those who prefer it.
- **An exception to rule 3.** It is not a crate and is on no list in
  `deps/`; the pinned hash is its list.
- **The cost.** A prebuilt binary in the APK that this repository does not
  build from source, and a Tor that changes only when the app is released.
  Updating it is a decision: a new AAR, its hash here and in the script.
  The build has not been made reproducible for it.
- **Where the file lives.** `~/.cache/openbmt/deps/tor-android/`, outside the
  repository, like the Kotlin compiler. `TORANDROID_AAR` names another place.
- The Key app makes no network connection and takes none of this: it does
  not compile the Tor part of `apps/android-common`, and its APK has no
  `libtor.so`.

## Test tools

Tools that check the code and are never part of a program or its lists.
They run on the builder's machine and follow rules 2 and 3 where they
apply: pinned, from the publisher's registry, checked against a digest.

- **ZXing core 3.5.3** (`com.google.zxing:core`, Apache License 2.0), a
  Java QR code library used only by `scripts/qr-crosscheck.sh` as an
  independent implementation: it encodes the same inputs and decodes the
  codes of `openbmt-qr`. The jar comes from Maven Central, is checked
  against the `.sha1` published beside it, and the script refuses a jar
  whose SHA-256 is not
  `8d8064c1636fdaef7189dd9055c7d59950a8940a12f2293956446ec3c109fd82`.
  The harness is run with JDK 21 (`javac`, `java`; no Maven or Gradle).
- **Hummingbird 1.7.6** (`com.sparrowwallet:hummingbird`) and its one
  runtime dependency **cbor 0.9** (`co.nstant.in:cbor`), the Java UR
  library of Sparrow, used only by `scripts/ur-crosscheck.sh` as an
  independent implementation of BC-UR: it encodes and decodes the same
  messages as `openbmt-ur`, and the parts are compared. The jars come from
  Maven Central, are checked against the `.sha1` published beside each, and
  the script refuses a jar whose SHA-256 is not
  `b236ede4c0ee1c348fa034d59c0af229a1760a967f36f5513f8c9569b1ec5b71`
  (Hummingbird) or
  `5ef465c47e34c6b13f93ece2ff49e94b933d69cf9f172cc4997418303884da73`
  (cbor). The harness is run with JDK 21. Both are under the Apache
  License 2.0, as their Maven Central POM files state; nothing of them is
  shipped.

## What this does not do

- **It does not review the packages.** Reading a diff before an update is
  not an audit. No independent review of the dependency set has been
  done yet.
- **It does not vendor.** The design asks for the Key app's crates to be
  copied into the tree and reviewed by someone other than the author of
  the app; that is not done yet.
- **It does not make builds reproducible.** That is separate work, and a
  launch condition for the Key app.
- **It trusts crates.io and the Rust toolchain** to deliver what was
  published.
