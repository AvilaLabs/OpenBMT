# Glossary

**Address batch.** A contiguous range of receive addresses assigned to one
counter, signed by key 1 and co-signed by the Key app.

**Approval session.** The owner reviews queued items with key 2 available.
The owner phone presents them; the Key app checks and signs the approved
items. Money movements and loosening changes require a session.

**Approver.** A Key app holding an identity key but no wallet key, used
when key 2 is a hardware wallet.

**BC-UR.** A format used by Sparrow and hardware wallets to transfer data
through animated QR codes.

**BIP321.** The standard for a `bitcoin:` payment request.

**Box.** The shop's optional server for relaying device messages and reading
POS orders. It holds no wallet key.

**Change.** Money returned to the shop's wallet by its own spending
transaction.

**Compact block filter.** A small summary used by a light client to find
blocks that might contain a relevant transaction.

**Confirmation.** Inclusion of a payment in a block. The shop's confirmation
depth starts at 1 block and its refund depth at 6. A refund can be paid at
the latter. Both show "Paid", with text distinguishing the depth.

**Counter.** The phone or tablet displaying the sale and payment code.
It holds no wallet signing key.

**Descriptor (wallet description).** Text describing how to derive the
wallet's addresses from its three public keys.

**Exposure.** Instant sales still waiting for their first block.

**Full node.** A computer that checks every block itself. Running one is
optional for the shop.

**Heartbeat.** A signed message from the owner phone telling the counter
that the owner's watcher is running.

**Instant.** Acceptance before the first block, within the shop's limit.

**Key app.** The separate app holding key 2 and providing the second
approval.

**Light check.** A check through Bitcoin peers and compact block filters
without running a full node.

**Loosening / tightening.** A settings change that permits more risk, or
one that reduces it.

**Multisig, two of three.** A wallet requiring any two of its three keys
to spend.

**Orbot.** A separate Android app providing Tor connections to other apps.

**PSBT.** A standard transaction file passed between signers.

**Recovery kit.** A printed page of wallet recovery information, without
secret keys or seed words.

**Release key.** A key signing an app for distribution, separate from the
wallet's keys.

**Reorganization (reorg).** Replacement of blocks by a different set of
blocks.

**Root.** An owner-phone or Key-app identity key pinned by a counter at
pairing.

**Satoshi (sat).** One hundred-millionth of a bitcoin.

**Tor.** A network that hides where a connection originates.

**xpub.** A public key used to derive addresses without being able to spend.
