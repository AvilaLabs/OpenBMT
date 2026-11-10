# Link rules: messages between the shop's own devices

Status: specification for `crates/openbmt-link`, written before the code.
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
we can manage.

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
  | Heartbeat | owner | counter |
  | Command | owner | counter |
  | Settings | owner | counter, Box |
  | Batch | owner | counter |
  | Device list | owner | counter, Box |
  | Check-in | counter | owner |
  | Records | counter | owner |
  | Paid notice | counter | Box |
  | POS notice | Box | counter, owner |

  A kind from a sender whose role may not send it is refused. An unknown
  kind is refused.
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
  members, and the reader's mailbox exists. With an empty list it takes
  nothing. The relay has no device list of the kind a device has: it
  cannot tell an owner from a counter, only a member from a key it does
  not know.
- **LK-21.** A mailbox exists only for a member. It is made the first
  time that member proves it holds the key (LK-23). A relay has at most
  64 members (the most it will hold). Replacing the list stores the new
  list first, and then the mailboxes of members that are gone are
  deleted; the merchant clears a relay by replacing the list with an
  empty one. How the list reaches a Box from the owner (a signed device
  list) is part of the Box pairing that is still open (section 5); until
  then it is put there by hand.
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

## 3. Reaching the relay

- **LK-30.** The conversation with a relay is frames over one TCP
  connection: a 4-byte magic, a version, a type, a 4-byte length, the
  body. One request, one answer. A frame over the limit closes the
  connection. A connection that sends nothing for 30 seconds is closed. A
  relay serves at most 32 connections at once.
- **LK-31.** The relay is reached on the local network by address and
  port, or through a Tor onion service by name through a SOCKS5 proxy
  (the driver's own SOCKS code, with a new circuit per connection).
  Nothing else: no domain name, no certificate.
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
- **They do not pair a Box.** How a Box with no screen gets onto the
  signed device list is still open (`docs/DESIGN.md` §16).
