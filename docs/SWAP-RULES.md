# Swap rules: Lightning without a node

Status: specification for `crates/openbmt-swap`, written before the code.
Each rule has an ID so that a test can name the rule it proves.

A shop can take Lightning without running a node. The customer pays a
Lightning invoice that a swap service made (Boltz, its API v2, reached over
its Tor address). The service locks its own bitcoin on chain in a Taproot
output that the shop can spend with a preimage and a claim key. The shop's
claim transaction pays straight to a fresh receive address of the shop's own
2-of-3 wallet. The service never holds the shop's money: if the shop does not
claim in time, the service takes its lockup back and the customer's payment
is cancelled. The shop's own LND or CLN connection (`LIGHTNING-RULES.md`) is
not changed by this.

The library has no network, no clock, no storage, no randomness and no
screen. The caller passes the service's replies, the time, the height of the
chain and 32 random bytes where a key or a preimage is made. Everything that
reads an untrusted reply and decides is here. This part has no words for a
person; those come with the screens.

The documents the rules rest on: the Boltz API v2 (`docs/api-v2.md`,
`docs/lifecycle.md` and `docs/0-conf.md` of boltz-backend), BIP 341 and
BIP 342 (Taproot), BIP 327 (MuSig2, for the key an output is made from) and
BOLT 11 (the invoice). The fixtures of the tests are written from the shapes
the documents give; none is captured from a service.

## 1. The request

- **SW-01.** The request for a reverse swap is a JSON object with exactly
  these members, in this order: `from` (`"BTC"`), `to` (`"BTC"`),
  `invoiceAmount` (an integer of satoshis), `preimageHash` (64 lower-case hex
  digits), `claimPublicKey` (66 lower-case hex digits, compressed) and
  `description` (a string). Nothing else is sent: no address, no account, no
  referral, nothing about the shop or the sale beyond these.
- **SW-02.** The invoice amount is at least `MIN_INVOICE_SATS` and at most
  `MAX_INVOICE_SATS`. Anything else is refused before a request is made.
- **SW-03.** The description is 1 to `MAX_DESCRIPTION_BYTES` bytes of UTF-8
  with no control character. It is written with `"` and `\` escaped and
  every other character as it is.
- **SW-04.** The secrets of a swap are made from 32 bytes for the preimage
  and 32 bytes for the claim key, both given by the caller from its random
  source. The claim key is never derived from a wallet key. A preimage of
  zeros, or bytes that are not a valid secret key (zero, or not below the
  order of the curve), are refused. The preimage hash is the SHA-256 of the
  preimage. The secrets do not print: their `Debug` shows no byte.

## 2. Reading the reply

- **SW-10.** At most `MAX_REPLY_BYTES` of a reply are read, nested at most
  `MAX_REPLY_DEPTH` deep, by the JSON reader of `openbmt-json` (UTF-8 only,
  no key twice, no bytes after the document).
- **SW-11.** The reply must have, with these types: `id` (a string),
  `invoice` (a string), `lockupAddress` (a string), `refundPublicKey` (66
  hex digits), `timeoutBlockHeight` (an integer that fits 32 bits),
  `onchainAmount` (an integer, not negative) and `swapTree` with `claimLeaf`
  and `refundLeaf`, each with `version` (an integer) and `output` (hex). A
  missing or mistyped member is refused. Members the library does not know
  are ignored, because the service may add some; nothing it does not read can
  matter.
- **SW-12.** The id is 1 to 64 characters of letters, digits, `-` and `_`.
- **SW-13.** Both leaves are of version 192 (`0xc0`, tapscript) and their
  scripts are at most `MAX_LEAF_BYTES` long.

## 3. The check of the reply

The reply is refused unless every rule of this part holds. The order is the
order of the rules, so that the refusal names the first that fails.

- **SW-20.** The invoice reads as BOLT 11 (`openbmt-lightning`, LN-16) and
  its payment hash is exactly the SHA-256 of the preimage the shop made. The
  signer of the invoice is not checked: the shop does not know the service's
  node.
- **SW-21.** The invoice names an amount, and it is exactly the invoice
  amount asked for, in millisatoshis.
- **SW-22.** The invoice is for the shop's network.
- **SW-23.** The invoice has not run out, and has at least
  `MIN_INVOICE_LIFE_SECS` seconds left at the time the caller gives.
- **SW-24.** The refund public key is a valid compressed key and is not the
  claim key.
- **SW-25.** The swap tree is rebuilt from the shop's claim key, the refund
  key of the reply, the preimage hash and the timeout of the reply. The two
  leaves of the reply are byte for byte the leaves that are rebuilt. The
  claim leaf is
  `OP_SIZE 32 OP_EQUALVERIFY OP_HASH160 <RIPEMD160(hash)> OP_EQUALVERIFY
  <claim key, x only> OP_CHECKSIG`, which only the preimage and a signature
  of the claim key satisfy. The refund leaf is
  `<refund key, x only> OP_CHECKSIGVERIFY <timeout> OP_CHECKLOCKTIMEVERIFY`.
- **SW-26.** The output key is the key of BIP 327 for the two keys, tweaked
  with the tree as BIP 341 says. The keys are taken in either order, because
  the order the service uses is not settled by its documents; the order that
  matched is recorded. The lockup address of the reply is exactly the address
  of that output key on the shop's network, and is for no other network.
- **SW-27.** The timeout is a block height (below 500,000,000) at least
  `MIN_CLAIM_BLOCKS` above the height of the chain tip the caller gives, so
  that the claim has time to confirm. This floor is for the claim only: it
  is not the time a customer is made to wait.
- **SW-28.** The amount on chain is at least `MIN_ONCHAIN_SATS` and at most
  the invoice amount. With the quote the caller gives (the service's
  percentage in parts per million of the invoice amount, rounded up, and its
  lockup fee in satoshis), it is at least the invoice amount less those two.
  The service may promise more than the quote, never less.

## 4. The lockup

- **SW-30.** A transaction the caller found is a lockup only if exactly one
  of its outputs pays the lockup address and that output holds at least the
  amount the service promised. Two outputs to the address, none, or too
  little are refused.
- **SW-31.** The result says whether the transaction is confirmed (the caller
  says where it was found, in the mempool or in a block) and whether it
  signals replacement. An unconfirmed lockup is a lockup the service can
  still replace; what the shop does with that is the caller's rule.

## 5. The claim

- **SW-40.** The claim transaction has one input, the lockup output, and one
  output, to the receive address the caller gives, which must be for the
  shop's network. Version 2, no lock time, the input signals replacement.
- **SW-41.** The fee is the rate the caller gives (satoshis per 1000 virtual
  bytes) times the virtual size of the finished transaction, rounded up. The
  claim is refused if the fee is more than half of the lockup's value, or if
  the output would be below the dust limit of its script.
- **SW-42.** The claim spends by the claim leaf: the witness is the signature
  of the claim key, the preimage, the leaf and the control block. The
  signature commits to the one input and the one output (sighash default).
- **SW-43.** Before the transaction is returned it is checked against the
  output script it spends, by the script interpreter of `miniscript`: the
  control block commits to the output key, the preimage satisfies the hash
  and the signature verifies. A transaction that does not pass is not
  returned.
- **SW-44.** No claim is built at or after the timeout: when the chain tip
  the caller gives is at or above it, the service may already have taken its
  lockup back.
- **SW-45.** The claim is by the script path only. The cooperative key-path
  signature of MuSig2 is not built here: the packages the project allows
  have no MuSig2 signing code, and the one part of BIP 327 that is needed to
  recognise the output (the key aggregation) is written in `keys.rs` and
  checked against the vectors of the BIP. Nothing in a claim depends on the
  service's help; the cost is a larger transaction that shows the swap's
  script on chain.

## 6. The state file

- **SW-50.** A swap's state is kept in a file of one framed encoding: the
  magic `OBM%`, one version byte (1), then the fields in a fixed order with
  fixed widths and a length byte or two before each text. It holds what a
  claim needs after a restart: the network, the secrets, the refund key, the
  order of the keys, the timeout, the amounts, the invoice, the receive
  address of the claim and the phase.
- **SW-51.** A decoder reads the exact length: a short file, a long file, an
  unknown version, an unknown network, order or phase, and a text that is
  too long are refused, and a file that decodes always encodes back to the
  same bytes.
- **SW-52.** A file is refused when what it holds does not agree with
  itself: a key that is not valid, an invoice that does not read or whose
  hash is not the preimage's, a receive address of another network, an amount
  out of the band of SW-28, a timeout that is not a height, an id that
  breaks SW-12.
- **SW-53.** The phase goes forward only: waiting, then claimed with the
  identifier of the claim transaction. A claimed swap is not claimed again,
  and a change of phase makes a new value; the old one is not touched.
- **SW-54.** The state's `Debug` shows no secret.
- **SW-55.** The decoder of the state file is fuzzed (`fuzz/fuzz_targets/`
  `swap_state_decode.rs`): any bytes give a refusal or a state that encodes
  back to the same bytes.

## 7. The numbers

| name | value | rule |
|---|---|---|
| `MIN_INVOICE_SATS` | 10,000 | SW-02 |
| `MAX_INVOICE_SATS` | 25,000,000 | SW-02 |
| `MAX_DESCRIPTION_BYTES` | 100 | SW-03 |
| `MAX_REPLY_BYTES` | 16 KiB | SW-10 |
| `MAX_REPLY_DEPTH` | 6 | SW-10 |
| `MAX_LEAF_BYTES` | 128 | SW-13 |
| `MIN_INVOICE_LIFE_SECS` | 300 | SW-23 |
| `MIN_CLAIM_BLOCKS` | 36 | SW-27 |
| `MIN_ONCHAIN_SATS` | 1,000 | SW-28 |

**[to tune]**: the service's own minimum and maximum, and its timeout in
blocks, were not read from a live service; the numbers above are the shop's
floors and caps, not the service's.
