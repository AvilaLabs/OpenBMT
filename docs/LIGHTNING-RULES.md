# Lightning rules: the invoice and the code of a Lightning payment

Status: specification for `crates/openbmt-lightning`, with the one change to
`openbmt-core` that it needs (LN-03). Each rule has an ID so that a test can
name the rule it proves. The rules implement `docs/DESIGN.md` section 8: the
counter shows a BIP 321 code with an on-chain address and a Lightning
invoice, and a Lightning payment is final at once. The node that makes the
invoices is Shop Lightning (`docs/SHOP-LIGHTNING-RULES.md`).

This library has no network, no clock, no storage and no TLS. It checks the
invoice and builds the code. An invoice that cannot be read is a refusal,
never a guess.

## 1. Where the money goes

- **LN-03.** The node decides where Lightning payments go, so the public key
  of the node (33 bytes, compressed) is part of the shop's signed settings
  (`Settings::lightning_node`, `None` when the shop has no node) and not of
  a counter's own configuration. It is part of the signed bytes, at the end:
  a flag byte that is 0 or 1 and, when it is 1, the 33 bytes. `validate`
  refuses a key that is not a point of the curve. Naming a node, or naming
  another one, is loosening and needs the Key app's co-signature, as a
  changed destination does; taking the node away is tightening. The Key app
  shows "Lightning payments go to node" and the key in full, as the line of
  the field, because a short form of a key can be made to match by grinding.
  Every invoice must be signed by this key (LN-17).

## 2. The invoice

- **LN-13.** An invoice is asked for with the sale's amount in
  millisatoshis (the sale's satoshis times 1,000; 1 to all the bitcoin there
  is), a lifetime in seconds (1 to 86,400) and a description. The lifetime is
  the lifetime of the counter's quote, `openbmt_counter::money::QUOTE_SECS`,
  300 seconds (CR-05). That constant is in the counter crate, which this one
  does not depend on, so the lifetime is a parameter that the counter passes.
  The description is `Sale <number>` and nothing else: the payer's wallet
  shows it, so it holds no shop name and no item.
- **LN-16.** The invoice is read by this crate. It is bech32 with a good
  checksum in one case, at most 1,023 characters (the length the checksum
  covers), starting `ln`, then the network (`bc`, `tb`, `tbs` or `bcrt`) and
  an amount written as BOLT 11 writes it (digits with no leading zero, one
  of `m u n p`, `p` only for whole tens of picobitcoin). After the 7 groups
  of the time come the fields, then the 104 groups of the signature. The
  payment hash, the description (as text), the expiry and the node key are
  read, each at most once, and wrong in length means refused (where BOLT 11
  would skip some); other fields are skipped. The signature is a 64-byte
  signature and a recovery number over the SHA-256 of the prefix and the
  data before the signature, and gives the public key of the signer.
  Format: https://github.com/lightning/bolts/blob/master/11-payment-encoding.md.
- **LN-17.** An invoice is shown only if it passes, in this order: the network
  is the shop's; the signer is the node key of the settings (LN-03), and so
  is the `n` field if the invoice has one; the amount is exactly the amount
  asked; the payment hash is the one expected; the expiry is the lifetime
  asked (3,600 seconds when the invoice has no expiry field, as BOLT 11
  says); the description is the sale's text and not a hash of one. Any
  mismatch refuses the invoice. The signature check is the part that proves
  the invoice belongs to the shop's node.

## 3. The code

- **LN-18.** The code is `bitcoin:<ADDRESS>?amount=<BTC>&lightning=<BOLT11>`
  (BIP 321, https://github.com/bitcoin/bips/blob/master/bip-0321.mediawiki).
  The address is the sale's address, of the shop's network. The amount is the
  decimal of BTC with a period, no trailing zeros (`0.00001`, `20.3`, `1`),
  and is the amount of the invoice: the invoice must name exactly that many
  satoshis. The BIP does not say that a wallet must compare the two, so they
  are made the same here.
- **LN-19.** The code is written in capitals for a QR code only when the
  address is bech32 (a version 0 or later witness address), because bech32
  and BOLT 11 are case-insensitive when written in one case and a base58
  address is not. For any other address the capitals are refused, and the
  QR code is made from the text as it is.

## 4. Payment

- **LN-21.** A Lightning payment is final at once (DESIGN section 8): there
  is no waiting for a block and no refund by the chain. The counter keeps
  the facts of a payment that is settled: the payment hash, the amount
  received in millisatoshis and, when it is known, the time it settled in
  seconds since 1970.
- **LN-23.** The crate has no unsafe code and no `unwrap`, `expect`, `panic`
  or indexing that can panic outside tests. It depends on workspace crates by
  path (`openbmt-core`) and on `bitcoin` 0.32.8, which is already in every
  list of `deps/`, with its `secp-recovery` feature: the same `secp256k1`
  package, with the module that gives the signer of an invoice.
  `scripts/deps-check.sh` passes with the lists unchanged.

## 5. Sources

```
BIP 321:
  https://github.com/bitcoin/bips/blob/master/bip-0321.mediawiki
BOLT 11:
  https://github.com/lightning/bolts/blob/master/11-payment-encoding.md
```
