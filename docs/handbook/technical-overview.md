# Technical design and trust boundaries

This section assumes familiarity with Bitcoin transactions, descriptors,
multisig and full-node validation. It examines the implementation choices
behind the merchant workflow. The [merchant guide](introduction.md)
starts without those prerequisites.

The reference chapters contain exact limits and conditions. The linked
rule documents describe individual acceptance and refusal rules, with IDs
used in the tests. Where a broader design description is less exact than
a rule, the implementation follows the rule document. None of these
internal checks constitutes an independent security audit.

## Start with the authority of each component

| Component | Authority | Boundary |
|---|---|---|
| Counter | Opens sales, uses approved receive addresses, produces signed records and applies acceptance policy | No wallet spending key or full descriptor. A compromised counter can still divert new sales or report false payment. |
| Owner phone | Holds wallet key 1 and an owner device identity; tracks coins, prepares spends and manages the queue | Cannot satisfy the wallet's two-signature requirement alone. Its sales and dollar claims are not independently known to the offline signer. |
| Key app | Holds its device identity and, in the default setup, wallet key 2; checks spends, batches and settings | No network view or independent sale records. Its signing library receives time and persistence services from the app. |
| Recovery key 3 | An independently generated third wallet key | Its independence is a setup condition. Generating it on key 2's device would undermine the intended separation. |
| Box | Relays sealed messages, reads POS orders and matches notices | Has an identity key, not a wallet key. It relays opaque ciphertext without access to the descriptor, payment addresses or transaction IDs. It can withhold messages and infer activity from orders. |
| Shop's full node | Additional payment evidence from a node named in signed settings | Does not replace the counter's light check. Authentication of the endpoint does not establish who operates it. |

The on-chain threshold is two of three keys, not two people. A person
controlling two usable wallet keys can spend, and the script cannot enforce
physical device separation. The intended claim about one compromised
component also depends on key 3 being generated independently
and the Key app's release key being held by a second independent person.
That second condition is unmet. Reproducible builds and an independent
rebuilder are also absent. The protocol threshold should not be confused
with an established single-component security result.

## Wallet keys, device identities and release keys

These are three different kinds of authority:

| Key class | What it signs | Where to inspect |
|---|---|---|
| Wallet keys 1, 2 and 3 | Bitcoin spending transactions under the wallet descriptor | [Wallet implementation](https://github.com/AvilaLabs/OpenBMT/blob/main/crates/openbmt-core/src/wallet.rs) and [signer rules](https://github.com/AvilaLabs/OpenBMT/blob/main/docs/SIGNER-RULES.md) |
| Device identity keys | Application records such as batches, settings, pairings and session envelopes | [Identity implementation](https://github.com/AvilaLabs/OpenBMT/blob/main/crates/openbmt-core/src/identity.rs) |
| App release keys | The software packages installed on devices | [Design and launch conditions](https://github.com/AvilaLabs/OpenBMT/blob/main/docs/DESIGN.md) |

Device identities use BIP340 tagged signatures and are not derived from
wallet keys. The owner phone holds both its wallet account secret and
its separate identity secret. The default Key app likewise has an
identity and wallet key 2. An approver-only Key app has its identity but
no wallet key. Counters and Boxes have identities without wallet keys.

An address batch carries owner-identity and Key-app-identity signatures.
A settings change is signed by the owner identity; a loosening change
also needs the Key app identity. The session request and response have
identity-signed envelopes. A spend inside that exchange separately needs
Bitcoin wallet signatures. Calling every signature "key 1" or "key 2"
would obscure these different checks.

See [session rules](https://github.com/AvilaLabs/OpenBMT/blob/main/docs/SESSION-RULES.md),
[batch implementation](https://github.com/AvilaLabs/OpenBMT/blob/main/crates/openbmt-core/src/batch.rs)
and [settings implementation](https://github.com/AvilaLabs/OpenBMT/blob/main/crates/openbmt-core/src/settings.rs).

## Descriptor choice and independent recovery

The wallet is native P2WSH `sortedmulti`, two of three. Account origins use
BIP48 script type `2h`, and the descriptor represents receive and change
with BIP389 `<0;1>/*` multipath notation. BIP380 provides the checksum.

The schematic below uses placeholders; it is not an importable wallet:

```text
wsh(sortedmulti(2,[fp1/48h/0h/0h/2h]xpub1/<0;1>/*,[fp2/48h/0h/0h/2h]xpub2/<0;1>/*,[fp3/48h/0h/0h/2h]xpub3/<0;1>/*))#checksum
```

The stored text has a pinned canonical form: full origins, `h` hardened
markers, slot order and checksum. `sortedmulti` sorts the derived public
keys for the script as described by BIP67, so script ordering and stored
slot ordering have different jobs. The parser requires distinct master
fingerprints and account xpubs and checks network, path and depth.

This choice makes the wallet describable outside OpenBMT. It does not
prove compatibility with every signer. The Core test-network restore
passed; Sparrow recovery and the actual hardware-signer matrix remain
unverified. The [wallet reference](wallet-reference.md) gives key 3 import
checks, recovery-kit fields and the tested import requirements.

Specifications: [BIP48](https://bips.dev/48/),
[BIP67](https://bips.dev/67/), [BIP380](https://bips.dev/380/) and
[BIP389](https://bips.dev/389/). The implementation's supported subset is
in `openbmt-core/src/wallet.rs`; support in external software must be
tested rather than inferred from a descriptor alone.

## The signer independently reconstructs what it can check

The signer accepts a bounded BIP174 PSBT version 0, not an arbitrary
transaction proposal or a wallet learned from that proposal. It requires
the full previous transaction for every input, derives input scripts from
the registered wallet, checks existing signatures and proves change by
derivation rather than output position.

Outputs are classified as derived change, active allowlisted destinations
or other destinations. A second change-like output counts as another
destination. Fees face both absolute and rate ceilings. Satoshi ceilings
apply per transaction, rolling day and rolling week, with separate tiers
for allowlisted and other destinations.

The signer counts authorized signatures, not chain confirmations. Its
rules handle repeated signing and mutually exclusive replacement spends
without treating a signed-but-unbroadcast transaction as harmless. New
cash-out destinations have a 24-hour activation wait. The clock is local;
an unlocked signer with a deliberately advanced clock can shorten that
wait.

The library has no network, storage or clock of its own. The caller must
persist changes before releasing a signature and must
supply the time. The
Key app's PIN, screenshot and accessibility controls are separate app
protections. The [wallet reference](wallet-reference.md#the-approval-session)
contains starter ceilings and limitations; the
[signer rules](https://github.com/AvilaLabs/OpenBMT/blob/main/docs/SIGNER-RULES.md)
and [BIP174](https://bips.dev/174/) provide the granular format checks.

## Address ownership and settings are separate protocols

Counters receive contiguous address ranges and pin the owner and Key app
identity roots. Both identities must sign a batch. The Key app checks the
wallet ID, counter membership and range, independently derives addresses,
and persists its receive mark before releasing the co-signature. A crash
may waste an address but must not allow reuse.

The counter persists its address use and exposure before showing their
results. An exhausted batch disables Bitcoin rather than reusing an
address. The minimum batch is 1,000 addresses, which exposes a useful
future range if a counter is stolen. These are availability and privacy
costs of operating without a descriptor on the counter.

Tightening takes effect with owner-identity authorization; loosening
requires the Key app identity as well. Counters check signed settings,
version floors and hard bounds. The first settings count as loosening.
The hardware-signer delay fallback is still planned.

Inspect [counter rules](https://github.com/AvilaLabs/OpenBMT/blob/main/docs/COUNTER-RULES.md),
[session rules](https://github.com/AvilaLabs/OpenBMT/blob/main/docs/SESSION-RULES.md)
and the [wallet reference](wallet-reference.md#address-batches).

## Header work and payment evidence have explicit limits

The counter maintains its own header chain, including in Box mode.
Mainnet starts at a compiled checkpoint at height 967,680. It checks
parentage, required difficulty, proof of work and median time, and selects
the best chain by accumulated work. This is a trust anchor and a bounded
header implementation, not validation from genesis.

Block inclusion and chain work do not establish that the counter executed
scripts or validated the UTXO set. Unconfirmed monitoring cannot prove
input values, input confirmation or fee rate. Peer diversity and conflict
monitoring reduce some risks; instant acceptance remains bounded risk
rather than transaction finality.

The [payment reference](payment-reference.md#header-chain-and-proof-assumptions)
details checkpoint, proof and storage bounds. The
[watcher rules](https://github.com/AvilaLabs/OpenBMT/blob/main/docs/WATCHER-RULES.md),
[network rules](https://github.com/AvilaLabs/OpenBMT/blob/main/docs/NETWORK-RULES.md)
and [policy implementation](https://github.com/AvilaLabs/OpenBMT/blob/main/crates/openbmt-core/src/policy.rs)
separate evidence collection from the decision to accept a sale.

## A relay is not a payment authority

The link encrypts content separately for each recipient with a fresh
message key and authenticates the sender's identity. Message numbering
and stored counters prevent replay under the stated persistence rules.
Padding reduces message-size detail. None of that makes relayed content
true: each recipient still checks the signed object and its policy.

The Box has order amounts and timing even when it lacks addresses and
transaction IDs. Tor hides the phone's address from peers, while BIP324
encrypts peer traffic without authenticating a peer. These solve different
problems. The own-node endpoint must currently be a Tor v3 address, and
its report must agree with the light check for the own-node label.

See the [network reference](network-reference.md),
[link protocol](https://github.com/AvilaLabs/OpenBMT/blob/main/docs/LINK-RULES.md)
and [Box rules](https://github.com/AvilaLabs/OpenBMT/blob/main/docs/BOX-RULES.md).

## The dependency and release boundary remains part of the review

The dependency policy lists what enters each shipped component. Some
transports and bindings are implemented in the repository to avoid adding
external packages. That reduces the package set while leaving more local
code to examine; package count alone establishes no security result.

The exact dependency lists, cryptographic libraries, JNI bindings and
transport tests are documented in
[Dependencies](https://github.com/AvilaLabs/OpenBMT/blob/main/docs/DEPENDENCIES.md).
Release independence, device behavior, hardware interoperability and an
outside audit remain open on the [status page](status.md).
