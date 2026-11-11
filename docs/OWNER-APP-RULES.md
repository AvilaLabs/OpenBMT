# Owner app rules: what the Android owner app guarantees

Status: specification for `apps/owner-android`, written with the first
build of the app. Each rule has an ID so that a test can name the rule it
proves (`oa_NN_*`, in `apps/owner-android/tests/AppModelTest.kt`, run by
`apps/owner-android/tests/run-jvm.sh`). The rules serve `docs/DESIGN.md` §2.9
(the owner phone holds key 1), §2.10 (the approval session) and §11
(cash-out).

The owner app is the owner's screen: it runs the owner node
(`openbmt-owner-node`, through `openbmt-owner-ffi`), shows what the node
knows, and builds the requests the Key app approves. The libraries judge
coins, queues, sessions and spends (`docs/OWNER-COINS-RULES.md`,
`docs/SPEND-RULES.md`, `docs/SESSION-RULES.md`) and know nothing of the
rules here. These are the app's own, and they cover only what the app
itself does: where it keeps secrets, what it lets the phone show, when it
runs, what it sends and what it never does.

**First build. No independent security audit has been done. It has not been
run on a phone. Not for real funds.**

## 1. What the app holds and what it shows

- **OA-01.** The owner's secret and Key 1's account key (the two secrets
  of the setup text) are kept in one file, sealed with AES-256-GCM under a
  key of the Android Keystore that never leaves it, in the app's
  `noBackupFilesDir`, written by atomic steps (new file, sync, rename, sync
  of the folder). Nothing of the setup text is kept in any other file,
  preference, cache or log. While the first screen is open the typed text is
  held in memory only, it is dropped when the setup is saved, and the
  clipboard is cleared then. The secrets are handed to the node at each
  start and by no other way.
- **OA-02.** Nothing is backed up and nothing moves to another phone:
  `allowBackup` is false, and the extraction rules exclude every domain for
  cloud backup and for device transfer.
- **OA-03.** The manifest asks for exactly two permissions, INTERNET and
  CAMERA, and declares the camera as a feature that is not required. CAMERA
  is declared for reading the Key app's response from a code; this build has
  no camera code. It has one activity, which is exported and is the
  launcher, and no service, receiver or provider. Cleartext traffic is off
  and the app is not debuggable. `apps/owner-android/tests/check-manifest.sh`
  reads the built APK and fails the build if any of this differs.
- **OA-04.** Every window of the app has `FLAG_SECURE`, set before any
  content is shown, so that no screenshot, screen recording or picture in
  the list of recent apps carries a balance, an address or a request. (The
  counter app does not set it, because its customer must see its screen. The
  owner app has no such reader.)
- **OA-05.** Every connection of the node goes through the SOCKS5 proxy of
  the app's setting (`127.0.0.1:9050`, Orbot, unless it was changed): its
  peers, the sending of payments and an onion route of the link. The
  phone looks up no name. The owner app has no setting that turns this off
  and the node is never started with `withoutTor`. Before the node starts,
  the app asks the proxy for a SOCKS5 greeting. If the proxy does not
  answer, nothing is sent around it: the node is not started, and the app
  says "This app sends everything through Tor, but Orbot is not running."
  and "Open Orbot and turn it on." A link route that is a local address
  (`direct`) is the owner's choice in the setup text and is not sent
  through Tor.
- **OA-06.** The node runs only while the app is on the screen. It starts
  when the activity becomes visible, and is stopped, cleanly, when it stops
  being visible, so that it lets go of its folder; the pump thread and the
  link thread end first. The app has no service and no notification, so the
  wallet is followed only while the app is open. The About screen and the
  README say so.

## 2. What the app does with what it is given

- **OA-07.** Everything that has a meaning is judged in Rust. The Key app's
  response, an address, a name, a label, the amount of a refund or a
  cash-out and the fee rate are passed to the library as typed (an address
  trimmed of spaces), and the library's words are shown when it refuses.
  Kotlin checks the shape of a number (digits only, at most 18) and undoes
  the transport armor of a response (OA-13); it checks no address, no
  amount for size and no response for its form. The setup text is in the
  app's own format and its shape is checked in Kotlin (OA-08); what its
  values mean is judged by the node when it starts.
- **OA-08.** The app holds Key 1 and no other secret key. The setup text
  has no line for Key 2's or Key 3's secret, and a line it does not know is
  refused, so a text that carries one is not accepted. Key 3 is a public key
  in the text, and is optional. The only secrets in the app are the two of
  OA-01.
- **OA-09.** The app never signs a spend by itself. A spend, a refund and a
  cash-out leave the app only as an entry in the node's queue and then as a
  request for the Key app; money moves only after the Key app's response is
  taken by the library (`take`). The app never calls the node's batch
  signing or its settings queue: the settings view is read-only. It builds
  no transaction, signs no input and broadcasts nothing but what the node
  sends after a response was taken.
- **OA-10.** Every state and every refusal is shown with the library's
  `words` where the library has them. The words of what the app decides
  itself (setting up, Orbot, what was typed, what was copied or saved, its
  own faults) are in `AppWords`, pass the checks of
  `openbmt_core::say::check` as far as text can be checked, and are listed
  in `docs/screens/owner-app.md` (SC-12).
- **OA-11.** The phone is set up only when both the sealed setup and the
  config exist. The setup is written first and the config last, so a
  crash leaves a phone that asks again and never one that starts on a
  half. A setup that is saved is read back and parsed at every start; a
  file that does not read or does not decrypt is a fault shown with words,
  and nothing else is offered but trying again.

## 3. Threads, requests and files

- **OA-12.** Three threads touch the node. One thread calls `pump(250)` in
  a loop. One thread, started only when the phone has a link, calls
  `linkStep` and `deliver` every 5 seconds. The screen thread makes every
  other call. No other thread calls the library, except the start, which
  runs on a worker thread before either begins.
- **OA-13.** A request for the Key app is shown and saved as exactly the
  bytes the library made. As a file it is those bytes. As text it is hex
  digits in lower case, 64 to a line, and reading the text back gives the
  same bytes. The Key app's response comes from a source behind one
  interface, today the pasted text and a file (a camera will be another),
  and is at most 1,000,000 bytes; a longer one is refused before the library
  sees it. A response is taken only when the owner presses the button of its
  source.
- **OA-14.** The settings view and the list of what waits to be delivered
  show what the node holds, in a fixed order, and change nothing. The
  node keeps no time for a signed object other than an address batch, and
  the app says so instead of showing a time it does not have.
- **OA-15.** The app opens no connection of its own. It has no HTTP
  client and no socket but the one greeting to the proxy of OA-05; every
  other byte that leaves the phone is the node's.

## 4. What these rules do not do

- **They do not protect a phone that is already lost to someone else.**
  A phone that is unlocked, rooted or running a hostile keyboard or
  accessibility service can read what the screen shows and what is typed.
  `FLAG_SECURE` and the sealed file are not a defense against that.
- **They do not audit the setup text.** The key ceremony is not built. The
  text holds two secrets in plain lines until it is sealed; where it comes
  from, how it was carried and when it is deleted are the owner's.
- **They do not make a spend safe.** The Key app's own rules and the
  approval (`docs/SIGNER-RULES.md`, `docs/SESSION-RULES.md`) decide what is
  signed. This app builds the request and carries the response; a request
  that asks for the wrong thing is stopped, if at all, by the Key app and
  its owner.
- **They do not hide that the phone uses Bitcoin.** Tor hides the phone's
  address from peers and the relay; the traffic is still the traffic of a
  Bitcoin wallet.
- **They do not keep the wallet following the chain while the app is
  closed.** OA-06 is the price of having no service.
- **They do not say the app has been run.** Every check here is on the
  desktop JVM, on the manifest of a built APK, or on the source. What needs
  a phone is listed in `apps/owner-android/README.md`.
