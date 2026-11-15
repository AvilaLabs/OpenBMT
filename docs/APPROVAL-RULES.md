# Approval rules

What a shop that uses its own wallet needs, besides the owner phone, to
change its settings. Without a Key app, the second signature is replaced by
one of two approvals, chosen by what the change can do. The code is
`crates/openbmt-core/src/approval.rs`. Tests are named after the rule
(`ap_01_...`). The classes and the counter's confirmation are in use: the
owner phone holds a Loosen change until a counter confirms it, and the
counters check it (`docs/COUNTER-RULES.md` CR-242). The wallet's signature of
AP-06 to AP-10 is asked for by the owner phone for a Redirect change and
checked by the counters (AP-14). Shops that keep the Key app keep its
signature for every loosening change, unchanged.

## 1. Classes of change

- **AP-01. Fields and classes.** The class of a change is computed from the
  old and the new settings, never declared. Redirect: `lightning_node` is set,
  or changed to another node. Loosen: every other field that loosens (limits,
  budgets, tolerance, confirmations lowered, heartbeat age raised, Tor turned
  off, customer Wi-Fi, minimum sale lowered, a device or counter added except
  as AP-12 says, a counter's share raised, the currency). Free: a Box added
  (with its place on the device list), the watch key set or changed, and any
  change that tightens, including taking the Lightning node away. The
  per-field direction is the one `settings::changed_fields` gives.
  `shop_lightning` takes the classes of AP-13.
- **AP-02. The highest class wins.** A change that touches several fields
  takes the highest class among them: Redirect over Loosen over Free. No
  change at all is Free.

- **AP-11. The shop's own node.** Setting `own_node` while `node_only` is
  on, changing it while `node_only` is on, turning `node_only` on and
  turning `light_fallback` on are Loosen: they decide which node every
  device trusts for its payments (NR-105). Taking `own_node` away, and
  turning `node_only` or `light_fallback` off, are Free. Setting or
  changing `own_node` while `node_only` is off is Free: the node is then a
  second check only (NR-98 to NR-103, WR-60 to WR-65).
- **AP-12. No counter yet.** When the settings in force name no counter, a
  change whose only loosening is counters added, with their shares, is Free:
  no counter can confirm it, and a confirmation has nothing yet to protect.
  Their entries on the device list, and those of a Box added with them, are
  Free with them. Any other loosening in the same change keeps its class. Once
  one counter is in force, every counter added is Loosen (AP-01), so that a
  counter added by the owner phone alone can never confirm the next change.
  For the same reason, a change that leaves no counter while the settings in
  force name at least one is Loosen: the last counter confirms its own
  removal, and the owner phone alone cannot empty the list and then fill it
  with counters of its own choosing. Removing some counters while one stays is
  Free, as any tightening is.

- **AP-13. Shop Lightning.** Setting `shop_lightning`
  (`docs/SHOP-LIGHTNING-RULES.md` SL-71), or changing its node's ID or its
  Lightning counter, is Redirect: the node decides where Lightning payments
  go, as `lightning_node` does. Changing the provider or the second device,
  and raising the cap or the most for one opening, are Loosen. Lowering
  either, and taking `shop_lightning` away, are Free. When `shop_lightning`
  is set, the "Lightning node" line of the wallet's message (AP-06) gives
  its node's ID.

- **AP-14. Carrying the signature.** A Redirect change is sent with the
  wallet's signature of the message of AP-06 for those settings, in the
  settings frame after the owner's signature: the signature's text in base64,
  at most 1,000 characters. The counter checks it against the first receive
  address of the wallet it pinned at pairing (AP-09) and refuses the settings
  when it does not verify (AP-10). The owner phone checks it the same way
  before it sends anything. A signature on a change of another class is
  refused, so that a frame has one meaning, and a shop that keeps the Key app
  does not use it.
- **AP-15. Not from the same phone.** A starter wallet's key is on the owner
  phone (`docs/START-SIMPLE-RULES.md`), so its signature would come from the
  phone that holds the owner root. The owner phone does not offer a Redirect
  change while the shop is on its starter wallet, and a counter paired with a
  starter wallet (CR-256) refuses one.

## 2. Counter confirmation

- **AP-03. Four words.** The four words that both screens show are BIP-39
  English words from the first 44 bits of
  `tagged_hash("OpenBMT/confirm-words/v1", settings_hash)`.
- **AP-04. The confirmation.** A counter signs
  `tagged_hash("OpenBMT/confirm/v1", settings_hash || counter_id)` with its
  identity key. A confirmation is the counter's id and that signature. It
  holds only for that settings hash and that counter. One counter's
  confirmation is enough for the whole shop.
- **AP-05. A counter in force.** The confirming counter must be a counter
  in the settings now in force, not in the new ones. A device that is not
  one is refused, whatever its signature.

## 3. Wallet message signature

- **AP-06. The message.** The wallet signs this text, with `\n` at the end
  of every line but the last, ASCII only:

  ```
  OpenBMT settings change
  Shop settings version {version}
  Lightning node {66 hex, or "none"}
  Settings {64 hex settings_hash}
  ```

- **AP-07. Compact signatures.** A 65-byte signature, in base64, with a
  header from 27 to 34 (Electrum and legacy) or from 39 to 42 (BIP-137
  P2WPKH), over the Bitcoin Signed Message hash of the text. The key it
  recovers, as a compressed key, must hash to the address. Any other
  header, including 35 to 38, is refused.
- **AP-08. BIP-322.** A BIP-322 "simple" signature, in base64: the witness
  of a P2WPKH spend, a signature with the sighash byte ALL and the
  compressed key, that signs the BIP-322 `to_sign` transaction for the text
  and the address. Trailing bytes, another count of items or another
  sighash type are refused.
- **AP-09. Only one address type.** The signing address must be a mainnet
  P2WPKH address (the first receive address of the single-key wallet).
  Every other address type and network is refused.
- **AP-10. A wrong one names the address.** A signature that is not
  accepted, or was not made by the key of the address, is refused with the
  address that had to sign in the refusal.
