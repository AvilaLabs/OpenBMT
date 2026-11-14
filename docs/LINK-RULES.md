# Link rules: messages between the shop's own devices

Status: specification for `crates/openbmt-link` (sections 1 to 5) and for
`crates/openbmt-courier` and the two nodes' link modules (section 6), written
before the code.
Each rule has an ID so that a test can name the rule it proves. The rules
implement `docs/DESIGN.md` §5.3 ("Record sync", "ciphertext") and research
track R5 for the part that carries bytes from one of the shop's devices to
another.

The shop's devices (counter, owner phone, Box) are in different places and
on different networks. They need to pass small signed things to each
other: the owner's heartbeat and commands to a counter, a counter's
check-in and records to the owner, signed settings and address batches to
a counter, a counter's signed "paid" to the Box. Each of those things is
already signed and checked by its own rules. The link adds three things
and no more: only the device it is for can read a message, the reader
knows which device sent it, and a relay in the middle learns as little as
possible.

The link is not a source of truth. Every rule elsewhere keeps checking
what arrives as if it came from an enemy.

## 1. The sealed message

- **LK-01.** A message is sealed for exactly one device and names exactly
  one sender. Both are identity keys (`openbmt_core::identity`).
- **LK-02.** The content is encrypted with ChaCha20-Poly1305. The key for
  each message is new: the sender makes a one-time secp256k1 key, takes
  the shared point with the reader's identity key, and derives the
  message key with HKDF-SHA256 over the shared point's x coordinate. The
  one-time public key, the reader's key and the sender's key are bound
  into the derivation. The nonce is all zeros, which is sound only
  because a key is never used for two messages; LK-03 is what keeps that
  true. The associated data of the AEAD is the signed header without the
  ciphertext hash: the hash is made from the ciphertext, so it cannot go
  into what the ciphertext is made with, and the signature over the hash
  binds the two the other way.
- **LK-03.** The one-time key comes from the caller's randomness, 32
  bytes each time. Sealing fails if the randomness fails. The library
  never reuses a one-time key, never derives one from the content, and
  never caches one.
- **LK-04.** The sender signs the message (BIP340, tag
  `OpenBMT/link/v1`) over: the version, the reader's key, the sender's
  key, the sender's message number, the kind, the one-time public key and
  the hash of the ciphertext. The signature is inside nothing: a relay
  can see that a message is well formed without being able to read it.
- **LK-05.** Opening checks, in this order, and stops at the first that
  fails: the frame (magic, version, exact length); the reader's key is
  this device's; the sender is on the reader's signed device list (the
  caller supplies the list); the signature; the message number is above
  the last one taken from that sender and not too far above it (LK-06);
  the ciphertext opens; the padding is well formed. Nothing of the
  content is returned on any failure.
- **LK-06.** Message numbers rise per sender and reader pair. A reader
  takes any number above the last and stores the new last number before
  it hands the content on. A number at or below the last is refused as a
  replay. Gaps are allowed: the link may lose messages, and what must not
  be lost has its own numbering (records, commands, heartbeats). A number
  more than 2^32 above the last one taken from that sender is refused, so
  that one message cannot move a sender's numbers out of reach; a sender
  that adds 1 each time never comes near it.
- **LK-07.** The content is padded before it is encrypted, up to the next
  of these sizes: 256, 1,024, 4,096, 16,384, 65,536 bytes. Larger content
  is refused; the sender splits it. A relay sees only which size step a
  message is in.
- **LK-08.** The kind is one byte, signed but not encrypted, so that a
  reader can refuse a kind it does not expect from that sender before it
  decrypts. The kinds and who may send each to whom:

  | Kind | From | To |
  |---|---|---|
  | Heartbeat | owner, watcher | counter |
  | Command | owner | counter |
  | Settings | owner | counter, Box |
  | Batch | owner | counter |
  | Device list | owner | counter, Box |
  | Check-in | counter | owner |
  | Records | counter | owner |
  | Records received | owner | counter |
  | Paid notice | counter | Box |
  | POS notice | Box | counter, owner |
  | Range | owner | counter |
  | Switch | owner | counter |
  | Store | Lightning counter | Box |
  | Stored | Box | Lightning counter |
  | Lightning report | Lightning counter | owner |

  The watcher is the owner phone's background watcher (LK-74). It sends
  heartbeats and nothing else. A kind from a sender whose role may not send
  it is refused. An unknown kind is refused.
- **LK-09.** The link gives no authority. A sealed command is still
  checked as a command, a sealed batch as a batch, with their own
  signatures and numbers. A message from the Box is a hint and never a
  heartbeat (CR-55).

## 2. The relay

A relay holds sealed messages until the reader fetches them. The Box is
one. A merchant with no Box can use any relay: it sees ciphertext.

- **LK-20.** A relay has a member list, set by whoever runs it: the
  identity keys of the shop's devices. It takes a sealed message only if
  it is well formed and the signature is valid (LK-04), is at most the
  largest size step plus the header, both the sender and the reader are
  members, the reader's mailbox exists (a leaving mailbox does not take
  messages, LK-28), and its number is above the
  highest the relay has taken from that sender for that reader. The last
  point stops a copy of an old message from pushing new ones out of a
  full mailbox; the relay keeps those numbers in memory only, so after a
  restart it knows only the messages it still holds. With an empty list
  it takes nothing. The relay has no device list of the kind a device has: it
  cannot tell an owner from a counter, only a member from a key it does
  not know.
- **LK-21.** A mailbox exists only for a member. It is made the first
  time that member proves it holds the key (LK-23). A relay has at most
  64 members (the most it will hold). Replacing the list stores the new
  list first, and then the mailboxes of members that are gone become
  leaving mailboxes (LK-28) and are deleted as LK-29 says; a member that
  is added back has its mailbox as a normal one again, with what it held.
  Replacing the list with an empty one makes every mailbox leaving. How
  the list reaches a Box from the owner (a signed device list) is part of
  the Box pairing that is still open (section 5); until then it is put
  there by hand.
- **LK-22.** A mailbox holds at most 256 messages and 4 MiB. When full,
  the oldest message goes. A relay promises nothing about delivery.
- **LK-23.** To fetch or to make a mailbox, the reader signs a fresh
  32-byte challenge from the relay (BIP340, tag `OpenBMT/link-fetch/v1`,
  over the challenge, the relay's own key, the action and the action's
  parameters: the cursor of a fetch, the number of an acknowledgement).
  The request names the challenge it answers. The challenge is used once
  and is good for 60 seconds, and only on the connection that asked for
  it. A connection holds at most one challenge: a new one replaces the
  old, and it goes when the connection closes. Without it nobody can
  read, list or empty a mailbox, and a request changed on the way (LK-33)
  no longer matches its signature.
- **LK-24.** Fetching returns the messages after a cursor the reader
  gives, oldest first, at most 64 at a time, and does not delete them.
  The reader says which it has taken; the relay then drops those.
- **LK-25.** A relay keeps no log of content and stores messages only as
  the sealed bytes. What it can see, and what the setup screen says it
  can see: the identity keys of sender and reader, the kind byte, the
  size step, and the time.
- **LK-26.** A relay that lies can do two things: hold messages back and
  hand out old sealed messages again. The first turns instant acceptance
  off (no fresh heartbeat) and shows as a missed check-in. The second is
  refused by LK-06. It cannot read, change or make a message.
- **LK-27.** A relay that is a Box answers a claim request while an
  invitation is open (`docs/BOX-RULES.md` BX-12) and refuses it otherwise;
  a plain relay always refuses it. A welcome request is handed to the hook
  its owner installed (BX-15) and a relay with none refuses it. The hook
  says whether the welcome was taken (and gives the member list that then
  applies), refused with the claim staying on offer, or refused with the
  claim withdrawn. Every failure of a welcome has the one refusal, so that
  nothing about the invitation leaks.

- **LK-28.** When a device stops being a member, its mailbox is not deleted
  at once: it becomes a leaving mailbox. It keeps the messages it held and
  takes no new message from anyone, and it cannot be made again. Its reader
  can fetch from it, with the same proof of key as for a member (LK-23),
  and can do nothing else: an acknowledgement is refused as from a device
  that is not a member. The reason: the settings that remove a device are
  put in its mailbox together with the Box's own copy, and the Box may
  apply them first.
- **LK-29.** A leaving mailbox is deleted when its reader has fetched to its
  end (a fetch that returns the last message it holds and says no more
  follow), or 7 days after it became leaving, whichever comes first. The
  caller of `set_members` and of `handle` gives the time; the relay reads no
  clock, and it ends the leaving mailboxes that are due when it is given
  the time with any request. A relay holds at most as many leaving
  mailboxes as it allows members: the one that has been leaving longest is
  deleted first.

## 3. Reaching the relay

- **LK-30.** The conversation with a relay is frames over one TCP
  connection: a 4-byte magic, a version, a type, a 4-byte length, the
  body. One request, one answer. A frame over the limit closes the
  connection. A connection that sends nothing for 30 seconds is closed. A
  relay serves at most 32 connections at once.
- **LK-31.** The relay is reached on the local network by address and
  port, or through a Tor onion service by name through a SOCKS5 proxy
  (the driver's own SOCKS code, with a new circuit per connection). A
  device that sends nothing around its Tor proxy (`docs/COUNTER-RULES.md`,
  CR-87) reaches an address through the proxy too, given to it as an
  address and never as a name. Nothing else: no domain name, no
  certificate.
- **LK-32.** The relay has an identity key of its own. A device that was
  given that key (through the signed device list) checks the relay's
  signature on its hello and refuses another relay. The relay's key
  gives it no other right. Only the hello is signed by the relay: later
  frames on a local connection can be dropped or delayed by someone on
  that network, with the effect LK-26 describes.
- **LK-33.** On the local network the frames are not encrypted a second
  time: the bodies are sealed already, and what LK-25 lists is visible
  to anyone on that network. The setup text says so. Over Tor the
  circuit hides it from the network.

## 4. Storage

- **LK-40.** A device stores, per sender, the last message number taken,
  and per reader, the last number it sent. Both are stored before the
  message counts or leaves. A sender that lost its counter must be
  paired again; it never starts over from zero by itself.
- **LK-41.** A relay stores mailboxes as files written whole or as an
  append-only log; a torn write loses at most the message being written.
- **LK-42.** A relay stores the leaving list beside the member list, in a
  file `leaving.set` of its own: for each leaving device its key (32 bytes)
  and the time it became leaving (u64 LE), then the first four bytes of the
  SHA-256 of those, written whole as the member list is. The leaving list is
  stored before the member list, so that a crash between the two leaves a
  device on both lists, which counts as a member, and never a removed member
  with no mailbox. A directory without the file has no leaving mailboxes. A
  file that fails its check stops the start, as the member list does. A
  mailbox file of a device on neither list is removed at the start (LK-41).

## 5. What these rules do not do

- **They do not hide who talks to whom from the relay.** The relay sees
  the keys. Only running the Box yourself keeps that inside the shop.
- **They do not give forward secrecy against a stolen reader key.** A
  thief who has a device's identity key and the relay's stored messages
  can read those messages. Mailboxes are small and are emptied as they
  are read; a sender's key alone opens nothing.
- **They do not deliver.** Anything that must arrive is asked for again
  by the rules that need it.
- **They do not protect a reader restored from an old copy.** A device
  whose inbox file is put back from an old copy will take old messages
  again. What must not repeat carries its own numbers (records, commands,
  heartbeats) and is refused there.
- **They cannot tell a first start from a lost file.** A missing number
  file looks like a first start. A paired device with no number file must
  be treated as needing a new pairing (LK-40); the caller makes that
  distinction.
- **They do not pair a Box.** The invitation, the claim and the welcome
  that do it are `docs/BOX-RULES.md` section 2; the relay only carries the
  claim and the welcome (LK-27).

## 6. Carrying the shop's messages

`crates/openbmt-courier` joins the sealed message and the relay into one
object that each side holds, and the counter node and the owner node use it
to carry what the earlier documents leave to "the transport": the owner's
heartbeat, commands and batches to a counter, and a counter's check-ins and
records to the owner. The courier adds no authority (LK-09): what it hands
over is still checked by the rules of the thing it carries.

- **LK-50.** A courier has the device's identity and role, the relay
  (route, key and timeouts), and a table of the devices it talks to with
  their roles, as the signed device list gives them. A message from a key
  that is not in the table is refused (LK-05), and so is a message to one.
  The courier opens a connection to the relay for each batch of work and
  drops it afterwards; it has no networking of its own beyond the client of
  section 3.
- **LK-51.** To send, the courier takes the next number for that reader and
  stores it (LK-40) before it seals, and seals before it hands the relay
  anything. A relay that cannot be reached is an error the caller may try
  again. A second try takes a new number and a new one-time key (LK-03): a
  number is never used for two messages, even when the first did not get
  through.
- **LK-52.** To poll, the courier makes the device's mailbox if it has not
  yet (LK-21), fetches from the start of the mailbox (LK-24), opens each
  message by LK-05 and hands the content of those that open to the caller
  with the sender, its role and the kind. A message that does not open is
  skipped and counted, and the reason is kept in a short list that drops its
  oldest line first. Every message that was taken or refused is acknowledged
  so that the mailbox empties. A message whose number could not be stored
  is not taken and not acknowledged. Because the mailbox is emptied, the
  courier starts each poll from the start; a relay that hands an old message
  out again meets LK-06.
- **LK-53.** A courier is told whether this is the device's first start. On
  a first start it makes the two number files (`link-inbox` and
  `link-outbox`) empty. On any other start a missing or unreadable file
  stops the start with the message that the device needs a new pairing
  (LK-40): the numbers are never started again from zero here. Each file is
  written whole to a temporary file, flushed and renamed over the old one.
- **LK-54.** A message of kind Records carries whole records, oldest first:
  for each record its length as 4 bytes little endian and then the bytes of
  `Record::to_bytes`. The sender puts in as many whole records as fit one
  message. A content that is empty, that stops inside a length or a record,
  that has a record which does not parse, or that has anything after the
  last record is refused whole.
- **LK-55.** A message of kind Records received says what the owner holds of
  a counter's chain: the number of records as 8 bytes little endian and then
  the 32-byte head after them (the hash of the last record, or 32 zeros for
  none). The counter takes it as the owner's mark only if the number is not
  more than the counter's own length and the head is the hash of its own
  record at that place. Any other is ignored and noted. A valid mark below
  the last one is taken: the owner may have lost part of its copy. The
  hash at a place below what the counter holds in memory is read from its
  stored records (CR-81); a read that fails leaves the last mark as it was
  and is noted.
- **LK-56.** The counter sends records to the owner when its chain is longer
  than the owner's mark and either some of the records after the mark were
  never sent or 30 seconds have passed since the last send. What it sends
  starts after the mark, oldest first (after what was already sent, when
  nothing is overdue). It sends at most 4 messages in one step. A mark that
  never moves therefore costs a resend every 30 seconds and no more. The
  records it sends come from memory, and from the stored records for the
  ones below what memory holds (CR-81); a read that fails is noted and the
  step sends nothing of what it could not read.
- **LK-57.** The counter checks in by appending a check-in record to its
  own chain (`openbmt_core::records`), through the same stored path as every
  other record, and sending that record as a message of kind Check-in. It
  does so once at start and then every 15 minutes (`docs/DESIGN.md` §2.9),
  whether or not its chain has changed. The configuration has two
  intervals, the shortest time between check-ins when the chain has
  changed and the longest when it has not; both are 15 minutes by default.
  The check-in is part of the chain, so it reaches the owner in the
  records as well. A check-in message that does not reach the relay, or
  that the relay refuses because the owner's mailbox is not made yet, is
  sent again at each later step, the same record, until it gets through
  or a newer check-in takes its place. No record is written for a resend.
- **LK-58.** The owner keeps a copy of each counter's chain. It takes a
  record into the copy only if its author is on the signed device list, the
  signature is valid, its number is the copy's length and its `prev` is the
  copy's head. The first record that fails stops the message; the records
  before it were taken. A record is written to the copy's file, which only
  grows, before it counts; a last record cut short by a crash is cut off at
  the next start, as the counter does with its own file.
- **LK-59.** A record the copy already holds (same number, same hash) is
  skipped without a word. A record with the number of one the copy holds and
  another hash, or that follows another record than the one the copy holds,
  is a divergence: the owner raises an alert naming the counter and the
  number and overwrites nothing.
- **LK-60.** A check-in from a counter is taken only if it is genuine and the
  copy, when it is long enough, holds it at its place
  (`Chain::check_against_check_in`). The owner keeps the latest one for each
  counter; one with a lower number than the kept one is ignored. A check-in
  numbered past the end of the copy, however high, finds the copy too short.
  When the kept check-in names records the copy holds differently, that is a
  divergence (LK-59). After taking records the owner sends the counter a
  Records received message for them, at most once in a step for each counter.
- **LK-61.** The owner sends its heartbeat to each counter on its list every
  20 seconds, and sends commands and batches when asked, as messages of kind
  Heartbeat, Command and Batch with the bytes of the signed thing as the
  content. The counter hands each to the same call a hand-fed one would use,
  and that call checks it by its own rules (CR-50 to CR-56 for a heartbeat,
  CR-14 for a command, `openbmt_core::batch` for a batch).
- **LK-62.** The owner raises an alert for a counter from which no check-in
  has arrived for 30 minutes, and one for a divergence. An alert is raised
  once and stays in the owner's list until it clears: a check-in clears the
  first, and the owner's own word clears the second.
- **LK-63.** A message that opened but whose content does not decode, or
  that the node refuses by its own rules, is noted and the step goes on.
  A device list is noted and dropped, because the device list travels inside
  the settings (CR-166). No message makes a step fail, and no message makes a
  step stop before the rest are looked at.
- **LK-64.** The owner sends signed settings, as messages of kind Settings with
  the bytes of the signed settings as the content, to every counter and Box on
  the device list of the settings and of the settings before them, so that a
  device the new settings remove gets the settings that remove it (the
  devices are kept with the delivery, LK-76, so this holds after a
  restart). The owner sends them when they are handed to the link and then
  at the end of each step, to each device that has not shown it holds the
  version. A device is sent a version again only after 120 seconds.
  Versions go to a device lowest first, one at a time, so that a counter
  checks each against the one before. Settings that do not check against
  the pinned roots, or that lack a signature they need given the settings
  before them, are not sent, and the link says why in words. The link
  carries at most 16 settings at once.
- **LK-65.** A counter shows it holds a version by the `Setting` record of its
  chain (CR-162) that the owner's copy of the chain holds. A Box has no such
  record, so a Box counts as holding a version once the relay took the
  message for it.
- **LK-66.** Settings stay with the link, and in the owner's outbox, until every
  device they are for holds them, the devices that LK-76 keeps included.
  The link says which devices still do not, and the outbox row of the
  owner's app names them. The link forgets settings when the owner's app
  takes them out of the outbox.
- **LK-67.** The counters and Boxes the owner talks to are the ones in the
  signed settings in force and in the settings waiting to be delivered, not
  a list the owner's app was started with. The link follows the settings
  whenever they change: a counter the settings add gets a copy and its
  heartbeats at once, and a Box they add is taken as a peer. A counter the
  settings remove stays on the link while settings carried by it are still
  for that counter (LK-64, LK-66), so that it gets the settings that removed
  it, and is dropped when they are done. Its copy stays on disk. A link
  needs no counter at the start. The counter lines of the owner's setup
  text, if there are any, are read and checked as keys, and the link uses
  them only while the owner holds no settings; the first settings replace
  them.

The next four rules are about address batches, which the owner keeps and
sends again (the settings above have their own outbox, and the two are not
merged).

- **LK-70.** The owner keeps each batch of addresses it sends to a counter
  until the owner's copy of that counter's chain shows that the counter holds
  it: a record of the counter's own, of kind `Setting`, that names the batch's
  range and the digest of its signed bytes (CR-167). The owner looks for
  those records in each copy at every step, as the copy grows. A batch is
  dropped from the pending list when its record is in the copy, or when the
  copy shows a later batch held (one that starts at or after the end of this
  one), because the counter has gone past it. A record with the range of a
  pending batch and another digest drops nothing.
- **LK-71.** A batch that is still pending is sent again by the link, as a
  message of kind Batch like the first, at most once in 120 seconds for each
  counter, at the step after the time is up. The pending batches of one
  counter go in one round, in the order of their first index. The first send
  is made when the batch is handed over and counts as the first of the 120
  seconds; a send that did not reach the relay is tried again at the next
  step. After a start the first step sends what is pending. The counter takes
  a batch it already holds as nothing (CR-168), so a send that was not needed
  does no harm.
- **LK-72.** The pending batches are kept in one file in the owner node's
  directory, `link-batches`, with the time each was first sent. The file is
  written whole (a temporary file, a flush, a rename, a flush of the
  directory) before the first send of a batch and after any change, and read
  back at the start. It is framed with the magic `OBM!` and version 1,
  holds at most 64 batches, and is in the order of the counter's bytes and the
  first index of the batch. A file that is cut, has a byte left over, has an
  entry out of order or twice, has a batch that does not decode, or does not
  encode to the bytes it was read from, refuses the start of the link.
- **LK-73.** A batch that has waited 24 hours raises an alert for its counter,
  naming the time of the oldest pending batch. The alert is raised once and
  clears itself when no batch for that counter has waited that long. While a
  batch waits, the owner app says so for that counter: its addresses are on
  their way, and nothing needs doing because the phone sends them again by
  itself. After 24 hours it says that the counter has not taken its new
  addresses and that the owner should check that it is on and linked. The
  words follow `docs/SCREEN-RULES.md`.
- **LK-74.** The owner phone's background watcher is a sender of its own: the
  watch key the signed settings name (CR-185), in the role Watch. It sends
  messages of kind Heartbeat to a counter and no other kind to anyone (LK-08);
  the courier refuses to seal any other, and a reader refuses any other that
  is made by hand. A counter lists the watch key of the settings in force as
  a Watch peer, and no other: the table follows the settings at each step,
  adds the key they name, drops one they no longer name, and a message from a
  key that is not in the table is refused (LK-05). The relay lists the watch
  key as a member like any sender (LK-20); the person who runs the relay adds
  it. The heartbeat inside is checked as CR-186 says, so a message from the
  Watch role is taken only if the content is signed by the watch key.
- **LK-75.** The owner phone's watcher while the app is closed (`WatchNode`)
  runs the watcher and a link that sends heartbeats and reads nothing: it
  starts with the watch key's secret, the counters, the relay and the network
  settings, and holds no owner secret and no key 1. It sends each counter a
  heartbeat signed by the watch key (CR-186) every `heartbeat_every_secs`
  (20 by default), calling the relay at most that often, and after a failure
  not again before `retry_secs` (5). It does not poll a mailbox: what the
  counters send to the owner is sealed to the owner's key (LK-02), so their
  check-ins and records wait for the owner app. Its number files are in a
  folder of their own, `watch-link`, in the node's folder, and the heartbeat
  number is the same file the owner node uses. It takes the node's folder
  and its lock, so it does not run with the owner node. A relay that answers
  and refuses (the watch key is not a member) is told from one that is down,
  and the link says so.

- **LK-76.** The devices a pending settings delivery is for, the counters
  and Boxes of the settings and of those before them (LK-64), are kept
  with the delivery in one file in the owner node's directory,
  `link-settings-roster`. It is written whole (a temporary file, a flush, a
  rename, a flush of the directory) when the settings are handed to the
  link and after any change, and read back at the start. It is framed with
  the magic `OBM$` and version 1; each entry holds the outbox number, the
  settings version and the devices with a role byte (1 counter, 2 Box),
  in order of number and of key, with none twice, at most 16 entries and
  256 devices each. A file that is cut, has a byte left over, has an entry
  or device out of order or twice, has a role that is not a counter or a
  Box, or does not encode to the bytes it was read from, refuses the start
  of the link. After a start the link talks to those devices and does not
  count the delivery done while one of them lacks the settings, as if the
  settings before them had been handed over again. The entry goes when the
  delivery is done, or when the owner's app says its outbox no longer holds
  that delivery.
  In every pass that sends settings, a first send and a retry alike, the
  copies go to the devices the settings remove first, then to the other
  counters, and to a Box last. A Box that applies the settings as soon as it
  has them then cannot close the mailbox of a removed device before that
  device's copy is in it (LK-28). A send that cannot reach the relay stops
  the pass, so a Box is not sent a copy before the ones ahead of it.
- **LK-77.** A message of kind Range carries one signed index range (`OBM*`,
  `docs/WALLET-RULES.md` WL-24) for a counter paired with the shop's own
  wallet (`docs/COUNTER-RULES.md` CR-235). The owner keeps and sends it again
  as LK-70 and LK-71 say for a batch, and drops it from the pending list when
  the copy of the counter's chain holds the counter's record of it (CR-235),
  or a later range held.
- **LK-78.** A message of kind Switch (byte 12) carries one switch from the
  starter wallet (`OBM]`, `docs/COUNTER-RULES.md` CR-257) for one counter.
  The owner keeps and sends it again as LK-70 and LK-71 say for a batch, and
  drops it from the pending list when the copy of the counter's chain holds
  the counter's record of CR-262 for that new wallet, either byte.

The two kinds that involve the Box are carried the same way. A counter sends
a Paid notice to each Box on its device list when a sale changes state, and
the Box sends a POS notice to the owner. The contents, how they are numbered
and stored, and what the receiver checks are in `docs/BOX-RULES.md` section
4 and its paragraph "What the code fixes". A counter takes a POS notice, opens
it and drops it with a note (LK-63).

The intervals in LK-56, LK-57, LK-61, LK-62 and LK-64 are fields of a
configuration with these defaults. A step that finds the relay unreachable
tries again after a pause (5 seconds by default), so that a loop that runs
several times a second does not wait for a connection each time.

## 7. Shop Lightning's store

The Lightning counter keeps the second copy of its node's state on the
shop's Box (`docs/SHOP-LIGHTNING-RULES.md` SL-73, SL-74). These rules say
how the two talk.

- **LK-80. The kinds.** Store (byte 14) goes from the Lightning counter to
  the Box that the settings in force name as its second device, and Stored
  (byte 15) goes back. Lightning report (byte 16) goes from the Lightning
  counter to the owner. A message of these kinds from or to any other
  device is refused (LK-08). A Box whose last settings named it as the
  second device of a counter keeps taking that counter's Store messages
  for the time `docs/BOX-RULES.md` BX-94 gives, so that a node closing its
  channels still stores twice.
- **LK-81. Numbers of their own.** Store and Stored messages take their
  numbers from a count of their own for each sender and reader pair, apart
  from every other kind, so that a Store sent straight to the Box and a
  Paid notice still waiting in the relay never refuse each other. Both
  counts are stored as LK-40 says.
- **LK-82. Straight to the Box.** A Store goes to the Box as the relay
  request "store" on the counter's relay connection, and the answer to that
  request is the Box's Stored message. A Store never waits in a mailbox,
  and a relay that is not a Box refuses the request. The request needs no
  challenge (LK-23): the seal proves the sender, and the number stops a
  replay.
- **LK-83. What they carry.** A Store carries a request ID of 16 random
  bytes, an action (write, read, list or remove) and a name (the node's
  names: 1 to 125 of the characters `A-Z a-z 0-9 _ - ~`, so that the name
  in hexadecimal is a file name the Box can keep, BX-93). A write carries
  one part of the blob, at most 60,000 bytes, with the part's number, the
  number of parts and the SHA-256 of the whole blob; a blob is at most
  16 MiB. A read names the part it asks for. A list names the name to list
  after, so that a long list comes in turns. A Stored carries the same
  request ID and action and then: for a part of a write, "taken", and for
  the last part, "written" with the SHA-256 of what the Box stored; for a
  read, the part with its number, the number of parts and the SHA-256 of
  the whole blob, or "none"; for a list, the names that fit and whether
  more follow; for a remove, "removed"; or a refusal with its cause. The
  counter checks that the request ID, the action and the SHA-256 are its
  own before it counts anything.
- **LK-84. The Lightning report.** It is sent with each check-in (LK-57)
  and carries what `docs/SHOP-LIGHTNING-RULES.md` SL-78 lists, each as a
  number of satoshis, a time or a cause code. It is not a record: a lost
  report is replaced by the next one, and the owner keeps only the newest.
