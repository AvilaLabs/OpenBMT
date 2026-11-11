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
  start and by no other way. While a key ceremony on this phone is not
  finished, the same secrets are in the sealed record of OA-26, a second
  file of the same kind that is deleted once the setup is saved.
- **OA-02.** Nothing is backed up and nothing moves to another phone:
  `allowBackup` is false, and the extraction rules exclude every domain for
  cloud backup and for device transfer.
- **OA-03.** The manifest asks for exactly two permissions, INTERNET and
  CAMERA, and declares the camera as a feature that is not required. CAMERA
  is declared for reading the Key app's response, a counter's request and,
  in a key ceremony, the Key app's answer and key 2 from a code, and is
  asked for only when the owner taps a scan button (OA-21, OA-22, OA-28).
  It has one activity, which is exported and is the launcher, and no
  service, receiver or provider. Cleartext traffic is off and the app is not
  debuggable.
  `apps/owner-android/tests/check-manifest.sh` reads the built APK and fails
  the build if any of this differs.
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
  signing or its settings queue itself: the settings view is read-only.
  The one way a batch and a settings change are queued is the node's
  `counterConfirm` (OA-23), which the owner presses after comparing the
  four words and which signs with the owner's key only what the Key app must
  then approve. It builds no transaction, signs no input and broadcasts
  nothing but what the node sends after a response was taken.
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

- **OA-12.** Four threads touch the node. One thread calls `pump(250)` in
  a loop. One thread, started only when the phone has a link, calls
  `linkStep` and `deliver` every 5 seconds. One thread, started only when
  the phone has a link, makes the two calls of a read of the till
  (`posRequest` and `posReply`, OA-18). The screen thread makes every
  other call. No other thread calls the library, except the start, which
  runs on a worker thread before any of them begins.
- **OA-13.** A request for the Key app is shown and saved as exactly the
  bytes the library made. As a code it is `ur:bytes` of those bytes
  (OA-20). As a file it is those bytes. As text it is hex digits in lower
  case, 64 to a line, and reading the text back gives the same bytes. The
  code of a counter's request (OA-22) and the code shown to a counter
  (OA-24) are likewise exactly the bytes the library read or made. The Key
  app's response comes from a source behind one interface, the Key app's
  code read by the camera (OA-21), a file or the pasted text, and is at
  most 1,000,000 bytes; a longer one is refused before the library sees
  it. A response is taken only when the owner presses the button of its
  source.
- **OA-14.** The settings view and the list of what waits to be delivered
  show what the node holds, in a fixed order, and change nothing. The
  node keeps no time for a signed object other than an address batch, and
  the app says so instead of showing a time it does not have.
- **OA-15.** The app opens no connection of its own but two. One is the
  greeting to the proxy of OA-05, the only socket it has. The other is the
  read of the till (OA-17), made by one class, `HttpsPosFetcher`, through
  that same proxy. No other file has an HTTP client or a socket; every
  other byte that leaves the phone is the node's.

## 4. The till

The owner node matches the shop's till against the counters' sales
(`docs/OWNER-COINS-RULES.md`, OC-30 to OC-37). The node parses and matches;
the app holds the till's token and does the HTTP, because the node has no
connection that is not Bitcoin's.

- **OA-16.** The read-only token of the till is pasted once into a hidden
  field, sealed with AES-256-GCM under its own key of the Android Keystore
  (not the key of OA-01), and kept in its own file in `noBackupFilesDir`,
  written by the same atomic steps. The field is emptied and the clipboard
  is cleared as soon as the token is taken. It is 1 to 512 printable
  characters with no space, which is the only thing Kotlin checks about it.
  It is read from the file for one read of the till and by no other code,
  shown on no screen, and put in no log, no error text and no address. The
  till's settings are given to the library first, so that a refused till
  seals nothing; a token that cannot be sealed takes the till off again.
  Taking the till off removes the library's records of it and the file.
- **OA-17.** A read of the till sends only what the library asked for: its
  method, its address and its body, with the token added as an
  `Authorization` header in Kotlin and nowhere else. The address must be
  `https://` to `connect.squareup.com` or `api.clover.com`, with a path and
  no port, login or space; any other is not sent. That holds for each
  request of a read, the pages of Square's payments (a GET of
  `/v2/payments`) as much as the pages of orders. The connection goes
  through the SOCKS proxy of OA-05 with the name left to the proxy, and if
  the proxy does not answer nothing is sent. It follows no redirect, keeps
  no cookie or cache, asks for no compression, uses the same generic
  User-Agent as the counter's fetcher, and times out after 20 seconds to
  connect, 20 seconds for each read and 60 seconds for a page. Only an
  answer of 200 has its body read, and a page longer than 8 MiB is
  refused whole. The status and the bytes go back to the library, which
  parses them and says what a refusal means; the app judges nothing in a
  page and queues no payment or refund from it.
- **OA-18.** The till is read only while the app is on the screen, by one
  thread that starts with the node and ends before it stops, and only when
  the phone has a link and a till is connected. A read starts at most once
  in 60 seconds, whoever asks: the thread works out the wait from the start
  of the last read, a button only wakes it, and the node refuses a read that
  starts sooner. The next page of a read that is going on is not held
  back. A read is at most 65 requests. The rate is not a promise of what a
  vendor allows: a vendor that asks to be read less often is shown its
  words and read again no sooner than a minute later.
- **OA-19.** The till screen shows the library's words and the app's own
  for the app's own states. It shows the vendor and location, when the
  till was last read (and that it was not, since the app was opened), the
  number of sales matched and how many of them by order of time, and every
  open alert with its words (why, and what to do next) and a button to
  acknowledge it. Acknowledging is a change of this phone's own ledger: no
  signed command exists for it, none is sent, and the Box and the counters
  do not learn of it. The screen says so. A 401 or a 403 is shown with the
  library's words, which say the token was refused and how to make a new
  read-only one, and the screen offers to replace the token.

## 5. The approval by camera and screen

- **OA-20.** The request screen offers to show the request as a code
  first, then to save it as a file and to copy it as text, then to go on to
  the response. The code is the request, exactly the bytes of OA-13, as
  `ur:bytes` parts drawn by the shared `AnimatedQrView` (one still code when
  they fit one part, otherwise a code that changes), at most 60% of the
  screen's height, the same type and sizing the Key app uses for its answer.
  While the code is on the screen the words say what to do next ("On the
  Key app, choose Scan and point it at this code"), and Done, offered only
  with a code on the screen, leads to the response. If no code can be made
  the words say so and say to save the file or copy the text instead.
- **OA-21.** The response screen offers the Key app's code first, then a
  file, then the pasted text. The scan is the shared `ScanFlow` with
  `UrReading`: it reads `ur:bytes` parts, shows the parts read so far, and
  takes no other type. A complete code gives its payload as bytes to the
  same place a file and the pasted text give theirs, longer than the most
  of OA-13 refused first, and the library judges them (OA-07), so a code of
  another app's object is refused by the library with its words. The camera
  is asked for only when the owner taps the scan button, never at start or
  when a screen opens. A wrong type of code, a code of another kind, a
  scan that is cancelled, a camera that is off for the app and a camera
  that did not start each have words that say what happened and what to do;
  the reading's and the camera's words are the shared ones. After any of
  them the response screen is shown again with the words, and the file and
  the pasted text are still offered. A scan is stopped when the app leaves
  the screen. The app keeps no frame and no code.

## 6. Adding a counter

The node judges the request, queues the pairing, the settings and the first
batch, and makes the code the counter takes (`docs/SESSION-RULES.md`, AS-69
to AS-73). The app reads the camera, shows the answers and carries the
owner's choices.

- **OA-22.** Adding a counter starts on the home screen, with two lines of
  words: scan the counter's code and approve on the Key app, then show the
  counter a code. The camera is asked for only when the owner taps "Scan the
  counter's code", and the scan takes `ur:bytes` and no other type, at most
  1,024 bytes, as exactly the bytes of the counter's code. The bytes go to
  the node's `counterScan` and nothing else is judged in Kotlin. A code that
  is not a counter's request, a request for another shop or network, a
  counter that is paired or queued already, a full queue, a cancelled scan
  and a camera that is off each have words that say what happened and what
  to do. Working out the four words takes a moment, so a page says so and is
  drawn first; the call stays on the screen thread (OA-12).
- **OA-23.** The owner sees the counter's name (foreign text, cleaned), its
  four words large, "Check that the counter shows these same four words,
  and that the Key app shows them too", the shares of the instant limit once
  it is added, the minutes the counter waits, and two buttons. Only "The
  four words match" calls the node's `counterConfirm`, which queues the
  settings and the first batch together; "The words differ" takes the queued
  request out. The shares are equal, 1/n each, and the words say that custom
  shares come later with the settings editor. A phone that holds no settings
  asks for the rate in satoshis to a dollar, a whole number above 0, and
  queues nothing without it. Leaving the screen queues no settings.
- **OA-24.** After the confirmation the approval is the request and
  response screens of OA-20 and OA-21; nothing in the app marks anything as
  approved. When the response has been taken, a counter whose pairing it
  approved waits for its code: the screen and the list of what waits to be
  delivered offer "Show the code for" it. The code is `ur:bytes` of the
  bundle `counterBundle` made, drawn by `AnimatedQrView` as in OA-20, with
  the words "Scan this with the counter. It shows Paired when it has taken
  it." A code in many parts (the first addresses make it about 36 KB) says
  that taking it takes about a minute and to hold both phones steady; the
  counter counts the parts it has. A code without the first addresses says
  so. If the Key app declined,
  nothing waits and the library's words of each entry are shown.
- **OA-25.** Leaving the code keeps the counter waiting. Only the owner's
  word, the button "The counter shows Paired", calls `counterDelivered` and
  marks what waited as delivered; "Show again" draws the code again from the
  node, which changes nothing. The app keeps no copy of the code.

## 7. Making a new shop wallet

The first start can make the wallet on this phone, with the Key app on a
second phone (`docs/DESIGN.md` §2.1 steps 4 and 5). The library does every
step (`docs/OWNER-COINS-RULES.md`, OC-40 to OC-46, and
`docs/SIGNER-RULES.md`, SR-64 to SR-69, through the ops 80 to 86 of the
binding, which name no node). The app reads the camera, shows the answers,
keeps the library's record between two steps and carries the owner's words.

- **OA-26.** The first start offers two buttons: "Make a new shop wallet",
  first, and "I have setup text", the paste path of OA-01 and OA-11. The
  network is the build's one choice (`AppConfig.NETWORK`): the app has no
  picker, and the library refuses a Key app's answer for another network.
  Key 1 is made with no words to show (`show_words` false, `docs/DESIGN.md`
  §5.4), and the app shows the library's words of the stage. After every op
  that returns a record the app seals the record in its own file in
  `noBackupFilesDir` (`ceremony.obks`, under the Keystore key of OA-01, by
  the same atomic steps) before it shows the next screen. A record that
  cannot be saved is said so, with a button to try again, and no next
  screen. Nothing of the record is kept in any other file, preference,
  cache or log, and it is held in memory only while the screens of the
  ceremony are open.
- **OA-27.** When the start finds the ceremony's file and no setup file,
  the app asks the library for the stage of the record (`ceremonyStage`)
  and resumes there, with the library's words of the stage: at stage 1 the
  scan of the Key app's code, at stage 2 the same scan again (the setup
  for the Key app is not kept, so the wallet is built again from the
  answer), at stage 3 the saving of the setup. A record that does not
  read is said so and only "Start again" goes on. "Start again" is always
  offered on the screens of the ceremony and goes through a screen that
  says the keys made so far are thrown away and nothing has been paid to
  them. Only its confirmation deletes the file, and nothing else on the
  phone. A ceremony's file that is found beside a saved setup is deleted.
- **OA-28.** The Key app's answer is read by the shared `UrScan` for
  `ur:bytes` and no other type, at most 1,024 bytes, as exactly the bytes
  of the Key app's code (OA-13), and goes to `ceremonyReadAnswer`; the
  camera is asked for only when the owner taps the scan button. The Key
  app's identity words (`key_app_root_words`) are shown large, with "The
  Key app shows the same words" and "They differ". Differ throws the
  answer away and goes back to the scan, and the words say why. When the
  answer holds no key 2, a screen takes key 2 as text, by a scan of a
  plain code or pasted, and passes it as `key2_text` (SR-64); when the
  answer holds key 2 the screen is not shown. Both are never passed
  (OC-41). The app checks only the length of the text; the library reads
  the key and refuses what is not one.
- **OA-29.** The wallet is built with `ceremonyBuildWallet`. The screen
  shows the checksum words large, the three fingerprints, the library's
  words of the pairs of keys with a line for each pair and its holders, and
  the first receive address in monospace. The wallet setup (`OBMw`,
  `wallet_setup`) is shown to the Key app as `ur:bytes` parts of the
  shared `AnimatedQrView` as in OA-20, exactly the bytes the library made,
  with the words that the Key app shows the same checksum words and
  fingerprints and that the owner compares them (OC-43). Only the owner's
  word "The Key app shows the same words" calls `ceremonyFinish`. "They
  differ" shows a screen that says the wallet is not finished, nothing
  should be paid to it, and to choose "Start again" on both phones, and
  leads to the confirmation of OA-27. The finished record is sealed, opened
  with `ceremonyOpen`, written as the setup text of OA-01 (`OwnerSetup`
  `format`, which `parse` reads back to the same values) and saved exactly
  as the paste path saves it: the setup first, then the config with the
  default address of Orbot. The ceremony's file is deleted only after both
  are saved, and the start goes on as after a pasted setup, through the
  gate of OA-05. A wallet made so has no peers and no relay link.
- **OA-30.** Making key 1 and building the wallet take a moment: a page
  says so and is drawn first, and the call stays on the screen thread
  (OA-12). No number of the ceremony is written in Kotlin: the batch
  minimum, the refund depth and the first block of the network are asked
  from the library (`ceremonyDefaults`, op 86), which holds the counter's
  minimum, the node's default depth and the header checkpoint of the watch.
  The app judges no key, no word list and no wallet.
- **OA-31.** Every state and every refusal of the ceremony is shown with the
  library's words where the library has them, and the words of the app's own
  steps are in `AppWords`, each sentence at most 140 characters, listed in
  `docs/screens/owner-app.md`. The app holds no secret of key 2 or key 3
  (OA-08) and signs nothing (OA-09): what it holds from the ceremony is key
  1 and the owner's secret, as before.

## 8. What these rules do not do

- **They do not protect a phone that is already lost to someone else.**
  A phone that is unlocked, rooted or running a hostile keyboard or
  accessibility service can read what the screen shows and what is typed.
  `FLAG_SECURE` and the sealed file are not a defense against that.
- **They do not audit the setup text.** The text holds two secrets in plain
  lines until it is sealed. A text from the ceremony on this phone never
  leaves the app before it is sealed; where a pasted text comes from, how it
  was carried and when it is deleted are the owner's. Key 1 has no PIN and
  no passphrase, and the ceremony shows no words of key 1.
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
- **They do not say the till reads work.** The shape of Square's and
  Clover's answers is from their published documentation and fixtures, not
  from a real account (`docs/POS-RULES.md`).
- **They do not say the camera and the screen work.** The code drawn for the
  Key app and for a counter, and the scan of the Key app's code and of a
  counter's, are the shared pieces, checked on the desktop; no phone has
  scanned a code of this app or shown one to the Key app or a counter yet.
  The code for a counter holds 1,000 addresses, so it has many parts; how
  long a scan of it takes is not measured. The same holds for the Key app's
  answer, key 2 and the setup shown to the Key app in the ceremony.
- **They do not say the app has been run.** Every check here is on the
  desktop JVM, on the manifest of a built APK, or on the source. What needs
  a phone is listed in `apps/owner-android/README.md`.
