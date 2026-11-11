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
  of the setup text) are kept in one file, locked behind a PIN (OA-60) and
  sealed with AES-256-GCM under a
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
- **OA-03.** The manifest asks for exactly five permissions: INTERNET,
  CAMERA, FOREGROUND_SERVICE, FOREGROUND_SERVICE_SPECIAL_USE and
  POST_NOTIFICATIONS, and declares the camera as a feature that is not
  required. CAMERA is declared for reading the Key app's response, a
  counter's request and, in a key ceremony, the Key app's answer and key 2
  from a code, and is asked for only when the owner taps a scan button
  (OA-21, OA-22, OA-28). The other three are for the background watch and
  are covered by OA-89. It has one activity, which is exported and is the
  launcher, one service that is not exported (OA-82), and no receiver or
  provider. Cleartext traffic is off and the app is not debuggable.
  `apps/owner-android/tests/check-manifest.sh` reads the built APK and fails
  the build if any of this differs.
- **OA-04.** Every window of the app has `FLAG_SECURE`, set before any
  content is shown, so that no screenshot, screen recording or picture in
  the list of recent apps carries a balance, an address or a request. (The
  counter app does not set it, because its customer must see its screen. The
  owner app has no such reader.)
- **OA-05.** Every connection of the node goes through a SOCKS5 proxy: the
  app's own Tor (OA-97, the choice "Built-in Tor", which a new setup has) or
  Orbot at the address of the config (`127.0.0.1:9050` unless it was
  changed, the choice "Use Orbot", OA-99). Its peers, the sending of payments
  and an onion route of the link all use it. The phone looks up no name. The
  owner app has no setting that turns this off and the node is never started
  with `withoutTor`. Before the node starts, the app has the proxy: the
  built-in Tor's port once Tor has a circuit (OA-97), or, with Orbot, the
  answer to a SOCKS5 greeting. If it has none, nothing is sent around it: the
  node is not started, and the app says so (OA-100 for the built-in Tor; for
  Orbot "This app sends everything through Tor, but Orbot is not running."
  and "Open Orbot and turn it on."). A link route that is a local address
  (`direct`) is the owner's choice in the setup text and is not sent
  through Tor.
- **OA-06.** The node runs while the app is on the screen, and while the
  background watch is on (OA-80 to OA-89). It starts when the activity
  becomes visible, and is stopped, cleanly, when it stops being visible, so
  that it lets go of its folder; the pump thread and the link thread end
  first. With the background watch off (the default), the app has no
  service and no notification, so the wallet is followed only while the app
  is open. With it on, a watcher that is not the node follows the wallet
  and tells the counters it is watching, and the node does not run (OA-84).
  The About screen and the README say so.

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
  show what the node holds, in a fixed order, and change nothing: the
  settings view has no input of its own, and a change of the settings goes
  through the editor of OA-32 to OA-35. The settings view shows the node's
  key in full. The node keeps no time for a signed object other than an
  address batch, and the app says so instead of showing a time it does not
  have.
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
  Taking the till off removes the library's records of it and the file;
  the file is kept in order as OA-94 says.
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
  shares are set in the settings editor. A phone that holds no settings
  asks for today's price of one bitcoin in whole dollars, above 0, and
  queues nothing without it; the node turns the price into satoshis to a
  dollar and judges its range (AS-75). Leaving the screen queues no
  settings.
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
  first, and "Use setup text", the paste path of OA-01 and OA-11. The
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
  plain code, by a scan of a `crypto-account`, `crypto-hdkey` or `bytes`
  UR (read by the shared `UrScan` and turned to text by the library's key
  text call, UR-31; a code it declines shows its words and the screen
  stays) or pasted (text only), and passes it as `key2_text` (SR-64); when
  the answer holds key 2 the screen is not shown. Both are never passed
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
  as the paste path saves it, after the screen that asks for the address
  of Orbot with the words that everything goes through Tor, prefilled with
  the default and checked as the paste path checks it (one helper shows
  both): the setup first, then the config with that address. The
  ceremony's file is deleted only after both are saved, and the start goes
  on as after a pasted setup, through the gate of OA-05. A wallet made so
  has no peers and no relay link.
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

## 8. Changing the settings

The node applies an edit to the settings it holds, checks it and sorts it
(`docs/SESSION-RULES.md`, AS-74 to AS-77). The app turns what is typed into an
edit, shows what the node says it would do, and queues it when the owner says
so. It builds no settings bytes and signs nothing.

- **OA-32.** The settings view offers "Change settings". The editor is a menu
  of seven screens and a Review: Currency, Instant sales, Payments, Refunds
  and overrides, Safety (the heartbeat, Tor and customer Wi-Fi), Counters'
  shares, and Lightning node. What is typed is kept in memory only, and only
  for the fields that differ from what the settings hold; "Start over"
  forgets it, and a phone that holds no settings says so and offers nothing
  to change. Nothing is sent to the node until Review.
- **OA-33.** Each field shows its words from `AppWords` (a heading of at most
  28 characters and one plain sentence of what it does), what the settings
  hold now, and a field to type in: whole units of the shop's currency
  (0 is off), a percent with
  up to two decimals, whole blocks, whole minutes, whole satoshis, or a
  choice of two. Done keeps the screen's fields and refuses with words a text
  that is not the kind of number its field takes; Back keeps nothing of that
  screen. Only the shape is judged here (OA-07): ranges, the order of the two
  depths and the total of the shares are the node's. The shares screen shows
  the total and what remains while the owner types. The Lightning screen
  takes a key or an address typed, pasted or scanned (plain text of a code,
  at the owner's press), or removes the node; it says that naming a node
  needs the Key app, which shows the key in full, and that Lightning must
  then be set up on each counter, which holds its own limited key.
- **OA-34.** Review calls the node's `previewSettings`, which stores nothing,
  and shows every change as a line with its name, its old value and its new
  one, those that need the Key app (loosening) apart from those that take
  effect when delivered (tightening), with the library's own words for a
  refusal. A changed limit is shown with its satoshis and the price they
  were counted at. When the library says a price is needed, or that the one
  given is outside its range, the review asks for today's price of one
  bitcoin in whole dollars, above 0, in an empty field: the price is never
  filled in or kept, it is forgotten when the review is left, and the node
  converts it (OA-07). A change that changes nothing is said so and cannot
  be queued. Only "Queue the change" calls `queueSettings`, with the bytes
  the preview gave, and then clears what was typed.
- **OA-35.** After the queue call the owner is taken to the screens that
  exist: a change that needs the Key app goes to the request screen of
  OA-20 and its response (OA-21), and a stricter one, which the node signs
  alone, is listed under what waits to be delivered. The editor has no
  approval flow of its own. Every state and refusal is shown with the
  library's words, and the app's own words are in `AppWords`, listed in
  `docs/screens/owner-app.md`.

### Connecting a Box

The shop's Box is paired with an invitation the Key app makes and the owner
types into the Box (`docs/SIGNER-RULES.md`, SR-80 to SR-82), a claim the Box
offers, a welcome the Key app makes, and a link the owner phone keeps to the
Box afterwards (`docs/SESSION-RULES.md`, AS-78 to AS-87). The app carries the
owner's choices and shows the answers; the claim is checked by the node and
the welcome is made by the Key app. OA-36 to OA-39 are kept for other work.

- **OA-40.** The home screen has "Connect your Box". Its first screen says in
  two lines what the steps are (type the invitation on the Box, type the
  Box's address here; the Key app approves; this phone hands the Box its
  welcome), holds a field for the Box's address, and has the buttons "Look
  for the Box" and "Scan the Box's address". The Boxes being added are
  listed on it, in the list of what waits to be delivered and after a
  response is taken (OA-43, OA-47). Nothing is saved by this screen.
- **OA-41.** The address is `host:port` on the local network or a v3 `.onion`
  name and a port, typed or read as plain text by the camera at the owner's
  press. The app judges only its shape (OA-07), drops spaces and capitals,
  and gives it to the node's `boxFetchClaim`: with the proxy of Orbot for an
  onion name and with none for an address on the local network. The call
  waits on the network for up to about a minute, so a page says so first and
  the call runs off the screen thread. A Box that cannot be reached, a
  server that is not a Box, a Box with no invitation open and a Box already
  paired are each the library's own words, shown on the first screen with
  what was typed kept.
- **OA-42.** A Box that answered is shown with its key in full. A phone with
  no settings asks for today's price of one bitcoin in whole dollars (as in
  OA-23); the node converts it. "Queue the Box for the Key app" calls
  `boxQueue`, which queues the claim and the settings that add the Box
  together or neither, and the screen then shows the Box's four words large,
  to compare with the Box's own screen and the Key app's, and the words of
  what comes next. The approval is the request and response screens of OA-20
  and OA-21; nothing here marks a Box approved.
- **OA-43.** The Box's welcome is handed over from the list of Boxes being
  added, for a Box whose welcome is ready, with the address the Box was
  found at already in its field. The call is `boxDeliver`, which takes the
  welcome out of the queue only when the Box has taken it. When it did not,
  the screen shows the library's words and the app's words that the approval
  is kept on this phone, nothing was lost, and what to try; the queued
  settings stay.
- **OA-44.** The link to the Box is saved only after the Box took its
  welcome, and only through the locked setup (OA-60): the app asks for the
  PIN, the library opens the stored setup with it, the link lines
  (`relay-route`, `relay-key`, `counter`, `heartbeat-every`,
  `check-in-grace`) are replaced by the Box's route, the Box's key as the
  relay key, the counters the library gave (up to 16) and no intervals (the
  default ones), the text is checked with the strict reading of the setup
  text, and the library locks it again under the same PIN and the file is
  replaced.
  The text is never stored unlocked, and a wrong PIN counts as at the unlock
  screen (OA-63). The app then starts again, which starts the link, with no
  second PIN. A link needs no counter line: the link follows the counters
  of the signed settings (LK-67), and the counter lines written are only a
  start. When the text cannot be saved, or the owner says "Not now", no
  link is made and the screen shows the lines to add by hand; the Box is
  paired either way.
- **OA-45.** The app builds no claim, welcome or settings bytes and signs
  nothing: they are the library's. What it holds of a Box is the address it
  was found at, in memory only and never written to a file or the clipboard.
- **OA-46.** The app does not offer to pair a Box without the Key app, until
  the shop has a way to sign settings without the Key app (`docs/DESIGN.md`
  §2.9, the fallback, which is not built): without it the settings that add
  the Box cannot get the signature `SignedSettings::verify` asks for, so the
  path could not finish. The library keeps its two calls for that path
  (`box_own_invitation`, `box_own_check`, BX-17) and their tests; the app
  has no button and no screen for them.
- **OA-47.** A Box that was not handed its welcome is listed with its four
  words and "Forget this Box" (also on the screen after queuing). The call
  is `boxForget`, which takes the queued claim and its settings, and an
  approved welcome waiting in the queue, out in one step; the Box is not
  told, and its invitation runs out by itself. A Box in an open session is
  refused with the library's words until the session is abandoned, and a
  Box that took its welcome has nothing to forget. The screen after it says
  how to add the Box again.
- **OA-48.** Every state and refusal of connecting a Box is shown with the
  library's words or the app's own in `BoxWords`, a heading of at most 28
  characters and each line of at most 140, listed in
  `docs/screens/owner-app.md`.

## 9. What these rules do not do

- **They do not protect a phone that is already lost to someone else.**
  A phone that is unlocked, rooted or running a hostile keyboard or
  accessibility service can read what the screen shows and what is typed.
  `FLAG_SECURE` and the sealed file are not a defense against that.
- **They do not audit the setup text.** The text holds two secrets in plain
  lines until it is sealed. A text from the ceremony on this phone never
  leaves the app before it is sealed; where a pasted text comes from, how it
  was carried and when it is deleted are the owner's. The ceremony shows no
  words of key 1. The record of a ceremony that is not finished (OA-26) is
  sealed by the Keystore only: the PIN is chosen at the end (OA-61).
- **They do not make a spend safe.** The Key app's own rules and the
  approval (`docs/SIGNER-RULES.md`, `docs/SESSION-RULES.md`) decide what is
  signed. This app builds the request and carries the response; a request
  that asks for the wrong thing is stopped, if at all, by the Key app and
  its owner.
- **They do not hide that the phone uses Bitcoin.** Tor hides the phone's
  address from peers and the relay; the traffic is still the traffic of a
  Bitcoin wallet.
- **They do not keep the wallet following the chain while the app is
  closed, unless the owner turns on "Watch in the background".** Off is the
  default, and OA-06 is then the price of having no service. The watcher
  of OA-80 to OA-89 follows the chain and sends heartbeats; it scans no
  wallet script, so a coin that arrives while only the watcher runs is found
  when the app is opened and its node scans. It reads nothing the counters
  send to the owner (records, check-ins, refund requests): those are sealed
  to the owner's key, which the watcher does not hold, and they wait for the
  owner app.
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

## 10. The PIN on key 1

Key 1 on the owner phone is kept the way the Key app keeps its key
(`docs/DESIGN.md` §5.4, "Phone key rules"; `docs/SIGNER-RULES.md` SR-71 to
SR-77). The frame, the cost and the PIN rules are in `openbmt-lock`
(`docs/LOCK-RULES.md`); the counting of wrong tries, the waits, the wipe order
and the 60-second rule are the shared `LockModel.kt` of
`apps/android-common`, which the Key app uses as well.

- **OA-60.** The setup file is `OBMx(setup text)`, sealed by the Keystore as
  OA-01 says: the phone and the PIN are both needed. The library locks a
  text under a PIN, opens a locked one and changes the PIN in one call
  (ops 100 to 103: whether stored bytes are a locked setup, lock, unlock,
  change the PIN), all through `openbmt-lock`, and stores nothing: the app
  stores what the call returns, and replaces the file only when the call
  returned. The setup text is read at each start and checked for its shape
  (OA-11); it is in memory only while the node starts, and the bytes it was
  read into are overwritten. The parsed setup is held while the app runs and
  for the minute after it goes to the background (OA-62), and dropped then.
  `key1.bin` in the node's folder is Key 1's signer state (its marks and
  the registered wallet), not the secret, and stays outside the PIN.
- **OA-61.** The PIN is chosen before the setup is first saved: at the end of
  the ceremony and on the paste path, after the text and the address of
  Orbot are checked and before the save, on the screen the Key app uses
  ("Choose a PIN": 6 digits or more, or a passphrase of 8 characters or
  more, typed twice, `pin-weak` and `pin-long` from the library). A setup
  saved before the lock existed (the bare sealed text) opens only into "Set
  a PIN for key 1", is checked for its shape first, and is saved again
  locked. The home screen offers "Change the PIN": it asks for
  the current PIN, a wrong one counts as a try (OA-63), and the file is
  replaced only when the library's call returned.
- **OA-62.** At start, and after the app has been in the background for more
  than 60 seconds on the clock that counts since the phone started (a clock
  that went back counts as more), the unlock screen comes before any other
  and before the node starts, and no call of the node is made until it is
  open. The 60 seconds include the time spent in the system's file picker or
  permission dialog: a result that comes back (a file picked, the picker
  cancelled, the permission answered), or a resume, after more than 60
  seconds drops the result unread and locks the setup the same way. A
  "Working" page is drawn before the library is asked, because the key from
  the PIN takes about a second (OA-12). A wrong PIN is refused with the
  rule `pin-wrong` and changes nothing; a file that is cut or damaged is
  refused with the rule `OBMx`, in the library's words.
  The work posted after a tap (the try of the PIN, the lock of a new PIN)
  runs only while the app is visible: if the app was hidden meanwhile it
  does nothing, no PIN is tried, nothing is unlocked and the count of wrong
  tries is given back. The start of the node refuses while the app is
  hidden, and relocking also stops a node that runs.
- **OA-63.** Wrong PINs are counted in a small file of their own, `tries.obkt`
  in the app's `noBackupFilesDir`, as SR-75 says: the count plus one is
  written before each try and 0 after a right PIN; a refusal that is not
  `pin-wrong` gives the step back; from 5 wrong tries the waits are 30
  seconds, 1 minute, 5 minutes, 15 minutes and 1 hour; from the 5th the
  screen says how many tries are left; a clock behind the last wrong try
  restarts the wait; a file that is not exact counts as 9 wrong tries. The
  code is the same file as the Key app's, not a copy.
- **OA-64.** The 10th wrong try wipes key 1 from this phone, in this order:
  the setup file, the ceremony's file if there is one, the note that key 2
  is a hardware signer and the record of the first address check (OA-72),
  the Keystore key that seals them, then the tries file. A wipe that stops
  half way is finished at the next start (the tries file still says 10).
  The screen says that this phone's copy of key 1 was wiped after 10 wrong
  tries, that the money is not lost because two of the three keys still exist,
  and the next step: follow the recovery kit to move to a new wallet
  (`docs/DESIGN.md` §5.4, "Key 1 replacement is a move to a new wallet"), or
  ask the person who holds key 2. The node's own folder, the config and the
  till's token are not touched.
- **OA-65.** The setup file, the ceremony's file and the tries file are in
  the app's `noBackupFilesDir`, and the manifest and both rule files
  exclude every domain from cloud backup and from device transfer (OA-02),
  so that a file restored without the Keystore key, which never comes
  back, is never the thing that is lost. The wrapping key is not bound to
  biometrics or to the lock screen.

## 11. A hardware signer as key 2

When key 2 is a hardware signer (`Holder::OtherSigner` of the ceremony), the
payments of the wallet (refunds, cash-outs, speed-ups) do not go to the Key
app: the phone reviews and signs them with key 1 and hands them to the
signer, whose screen is the second review. Settings, batches and pairings
still go to the Key app. The library does all of it (`docs/SESSION-RULES.md`,
AS-90 to AS-99, and the ops 120 to 126 of the node's call); the app's part
is the screens and the transports, which are the rules of this section.
Status: built. The model (`HardwareModel`, `HardwareWords`) and the screens
(`Hardware.kt`) use the library's ops 120 to 126, the BBQr and UR calls of
the shared code, the system's document pickers and the camera scan. Whether
key 2 is a signer is read from a file the ceremony writes from its pairs of
keys (the setup text has no line for it), so a setup that is pasted has no
signer screens. The ceremony's last screen offers "Register the wallet on
your signer" as its main button when key 2 is one, and the open approvals
list a session of spends with "Continue on the signer". The camera scan and
the pickers are not yet counted in the 60 seconds of OA-62 (the review fixes
for that are not in this tree: see the TODO(OA-60) in `Hardware.kt`).

- **OA-70.** The person picks how a payment reaches the signer, from a
  list that names animated codes of a UR (`crypto-psbt`, in fragments of at
  most 200 bytes), animated codes of a BBQr (type P, in parts of at most 400
  characters) and a file (`<name>.psbt`, made with `ACTION_CREATE_DOCUMENT`
  and opened with `ACTION_OPEN_DOCUMENT`), each with devices that use it
  as examples. No choice is made for them. What comes back is read the same
  way, from a UR (`crypto-psbt` or `psbt`), a BBQr (type P or T, in any
  order) or a file, and is given to the library as bytes, which accepts a
  binary, base64 or hexadecimal PSBT or a finished transaction (AS-93).
- **OA-71.** The wallet is shown to the signer right after the ceremony
  and from the wallet screen ("Register the wallet on your signer"): the
  Coldcard multisig text as one plain-text code when the library says it
  fits, else as animated `ur:bytes`, BBQr type U or a `<name>.txt` file;
  and the descriptor (the form with `/0/*`, which is also the form of the
  UR) as one plain-text code when it fits, as `ur:crypto-output` or as a
  file. The person picks the way.
- **OA-72.** After the registration the screen asks the person to compare
  the first receive address with the signer's address explorer. Checked or
  skipped is stored with the time (the record of AS-99) and the wallet
  screen shows which, and when. A skipped check stays visible until it is
  done.
- **OA-73.** Before a payment is handed to the signer the screen lists, in
  the library's words, what the signer will show and what to compare: each
  address and amount, the change back to the wallet and the fee. The words
  say that approving on the signer is the second review, and that the
  payment must not be approved there unless every line matches.
- **OA-74.** A step that was refused by the library shows the library's own
  words (different payment came back; this signer did not sign; the file is
  not readable) and nothing else changes. A code that is not complete says
  that a part is missing and keeps reading, in any order.

## 12. Watching in the background

Instant sales need a heartbeat from the owner's phone no older than
`heartbeat_max_age_secs` (CR-50 to CR-53). Without this section the
heartbeats stop a few minutes after the owner closes the app. The setting
below keeps them going, with a key that does nothing else: the watch key
(CR-185, CR-186, LK-74, LK-75). Key 1 and the owner's identity key, which
signs batches, settings and pairings, are never in the service.

- **OA-80.** "Watch in the background" is a setting of the owner app and is
  off by default. Its row has the heading "Watch in the background", what
  it does ("The phone keeps following the shop's wallet while the app is
  closed, and tells the counters it is watching." and "Without this,
  instant sales stop about ten minutes after you close the app.") and the
  next step ("Turn it on to keep instant sales working. Android shows a
  notification while it runs."). It is on while the marker file
  `watch-on` is in the app's `noBackupFilesDir`, and nothing else says so.
  Turning it on makes the watch key (OA-81), writes the contents of the
  watcher, puts the key in the settings to be approved (OA-83) and starts
  the service; every step that fails leaves it off, with words.
- **OA-81.** The watch key is 32 random bytes drawn on this phone with
  `SecureRandom`, never derived from a wallet, a seed or an identity key.
  Its secret is in its own file `watch-key.obks`, and what the watcher
  starts from in `watch.obks` (the network, the address of Orbot, the
  relay and how to reach it, the counters' roots, the folder, the secret and
  the heartbeat pace), each sealed with the Keystore key
  `openbmt-owner-watch-v1`, which is a key of its own and asks for no PIN.
  The contents hold no wallet, no key 1 and no owner key. They do reveal the
  shop's counters and its relay to someone who can read the app's storage
  without the PIN and use the phone's Keystore, as the till's token
  (OA-16) reveals the till; they do not reveal a wallet description. The
  contents are read at the start of the library's watcher, which refuses
  anything that is not exactly what the library wrote (magic `OBM#`,
  version 1, at most 16 KiB, at most 64 counters and 16 first peers, every
  key a key, nothing left over, and the bytes encode again to the same
  bytes). The wipe of OA-64 deletes both files and the marker.
- **OA-82.** The service is a foreground service of type `specialUse` with
  a notification that cannot be swiped away: the title "Watching the shop",
  the text "Instant sales stay on while this runs." and one action,
  "Stop". It is started while the app is on the screen, when the setting is
  on, so that Android allows it, and it does nothing until the app is
  hidden and the wallet is handed over (OA-84). It returns
  `START_NOT_STICKY`, so Android does not start it again after it is
  stopped, and the app has no boot receiver, so it does not start by
  itself after the phone restarts: the owner opens the app once.
- **OA-83.** Turning the setting on asks for the notification permission
  on Android 13 and later, and not on earlier versions, and not again when
  it is granted. If it is refused the setting stays off and the screen says
  "Notifications are off", "Android needs to show a notification while the
  phone watches in the background, and notifications are off for this app."
  and "Allow notifications for OpenBMT Owner in Android's settings, then
  turn this on again." Naming the watch key in the settings is a loosening
  (CR-185): it is queued with the library's preview and `queue_settings`,
  shows the Key app's approval as any other change (OA-34, OA-35), and
  reaches the counters by settings delivery. The heartbeats count from the
  counters' next settings.
- **OA-84.** The node and the watcher never run together: the node has to
  let go of its folder first. When the app is hidden, the node writes the
  contents of OA-81 as it is then (its counters), stops, and the service
  starts the watcher; when the app is shown, the watcher stops and has let
  go of the folder before the node starts. The PIN lock and relock
  (OA-60 to OA-65) govern the screens only: the watcher asks for no PIN and
  holds no setup.
- **OA-85.** "Stop" on the notification turns the setting off: the marker
  goes, the watcher stops and the service ends. Turning the setting off in
  the app does the same, deletes both files, and queues the removal of the
  watch key in the settings, which is a tightening (CR-185). After "Stop" the
  settings still name the key until the app is opened and the setting is
  turned off there; heartbeats under it do no harm while it is named, and
  the key's secret is still in its file.
- **OA-86.** The watcher signs a heartbeat and nothing else: it holds the
  watch key, which the counters accept for heartbeats only (CR-186, LK-08).
  It never signs a spend, a batch, the settings or a pairing, holds no owner
  secret and no key 1, and every op of a node refuses its handle with "Open
  the owner app to do this." It reads no message that a counter sends to the
  owner, which the watch key cannot open (LK-75).
- **OA-87.** The watcher keeps the link's existing pace: a heartbeat to each
  counter every 20 seconds (5 seconds before a try again after a failure),
  and a turn of the library's loop that waits at most 1000 milliseconds for
  the network. It takes no wake lock and sets no alarm and no job, and the
  service asks for no battery exemption. Android may stop it with the phone
  asleep; then the heartbeats stop and the counters wait for blocks.
- **OA-88.** Everything goes through the proxy of OA-05. Before the watcher
  starts, and again about every 30 seconds while the proxy does not answer,
  the app asks it for a SOCKS5 greeting; if it does not answer the watcher is
  not started and the notification says "Not watching: Orbot is not
  answering. Instant sales are off." It goes back to "Instant sales stay on
  while this runs." when the proxy answers and the watcher has started.
- **OA-89.** The manifest has the three additions of OA-03 and one
  `<service>` with `android:foregroundServiceType="specialUse"`,
  `android:exported="false"` and the property
  `android.app.PROPERTY_SPECIAL_USE_FGS_SUBTYPE` with the value "Follows the
  shop's own wallet and tells the shop's counters it is watching". There is
  no `BOOT_COMPLETED`, no receiver, no provider, no activity alias, no
  wake-lock permission and no alarm permission; the Key app's and the
  counter app's manifests do not change. `tests/check-manifest.sh` checks
  all of it on the built APK.

## 13. A change of light and dark

- **OA-90.** A change between light and dark while the app runs does not
  finish and start the activity. The activity reads the palette of the new
  mode, sets the window colour, the bar colours and the status-bar icons
  again, asks for the insets again, and asks the screens to be drawn again in
  place. A page's padding for the bars is worked out whenever its views are
  built, so no gap shows above its band. With the node up the owner app draws
  the home screen; locked, the unlock screen, which still asks for the PIN;
  with no setup saved, the first screen. Nothing is drawn while a file picker
  or the camera is open, while the app is out of view, or while a key ceremony
  is in progress, whose screens are not drawn from state. What the app does on
  leaving and coming back is decided by OA-62, as before.

## 14. The look of the screens

- **OA-91.** Every screen is a page of the shared look: a bar at the top, a
  body that scrolls, and the buttons in a footer that stays. A message, a
  result, a refusal and a state are one panel (`resultPanel`); a screen that
  only waits has the ring (`working`); a screen of a code has the words above
  the code in its card; a scanner has a black bar and the frame over the
  preview; every other screen has its words under the bar, as a heading
  that differs from the bar's title, and the next step under the content.
  The words of every screen are the app's own, as the lists in
  `docs/screens` and the strings of the screens give them: place, size,
  colour and motion change, no word does. A sentence may be cut where it
  ends. The only words this look adds are the group titles of home ("Open
  approvals and queues", "Money", "Counters and devices", "Alerts and
  notes", "Settings and this phone") and the group titles of the settings
  page. The owner app plays no outcome sound: it has the ticks and the click
  of a scan, the shake of a refused PIN on the unlock screen and the screen
  that changes the PIN, the fade where the status card of home changes
  tone, and the logo that clicks together once over "The new wallet", the
  first time that screen shows in a ceremony.
- **OA-92.** Home has the network status as a pill with its dot and heading
  in the tone's colours, the balance sentence cut at its full stop ("The
  wallet can spend", the amount, "Checked to block ... of ..."), and under
  the amount the words and buttons of recovery and rescan and, with key 2 on
  a signer, the first-address line, all in one card. When the status is not
  good its what and next show under the pill. Under the card is one card of
  four rows (Money, Counters and devices, Alerts and notes, Settings and this
  phone), each with its sub-line and each opening a page of its buttons as
  rows. "Spend" is the footer's primary. A card for what waits has a row for
  open approvals, what waits to be delivered and the refund queue only for a
  count that home already has and that is above zero; home adds no call into
  the library to get one, and a row with no count is listed under its group.
- **OA-93.** The settings page has the version as the bar's sub-line. The
  six lines of the instant limits ("Largest instant sale, light check" and
  the rest) are cut at their comma into a table whose rows are the checks
  with their first letter in upper case, and whose columns are the first
  halves; "Underpayment still counted" follows as a key and a value. Every
  other line is in a folded group (Currency, Payments, Tips, Refunds and
  overrides, Safety, Counters' shares, Lightning node, and Watch in the
  background when it has a line) that opens as key and value lines. The
  read-only sentence is a footnote and "Change settings" is the footer's
  primary.
- **OA-94.** Every operation on the till's token file (save, load and
  remove) runs on one single-thread queue that the controller owns, in the
  order it was asked, and no other thread touches the file; the screen
  learns whether the file is there from the queue. Each till screen has a
  number that goes up whenever the till is taken off or connected, and a
  callback draws only if the number is still the one it started under;
  otherwise it does nothing. While a removal is in flight the till page
  shows no actions and no back arrow, and comes back when the removal is
  done. A remove followed by a save leaves the new token.
- **OA-95.** The Currency screen is the first of the editor. It shows the
  currency the settings hold and the ten choices as the library names them
  (`US dollar (USD)` and the rest, ending with `Bitcoin Standard (sats)`),
  and keeps the choice in memory with what else was typed. A choice other
  than the one held is a new currency: it forgets every limit typed so far
  and the price, shows every limit on the Instant sales and Refunds and
  overrides screens empty, with the words that say every limit is set again
  (CR-198), and puts the currency into the edit. A limit left empty is not
  sent, and the library refuses the review naming it; a limit typed is sent
  even when it is the number the old limit had. Choosing the held currency
  again leaves no change. Bitcoin Standard asks for no price.
- **OA-96.** Where the editor's words said dollars, they say the currency
  the limits are typed in (the new one while it is being changed), by the
  currency's name in the place of "dollars": the words of the limits and of
  the two screens that hold them, the words of a value that is not a whole
  number, the words of a price that is not one, the line "Now:" of a limit,
  and the label of the price field. Nothing else in those sentences changes.
  The settings view has a line `Currency` whose value is the choice's name
  in full, in a group of its own, and shows each limit in the currency's
  name with its satoshis, or with no satoshis for Bitcoin Standard, where
  they are the same number.

## 15. The built-in Tor

- **OA-97.** The app runs the Tor Project's C Tor itself, from the native
  library of the Guardian Project's `tor-android` (`docs/DEPENDENCIES.md`),
  in its own process and not as an Android component: the manifest does not
  change. `TorService` (`src/android/TorService.kt`) makes the same native
  calls in the same order as the library's own class, whose name it takes
  because the native functions are bound by it, and says so in its header.
  Tor has one SOCKS listener, on 127.0.0.1 at a port Tor picks, and its
  control socket is the one the native code gives. The app reads that socket
  with a strict reader (`TorControl.kt`): three-digit code, separator, text of
  printable ASCII, carriage return and line feed, at most 1024 bytes a line
  and 8 lines a reply, no data reply, a code that does not change inside a
  reply, and a reply that is not complete is refused. It sends five commands
  and no others: `AUTHENTICATE`, `SETEVENTS STATUS_CLIENT`, `GETINFO
  status/bootstrap-phase`, `GETINFO net/listeners/socks`, and `SIGNAL
  SHUTDOWN` to stop Tor. The proxy is handed to the node when Tor reports its
  first circuit, as `127.0.0.1:port`. Status goes to the app by a plain
  callback. One run is shared by every holder of a lease: the screens hold
  one while the app is on show and let go of it when the app is hidden, and
  the background watch holds one from its service's start to its end (OA-84),
  so the port the watcher uses is the one the node had (OA-81). The last
  lease let go of shuts Tor down.
- **OA-98.** Tor's data and cache folders are inside the app's
  `noBackupFilesDir`, in the folder `tor`, made readable, writable and
  searchable by the app alone; nothing of Tor is backed up or moved (OA-02).
  The command line has no option but the ones OA-97 needs: no `torrc` is
  written, so none is read, and no other option is given. A control socket
  left by an earlier run is deleted before Tor starts. The library file
  `libtor.so` is taken from the AAR pinned in `docs/DEPENDENCIES.md` and
  nothing else of it is; the build checks the AAR's SHA-256 and stops if it
  differs.
- **OA-99.** Settings and this phone has a row "Tor". It opens a page with
  two choices, "Built-in Tor" and "Use Orbot", the words of the one in use
  and no third choice: there is no way to turn Tor off. The choice is kept in
  the config file as the line `tor: builtin` or `tor: orbot` after the line
  `proxy:`, which keeps the address of Orbot for either choice. A new setup
  is saved with "Built-in Tor" and asks for no address. A config of the
  earlier form, which has only the line `proxy:`, is read as "Use Orbot" at
  that address and stays so until the owner changes it. "Use Orbot" asks for
  the address, checked as before. A choice is saved, the watcher is stopped
  so that it starts again from the new config, and then the app starts again
  with it. "Built-in Tor" says "Tor", "This app runs Tor itself, so Orbot is
  not needed." and "Nothing to do. To use Orbot instead, choose Use Orbot."
  "Use Orbot" says "Tor through Orbot", "This app sends everything through
  Orbot, which must be installed and running." and "Keep Orbot running. To
  stop needing it, choose Built-in Tor."
- **OA-100.** With the built-in Tor the screen says "Starting Tor", "This app
  is connecting to the Tor network, which can take up to a minute." and "Wait
  on this screen." while the app waits, for up to 90 seconds, for Tor's first
  circuit. If Tor stops, does not load or does not connect in time, nothing
  is sent: the node is not started, and the screen says "Tor did not start",
  "This app could not connect to the Tor network." and "Check that the phone
  is online, then try again. If it keeps failing, choose Use Orbot.", with a
  button "Try again" that starts again, with a new run if the old one ended.
  A read of the till that finds no Tor says the same. The background watch
  does not start while the built-in Tor has no circuit.
