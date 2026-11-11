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
| First start | The pairing text (pasted or from a file), the manager's name and PIN, the address of Orbot. |
| Home | Whether Bitcoin is ready, the price and each source's note, New sale, Sales, the cashier's name, About. |
| New sale | A keypad in the shop's currency. |
| Sale | The payment code, the amount in currency and in sats, what has arrived, the library's words in the colour of their tone, Requote, Cancel, Override, Copy. |
| Sales and details | The sales the counter holds, one sale's judgment and its refunds. |
| Refunds | Ask for a refund, withdraw a request, refund in cash. |
| About | The version, what is not built, "Not for real funds", the run-while-visible note. |

Every refusal of the library is shown through its `words`; the app's own
states and faults have words written in `src/model/AppWords.kt` under
`docs/SCREEN-RULES.md`, listed in `docs/screens/counter-app.md`.

Not built yet: reading the pairing code with the camera (pairing is pasted
text or a file, behind one `PairingSource` interface so a camera can be added
without other changes), a lock screen of the app's own, a list of cashiers
and managers with their own PINs, end-of-day totals and receipts, and changes
to the shop's settings after pairing.

## Pairing text

The app's own plain-line format (no pairing ceremony is built yet): a header,
the network, the counter's identity secret, the owner's and approver's public
roots, the wallet checksum, the shop's signed settings and the first address
batch in hex, and optional peers. `src/model/PairingText.kt` documents it,
parses it and refuses anything else, with the line it stopped at. The
pairing is kept sealed with an Android Keystore key (AES-GCM) in
`noBackupFilesDir`.

## Rules the app keeps without a device

- **One permission, INTERNET.** The counter connects to the Bitcoin network
  and fetches the price. `tests/check-manifest.sh` reads the built APK with
  `aapt2` and fails the build if the list differs.
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
the JVM tests, builds the library for `arm64-v8a` and `x86_64` through
`scripts/counter-ffi-build.sh`, compiles with `-Werror`, dexes, aligns for
16 KiB pages, signs with a local test key and checks the manifest. The result
is under `~/.cache/openbmt/target/counterapk/apk`, never in the repository.

## Tests

- `tests/run-jvm.sh` runs the common model tests, then the counter's model
  tests (PIN store, pairing text, amount entry, QR layout, price schedule,
  Tor gate, sale model, screen choice, words). They need no device.
  `OPENBMT_WRITE_SCREENS=1` rewrites `docs/screens/counter-app.md`.
- `tests/check-manifest.sh APK` checks the permission list (exactly INTERNET),
  backup, components, SDK levels.

## Signing

The build makes a local test key with `keytool` the first time and keeps it
under `~/.cache/openbmt/tmp/counterapk/`. It is not committed and it is not a
release key.

## Not checked without a device

Everything that needs a running Android: that the library loads and starts,
how the screens look and lay out, the keyboard types of the fields, the file
picker, the Keystore sealing, the SOCKS probe against a real Orbot, real
fetches of the sources, the QR code scanned by a wallet, and the node stopping
cleanly when the app is sent to the background.
