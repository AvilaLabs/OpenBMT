# OpenBMT Owner app for Android

The first build of the owner's screen: a real APK that runs the owner node
(`openbmt-owner-node`, through `openbmt-owner-ffi`) on plain `android.widget`
screens. It is built with the Android SDK's own tools and no Gradle, no
AndroidX, no Jetpack Compose and no library beyond the Kotlin standard
library. The code shared with the Key app and the Counter app is in
`apps/android-common`; nothing in `apps/android-common`, `apps/counter-android`
or `apps/key-android` was changed for this app.

**First build. No independent security audit has been done. It has not been
run on a phone or an emulator. Not for real funds.** The APK is signed with a
local test key; release signing is a separate step (see "Signing"). The rules
the app keeps are in `docs/OWNER-APP-RULES.md` (OA-01 to OA-19).

## What it does

| Screen | What it shows |
|---|---|
| First start | The setup text (pasted or from a file) and the address of Orbot. |
| Home | The receipts line, the number of alerts, the link state, the payments in progress. |
| Receipts line | What the node counts as received, and the offer to merge. |
| Spend | Builds the request for the Key app, shows it as text or saves it as a file, takes the response by paste or file. |
| Sessions | The approval sessions the node holds. |
| Refund queue | Queue a refund from a sale with the facts the node verified, cancel, requeue. |
| Cash out | Queue a cash-out of an amount or of everything. |
| Old addresses | The old addresses and a request for a deep check. |
| Link and counters | The relay, each counter's records, head and last check-in. |
| Alerts | The node's alerts, with clear. |
| Till | Connect a Square or Clover till with a read-only token, see when it was last read, how many sales matched, and each open alert with its words and an acknowledge button. |
| Refund notes and Notes | What the node noted about refunds and everything else. |
| Waiting | What is signed and waits to be delivered, by kind and age (read-only). |
| Settings | The settings the node holds, read-only. |
| About | The version, what is not built, "Not for real funds", the run-while-visible note. |

Every state and refusal is shown through the library's `words` where the
library has them. The app's own states and faults have words written in
`src/model/AppWords.kt` under `docs/SCREEN-RULES.md`, listed in
`docs/screens/owner-app.md`.

Not built yet: the key ceremony (the setup text is made by hand, see below),
reading the Key app's response with the camera (it is pasted text or a file,
behind one `ResponseSource` interface so a camera can be added without other
changes; CAMERA is declared and unused), queuing a pairing, and any change to
the settings (the settings view is read-only).

## The till

The Till screen matches the shop's orders against the counters' sales on this
phone (`docs/OWNER-COINS-RULES.md`, OC-30 to OC-37). The owner node parses
the vendor's pages and does the matching; the app holds the token and does
the HTTP (OA-16 to OA-19).

- **Connect.** The vendor (Square or Clover), the location id (Clover: the
  merchant id), the exact names of the Bitcoin tender, the shop's currencies,
  and a read-only token pasted once into a hidden field. The token is sealed
  under its own Keystore key in its own file in `noBackupFilesDir`, is never
  shown again and can be removed with the till. The settings go to the library
  first, so a refused till seals nothing.
- **Reading.** One thread reads the till while the app is on the screen, at
  most once a minute, through Orbot (nothing is sent if Orbot does not answer),
  with `HttpsURLConnection`: no redirect, no cache, a fixed User-Agent, 20
  second timeouts, 8 MiB at most per page, only `connect.squareup.com` and
  `api.clover.com`. The `Authorization` header is set in
  `src/android/HttpsPosFetcher.kt` and nowhere else; it is never logged.
- **Alerts.** Each open alert is shown with the library's words (why, and what
  to do next). **Acknowledge marks it on this phone only.** No signed command
  for it exists, so the Box and the counters do not learn of it. The screen and
  the About screen say so.
- **A 401 or 403** is shown with the library's words (the token was refused;
  make a new read-only one in the till's system) and the screen offers to
  replace the token.
- **Not checked without a real account.** The vendors' answers are known from
  their published documentation and the fixtures of `openbmt-pos-readers`
  only, and the hosts and the paths are marked for verification in
  `openbmt-owner-node/src/pos.rs`.

## Setup text

The app's own plain-line format, because no ceremony is built yet: a header
`openbmt-owner-setup 1`, then one `key: value` line each for the network, the
owner's secret, the approver root, the wallet descriptor, Key 1's account key,
the batch minimum, the refund depth and the wallet birthday, and optionally
Key 3's public key, up to 8 peers, a relay route (`direct host:port` or
`onion name:port`), the relay key, 1 to 16 counters and two link times.
`src/model/SetupText.kt` documents it, parses it and refuses anything else,
with the line it stopped at. A line for Key 2 or Key 3's secret is refused:
the app holds Key 1 and no other secret key (OA-08). The setup is kept sealed
with an Android Keystore key (AES-256-GCM) in `noBackupFilesDir` and handed
to the node at each start (OA-01).

## Why `FLAG_SECURE` here and not in the Counter app

The owner sees balances, addresses and requests, so every window of this app
sets `FLAG_SECURE` before it shows anything (OA-04): no screenshot, screen
recording or picture in the list of recent apps carries them. The Counter app
does the opposite on purpose, because its customer must see its screen and
scan the code from it.

## Rules the app keeps without a device

- **Two permissions, INTERNET and CAMERA.** CAMERA is not used by this build;
  the camera is a feature that is not required. `tests/check-manifest.sh`
  reads the built APK with `aapt2` and fails the build if anything differs.
- **No backup**, no exported component but the launcher, no service, receiver
  or provider, cleartext off, not debuggable.
- **Tor, and no way around it (OA-05).** Peers, sending and an onion link
  route go through Orbot's SOCKS proxy (`127.0.0.1:9050` unless changed on the
  first screen). If the proxy does not answer, the node is not started and
  the app says "This app sends everything through Tor, but Orbot is not
  running." and "Open Orbot and turn it on."
- **The node runs only while visible (OA-06).** There is no service and no
  notification, so the wallet is followed only while the app is open.
- **Threads (OA-12).** One thread pumps, one thread takes the link's turns
  (only when there is a link), one thread reads the till (only when there is a
  link, at most once a minute, OA-18), and the screen thread makes every other
  call.

## Build

```
scripts/owner-apk-build.sh
```

Needs a JDK 21, the Android SDK (platform 36, build-tools 36.0.0), the NDK,
and Kotlin 2.2.20, as for the Key app (`apps/key-android/README.md`). It runs
the JVM tests, builds the library for `arm64-v8a` and `x86_64`, compiles with
`-Werror`, dexes, aligns for 16 KiB pages, signs with a local test key and
checks the manifest. The result is under `~/.cache/openbmt/target/ownerapk/apk`,
never in the repository.

## Tests

- `tests/run-jvm.sh` runs the common model tests, then the owner app's model
  tests (words, setup text, config, request and response text, Tor gate,
  screen choice, forms, the till's token store, address check, page cap, pace
  and read loop) and one test for each rule, named `oa_NN_...`. They
  need no device. `OPENBMT_WRITE_SCREENS=1` rewrites
  `docs/screens/owner-app.md`.
- `tests/check-manifest.sh APK` checks the permission list, backup,
  components and SDK levels.

## Shared code copied into this app

Nothing is copied from the other apps; the owner app compiles
`apps/android-common` as it is. What is written again here, because it is the
app's own: the Tor gate and its SOCKS5 probe (`src/model/TorGate.kt`, as the
Counter app has), the till's fetcher (`src/android/HttpsPosFetcher.kt`, the
same shape as the Counter app's `HttpsFetcher`), the config file, the file
picker and the save-as-file helper (`src/android`).

## Signing

The build makes a local test key with `keytool` the first time and keeps it
under `~/.cache/openbmt/tmp/ownerapk/`. It is not committed and it is not a
release key.

## Not checked without a device

Everything that needs a running Android: that the library loads and starts,
how the screens look and lay out, the keyboard types of the fields, the file
picker and the save of a request as a file, the clipboard copy and clear, the
Keystore sealing (of the setup and of the till's token), the hidden token
field and its paste, a read of a real Square or Clover account through Orbot
(the hosts, the headers a vendor needs such as Square's version, the shape of
its answers, its rate limits and what a refused token looks like), the till
thread starting and stopping with the app, `FLAG_SECURE` in a screenshot and
in the list of recent apps, the SOCKS probe against a real Orbot, the node following the chain
through Orbot, a link to a real relay and counters, the Key app's response
round trip with a real Key app, and the node stopping cleanly when the app is
sent to the background.
