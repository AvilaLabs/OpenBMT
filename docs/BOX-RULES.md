# Box rules: the merchant's own small server

Status: specification for `crates/openbmt-box`. All sections are
implemented, section 4 (the POS on the Box, and its way to a vendor over
curl), section 5b (the status page) and section 5c (the till listener)
included. Section 5d (Shop Lightning's store) is specified and not built yet.
Each rule has an ID so that a test can name the rule it proves. The rules
implement `docs/DESIGN.md` §5.3 and close the open item of §16: how a Box,
which has no screen, is paired.

The Box is a program on a small computer the merchant owns. It does three
jobs and no others: it is the relay between the shop's devices
(`docs/LINK-RULES.md` §2), it reads the POS and matches orders to sales
(`docs/POS-RULES.md`), and it keeps what both need on disk. With Shop
Lightning on, it also keeps the sealed second copy of the Lightning
counter's state (section 5d), which it cannot open. It holds no
bitcoin key, derives no address, checks no payment and cannot move funds.
What it says is a hint to the other devices and never a proof.

## 1. What a Box holds

- **BX-01.** A Box has an identity key of its own, made from the system's
  randomness the first time it starts and stored in its data directory. The
  same key is the relay's key (LK-32). The key signs the relay's hello and
  the Box's sealed messages, and nothing else.
- **BX-02.** One process uses a Box's data directory at a time (NR-79).
- **BX-03.** A Box that is not paired takes no sealed message, makes no
  mailbox and reads no POS. It answers a hello, and a claim while an
  invitation is open (BX-12), and refuses everything else.
- **BX-04.** A Box never holds: a wallet description, an address, a
  transaction ID, a key that signs for the shop, or the owner's own POS
  sign-in. The POS token it holds is its own (BX-41).

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
up the Box, POS token included, so it is trusted for this anyway; the
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
  list becomes empty, the POS reading stops and the POS token is erased
  from its storage. It stays pinned to its roots (BX-16).
- **BX-24.** The roles the Box gives the other devices when it opens a
  sealed message (LK-05) come from the same settings: the owner root's key
  is the owner, keys with a share are counters, keys in the Box set are
  Boxes.

## 4. The POS on the Box

- **BX-40.** The Box runs the reading and matching of `docs/POS-RULES.md`
  on what the counters tell it in paid notices (LK-08). A paid notice is
  signed by the counter and names: the counter's key, the sale index, the
  amount and currency in the shop's money, when the sale opened, and one
  of "opened", "paid", "unpaid again" and "final" with its time. The Box
  checks the signature, that the counter is on the list, and that the
  notice's number is above the last from that counter, and builds the
  sale facts of PS-05 from the notices alone. A notice never carries an
  amount in satoshis, a rate, an address or a transaction ID, and the Box
  is never given a counter's records.
- **BX-41.** The POS sign-in the Box uses is entered in the Box's setup and
  stored in its data directory, in a file only the Box's user can read.
  The setup says what the token can do. It is separate from the owner's
  own read-only sign-in, so the owner's matching does not depend on the
  Box.
- **BX-42.** The Box sends what it finds (matches, alerts) to the owner as
  sealed messages of kind POS notice. The owner's device treats them as
  the Box's report and shows them as such; its own matching decides.
- **BX-43.** The Box closes an order in a POS only through a connector
  that can, only for a sale with a counter-signed "paid" notice, and with
  the idempotency key of PS-81. With no such connector it closes nothing.

- **BX-44.** The Box reaches a POS vendor over HTTPS only through the
  system's `curl`, run as a child process once for each request. It has no
  TLS library and no HTTP library, and the dependency lists do not change
  for it: curl and its TLS library come from the system and get the
  system's security updates. The path of curl is a setting, `curl` from
  `PATH` unless the shop's setup names another. A curl that is not there, or
  that the system will not run, is a refusal with words ("curl is not
  installed on this Box"), never a panic and never another way out.
- **BX-45.** The POS token is never in the arguments of curl and never in
  its environment. A request is given to curl as a config on its standard
  input (`--config -`): every header, the token's among them, and the body.
  The arguments hold the flags of BX-46 and the URL only. The child is run
  with an empty environment (so that no proxy variable and no user config
  file can change what it does), but for the places of the certificates, and
  `-q` is its first argument. A header or a body that cannot be written
  into a config is refused before curl is run.
- **BX-46.** Every request is bounded. The flags are `--proto =https`,
  `--tlsv1.2` (or later), `--max-time` of 30 seconds, a connect time of 15,
  `--max-filesize`, `--silent --show-error` and `--write-out '%{http_code}'`;
  redirects are not followed. The reply goes to standard output and is read
  with a cap of 8 MiB (PS-90): curl is ended when the cap is passed. A
  request body is at most 16 KiB. A time out, a name or server that cannot
  be reached, a TLS failure, a reply over the cap, a curl that failed in
  another way and an output with no status are each a refusal with words;
  the reading fails and changes nothing (PS-74).
- **BX-47.** When the settings in force say Tor is on, every request goes
  through the SOCKS5 proxy of the setup (`127.0.0.1:9050` unless it names
  another) with `--socks5-hostname`, so that the proxy resolves the name.
  A Box that cannot tell whether Tor is on takes it as on. If the proxy is
  not there, or refuses, the request is refused and nothing else is tried:
  there is no fallback to a direct request.
- **BX-48.** The status of a reply decides what it is. 200 to 299 is read.
  401 and 403 are "the token was refused": the words say to make a new
  read-only token at the vendor and give it with `pos-token`. 429 is "wait":
  the reading is not made again for at least a minute, or the time the
  vendor named if that is longer. A redirect is refused, and any other
  status is refused with it named.
- **BX-49.** The Box reads the POS of the vendor its own `pos-config` names
  (Square or Clover) with the reader of `openbmt-pos-readers` for it:
  one request for each page (and, for Square, the payments of its orders),
  a page at a time, the cursor stored after each
  page and a page that does not move it refused (PS-74, PS-80). A reading is
  made once a minute at most, a paid notice's wish for one at once
  included. The Box closes no order through the vendor's API: Square is
  read-only (`docs/DESIGN.md` section 5.6) and no Clover close is built.

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

## 5a. What the code fixes

These are details the rules above leave open. None of them changes a rule.

- **Files.** The data directory holds `box.key` (the identity secret),
  `invitation` (the invitation, its box nonce and the time it was entered),
  `roots`, `settings` (the signed settings in force), `link-inbox` and
  `link-outbox` (the message numbers of LK-40), `pos.token` (BX-41),
  `pos-notices` and `pos-ledger` (the POS side, below), and the directory
  `relay/` (the relay's member list and mailboxes). Every file is written
  with mode 0600.
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
  owner go to BX-21; a paid notice goes to the POS side (BX-40); a message
  of any other kind is noted as not handled yet and dropped.

### The POS on the Box (BX-40 to BX-43)

These details too leave every rule of section 4 as it is.

- **The paid notice.** A counter sends it as a sealed message of kind Paid
  notice to each Box on its device list (LK-08). The content is exactly 141
  bytes, strict, with the magic `OBMc` (157 bytes, and version 2, when the
  notice names a till sale, BX-77):

  ```text
  "OBMc" | 1 | counter key (32) | number (8) | sale index (4)
         | amount (8) | currency (3) | opened (8) | state (1)
         | state time (8) | signature (64)
  signature = BIP340 over the tagged hash
              "OpenBMT/paid-notice/v1" of every byte before it
  ```

  Integers are little endian. The amount is in minor units of the currency,
  the currency is three capital letters, the state is 1 opened, 2 paid, 3
  unpaid again or 4 final, and the number is from 1 and zero is refused.
  There is no field for a satoshi amount, a rate, an address or a
  transaction ID, and the length is fixed, so none can be added to a notice
  that reads. The sale reference of PS-20 is not carried: the Box works it
  out from the counter's key and the sale index.
- **What a notice says.** A notice is the state of the sale and not a
  change to it. The time of a "paid" notice is when the sale was first
  reported paid (so a "paid" notice sent after the sale was unpaid again
  carries the same time); the time of the other three is when the state
  was reached. "Unpaid again" and "final" clear each other, and "paid"
  clears both. A sale that is unpaid again is never also final.
- **When a counter sends one.** When a sale opens, and whenever the sale as
  the counter's own records give it (PS-06, PS-09) changes state, or the
  time it was first reported paid changes. The counter reads its own
  records to know: it writes nothing and asks nothing of the sale. The
  number is the counter's, from 1, shared by all its Boxes. The number and
  the notices not yet taken by the relay are stored whole in
  `notice-outbox` in the link's directory before anything is sent, and a
  restart sends again what the relay did not take. The relay taking it is
  the only acknowledgement there is. A queue is at most 4,096 notices, and
  when it is full the oldest go and a note says so. With no Box on the
  list nothing is read, queued or sent; a Box added later is sent the
  notices of the whole chain, the newest 4,096 of them. A notice that is
  lost is made good by the next one about the sale, which carries the
  whole sale. A failure to send or to store is a note on the counter and
  never an error in front of the cashier, and never changes a sale.
- **What the Box checks, in order.** That the notice reads; that the key
  that signed it is the key that sent the message; that the key is a counter
  in the settings in force (BX-24), which is how a counter taken off the list
  stops being heard; that the signature is good; that the number is above the
  last taken from that counter; that the amount, the currency and the time
  the sale opened are those earlier notices gave, when it has seen the sale.
  A notice that fails is dropped with a note, and its number is not used up.
  A notice for a sale the Box has not seen makes the sale from the notice.
- **What is stored (BX-50).** The Box keeps, for each sale, the fact of
  PS-05 as the notices gave it, and for each counter the last number taken
  (at most 256 counters; the numbers of counters no longer on the list are
  let go when the table is full). They are in `pos-notices` with the closes
  and the POS notices waiting for the relay, written whole, and the new
  value is stored before it takes effect, so a number is never taken
  without being stored. Messages that come together are taken as one write.
- **How many sales (PS-72).** At most 16,384 sales, about 1.2 MB. When a
  new sale would be the 16,385th, the Box drops up to 1,024 of the oldest
  sales that are final, not unpaid again, first reported paid more than the
  matching window (PS-33, 30 minutes) ago and with no close under way. A
  sale dropped is not judged again; a match to it is kept as it is (PS-36).
  The record of its close goes with it, so the closes recorded never
  outnumber the sales. If none can be dropped the new notice is refused
  with a note: nothing else is ever dropped for room.
- **The timer.** The time is the clock the Box is given. The POS is read
  every 30 seconds, and at once at the next turn of the worker after a
  paid notice was taken; a notice that is refused does not bring a reading.
  Each reading is a sync of PS-74 on all the sales the Box holds. A reading
  that fails (the POS is down, times out, answers with something that does
  not read, refuses the sign-in) is a note and changes nothing; the next one
  is at the next time. A POS that asks to wait is not read again before it
  said, if that is later. The timer needs a token and a paired Box that the
  settings list; with the Box unpaired or turned off the connector is told to
  forget the token and nothing is read (BX-03, BX-23).
- **The token (BX-41).** It is the one line of `pos.token`, at most 1,024
  bytes with no control character, in a file with mode 0600. The Box
  refuses to read it, with an error that names the file and says what its
  mode is, when the mode has any bit of the group or of others; the relay
  runs on and the POS is not read. A Box with no token file reads no POS.
  The console command `pos-token` takes the next line of standard input as
  the token and stores it; `pos` prints the state of the POS side (whether
  it reads and why not, whether the connector can close, the time of the
  last reading and its error, and counts) and never the token. A Box that
  is unpaired or turned off takes no token.
- **What the connector is.** The `Connector` trait of `openbmt-pos` is the
  reader of PS-80 and four things the Box needs: `set_token`, `can_close`
  (false unless the connector says so), `find_order` and `close` (PS-81).
  The binary holds no connector, so on its own it keeps the notices and
  reads no POS; a program that links the library gives it one.
- **The POS notice.** The Box sends what a reading made new, and what its
  closing did, to the owner as sealed messages of kind POS notice (LK-08),
  one thing in each, strict, with the magic `OBMd`:

  ```text
  "OBMd" | 1 | Box key (32) | number (8) | time (8) | item (1) | body
         | signature (64)
  signature = BIP340 over the tagged hash
              "OpenBMT/pos-notice/v1" of every byte before it
  ```

  The item is 1 a match (the order, the tender, the sale, how it was
  matched, whether by order of time, when), 2 an alert (as the ledger of
  PS-71 stores it, with its why and next step as the alert holds them), 3 an
  order the Box closed, or 4 a sale reported unpaid again after its order
  was closed. The number is the Box's, from 1. There is no field for a
  satoshi amount, an address or a transaction ID. A reading that finds
  nothing new sends nothing. At most 256 are kept for the relay; the oldest
  go first, with a note.
- **What the owner does with it (BX-42).** The owner takes a POS notice only
  from a key it holds as a Box, with that Box's signature on it and a number
  above the last from that Box. It keeps the newest 256 as the Box's report
  and shows them as such. It does not merge them into its own matching and
  does not act on them. Anything it refuses is a note. The list and the
  numbers are in memory. A counter takes a POS notice, opens it and drops it
  with a note.
- **Closing an order (BX-43).** On each turn the Box looks at the sales whose
  latest notice is "paid" or "final", which is what "a sale with a
  counter-signed "paid" notice" comes to once a sale can be final, and for
  each that has no close recorded asks the connector, which is the only one
  that knows the POS, for the one open order that carries the sale's
  reference (`find_order`). With no order named nothing is closed and the
  sale is tried again on a later turn. The connector is asked about at
  most 16 sales in a turn, taken in turn so that a sale with no order does
  not keep the others waiting, and only about sales first reported paid
  inside the matching window (PS-33): an older one is left to the owner.
  With one, the attempt is stored,
  then the connector is called with the idempotency key of PS-81, and then
  the result is stored with a POS notice to the owner. A stop between the
  two leaves the attempt, and the next turn calls again with the same key
  and the same order. A sale gets at most one close: the Box records
  it, and the key of PS-81 makes the POS refuse a second one even after
  the Box dropped the record with the sale. At most 16 orders are closed
  in a turn. A sale that is not reported paid now is
  refused before the connector is called, by `CloseRequest::for_sale`.
  A sale that goes unpaid again after its order was closed makes one POS
  notice of item 4 to the owner. Nothing is undone, and the sale is not
  closed again if it is paid once more.
- **A stop in the middle.** A reading's ledger is stored before the POS
  notices it made. A stop between the two loses those notices, which are
  hints; the owner's own matching sees the same conditions.
- **Files that do not read back.** A `pos-notices` or `pos-ledger` that is
  there and does not read is not read over and not made new: the POS side
  is off, a note says which file, and the relay runs on.

### The way to a vendor (BX-44 to BX-49)

These details too leave every rule of section 4 as it is.

- **The file `pos-config`.** The Box's own file, written whole (BX-50) and
  read strictly, with the magic `OBMq` and version 1. It holds the vendor
  (1 Square, 2 Clover), the currency, the vendor's location (Square's
  location id, or Clover's merchant id, which is 1 to 32 letters and
  digits), 1 to 8 exact names of the Bitcoin tenders (PS-10) and three
  optional texts: the path of curl, the Tor proxy as `host:port` and the
  vendor's host (`https://` and a name; the vendor's own when it is left
  out, which for Clover is the North American one). It holds no token. A
  file that is there and does not read back, or whose values are not valid,
  leaves the POS side off with a note; it is not read over and not replaced.
- **`pos-setup`.** The console command takes the next line of standard
  input as `key=value` words separated by `|`: `vendor`, `location`,
  `currency` and `tenders` (separated by `;`) are needed, `curl`, `socks`
  and `host` may be given. It stores the file and says the setup is used
  from the next start of the Box. A word that is not a key the Box knows, a
  key twice, a key left out or a value that is not valid is refused with
  words and nothing is stored.
- **One location.** The Box serves the one location the setup names. The
  counters at it are the counters on the device list in force, whatever the
  list is when the settings change, so a counter added or taken off is heard
  or not at the next step. A list the POS settings do not allow (PS-14: at
  most 16 counters to a location) is not taken, with one note, and the old
  list stays.
- **What curl is run with.** For a Square read: `curl -q --config - --silent
  --show-error --proto =https --tlsv1.2 --max-time 30 --connect-timeout 15
  --max-filesize 8388608 --request POST --write-out %{http_code}` and the
  URL, with `--socks5-hostname <proxy>` when Tor is on. The config has the
  headers (`Authorization: Bearer <token>`, `Accept`, and for a POST
  `Content-Type`) and, for a POST, the body as `data-binary`. A body that
  starts with `@` is refused, because curl would read it as a file. The
  environment is empty but for `SSL_CERT_FILE` and `SSL_CERT_DIR`, when
  they are set. `Square-Version` is not sent: the reply is in the version
  set for the application at the vendor.
- **What a failure comes to.** Each refusal of BX-44 to BX-49 becomes the
  POS side's `PosError` for the sync (401 and 403 a refused sign-in, 429 a
  request to wait, a time out or an unreachable server "not reached", the
  rest an answer that did not read) and a note of the form `the POS was not
  reached (<kind>): <rule>: <detail>`, whose words the `notes` and `pos`
  commands show. The reading after a 429 is not made before the later of
  the minute and the time the vendor asked for, which is a minute when the
  vendor did not say (curl is not asked for the header).
- **Square payments.** After the search of a page that has orders, the Box
  asks for the payments of the same orders (ListPayments, a GET, PS-95)
  through the same runner, the same token, the same caps and the same proxy,
  one request for each page of payments, at most 64 pages of 100. It stops
  at a page with no cursor or one that repeats the cursor it was asked from.
  More pages than that fail the poll with a note saying so: a page is never
  left unread in silence, since a Bitcoin tender's payment could be on it.
  The
  replies are handed to the reader with the orders, so a tender of type
  `OTHER` is known by its payment's source too and a payment cancelled after
  the order closed is seen (PS-96, **[verify, E5]**). A page with no order
  asks for none, and a Clover poll asks for none: Clover's payments come with
  its orders. A payments request that is refused fails the poll like any
  other (BX-46 to BX-48).

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
  waiting for an invitation. A Box that holds one that has not run out, or
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
- **BX-59.** No page holds a key, an invitation (whole or in part), a POS
  token, an address of a device, or the Box's own key. The shop is shown by
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

## 5c. The till listener

A till that sits in front of the shop's counters (an Odoo point of sale, for
one) can ask the Box for a bitcoin sale. The cashier presses Bitcoin in the
till, the till asks the Box, the Box asks one counter, the cashier taps Start
on the counter, the customer pays, and the till closes its own sale. The
listener is how the till talks to the Box. It is off unless it is asked for,
and it adds no way to move money: the counter still makes the quote, shows the
code and checks the payment.

- **BX-70.** The listener is plain HTTP/1.1 written with the standard library
  alone, on its own listener and its own threads, separate from the status
  page, and it answers JSON only. A request is at most 4 KiB, head and body
  together. Every line of the head ends in CR LF and holds printable ASCII
  only, with no folded line, no `Transfer-Encoding` and no `Content-Encoding`.
  One request is served for each connection (`Connection: close`). A request
  not whole 5 seconds after the connection was made is answered 400 and
  closed, the answer must be written within 5 seconds, and no more than 16
  connections are served at once; one more is closed at once. The listener is
  off unless the Box is started with `--till-listen <addr:port>` (IPv4 or
  IPv6, as BX-53); the Box prints `till listener: <addr>` when it is serving,
  and an address that is not an address and a port, or that cannot be bound,
  stops the Box at the start, saying so.
- **BX-71.** The till key is 32 bytes from the system's randomness.
  `openbmt-box till-key <data dir>` (or `till-key` on the console of a Box
  that runs) makes one, prints it once as 64 lowercase hex with the words
  `tillKeyMade`, and stores in `till-key` in the data directory, written
  whole with mode 0600 (BX-50), the tagged hash (`OpenBMT/till-key/v1`) of
  its bearer token and its reply key (BX-80), never the key itself. A new
  key replaces the old one, which stops working at the next request. Every
  request carries `Authorization: Bearer <token>`, the token of BX-80. The
  Box compares the hash of what it was given with a copy of the stored hash
  it holds in memory, in constant time. The copy is replaced when the Box
  makes a key, and read again when another process has replaced the file,
  so a new key works at the next request. A request with a wrong token
  never waits for the Box's state: only an accepted request takes it. No key file, no header, a header that is not a bearer
  token or a wrong token is a 401 with the words `tillKeyWrong`. The Box
  never prints or logs the key or the token.
- **BX-72.** `POST /till/v1/sales` takes `Content-Type: application/json`
  and an object of strings: `counter`, `amount`, `currency`, `request` and
  `reference` (the last may be left out). `counter` is the first 4 bytes of a
  counter's device key as 8 lowercase hex (the owner app's "Counter 1a2b3c4d")
  and must match exactly one counter of the settings in force, else 404
  `tillUnknownCounter`. `currency` must be the shop's currency of the settings
  in force, else 409 `tillWrongMoney`. `amount` is a decimal string above zero
  with at most as many decimals as the shop's currency has minor units (none
  for yen, won and satoshis), else 400 `tillBadAmount`. `reference` is cut to
  40 characters, with anything outside printable ASCII dropped. `request` is 1
  to 64 of `[A-Za-z0-9_-]`; the same `request` again answers the same sale and
  sends nothing new. An object that does not read is a 400 `tillBadRequest`
  with no words. The answer is 200 `{"sale":"<32
  hex>","state":"waiting","amount","currency","request","reference"}`, the
  last four as the Box took them: the amount written with exactly as many
  decimals as the currency has minor units, and the reference after it was cut
  (empty when there is none). A Box that has not joined the shop answers 503
  `tillNotPaired` to every request that carries the key.
- **BX-73.** `GET /till/v1/sales/<sale>` answers 200
  `{"sale","state","amount","currency","request","reference"}`, the last four
  as BX-72 gives them, where state is one of `waiting`, `opened`, `paid`,
  `unpaid`, `expired` and `cancelled`, and for the last four also `words`, an
  object of `heading`, `what` and `next`: for `paid` the plain line "Paid in
  bitcoin." in `what` with the other two empty, and for the rest the words
  `tillUnpaid`, `tillExpired` and `tillCancelled`. A sale the Box does not
  know, or an id that is not 32 lowercase hex, is a 404 `tillLost`.
- **BX-74.** `POST /till/v1/sales/<sale>/cancel` makes the sale `cancelled`
  unless it is `paid`, which stays `paid`, and sends the counter a cancel. The
  answer is the sale as BX-73 gives it. An unknown sale is a 404 `tillLost`.
- **BX-75.** Every body the listener writes is JSON in UTF-8 with
  `Content-Type: application/json`. A refusal is its status with `{"error":
  "<key>","words":{...}}` and the words of the key. Any other path, and any
  other method on a path of the three, is a 404 with `{"error":
  "tillNotFound"}` and no words; a request that does not read is a 400 and one
  over its limit a 413, each with an error key and no words. The paths are
  matched exactly, with no query, before the till key is looked at, so a path
  that does not exist says nothing about the key.
- **BX-76.** The Box asks the counter with a sealed message of kind POS notice
  to that counter only (LK-08), whose content is a sale request (`sale` 16
  bytes the Box makes, the amount in minor units, the currency, the reference
  and the time the request lapses) or a sale cancel (`sale`), with the magic
  `OBM+`. A request lapses 5 minutes after it was made; a sale nobody started
  by then is `expired`. At most 32 sales wait for a counter at once; one more
  is a 429 `tillBusy`. Sales and the messages not yet sent are in memory only,
  so a restart forgets them, and a sale it does not know is `tillLost`. A sale
  that has ended is kept one hour and then dropped. A message the relay did not
  take is sent again at the next step and then left off the list when the sale
  can no longer use it.
- **BX-77.** A paid notice may name the till sale it answers: `PaidNotice` and
  `NoticeDraft` carry `till_sale`, 16 bytes or none, signed with the rest. A
  notice with none is as it always was: version 1, 141 bytes, the same
  signature. A notice with one is version 2 and 157 bytes, the 16 bytes
  between the state time and the signature. Both read strictly.
- **BX-78.** The Box moves a till sale to `opened` on a counter-signed
  "opened" notice, and to `paid` only on a counter-signed "paid" or "final"
  notice, each naming that sale, from the counter that was asked, with the
  same amount and currency, with a good signature and a number above the last
  from that counter. Any mismatch is ignored with a note, and its number is
  not used up. "Unpaid again" before `paid` moves the sale to `unpaid`. After
  `paid`, a later "unpaid again" does not change what the till was told; it
  goes to the owner through the POS notices of BX-42, as it does for any sale.
- **BX-79.** A till sale never closes a POS order by itself, and BX-43 is
  unchanged: the Box closes an order only as BX-43 says. The till closes its
  own order when it reads `paid`.
- **BX-80. The Box's answers are signed.** The listener is plain HTTP
  (BX-70), so whoever is on the path between the till's server and the Box
  sees what is sent. The key the till holds is therefore never sent. The till
  sends as its bearer token the tagged hash `OpenBMT/till-bearer/v1` of the
  key, as 64 lowercase hex, and keeps the reply key, the tagged hash
  `OpenBMT/till-reply/v1` of the key, which the Box keeps too. Every request
  carries `X-OBM-Nonce`, 32 lowercase hex the till makes fresh for that
  request; a request without one is a 400 `tillBadRequest`. Every answer,
  refusals included, carries `X-OBM-Reply`, 64 lowercase hex: the HMAC-SHA256
  under the reply key of these, joined by line feeds: `OBM-TILL-REPLY-2`, the
  request's method, its path, its nonce, the SHA-256 of the request's body as
  64 lowercase hex (of no bytes for a request without a body), the answer's
  status as three digits, and the answer's body. The till takes an answer only
  when it carries the mac of the request it sent, body included, so an answer
  changed or made up on the path, an old one sent again, or a true answer to a
  request that was changed on its way, is refused: seeing the bearer token
  does not let anyone sign. The till also checks that the amount, currency,
  request and reference of every answer that names a sale are the ones it
  asked for, and cancels the sale (BX-74) and fails the payment if they are
  not. A `till-key` file of the earlier form, which holds no reply key, is
  answered 401 `tillKeyOld` until a new key is made.

What the code fixes. `till-key` takes no lock on the data directory (BX-02):
it writes only that one file, by renaming a whole file over the old, and the
Box reads the file at each request, so a key made while the Box runs works at
once. The file holds a version byte, the tagged hash `OpenBMT/till-key/v1` of
the token the till sends, and the reply key, each made as BIP340 tagged hashes
are, so the Box needs no hashing code of its own. A file in the earlier form,
which holds only a hash of the key, gets `tillKeyOld` until a new key is made.
The shop's currency is named by its three letters, `SAT` for Bitcoin Standard
(CR-196). A sale counts as waiting from BX-72 until
the counter's "opened" notice; at most 32 do, and at most 4,096 sales of any
state are kept, so that a till that makes and cancels sales cannot grow the
table without limit. An opened sale that nothing comes of for an hour counts as
`unpaid`. A "paid" or "final" notice moves a sale that is `unpaid`, `expired`
or `cancelled` to `paid` while the Box still keeps it, because the counter's
word that it was paid is what the till needs most; a till that cancelled a
sale and then reads `paid` has been paid. The numbers of the counters' notices
are checked for the till against the last till notice taken from the counter,
in memory, because the till works on a Box that reads no POS.

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
  back (LK-26), report orders wrongly, and with a closing connector mark an
  order paid that was not. The owner's own matching is what catches the
  last two.
- **They do not hide the shop's orders and revenue from whoever runs the
  Box.** For a hosted Box that is the host, and the setup says so.
- **They do not protect the POS token from someone with the Box's disk.**
- **They do not stop whoever runs the Box from guessing at payments.**
  The Box knows each sale's amount in the shop's money and its time, as
  the POS vendor does. With a public price that gives a rough amount in
  bitcoin and a time window to search the chain with. Only running the
  Box yourself keeps that inside the shop.
- **They do not stop whoever has the till key from asking a counter to
  start a sale.** The cashier still taps Start on the counter, the counter
  makes the quote and checks the payment, and nobody can move money with the
  key. The key is kept as a hash, so a copy of the Box's disk does not give
  it away, but a till that is taken over holds it.
- **They do not cover push wake-ups, a full node on the Box, updates or
  packaging.** Those are separate work (`docs/DESIGN.md` §5.3, R4).
