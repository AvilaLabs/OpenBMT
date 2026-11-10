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
| Key app library | `deps/openbmt-key-ffi.txt` | 20 |

Where they come from:

- **The base, 19.** The `bitcoin` crate family (`bitcoin`, `secp256k1`,
  `miniscript`, `bip39`, hashes, bech32, base58) and what builds the C
  code of `secp256k1` (`cc` and two helpers, build time only).
- **The link between devices, about 16 more.** ChaCha20-Poly1305 from the
  RustCrypto project, used to seal messages (LK-02).
- **Encrypted peer connections, 3 more.** The `bip324` crate and its two
  helpers, from the rust-bitcoin project, with no further dependencies.
- **The books export, 4 more.** Argon2, for the passphrase (BK-24).
- **The Key app library, 1 more.** `jni-sys`, the declarations of the
  Java native interface, with no dependencies of its own. The Key app
  holds a signing key and has the smallest set of all: the base plus
  this one crate. Its binding to the app is written by hand, in place of
  a binding generator that brought in about 40 packages.

Two implementations of ChaCha20-Poly1305 are in the counter: RustCrypto's
for sealed messages and rust-bitcoin's inside `bip324`. Using the second
for both would remove about 13 packages. It was looked at and not done:
its tag comparison is not constant-time and it has had less review, and a
smaller list is not worth a weaker primitive.

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
