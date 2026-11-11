# OpenBMT Counter app for Android

The first build of the cashier's screen: a real APK that runs the counter
(`openbmt-counter-ffi`) on plain `android.widget` screens. It is built with
the Android SDK's own tools and no Gradle, no AndroidX, no Jetpack Compose and
no library beyond the Kotlin standard library. The code shared with the Key
app is in `apps/android-common`.

**First build. No independent security audit has been done. It has not been
run on a phone or an emulator. Not for real funds.** The APK is signed with a
local test key; release signing is a separate step (see "Signing").

## What it does

| Screen | What it shows |
|---|---|
| First start | The counter's name, the manager's name and PIN, the address of Orbot, and Pair with the Key app. |
| Pairing | Scan the Key app's offer; the counter's own request as a code with its four words and They match / They differ; scan the owner phone's bundle. |
| Home | Whether Bitcoin is ready, the price and each source's note, New sale, Sales, the cashier's name, About. |
| New sale | A keypad in the shop's currency. |
| Sale | The payment code, the amount in currency and in sats, what has arrived, the library's words in the colour of their tone, Requote, Cancel, Override, Copy. |
| Sales and details | The sales the counter holds, one sale's judgment and its refunds. |
| Refunds | Ask for a refund, withdraw a request, refund in cash. |
| About | The version, what is not built, "Not for real funds", the run-while-visible note. |

Every refusal of the library is shown through its `words`; the app's own
states and faults have words written in `src/model/AppWords.kt` under
`docs/SCREEN-RULES.md`, listed in `docs/screens/counter-app.md`.

Pairing is the camera's (`docs/COUNTER-RULES.md` CR-90 to CR-99, DESIGN
section 2.9). On the first screen the manager types the counter's name, a PIN
and the address of Orbot and taps Pair. The app then asks for the camera, only
when Scan is tapped, and reads the Key app's offer (`ur:bytes`, whole or in
parts) and hands it to the library's `pair_begin`. The library makes the
counter's own key and a fresh number from the system's random source and
returns the counter's request and four words. The screen shows the four words
as its largest text and the request as a code that changes if it needs several
parts (`AnimatedQrView`), with They match and They differ. They differ ends the
ceremony, and the words say to start again from the Key app. They match is the
only way to the camera for the owner phone's bundle; the bundle goes to
`pair_finish`, which checks it (CR-92 to CR-96) and returns the stored pairing
record. The app seals that record with an Android Keystore key and hands it
back at every start (`StartConfig`); it keeps nothing of a ceremony that is
under way. If the app is left in the middle, the library forgets the ceremony
and the next first screen says so. There is no pairing text, no paste and no
file picker.

The UR reading of a code that changes is described in `docs/UR-RULES.md`
(UR-20): after the first `ur:` part the camera stays on, each part goes to one
decoder of the library, and the page says "Reading the animated code: N of
about M parts" until the message is complete. Any other type of UR gets the
words "Not the code expected". The words of reading are listed in
`docs/screens/counter-app.md`.

Not built yet: a lock screen of the app's own, a list of cashiers
and managers with their own PINs, end-of-day totals and receipts, and changes
to the shop's settings after pairing.

## The camera scan

Scan reads one QR code with the platform's Camera2 only (no AndroidX, no
CameraX, no ML Kit, no ZXing at runtime). `apps/android-common`'s `QrScanner`
takes frames from an `ImageReader` (YUV_420_888, near 1280 by 720), keeps the
Y plane with its row stride, and gives it to the reader of `crates/openbmt-qr`
through one JNI function (`crates/openbmt-qr-jni`) on a background thread, at
most about eight times a second, the latest frame only. The camera is released
on the first code, on Cancel and when the app is not visible. Nothing is kept
from a frame: it is zeroed after it is read and never written to disk or sent
anywhere (CR-90). The scan page has a preview so the manager can aim.

If the permission is refused, or "don't ask again" was chosen, a words page
says the camera is off for this app, that pairing needs it, and to allow it in
the phone's settings, and that a manager does this.

## The pairing record

What the library returns from `pair_finish` is one framed record (`OBMt`,
CR-97): the counter's identity secret, the two roots, the wallet check, the
network, the settings floor, the signed settings, the first batch and the
peers. The app does not read it: it seals the bytes with an Android Keystore
key (AES-GCM) in `noBackupFilesDir`, and gives them to `Shop.pairing` (for the
network and the Tor setting, display only) and to `StartConfig`. The library
checks the record again at every start.

## Rules the app keeps without a device

- **Two permissions, INTERNET and CAMERA.** The counter connects to the
  Bitcoin network and fetches the price (INTERNET). CAMERA is asked for at run
  time only when the manager taps Scan, to read the pairing code (CR-90); a
  `uses-feature` for `android.hardware.camera.any` says the camera is not
  required. `tests/check-manifest.sh` reads the built APK with `aapt2` and
  fails the build if the list differs from exactly these.
- **No backup.** `allowBackup="false"`, and `dataExtractionRules` exclude every
  domain, for cloud backup and for device transfer.
- **No exported component but the launcher activity.** No service, receiver
  or provider.
- **No `FLAG_SECURE`.** The counter holds no spending key, and the sale
  screen is made to be seen by the customer, who scans the code from the
  cashier's screen. A blank screen in recents would protect nothing of
  value here.
- **The node runs only while visible (CR-88).** It starts when the activity
  is visible and stops, cleanly, when it is not; the screen is kept on
  (`FLAG_KEEP_SCREEN_ON`) while it is open. There is no foreground service and
  no notification, so the app must be kept open at the counter. The About
  screen says so.
- **The manager's PIN (CR-84 to CR-86).** PBKDF2-HMAC-SHA256, 600,000
  iterations, a 16-byte salt, in `noBackupFilesDir`, compared in constant
  time. A wrong try holds the PIN for 1, 2, 4 seconds and so on, up to 5
  minutes, and the count is written before the answer is given. Cashier
  actions need no PIN. An override, and a refund asked for or withdrawn by a
  manager, are made only after the PIN was checked, and name their actor.
- **Tor (CR-87, NR-26).** When the signed settings have Tor on, the node and
  every price fetch go through Orbot's SOCKS proxy (`127.0.0.1:9050`, a
  setting of the first screen). If the proxy does not answer, nothing is sent
  around it: the node is not started and no price is fetched. The app says "Tor
  is on for this shop, but Orbot is not running." and "Open Orbot and turn
  it on."
- **The price (CR-89, RT-40 to RT-45).** One `HttpsURLConnection` per source
  on a fixed 60-second grid, never because a sale opened; a 64 KiB read limit;
  a 20-second timeout; no cookies, no cache, no redirect, no compression; a
  fixed generic User-Agent. The bytes go to the library (`rate_parse`,
  `rate_combine`); no JSON is parsed in Kotlin.
- **Threads.** One background thread runs `pump(250)` and posts what it
  reports to the screen thread. Every other call into the library is made on
  the screen thread, except the start, which runs before the pump begins, and
  the PIN's PBKDF2, which runs off the screen thread.

## Build

```
scripts/counter-apk-build.sh
```

Needs a JDK 21, the Android SDK (platform 36, build-tools 36.0.0), the NDK,
and Kotlin 2.2.20, as for the Key app (`apps/key-android/README.md`). It runs
the JVM tests, builds the libraries for `arm64-v8a` and `x86_64` through
`scripts/counter-ffi-build.sh` and `scripts/qr-jni-build.sh`, compiles with
`-Werror`, dexes, aligns for 16 KiB pages, signs with a local test key and
checks the manifest. The result is under
`~/.cache/openbmt/target/counterapk/apk`, never in the repository.

## Tests

- `tests/run-jvm.sh` runs the common model tests, then the counter's model
  tests (PIN store, pairing steps, amount entry, QR layout, price schedule,
  Tor gate, sale model, screen choice, words). They need no device.
  `OPENBMT_WRITE_SCREENS=1` rewrites `docs/screens/counter-app.md`.
- `apps/android-common/tests/qr-jni-test.sh` builds the QR reader for the
  host and reads frames made by the repository's encoder through the same
  Kotlin calls the scan makes, then runs the UR call and a reading of a
  sequence through the Kotlin the apps use. The build runs it.
- `tests/check-manifest.sh APK` checks the permission list (exactly INTERNET
  and CAMERA, the camera not required), backup, components, SDK levels.

## Signing

The build makes a local test key with `keytool` the first time and keeps it
under `~/.cache/openbmt/tmp/counterapk/`. It is not committed and it is not a
release key.

## Not checked without a device

Everything that needs a running Android: that the library loads and starts,
how the screens look and lay out, the keyboard types of the fields, the
camera scan (the permission dialog, the preview, reading a code
from a screen and from paper, reading a code that changes at five parts a
second, releasing the camera), the Keystore sealing, the
SOCKS probe against a real Orbot, real fetches of the sources, the QR code
scanned by a wallet, and the node stopping cleanly when the app is sent to the
background.
