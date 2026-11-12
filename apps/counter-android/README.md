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
| First start | The counter's name, the manager's name and PIN, and Pair with the Key app. |
| Network | The address of Orbot (`127.0.0.1:9050` unless changed), saved behind the manager's PIN (CR-87). Reached from home, and from the screen that says Orbot did not answer. |
| Tor | Only while the shop's settings have Tor on (CR-203): the words of the choice in use and the two choices, Built-in Tor (behind the manager's PIN) and Use Orbot (which goes on to the Network screen). Reached from Settings, and from the screen that says Tor did not start. |
| Pairing | Scan the Key app's offer; the counter's own request as a code with its four words and They match / They differ; scan the owner phone's bundle. |
| Home | Whether Bitcoin is ready, the price and each source's note, the own node's line with Try again (when one is set), the Box's line with Try again (when the pairing holds a relay, CR-136), New sale, Sales, Today, the cashier's name (or who is at the till with Lock), Staff, the Lightning line (only while a connection is saved, CR-116), Your own node, Lightning, About. |
| Your own node | The counter's own setting, behind the manager's PIN (CR-100): the Tor address of the shop's own node, saved or removed. A home-network address is refused, and Orbot must be on (CR-101, CR-102). It is kept in the app's config file, not in the pairing. |
| Lightning | The connection to the shop's own Lightning node, behind the manager's PIN (CR-115): LND by scanning or pasting the `lndconnect://` code, Core Lightning by address, rune and certificate (PEM or SHA-256 fingerprint). The test of LN-20 runs against the node the owner named in the shop's settings; only when it passes is the connection sealed with the Keystore and saved in `noBackupFilesDir` (`lightning.obkl`). Remove deletes it at once. One thread sends the library's requests over HTTPS (no redirect, at most 1,000,000 bytes, 20 s to connect through Tor and 10 s directly, 15 s to read), through Orbot when the host ends in `.onion` or Tor is on, with the node's certificate pinned by the library and the host name not checked (CR-114). |
| New sale | A keypad in the shop's currency. |
| Sale | The payment code, the amount in currency and in sats, what has arrived, the library's words in the colour of their tone, which check was done (light check, or the own node and the light check, CR-105; or "Lightning, final" for a sale paid by Lightning, CR-109), the on-chain code first and the Lightning code when the invoice has passed (CR-113) or the line that says Lightning is not available for this sale, Requote, Cancel, Override, Copy. |
| Sales and details | The sales the counter holds, one sale's judgment and its refunds. |
| Refunds | Ask for a refund, withdraw a request, refund in cash. |
| Who is at the till | While cashiers are listed and nobody is signed in (CR-147): the cashiers' names and the manager's, then that person's PIN. |
| Staff | Behind the manager's PIN (CR-142): the cashiers, add one, remove one, a new PIN for one. |
| Today | The day's totals from the counter's own records, from midnight to now (CR-152 to CR-155); the lines per person behind the manager's PIN. |
| About | The version, what is not built, "Not for real funds", the run-while-visible note. |

Every refusal of the library is shown through its `words`; the app's own
states and faults have words written in `src/model/AppWords.kt` under
`docs/SCREEN-RULES.md`, listed in `docs/screens/counter-app.md`.

Pairing is the camera's (`docs/COUNTER-RULES.md` CR-90 to CR-99, DESIGN
section 2.9). On the first screen the manager types the counter's name, the
manager's name and a PIN and taps Pair. The app then asks for the camera, only
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

Cashiers and the till lock (CR-140 to CR-151). The manager's PIN opens Staff,
where cashiers are added (the manager types the name, the cashier types a PIN
of 4 to 8 digits twice), removed, or given a new PIN. The list is
`StaffStore`: one strict text file in `noBackupFilesDir`, written like the
PIN and the pairing, at most 30 names, each with its own PBKDF2 entry, wrong
tries and hold (`PinCheck`, shared with `PinStore`). Each add and remove goes
to the library's `staff_record` as a record for the owner; a new PIN is not a
record. With no cashier listed the app is as it was. With one or more, the
first screen is "Who is at the till?" (`ScreenModel`), home shows who is
signed in and a Lock button, and `TillLock` holds the sign-in in memory only:
it locks on Lock and after more than five minutes out of view, never while a
sale's screen is open. Sales and cancels carry the signed-in name into the
record (`open` and `cancel` take it), and a cashier's override needs no
second PIN. Today (`day_totals`, `DayWindow`, `TodayModel`) counts the
counter's own records from local midnight to now; the lines per person need
the manager's PIN. `tests/AppModelTest.kt` checks the file, the lock, the
window, the lines and that the extraction rules leave the list out.

Not built yet: several managers with their own PINs, receipts, and changes
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
- **Tor (CR-87, NR-26, CR-201 to CR-204).** When the signed settings have Tor
  on, the node and every price fetch go through a SOCKS proxy: the app's own
  Tor (the Tor Project's C Tor, from the native library of `tor-android`,
  pinned in `docs/DEPENDENCIES.md`; the default of a new counter) or Orbot
  (`127.0.0.1:9050` unless the manager changed it on the Network screen, behind
  the PIN), chosen under Tor in Settings. A counter set up before the choice
  keeps Orbot. The code that runs Tor is the shared `apps/android-common/tor/`,
  which only the owner and counter builds take; with Tor off in the settings
  nothing of it runs. Tor lives in `noBackupFilesDir/tor` and runs while the
  app is shown. If there is no proxy, nothing is sent around it: the node is
  not started and no price is fetched. The app says "Tor did not start" for
  the built-in Tor, and "Tor is on for this shop, but Orbot is not running."
  for Orbot.
- **The price (CR-89, RT-40 to RT-45).** One `HttpsURLConnection` per source
  on a fixed 60-second grid, never because a sale opened; a 64 KiB read limit;
  a 20-second timeout; no cookies, no cache, no redirect, no compression; a
  fixed generic User-Agent. The bytes go to the library (`rate_parse`,
  `rate_combine`); no JSON is parsed in Kotlin.
- **Threads.** One background thread runs `pump(250)` and posts what it
  reports to the screen thread. Every other call into the library is made on
  the screen thread, except the start, which runs before the pump begins, and
  the PIN's PBKDF2, which runs off the screen thread.
- **The link to the owner.** Only when the pairing holds a relay (CR-130,
  CR-131), one more thread calls `link_step` every 5 seconds (CR-135). An
  onion relay goes through Orbot, whose address is handed to the library only
  when it answers; a direct relay goes direct, unless the shop's Tor setting
  is on, when it goes through the proxy (CR-134). Try again on the home
  screen calls `link_step` at once, on a thread of its own.

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
  Tor gate, sale model, screen choice, words, the Lightning inputs and the
  steps of a test), then the Lightning transport against a local HTTPS server
  with a self-signed certificate made by `keytool` (a matching pin sends the
  request, a different pin is refused before any request byte, a redirect and
  a body past the limit are refused, `.onion` names go to the proxy
  unresolved). They need no device.
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
