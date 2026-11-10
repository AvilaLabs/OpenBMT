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
| Approval session | A request file is picked, its lines are shown one by one, each is approved or declined with a reason, and the answer file is saved. |
| Limits and status | The ceilings, marks, totals and saved addresses, and a form to change the ceilings. |

Every refusal of the library is shown through its `words`; the app's own
states and faults have words written in `src/model/AppWords.kt` under
`docs/SCREEN-RULES.md`. A `WordsView` draws a `Words`: the colour from the
tone, the heading, what happened, what to do next, who does it, quoted text
marked as from elsewhere, and a "Details" fold with the rule, the time and the
detail.

Not built yet: a camera and QR (so every scan is a paste or a file here), the
allowlist enrolment, the pairing offer, BC-UR frames, a lock screen of the
app's own, and the wipe after wrong tries.

## Rules the app keeps without a device

- **No permission.** The manifest has no `uses-permission` of any kind, so no
  network permission either. `tests/check-manifest.sh` reads the built APK's
  manifest with `aapt2` and fails the build if one appears.
- **No backup.** `allowBackup="false"`, `fullBackupContent` and
  `dataExtractionRules` exclude every domain, for cloud backup and for device
  transfer.
- **No exported component but the launcher activity.** No service, receiver
  or provider.
- **`FLAG_SECURE` on the window**, so no screenshot, no recording and a blank
  recent-apps preview. The app opens no dialog (a dialog is another window).
- **Typed words stay with the app's own field.** Every text field has
  `textNoSuggestions|textVisiblePassword` and
  `IME_FLAG_NO_PERSONALIZED_LEARNING`, so a keyboard is asked not to suggest, remember or learn what is typed. This
  asks the keyboard; it does not stop a keyboard that ignores it.
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

## Build

```sh
scripts/key-apk-build.sh
```

The script checks each tool and stops with a message when one is missing, runs
the JVM tests, builds the Rust library for `aarch64-linux-android` and
`x86_64-linux-android` (`scripts/key-ffi-build.sh`, then strips them), compiles
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
needs the framework.

## Signing

The build makes a throwaway test key with `keytool` under
`~/.cache/openbmt/tmp/keyapk/` and signs with it. It is not a release key and
is never committed. A release is signed in a separate step with the Key app's
own release key (`docs/DESIGN.md`, "Release key"), held by a second person
and kept offline.

## What is not checked

Nothing here has run on a device. Not checked: that the library loads and
answers on a phone, the Keystore and StrongBox behaviour, the file pickers,
the layout, the keyboard flags, `FLAG_SECURE` in the recent-apps list, and the
refusal when debugging or an accessibility service is on.
