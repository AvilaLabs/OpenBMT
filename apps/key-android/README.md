# OpenBMT Key app for Android

The first build of the Key app: a real APK that shows the words of
`openbmt-key-ffi` on plain `android.widget` screens. It is built with the
Android SDK's own tools and no Gradle, no AndroidX, no Jetpack Compose and no
library beyond the Kotlin standard library.

**First build. No independent security audit has been done. It has not been
run on a phone or an emulator. Not for real funds.** The APK is signed with a
local test key; release signing is a separate step (see "Signing").

## What it does

| Screen | What it shows |
|---|---|
| Refusal | Full-screen words when USB or wireless debugging or an accessibility service is on. Nothing else is offered. |
| First start | Make key 2 from the device, dice (hashed or skew-corrected), coin flips or typed words, or be an approver only. |
| The words | The new key's words, once. |
| Wallet | Accept the wallet (pasted description, the owner's identity typed in), the check words, the kit check. |
| Approval session | A request is picked from a file or scanned (one code or a code that changes), its lines are shown one by one, each is approved or declined with a reason, and the answer is saved as a file or shown as a code. |
| Limits and status | The ceilings, marks, totals and saved addresses, and a form to change the ceilings. |

Every refusal of the library is shown through its `words`; the app's own
states and faults have words written in `src/model/AppWords.kt` under
`docs/SCREEN-RULES.md`. A `WordsView` draws a `Words`: the colour from the
tone, the heading, what happened, what to do next, who does it, quoted text
marked as from elsewhere, and a "Details" fold with the rule, the time and the
detail.

Every place that takes pasted text or a file has a third choice, Scan, which
reads one QR code with the phone's camera (see "The camera scan" below).

A request can also be read from a code that changes, and the answer can be
shown as one (see "Animated codes" below).

Pair a counter: from the home screen (reached once the wallet is accepted) the
app shows its pairing offer as a `ur:bytes` code (`pairOfferBytes`), headed
"Scan this with the counter". The offer names the owner phone's key, this
app's key, the wallet's ID and the network, and holds nothing secret. The
counter reads it, makes its own signed request and shows four words; the
owner phone carries that request in an approval session, and the Key app's
pairing line reads "Pair counter" with the counter's name in quotation marks,
the four words as the largest text on the screen, and the instruction to
compare them with the counter's screen (`LineModel`, tested in
`tests/AppModelTest.kt`).

Not built yet: the allowlist enrolment, a lock screen of the app's own, and
the wipe after wrong tries.

## Rules the app keeps without a device

- **No network permission; only the camera.** The manifest has one
  `uses-permission`, `CAMERA`, and a `uses-feature` for
  `android.hardware.camera.any` that is not required. Signing data comes in
  by QR code, and the camera is no network path: the app has no `INTERNET`
  permission, so nothing it reads or shows can leave the phone by a network.
  The permission is asked for at run time, only when the person taps Scan.
  `tests/check-manifest.sh` reads the built APK's manifest with `aapt2` and
  fails the build if the list differs from exactly that.
- **No backup.** `allowBackup="false"`, `fullBackupContent` and
  `dataExtractionRules` exclude every domain, for cloud backup and for device
  transfer.
- **No exported component but the launcher activity.** No service, receiver
  or provider.
- **`FLAG_SECURE` on the window**, so no screenshot, no recording and a blank
  recent-apps preview. The app opens no dialog (a dialog is another window).
- **Typed words stay with the app's own field.** Every text field has
  `textNoSuggestions|textVisiblePassword` and
  `IME_FLAG_NO_PERSONALIZED_LEARNING`, so a keyboard is asked not to
  suggest, remember or learn what is typed. This asks the keyboard; it does
  not stop a keyboard that ignores it.
- **Refuses to run** while `Settings.Global.ADB_ENABLED` or the wireless
  debugging setting is 1, or an accessibility service is on, at start, at each
  return to the app and before each step that changes anything. A setting that
  cannot be read counts as on. Views also set `filterTouchesWhenObscured`.
- **The saved state is encrypted** with AES-256-GCM under an Android Keystore
  key (StrongBox when the phone has one, the trusted environment otherwise).
  When the system reports software only, the app says so on screen
  ("Key kept in software"). Each write gets a fresh 12-byte IV and goes to a
  temporary file in `noBackupFilesDir`, is synced, then renamed over the real
  file. `load` returns null on a first start and throws on a file that does not
  decrypt, and the app then shows the library's words and writes nothing.
- **The pairing offer holds nothing secret and is drawn only from the
  library.** "Pair a counter" shows the bytes of `pairOfferBytes` as a code
  and nothing else: they are drawn only when they have the length and the
  frame of an offer (`PairOfferModel`), and the screen is reachable only from
  the home screen, which exists once the wallet is accepted. The code names
  the roots, the wallet's ID and the network; it carries no seed, no key
  material and no setting.
- **The pairing line puts the four words first.** The line of a pairing item
  is headed "Pair counter" with the counter's name, cleaned and in quotation
  marks (the name was typed on another device, SC-08), shows the four words
  as the largest text on the screen and says to compare all four with the
  counter's screen before approving. A pairing whose words could not be
  worked out says so and cannot be approved.

## The camera scan

Scan reads one QR code with the platform's Camera2 only: no AndroidX, no
CameraX, no ML Kit and no ZXing at runtime. `apps/android-common`'s
`QrScanner` takes frames from an `ImageReader` (YUV_420_888, near 1280 by
720), keeps only the Y plane with its row stride, and gives it to the reader
of `crates/openbmt-qr` through one JNI function (`crates/openbmt-qr-jni`), on
a background thread, at most about eight times a second, the latest frame
only. The camera is released on the first code, on Cancel and when the app
goes to the background. A frame is zeroed after it is read and is never
written to a file or sent anywhere. The scan page has a preview so the
person can aim.

- Scan fills the field next to it (the wallet description, the owner's
  identity, key 3's account key, the wallet text of the printed kit) with the
  text of the code, or, on the home screen, opens a request from the bytes of
  the code, as a request file would.
- The camera permission is asked for only when Scan is tapped. If it is
  refused, or "don't ask again" was chosen, a words page says the camera is
  off for this app, to allow it in the phone's settings or to paste or choose
  a file instead, and that the key holder does this.
- A code whose text starts with `UR:` or `ur:`, scanned into a text field
  (the wallet description, an identity, a key), gets words that say the
  field takes plain text. Only the home screen's Scan reads a UR.

## Animated codes

A request or an answer can be too long for one QR code, so it travels as a
BC-UR sequence (`docs/UR-RULES.md`, `crates/openbmt-ur`): the parts of one
message, shown one after another.

- **Types (UR-20).** OpenBMT's own framed requests and answers travel as
  `ur:bytes`. `ur:crypto-psbt` is for a plain PSBT to or from another wallet
  and nothing else. The home screen's Scan takes both types; the library
  then decides whether the bytes are a request it can answer. Any other type
  gets the words "Not the code expected".
- **Reading.** Scan on the home screen opens a request from one plain code,
  one whole `ur:` code, or a code that changes. After the first `ur:` part
  the camera stays on, each part goes to one decoder of the library, and the
  page says "Reading the animated code: N of about M parts" until the
  message is complete. A part that is misread, or from another code, is
  skipped. If the parts join to a message that fails its checks, the words
  say so and reading starts again from the first part. The decoder is
  dropped when the scan finishes, is cancelled or the app goes to the
  background. The camera rule and the manifest are those of the camera scan.
- **Showing.** When the last line has an answer, "Show as code" sits beside
  "Save the answer file", and the home screen has "Show the last answer as a
  code" while a saved answer is held. The answer is shown as `ur:bytes`: one
  still code when it fits one part, otherwise the parts one after another,
  five a second, round and again, until Back. Each part is drawn with a
  quiet zone of 4 modules, in upper case for the alphanumeric mode, with at
  most 230 bytes of the message in a part so that no frame is over QR
  version 12 at level L (a test of the library draws every part of messages
  up to 100 KB and asserts it). If no code can be made, the words say to save
  the file instead.

The 230 bytes and the five parts a second are first guesses, to be tuned on
a device.

## Build

```sh
scripts/key-apk-build.sh
```

The script checks each tool and stops with a message when one is missing, runs
the JVM tests (and the harness test of the camera scan's native reader,
`apps/android-common/tests/qr-jni-test.sh`), builds the Rust libraries for
`aarch64-linux-android` and `x86_64-linux-android`
(`scripts/key-ffi-build.sh` and `scripts/qr-jni-build.sh`, then strips
them), compiles
the resources (`aapt2`), the Kotlin (`kotlinc` against `android-36`'s
`android.jar` and the wrapper in `crates/openbmt-key-ffi/bindings/kotlin`),
dexes with `d8 --release --min-api 29` together with `kotlin-stdlib.jar`,
assembles the APK with the `.so` files stored under `lib/<abi>/`, aligns it
(`zipalign -P 16 -f 4`), signs it with `apksigner`, then runs
`apksigner verify --print-certs`, `aapt2 dump badging` and the manifest
checks, and prints the size and the SHA-256. All output goes to
`~/.cache/openbmt/target/keyapk/apk/`, never into the repository.

Tools (none is downloaded):

| Tool | Where the script looks |
|---|---|
| JDK 21 | `JAVA_HOME`, default `~/Android/jdk21` |
| Kotlin 2.2.20 compiler | `KOTLIN_HOME`, default `~/.cache/openbmt/tools/kotlinc` (the `kotlin-compiler-2.2.20.zip` release, whose SHA-256 CI checks); the stdlib's SHA-256 is pinned in the script |
| Android SDK, platform `android-36`, build-tools `36.0.0` | `ANDROID_SDK`, default `~/Android/Sdk` |
| NDK `26.3.11579264` | `ANDROID_NDK_HOME` |
| Rust with the two Android targets | `rustup` |

The Kotlin compiler is not part of the repository.

## Tests

`tests/run-jvm.sh` compiles the wrapper, the pure sources in `src/model/` and
`tests/AppModelTest.kt` on the desktop JVM and runs the test. No Android class
is used in `src/model/`: the screen model, the words model, the app's own words
(checked against the rules of `openbmt_core::say::check` as far as text can be,
and against the rows of `docs/screens/key.md`), the line text, and the saved
state's file format with a fake cipher. `src/android/` holds everything that
needs the framework. The tests of the shared code (`apps/android-common/tests`)
also run the camera scan's pure parts (choosing a frame size, copying a Y
plane with its row and pixel stride, the throttle, the reader's reply framing,
the UR check) and the animated code's pure parts (the framing of the UR call
with every cut and changed byte of a reply, the frame timing and layout, the
reading of a sequence step by step, the words), and `qr-jni-test.sh` loads
the host-built `libopenbmt_qr_jni.so`, reads frames made by the repository's
encoder, draws every part of sequences for both types and asserts the QR
version, and reads a shuffled, lossy sequence back to its message.

## Signing

The build makes a throwaway test key with `keytool` under
`~/.cache/openbmt/tmp/keyapk/` and signs with it. It is not a release key and
is never committed. A release is signed in a separate step with the Key app's
own release key (`docs/DESIGN.md`, "Release key"), held by a second person
and kept offline.

## What is not checked

Nothing here has run on a device. Not checked: that the library loads and
answers on a phone, the Keystore and StrongBox behaviour, the file pickers,
the camera scan (the permission dialog, the preview and its shape, that a
code on a screen and on paper is read, how long it takes, the camera being
released when the app is left), the animated code (that another phone, a
wallet or a camera reads the parts at five a second and at 230 bytes a
part, how long a sequence takes, how the parts look on a screen, the camera
held on a code that changes), the layout, the keyboard flags, `FLAG_SECURE`
in the recent-apps list, and the refusal when debugging or an accessibility
service is on.
