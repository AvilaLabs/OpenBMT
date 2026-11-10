# Security review, round 4: the first code against draft 3.1 (2026-11-09)

Design and code review, round 4. Findings and the fixes adopted.

These are internal design reviews. No independent security audit has been done
yet.

Round 4 read every line of the first code (`crates/openbmt-core`) next to
draft 3.1 of the design and asked two questions of each rule. Does the code
enforce the rule as the design states it? Does the rule still hold when one
component is compromised? Verdict: **one finding broke the single-component
claim as written (R4-1)**. It and the ten others are fixed inside the design.
Draft 3.2 of `docs/DESIGN.md` carries the changes.

Each finding says where its fix stands: **in code** (in `openbmt-core`, with
tests), or **design** (written into draft 3.2, to be built with the part of
the product named).

## A path to the incoming payments

- **R4-1. Four-word fingerprints could be searched for.** Draft 3.1 had the
  counter show its two pinned roots as four-word fingerprints, for the owner
  to compare with both phones. Four BIP39 words carry 44 bits, and the words
  came straight from a hash. A compromised owner app that wanted a counter to
  pin a fake Key app root needed only a key whose four words equal the real
  root's words: about 2^44 key tries, which is within reach of rented
  hardware. With a fake approver root pinned, the owner root and the fake root
  together sign batches of the attacker's addresses, and the counter accepts
  them. One compromised component then takes every incoming payment. This
  broke the round-2 fix for finding A1.
  Fix, in three parts:
  - **Stretch (in code).** Every four-word check runs the hash through 2^20
    rounds of SHA-256 first (`words::stretch`). Finding a second input whose
    words equal a given set now costs about 2^44 tries of about 2^21 hash
    steps each, about 2^65 in all.
  - **Per-ceremony words (design, pairing).** The counter fixes the roots it
    was given, and only then makes a fresh random number. The words come from
    the roots, the wallet ID, the counter's key and that number together.
    Nothing can be searched for ahead of time, because the number does not
    exist yet when the roots are chosen. A wrong root passes with a chance of
    1 in 2^44 per ceremony, and each try needs the owner to run the ceremony
    again.
  - **Full hashes in software, roots from the Key app (design, pairing).**
    The counter reads the roots from the Key app's own screen, not from the
    owner phone. The Key app scans the counter's pairing QR and compares the
    full hash of the roots with its own. The words are what the person
    checks; the software never relies on 44 bits.

  A limit that stays, and is written into the code's documentation: a party
  that supplies both of the things being compared can make two that share
  their words in about 2^22 tries, stretched or not. The words must never be
  the only check in that case. R4-2 is one such case.

## Losses without a thief

- **R4-2. The kit was checked by words alone.** The owner phone builds the
  wallet description (it controls the key 1 slot) and also prints the recovery
  kit. A compromised owner phone could prepare two descriptions that share
  their four words, give one to the Key app and print the other. The owner's
  comparison of the kit's words with the Key app's words would pass. The
  printed kit would then describe a wallet that holds nothing, and a recovery
  that depends on the kit would fail.
  Fix (design, setup and kit): after any print, the Key app scans the kit's
  QR and compares the whole description with its own, byte for byte. The
  monthly outside check is set up from the printed kit, so a wrong kit also
  shows up there as missing payments.

## The Key app's batch check

- **R4-3. A batch for an unknown counter was co-signed.** The Key app checked
  that a batch's addresses were the wallet's own, but not that the counter it
  was made for is on the signed device list.
  Fix (in code): `BatchError::UnknownCounter`. The check runs before the Key
  app signs and before its high-water mark moves.
- **R4-4. Unsigned input could move state, and state could be lost after a
  signature left.** The Key app did not check key 1's signature before it
  advanced its high-water mark, so a batch that the owner phone never signed
  could use up address indexes once it was shown to the Key app. Separately,
  nothing made a device store its new state before it released the thing that
  depended on it. A crash between the two could let an address index be
  issued twice, or let a counter forget instant exposure it had already
  taken.
  Fix (in code): the Key app's checks run in a fixed order: own wallet, by
  full hash; size and shape; key 1's signature; counter on the device list;
  range starts exactly at the high-water mark; addresses derived by the Key
  app match. The stateful operations take a "store this first" step and
  release their result only if the store succeeds: the co-signature, the next
  address, and the acceptance of an instant payment.

## Signatures and encodings

- **R4-5. The signing interface accepted a bare hash.** Every signed message
  is hashed with a tag that names its type, so a signature on a batch can
  never be read as a signature on a settings file. The functions that sign and
  check a bare 32-byte hash were public, which let a caller skip the tag.
  Fix (in code): they are internal to the crate. The public interface takes a
  message type and a body.
- **R4-10. Decoders were too accepting.** Checking a chain of records did not
  bound the size of a record body, and the encodings did not rule out two
  byte strings that decode to the same message.
  Fix (in code): record bodies over the size limit are rejected before any
  signature work. Each signed message has exactly one valid encoding, with a
  type marker, a version byte and size limits, and every decoder has a fuzz
  target.

## Instant-payment limits and settings

- **R4-6. No per-counter slice.** The design says each counter has its own
  signed slice of the shop's instant budget. The settings had one number per
  check type and nothing per counter, so two counters could each take the
  whole budget.
  Fix (in code): the signed settings give each counter a share in hundredths
  of a percent. Shares add up to at most 100%, and a counter's slice is the
  budget times its share. Adding a counter or raising a share is a loosening
  change and needs the Key app. Removing a counter needs only the owner and
  leaves the other slices unchanged.
- **R4-7. Settings had no hard bounds and could be rolled back on a new
  counter.** A validly signed settings file could carry extreme values (a
  100% underpayment tolerance, zero confirmations). A newly paired counter,
  which has seen no settings yet, would accept an old and looser file.
  Fix (in code): every device refuses settings outside fixed bounds
  (tolerance at most 5%, at least 1 confirmation, watcher heartbeat between
  1 minute and 1 hour, shares at most 100% in total), and a settings store
  can start from a version floor. The floor is named in the pairing entry
  (design, pairing). The bounds are first values, to be tuned.
- **R4-8. The own-node tier on shared Wi-Fi.** The higher own-node limits
  apply only because the counter's light client also saw the payment. On
  Wi-Fi that customers can join, without Tor, that second sighting is not
  independent of the network the customer controls.
  Fix (in code): in that case the sale is judged and labeled as "your Box
  reports it", with the lower limits. The light check is off there, as
  before.
- **R4-9. The fee-bump test used the wrong amount.** The design says a
  replacement that still pays the same address at least the same amount is a
  fee bump. The code compared the replacement with the amount counted for the
  sale, so an honest fee bump of a slightly short payment was called a cancel
  and paused instant.
  Fix (in code): the replacement is compared with what the original paid, and
  the original must have paid the sale's address something.

## Labels treated as identity

- **R4-11. Key fingerprints.** The design said the app compares "the
  fingerprints of all three keys" to catch one seed in two slots. A BIP32
  fingerprint is 32 bits, and a key with a chosen fingerprint is easy to
  make.
  Fix (in code and design): the full account public keys are compared. Two
  equal fingerprints still block the setup, as an early warning, but no check
  relies on fingerprints alone. The design now calls them labels.

## Checked and left as built

- Parsing a wallet description takes the network as a parameter, so a testnet
  description is never read as mainnet.
- The first settings a device sees are treated as a loosening change and need
  both signatures.
- The 24-hour window for a second cancel includes its last second, and a
  clock that runs backwards counts as inside the window.
- An edit to the last record of a chain, made with a stolen device key, is
  not visible until the next check-in. This is inherent in a per-device
  chain. The loss bounds do not depend on it.

## Open after round 4

- How a Box, which has no screen, is paired. Until that is designed, a Box's
  reports stay in the lowest tier.
- The pause after a cancel is timed by the device clock. The loss bound does
  not depend on it, but block height may be the better clock; to decide when
  the counter is built.
- Checking old records needs the device list as it was when they were
  written. The crate documents this; the sync design has to carry it.
- The stretch adds a short delay each time words are shown. It has not been
  measured on a phone.
