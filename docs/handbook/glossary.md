# Glossary

**Address batch.** A contiguous range of receive addresses assigned to one
counter, signed by the owner and Key app device identities. These are
separate from their Bitcoin spending keys.

**Approval session.** The owner reviews queued items with key 2 available.
The owner phone presents them; the Key app checks and signs the approved
items. Money movements and loosening changes require a session.

**Approver.** A Key app holding an identity key but no wallet key, used
when key 2 is a hardware wallet.

**BC-UR.** A format used by Sparrow and hardware wallets to transfer data
through animated QR codes.

**BIP321.** The standard for a `bitcoin:` payment request.

**Bitcoin (BTC).** The currency accepted by OpenBMT. One bitcoin is
100,000,000 satoshis.

**Blockchain.** The shared transaction record, extended in groups called
blocks. A payment included in a block has its first confirmation.

**Box.** The shop's optional server for relaying device messages and reading
POS orders. It holds no wallet key.

**Cash-out.** Sending bitcoin to an exchange the owner chooses, followed
by conversion and withdrawal under that exchange's terms. OpenBMT does
not perform the sale or bank withdrawal.

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

**Exchange rate.** The quoted relationship between bitcoin and dollars.
A checkout quote calculates what to send; it does not convert the receipt.

**Exposure.** Instant sales still waiting for their first block.

**Full node.** A computer that checks every block itself. Running one is
optional for the shop.

**Heartbeat.** A signed message from the owner phone telling the counter
that the owner's watcher is running.

**Identity key.** A device key signing OpenBMT messages and approvals.
It is separate from the wallet keys that authorize Bitcoin spends.

**Instant.** Acceptance before the first block, within the shop's limit.

**Key app.** The separate app holding key 2 and providing the second
approval.

**Light check.** A check through Bitcoin peers and compact block filters
without running a full node.

**Lightning invoice.** A request payable over Lightning, supplied by the
shop's own Lightning node in OpenBMT.

**Loosening / tightening.** A settings change that permits more risk, or
one that reduces it.

**Multisig, two of three.** A wallet requiring any two of its three keys
to spend.

**On-chain.** A payment recorded through the Bitcoin blockchain, as distinct
from an individual payment made through Lightning channels.

**Orbot.** A separate Android app providing Tor connections to other apps.

**Private key.** Secret information used to authorize spending. OpenBMT's
wallet needs two of its three wallet keys to spend.

**PSBT.** A standard transaction file passed between signers.

**Recovery kit.** A printed page of wallet recovery information, without
secret keys or seed words.

**Recovery words (seed words).** A secret word sequence that can recreate
a key. It is not a payment address or the public wallet description.

**Release key.** A key signing an app for distribution, separate from the
wallet's keys.

**Reorganization (reorg).** Replacement of blocks by a different set of
blocks.

**Root.** An owner-phone or Key-app identity key pinned by a counter at
pairing.

**Satoshi (sat).** One hundred-millionth of a bitcoin.

**Tor.** A network that hides where a connection originates.

**Unconfirmed.** Seen as a transaction but not yet included in a block.
It can still be replaced and is subject to the shop's instant-acceptance
limits.

**Watch-only.** A wallet view using public information to display payments
without holding keys that can spend them.

**xpub.** A public key used to derive addresses without being able to spend.
