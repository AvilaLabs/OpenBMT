# Existing-install authority adoption

Status: fail-closed storage seam and cryptographic core are implemented; the product flow is not wired and must not be represented as available.

## Decision

An existing OpenBMT installation may be adopted only with approval from the Owner identity already pinned by that counter's strictly verified pairing record. Physical possession of the register, a newly chosen manager PIN, a local recovery code, ledger possession, or a payment address is not enough. The Owner identity is separate from wallet spending keys, so this approval grants no ability to sign or redirect Bitcoin.

`openbmt-core::adoption` defines the approved protocol:

- a counter-signed, domain-separated request;
- the exact merchant/store/location/register/device scope;
- a fixed manifest for ledger, catalog, cash sessions, inventory, and the required pairing record, including presence, byte length, and SHA-256;
- a nonzero 32-byte nonce and diagnostic creation time; the integration must generate the nonce freshly and persist its one-time consumption;
- an Owner-root signature over the complete canonical request; and
- exact verification against the already trusted Owner root, counter identity, scope, manifest root, and pending nonce.

The `OBAR` request and `OBAG` grant codecs are strict and versioned. They do not expose a wallet, payment destination, transaction, or spend operation.

## Current safe storage seam

`CheckoutInstallationScopeStore.createAdoptionRequired` can record an authenticated descriptor in `EXISTING_INSTALL_ADOPTION / REQUIRED` without rewriting business or pairing files. Fresh enrollment and authority use remain blocked in that state. The descriptor marker prevents deletion from silently turning an established installation into a fresh one.

The current Kotlin permit/preflight interfaces are scaffolding only. They carry caller assertions, not verified Owner proof; they do not persist the grant digest, identities, nonce, or manifest root. Android does not invoke them. Existing installs remain unavailable until the native verifier and durable evidence below exist.

## Required wiring before adoption can ship

1. Read the five fixed files under an adoption lock, strictly validate `pairing.obks`, and derive the expected counter and Owner identities from the verified pairing—not display metadata.
2. Create and persist a fresh pending nonce and exact manifest snapshot, then have the existing counter identity sign the typed request.
3. In the Owner app, strictly decode the request, verify that the signer is an authorized counter in effective signed settings, show the exact scope and manifest fingerprint, and sign only after explicit approval.
4. Back on checkout, verify the grant against the pinned pairing identities, pending nonce, exact scope, and a freshly recomputed manifest. The native verifier—not a caller-provided lambda—must be the only path that can authorize the storage transition.
5. Persist authenticated adoption evidence: grant digest, manifest root, Owner root, counter identity, and consumed nonce. Recheck the manifest immediately before manager bootstrap and before completion.
6. Keep source bytes immutable and business/payment writes closed throughout the descriptor's monotonic `REQUIRED -> PENDING_MANAGER_ENROLLMENT` adoption transition and the separate provisioning transition to `COMPLETED`. A crash must resume idempotently or stay unavailable; it must never reset or replace legacy data.
7. Preserve all historical rows as legacy/unverified. Adoption never assigns the new manager to old activity.

If pairing is absent/corrupt or the pinned Owner identity is unavailable, automatic in-place adoption is not authorized. The supported outcome is a separate preservation/export-and-repair or explicit re-pair/recovery design—not a local-PIN bypass.

## Existing proof

The core protocol has focused tests for canonical round trips, every proper prefix and trailing byte, all single-bit mutations, wrong signers, cross-domain signatures, zero nonces, malformed/fixed manifests, required pairing, and changes to every exact binding. Kotlin storage tests cover file preservation, scope-bound one-way state changes, descriptor-loss refusal, and denial before approval. Neither set is evidence of an end-to-end Android adoption flow.
