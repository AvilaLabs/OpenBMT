# Box rules: the merchant's own small server

Status: specification for `crates/openbmt-box`. Sections 1, 2, 3 and 5 are
implemented; section 4 (the POS on the Box) is not yet.
Each rule has an ID so that a test can name the rule it proves. The rules
implement `docs/DESIGN.md` §5.3 and close the open item of §16: how a Box,
which has no screen, is paired.

The Box is a program on a small computer the merchant owns. It does three
jobs and no others: it is the relay between the shop's devices
(`docs/LINK-RULES.md` §2), it reads the POS and matches orders to sales
(`docs/POS-RULES.md`), and it keeps what both need on disk. It holds no
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
  and forgets it when it is used, when the time is up, or when another is
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
  data directory is erased, which also gives it a new key.
- **BX-17.** Without a Key app (key 2 on a hardware wallet and no approver
  device) the owner's phone makes the invitation and checks the claim, and
  its screen says that pairing rests on the owner's phone alone, as it
  does for a counter (`docs/DESIGN.md` §2.9).

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
  force and the owner's key, and is replaced when new settings are stored
  (LK-21). Nobody sets it by hand on a paired Box. A device taken off the
  list loses its mailbox at once.
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

## 5. Storage

- **BX-50.** Everything a Box stores is written whole to a temporary file,
  flushed and renamed, or appended to a log whose torn tail is cut at the
  next start. A value is stored before it takes effect.
- **BX-51.** A Box that cannot read its key, its roots or its settings
  does not start as paired and does not make new ones over them: it stops
  and says which file is damaged.

## 5a. What the code fixes

These are details the rules above leave open. None of them changes a rule.

- **Files.** The data directory holds `box.key` (the identity secret),
  `invitation` (the invitation, its box nonce and the time it was entered),
  `roots`, `settings` (the signed settings in force), `link-inbox` and
  `link-outbox` (the message numbers of LK-40), `pos.token` once section 4
  exists, and the directory `relay/` (the relay's member list and
  mailboxes). Every file is written with mode 0600.
- **Welcome request.** The welcome and the first settings reach an unpaired
  Box over its relay connection as one request (`docs/LINK-RULES.md`,
  LK-27): the length of the welcome as 4 bytes little endian, the welcome,
  then the signed settings. The answer is "welcomed" or one refusal for every
  cause, so that nothing about the invitation leaks. A welcome that does not
  read or whose MAC does not check changes nothing and leaves the invitation
  open, stored, with the same nonce and expiry, so a stranger on the network
  cannot spend it: the invitation has 130 bits and a welcome with a right
  MAC needs it. One that passes the MAC and then fails (settings signed by
  other roots, settings that do not list the Box) spends it. An unpaired Box
  looks at one welcome request a second across all connections while an
  invitation is open, on the time of the request; any other in that second
  gets the same refusal. That bounds the work and the log, and means that
  a real welcome sent in the same second as another request is refused and
  sent again.
- **What an unpaired Box answers (BX-03).** A hello, a claim while an
  invitation is open, and a welcome. A request for a challenge is refused,
  so a mailbox, a fetch, an acknowledgement and a sealed message are all
  refused, and the member list is empty.
- **Time (BX-12).** An invitation is good while fewer than 1,800 seconds have
  passed since it was entered, on the Box's clock. It is kept in
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
  owner go to BX-21; a message of any other kind is noted as not handled yet
  and dropped.

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
- **They do not cover push wake-ups, a full node on the Box, updates or
  packaging.** Those are separate work (`docs/DESIGN.md` §5.3, R4).
