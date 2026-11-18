# Box rules: the merchant's own small server

Status: specification for `crates/openbmt-box`. All sections are
implemented, section 5b (the status page) and section 5d (Shop Lightning's
store) included.
Each rule has an ID so that a test can name the rule it proves. The rules
implement `docs/DESIGN.md` §5.3 and close the open item of §16: how a Box,
which has no screen, is paired.

The Box is a program on a small computer the merchant owns. It does two
jobs and no others: it is the relay between the shop's devices
(`docs/LINK-RULES.md` §2), and it keeps what the relay needs on disk. With
Shop Lightning on, it also keeps the sealed second copy of the Lightning
counter's state (section 5d), which it cannot open. It holds no
bitcoin key, derives no address, checks no payment and cannot move funds.
What it says is a hint to the other devices and never a proof.

## 1. What a Box holds

- **BX-01.** A Box has an identity key of its own, made from the system's
  randomness the first time it starts and stored in its data directory. The
  same key is the relay's key (LK-32). The key signs the relay's hello and
  the Box's sealed messages, and nothing else.
- **BX-02.** One process uses a Box's data directory at a time (NR-79).
- **BX-03.** A Box that is not paired takes no sealed message and makes
  no mailbox. It answers a hello, and a claim while an invitation is open
  (BX-12), and refuses everything else.
- **BX-04.** A Box never holds: a wallet description, an address, a
  transaction ID, or a key that signs for the shop.

## 2. Pairing: the invitation

A counter is paired by showing a code on its own screen. A Box has no
screen, so the secret travels the other way: the Key app makes it, and the
merchant types it into the Box.

- **BX-10.** The Key app makes an invitation: 130 bits from its own
  randomness. It is shown as 30 characters in six groups of five: 26
  characters of the invitation in Crockford base32 and 4 check characters
  (the first 20 bits of the tagged hash `OpenBMT/box-invite-check/v1` of
  the 26). Reading is forgiving in the usual Crockford way (case, `O` as
  `0`, `I` and `L` as `1`, hyphens and spaces ignored); a wrong check is
  refused before anything else happens.
- **BX-11.** An invitation is good for 30 minutes on the Key app's clock
  and for one Box. The Key app holds one open invitation at a time; a new
  one replaces the old. An invitation is spent by the first claim the Key
  app accepts or refuses with it.
- **BX-12.** The merchant gives the invitation to the Box through the
  Box's own setup (a settings field of the host system, a file read once at
  start, or the console). The Box keeps it for 30 minutes on its own clock
  and forgets it when it is used, when the time is up (or when its clock
  reads earlier than the moment it was entered), or when another is
  entered. While it holds one, the Box answers the relay request "claim"
  with:

  ```text
  claim: "OBM5" | 1 | box key (32) | box nonce (32) | mac (32)
  mac = HMAC-SHA256(k_claim, "OBM5" | 1 | box key | box nonce)
  ```

  The keys come from the invitation by HKDF-SHA256 with the salt
  `OpenBMT/box-invite/v1` and the labels `claim` and `welcome`. The box
  nonce is new for every invitation the Box is given, and stays the same
  while that invitation is open.
- **BX-13.** The owner's phone fetches the claim from the Box's address
  (typed, or scanned from the host system's own screen) and carries it to
  the Key app in an approval session. The phone cannot check a claim and
  cannot make one: it never sees the invitation.
- **BX-14.** The Key app checks the claim's frame and its MAC against the
  open invitation, in full and in constant time. A claim that fails spends
  the invitation (BX-11). On success the Key app shows "Box found" with the
  four fingerprint words of the Box's key, and the owner approves adding
  that key to the signed settings as a Box (BX-20). Adding a device is a
  loosening change: both roots sign.
- **BX-15.** With the approval the Key app makes the welcome:

  ```text
  welcome: "OBMM" | 1 | owner root (32) | approver root (32) | mac (32)
  mac = HMAC-SHA256(k_welcome, "OBMM" | 1 | box key | box nonce
                    | owner root | approver root)
  ```

  The owner's phone hands the Box the welcome and the signed settings. The
  Box checks the MAC with its invitation, then pins the two roots, then
  checks the settings under those roots (they must list the Box's own key
  as a Box), stores all three, and only then counts as paired. A welcome
  whose MAC does not check changes nothing: the invitation stays open. Once
  the MAC has checked, anything that fails leaves the Box unpaired with the
  invitation spent.
- **BX-16.** Roots are pinned once. A paired Box takes a welcome from
  nobody. To pair it with another shop, or again with the same one, its
  data directory is erased, which also gives it a new key. The one answer
  it still gives: the very welcome and settings it was paired with, byte
  for byte, are answered as paired and change nothing, so an owner's phone
  that lost the Box's first answer can finish pairing.
- **BX-17.** Without a Key app (key 2 on a hardware wallet and no approver
  device) the owner's phone makes the invitation and checks the claim, and
  its screen says that pairing rests on the owner's phone alone, as it
  does for a counter (`docs/DESIGN.md` §2.9).
- **BX-18. A shop with its own wallet.** A shop set up on the owner phone
  with its own wallet or a starter wallet has no Key app and no approver
  root (`docs/COUNTER-RULES.md` CR-227 to CR-236). Its welcome says so: it
  names the owner root and marks the shop as one of its own wallet, and the
  Box then checks settings as a counter of that shop does (CR-236 and
  section 24 of `docs/COUNTER-RULES.md`), with the owner root alone. The
  first settings, taken with the welcome, rest on the owner's phone alone,
  as BX-17 says. After them, a Free change is taken with the owner's
  signature; a Loosen change is taken only with a counter's confirmation
  (`docs/APPROVAL-RULES.md` AP-04, AP-05) by a counter of the settings in
  force, joined by the owner phone as `docs/OWNER-APP-RULES.md` section 19
  says; counters added while none is in force are Free (AP-12); a Redirect
  change is refused. The mark is pinned with the roots
  (BX-16): a Box never moves between the two kinds, and a welcome of the
  other kind is refused as a welcome for other roots. The owner phone keeps
  the Box's address and relay key with the shop's own wallet, hands the
  relay to each counter paired after the Box in its bundle (CR-130), and
  shows the relay code of CR-132 for each counter paired before.

What the invitation protects against. Someone on the shop's network, or a
phone that has been taken over, cannot get a key of their own onto the
device list as the Box: they would have to make a claim, and that takes the
invitation, which never passes through the phone or the network. They
cannot make the Box pin other roots: that takes the welcome key. A copy of
an old claim names the same Box key and gains nothing. The invitation has
130 bits, so nothing here rests on a short code or on how many tries are
allowed.

What it does not protect against: the computer the invitation is typed on.
Whoever reads the invitation there, within its 30 minutes and before the
real Box is approved, can stand in for the Box. That computer already sets
up the Box, so it is trusted for this anyway; the
setup text says to use the Box's own console or the host system's own
screen and not a shared computer.

## 3. The device list on the Box

- **BX-20.** The signed settings name which devices are Boxes: a set of
  keys, each also on the device list. A key is a counter or a Box, not
  both. Adding a Box is loosening; removing one is tightening.
- **BX-21.** A paired Box takes new settings only as signed settings that
  check against its pinned roots and carry a higher version than the ones
  it holds (the version floor of `openbmt_core::settings`). They arrive as
  a sealed message of kind Settings from the owner (LK-08) or by hand
  through the setup.
- **BX-22.** The relay's member list is the device list of the settings in
  force, the owner's key and the watch key the settings name, if any
  (CR-185, LK-74), and is replaced when new settings are stored (LK-21).
  Nobody sets it by hand on a paired Box. A device taken off the list, or a
  watch key the settings no longer name, takes no more messages, and its
  mailbox is kept to be fetched until it has been or for 7 days (LK-28,
  LK-29), so that the device learns it was removed.
- **BX-23.** Settings that no longer list this Box turn it off: the member
  list becomes empty. It stays pinned to its roots (BX-16).
- **BX-24.** The roles the Box gives the other devices when it opens a
  sealed message (LK-05) come from the same settings: the owner root's key
  is the owner, keys with a share are counters, keys in the Box set are
  Boxes.

## 5. Storage

- **BX-50.** Everything a Box stores is written whole to a temporary file,
  flushed and renamed, or appended to a log whose torn tail is cut at the
  next start. A value is stored before it takes effect. Every file the Box
  writes, the relay's mailboxes and lists and the chain data among them, is
  made with mode 0600, and every folder it makes with mode 0700; a
  temporary file left with a wider mode is emptied and set to 0600 before
  it is written, so the file it becomes is never wider.
- **BX-51.** A Box that cannot read its key, its roots or its settings
  does not start as paired and does not make new ones over them: it stops
  and says which file is damaged.
- **BX-52.** A Box may be ended at any instant, by any signal, SIGKILL
  included: it installs no handler and needs none. At the next start every
  file holds either the value from before the write or the one after it, and
  a temporary file left behind is overwritten by the next write. A Box run
  as a service (`--no-console`) reads no standard input, takes an invitation
  only from `--invitation-file` (or from the status page, BX-57, when that
  is on) and runs until a signal ends it.
- **BX-53.** The Box listens on the address it is given, IPv4 or IPv6, and
  accepts what that address allows. `0.0.0.0:8986` accepts IPv4 only.
  `[::]:8986` is the IPv6 any-address: on Linux it also accepts IPv4
  connections, as IPv4-mapped addresses, unless the host sets
  `net.ipv6.bindv6only` to 1, in which case it accepts IPv6 only. The Box
  does not change that setting. A host that reaches the Box by IPv6 (a
  StartOS raw TCP interface lists IPv6 addresses) needs `[::]` or an IPv6
  address in the bind.
- **BX-63. A restored Box.** A Box whose data folder is put back from a
  backup holds the settings it had then, so a counter removed since works
  on it again. The owner phone sends the settings in force to every Box when
  it starts and every six hours, so a restored Box takes the newer settings
  within that time.
## 5a. What the code fixes

These are details the rules above leave open. None of them changes a rule.

- **Files.** The data directory holds `box.key` (the identity secret),
  `invitation` (the invitation, its box nonce and the time it was entered),
  `roots`, `settings` (the signed settings in force), `link-inbox` and
  `link-outbox` (the message numbers of LK-40), and the directory `relay/`
  (the relay's member list and mailboxes). Every file is written with mode
  0600.
- **Welcome request.** The welcome and the first settings reach an unpaired
  Box over its relay connection as one request (`docs/LINK-RULES.md`,
  LK-27): the length of the welcome as 4 bytes little endian, the welcome,
  then the signed settings. The answer is "welcomed" or one refusal for every
  cause, so that nothing about the invitation leaks. A welcome that does not
  read or whose MAC does not check changes nothing and leaves the invitation
  open, stored, with the same nonce and expiry, so a stranger on the network
  cannot spend it: the invitation has 130 bits and a welcome with a right
  MAC needs it. One that passes the MAC and then fails (settings signed by
  other roots, settings that do not list the Box) spends it. Welcome
  requests are not limited in number: checking one is a single MAC, and a
  limit would let a stranger hold the real welcome back.
- **What an unpaired Box answers (BX-03).** A hello, a claim while an
  invitation is open, and a welcome. A request for a challenge is refused,
  so a mailbox, a fetch, an acknowledgement and a sealed message are all
  refused, and the member list is empty.
- **Time (BX-12).** An invitation is good while fewer than 1,800 seconds have
  passed since it was entered, on the Box's clock. One whose entry time is
  later than the Box's clock is expired and removed, the same as one past
  1,800 seconds (a clock that went back cannot lengthen it). It is kept in
  `invitation`, so a restart does not extend it. A damaged `invitation` file
  is forgotten, and so is one found on a paired Box.
- **Order of the writes (BX-15, BX-50).** The welcome writes `settings`,
  `link-inbox`, `link-outbox` and last `roots`. A Box is paired when `roots`
  is there. A stop or a failed write before that leaves it unpaired with the
  invitation spent and a stale `settings` that is overwritten by the next
  pairing; a stop after it leaves it paired with everything in place. The
  Box is never half paired. Unpaired was chosen over finishing the pairing
  at the next start because the invitation is spent and the welcome is gone:
  there is nothing to finish from.
- **Checking stored settings (BX-21, BX-51).** The settings store of
  `openbmt_core::settings` cannot be seeded with settings that were taken as
  a tightening and signed by the owner alone, so the Box keeps the settings
  in force itself and checks new ones with `SignedSettings::verify` against
  them: that gives the same version floor, the same loosening rule and the
  same roots. At a start the stored settings are checked again for both
  signatures that are there and for `validate`; a failure is damage (BX-51).
- **Damage (BX-51).** A missing `box.key` next to `roots` or `settings` is
  damage too, and so is `settings` missing next to `roots`. A new key is made
  only for a directory with none of them.
- **Members (BX-22).** The relay holds at most 64 members (LK-21). Settings
  whose device list and owner come to more are refused, so that no change
  can leave the relay with a list it cannot take.
- **Turned off (BX-23).** The Box that settings no longer list keeps its
  files and its roots. With no members nothing can reach it, so only settings
  put in by hand through the setup can list it again.
- **Number files (LK-40).** They are made empty when the welcome is taken. A
  paired Box that finds either one missing does not start: it needs a new
  pairing.
- **Invitation file (BX-12).** The first line of the file is the invitation.
  The file is deleted once it is read, whether or not the invitation was
  accepted.
- **Reading its own mailbox (BX-21, BX-24).** The Box opens what is sealed
  for it with the roles of BX-24 and the rules of LK-05. Settings from the
  owner go to BX-21; a message of any other kind is noted as not handled
  yet and dropped.

## 5b. The status page

A store that lists the Box on a home server asks that the app open to a page
in a browser. The Box has none of its own, and an invitation has been a file
copied in by hand. The Box can serve one small page, off unless it is asked.
The page is a convenience for the person who runs the Box: it shows what the
console's `state` shows, and takes the invitation that the file takes. It
adds no way to do anything else.

- **BX-54.** The page is plain HTTP/1.1 written with the standard library
  alone, read by a strict reader of its own. A request is its request line
  and headers (at most 8 KiB, every line ending in CR LF, printable ASCII
  only, no folded line, none of `Host`, `Origin`, `Referer` or
  `Content-Length` twice, no `Transfer-Encoding` or `Content-Encoding`) and a
  body of at most 1 KiB. The version is `HTTP/1.1` and `Host` is present.
  The only requests answered are `GET /` and `POST /invitation`; the target
  is matched exactly, with no query. Anything else is answered with the
  status alone and a fixed short text, no detail: 400 for a request that does
  not read, 404 for another target, 405 (with `Allow`) for another method on
  these two, 413 for a head or body over its limit. A `GET` has no body. A
  `POST` has a `Content-Length` and the type
  `application/x-www-form-urlencoded`. One request is served for each
  connection (`Connection: close`). A request that is not whole 5 seconds
  after the connection was made is answered 400 and closed, and no more than
  16 connections are served at once; one more is closed at once.
- **BX-55.** `GET /` answers one self-contained HTML page: no script, no
  external font, image or style, and a small inline style that suits light and
  dark and a phone. It shows the Box's state in the words of the console
  (`say::Words`; SC-01 to SC-12): waiting for an invitation, invitation open,
  paired (with the shop's four short words) or turned off, each with what it
  means and what to do next. It also shows the version, the address the
  devices give the Box as its relay (the host the visitor used and the
  relay's port), how many devices are on the list the relay serves, and the
  words for the Box's last note, if any. While the Box waits for an
  invitation it holds the form that posts one.
- **BX-56.** Every answer carries `Content-Security-Policy: default-src
  'none'; style-src 'unsafe-inline'; form-action 'self'; frame-ancestors
  'none'`, `X-Content-Type-Options: nosniff`, `Referrer-Policy:
  no-referrer`, `Cache-Control: no-store` and `Connection: close`, and a
  `Content-Length` that is the body's.
- **BX-57.** `POST /invitation` takes the form field `invitation` and gives
  it to the Box by the path the invitation file takes (BX-10 reading, check
  characters first; BX-12 holding and claim), and only while the Box is
  waiting for an invitation. The form also takes the field `setup`, the
  setup code of BX-62, and the Box checks it before it reads the
  invitation: a wrong or missing code is a 403 with words, changes nothing,
  and makes the page refuse every invitation for ten seconds. A Box that
  holds one that has not run out, or
  is paired, or is turned off, refuses with words and changes nothing. The
  answer is the page with the outcome in words above the state: taken (the
  claim is on offer), not read or wrong check characters (422), already
  held or already paired (409). A body that is not well formed, or has the
  field twice, is a 400.
- **BX-58.** A `POST` is refused (403, with words) unless its `Origin`
  header, or when there is none its `Referer`, names the same host and port
  as `Host`, compared without regard to case. An `Origin` that is `null`,
  has a path or user information, or is not `http` or `https`, does not
  match; with neither header the request is refused.
- **BX-59.** No page holds a key, an invitation (whole or in part), an
  address of a device, or the Box's own key. The shop is shown by
  four words derived from its owner root, and the last note by its heading
  and its two sentences only, never its detail. All text is escaped.
- **BX-60.** The page runs on its own listener and its own threads. A thread
  takes the Box's state only after a whole request has been read, and only
  for the one read or the one entry, so a connection that sends nothing,
  or sends slowly, uses only the page's own slots. The relay serves, and
  the Box pairs, while the page is full of slow connections.
- **BX-61.** The page is off unless the Box is started with `--status-page
  <addr:port>`, which names where it listens (IPv4 or IPv6, as BX-53). The
  Box prints `status page: <addr>` when it is serving. An address that is not
  an address and a port, or that cannot be bound, stops the Box at the
  start, saying so.
- **BX-62. The setup code.** While the Box is not paired and the page is
  on, it draws a setup code at each start, eight characters of Crockford
  base32 from the system's random source, shown as two groups of four,
  prints it after the page's address as `setup code: XXXX-XXXX`, and keeps
  it in memory only. Whoever reads the Box's log on the host system can
  enter an invitation on the page; whoever can only reach the page cannot.
  The invitation file and the console need no code: they are the host
  system's own.

What the code fixes. The shop's four words are those of the owner root's
fingerprint (the ones the Key app shows for it); they let the keeper tell two
shops apart and are not the root. The count is of the devices on the relay's
member list (BX-22), not of connections: the relay's server does not count
them. The page takes an invitation only from the state `Unpaired`; the
console's `invite` still replaces one that is open (BX-12), so a page that
anyone on the network can reach cannot push out the invitation the keeper
has just entered. The invitation file keeps working beside the page. The
page is plain HTTP: whoever can read the network between the browser and the
Box can read the invitation as it is typed, which the Umbrel and StartOS
hosts answer with their own login and TLS in front of it.

## 5d. Shop Lightning's store

- **BX-90. Who it keeps for.** A Box keeps the Shop Lightning state of the
  Lightning counter that its settings name with this Box as the second
  device (`docs/SHOP-LIGHTNING-RULES.md` SL-71), and of no other device. It
  answers the "store" request as `docs/LINK-RULES.md` LK-80 to LK-83 say.
- **BX-91. Stored whole before the answer.** The parts of a write are put
  in a file of their own as they come. When the last part is in and the
  bytes have the SHA-256 the parts name, the file is synced, renamed over
  the old blob of that name and the directory synced, and only then does
  the Box answer "written". A write whose parts stop for 60 seconds is
  dropped. At most 8 writes are in progress for the counter at once; one
  more is refused with its cause.
- **BX-92. How much.** At most 1 GiB and 10,000 names for the counter. A
  write past either is refused with its cause, and the status page says the
  store is full (section 5b).
- **BX-93. Where.** In `lightning/<counter ID in hex>/` of the data
  directory, one file per name, the file named by the hexadecimal of the
  name, mode 0600 as every file of the Box (section 5a).
- **BX-94. When the settings change.** When the settings stop naming this
  Box as a counter's second device, the Box keeps that counter's state and
  goes on answering its store requests for 30 days, so that a node closing
  its channels still stores twice (SL-77). Then it deletes that counter's
  directory. A counter named again within the 30 days finds its state as it
  was.
- **BX-95. Tested.** A write counts only after the file is in place; a
  write cut off leaves the old blob; a part with the wrong SHA-256, a write
  past the limits, and a store request from any device other than the named
  counter are refused; a counter that is no longer named is served for 30
  days and then its state is gone.

## 6. What these rules do not do

- **They do not make the Box trusted.** A Box that lies can hold messages
  back (LK-26).
- **They do not cover push wake-ups, a full node on the Box, updates or
  packaging.** Those are separate work (`docs/DESIGN.md` §5.3, R4).
