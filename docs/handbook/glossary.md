# Glossary

**Address batch.** A signed list of receive addresses for one counter, a
contiguous range, signed by key 1 and co-signed by the Key app.

**Approval session.** A session where the owner uses key 2 to approve queued
items. The owner phone shows them, and the Key app checks and signs each.
Nothing that moves money or loosens a limit happens outside one.

**Approver.** A Key app that holds an identity key only and no wallet key,
used when key 2 is a hardware wallet.

**BC-UR.** The format for sending data in animated QR codes, used by Sparrow
and hardware wallets.

**BIP321.** The standard for a `bitcoin:` payment request.

**Box.** The shop's own small server. Relays messages between the shop's
devices, reads the POS. Holds no key.

**Change.** The part of a spend that returns to the shop's own wallet.

**Compact block filter.** A small summary of a block that lets a light client
tell whether a block might matter to it.

**Confirmation.** A block that holds the payment. The shop sets two depths:
the confirmation depth (1 block at first) and the refund depth (6 at first),
after which a refund can be paid out. The counter shows "Paid" at both, with
words saying which.

**Counter.** The shop's phone or tablet that shows the sale. Holds no key.

**Descriptor (wallet description).** The text that says how the wallet's
addresses are made from its three public keys.

**Exposure.** The total of instant sales not yet in a block.

**Full node.** A computer that checks every block itself. The shop's own
node is optional.

**Heartbeat.** A small signed message from the owner phone that tells the
counter the owner's watcher is alive.

**Instant.** Handing over goods before the first block, within the shop's
limit.

**Key app.** The separate app that holds key 2 and is the second authority.

**Light check.** Asking Bitcoin peers directly, with compact filters, instead of
running a node.

**Loosening / tightening.** A change that allows more risk or less.

**Multisig, two of three.** A wallet that needs two of its three keys to spend.

**Orbot.** An Android app that provides Tor for other apps.

**PSBT.** The standard file for a transaction that is passed between signers.

**Recovery kit.** A printed page of instructions for restoring the wallet. It
contains no secrets.

**Release key.** The key that signs an app for download. It is not a wallet
key.

**Reorganization (reorg).** When blocks are replaced by a different set.

**Root.** The identity key of the owner phone or the Key app that a counter
pins at pairing.

**Satoshi (sat).** One hundred-millionth of a bitcoin.

**Tor.** A network that hides where a connection comes from.

**xpub.** A public key that can derive addresses without being able to spend.
