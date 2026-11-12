# Swap rules: Lightning without a node

Status: specification for `crates/openbmt-swap`. Parts 1 to 6 and the numbers
were written before the code. Parts 7 to 10 (the live terms, following a
swap, the HTTP bytes, the pretend service) are built and tested against
fixtures and the pretend service. The counter drives them (CR-218 to CR-226
in `docs/COUNTER-RULES.md`) and an end-to-end test runs every path against a
private regtest node. No swap has been made with a real service.
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

The swap service is a setting (SW-82): a provider name, its pinned onion
name and a path prefix. It is off by default, and the library has no provider
built in: it names none and reaches none.

The documents the rules rest on: the Boltz API v2 (`docs/api-v2.md`,
`docs/lifecycle.md` and `docs/0-conf.md` of boltz-backend), BIP 341 and
BIP 342 (Taproot), BIP 327 (MuSig2, for the key an output is made from) and
BOLT 11 (the invoice). The fixtures of the tests are written from the shapes
the documents give, except the replies of `GET /v2/swap/reverse` and
`GET /v2/swap/submarine` of a live service, read on 2026-11-12 and kept in
`src/tests/data/` with a comment line giving the date and the path.

## 1. The request

- **SW-01.** The request for a reverse swap is a JSON object with exactly
  these members, in this order: `from` (`"BTC"`), `to` (`"BTC"`),
  `invoiceAmount` (an integer of satoshis), `preimageHash` (64 lower-case hex
  digits), `claimPublicKey` (66 lower-case hex digits, compressed) and
  `description` (a string). Nothing else is sent: no address, no account, no
  referral, nothing about the shop or the sale beyond these.
- **SW-02.** The invoice amount is at least `MIN_INVOICE_SATS` and at most
  `MAX_INVOICE_SATS`. Anything else is refused before a request is made.
  When the service's live terms are known (SW-60), the band is the shop's
  and the service's together: at least the larger minimum, at most the
  smaller maximum (SW-62).
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
  with the tree as BIP 341 says. The keys are taken in one order only, as the
  service documents it: the service's refund key first, then the shop's claim
  key, unsorted. A reply whose address matches only the other order is
  refused as a wrong lockup address. The lockup address of the reply is
  exactly the address of that output key on the shop's network, and is for no
  other network.
- **SW-27.** The timeout is a block height (below 500,000,000) at least
  `MIN_CLAIM_BLOCKS` above the height of the chain tip the caller gives, so
  that the claim has time to confirm. This floor is for the claim only: it
  is not the time a customer is made to wait.
- **SW-28.** The amount on chain is at least `MIN_ONCHAIN_SATS` and at most
  the invoice amount. With the quote the caller gives (the service's
  percentage in parts per million of the invoice amount, rounded up, and its
  lockup fee in satoshis), it is at least the invoice amount less those two.
  The service may promise more than the quote, never less. The quote is
  made from the service's live terms (`Quote::from_pairs`, SW-60, SW-63).

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
- **SW-46.** A claim that is not yet in a block can be built again at
  another fee rate, from the same lockup, to the same address, with the
  same rules as SW-40 to SW-44. A swap that was not claimed is refused. The
  new claim's transaction id replaces the one the state holds, so the state
  names the claim that was sent last. The caller sends it to take the place
  of the earlier one (it signals replacement) and keeps the lockup, which
  the machine does not hold after the claim.

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

## 7. The live terms

- **SW-60.** The reply of `GET /v2/swap/reverse` is read for the BTC to BTC
  pair only, within `MAX_HTTP_BYTES` and 8 levels. It gives the least and
  the most invoice amount, the percentage fee as parts per million, and the
  claim and lockup miner fees in satoshis. The least is not zero and not
  above the most. A reply with no BTC to BTC pair, or with a member of it
  missing or mistyped, is refused. Every other pair is ignored.
- **SW-61.** A percentage is turned into parts per million from the text of
  the number, with no floating point: `0.6` is 6000 and `0.25` is 2500. More
  than 4 decimal places, an exponent, a negative number and more than 10
  percent are refused.
- **SW-62.** An invoice amount outside the service's live limits is refused
  before a request is made, with a refusal that names the service's limits.
  The shop's band of SW-02 is refused as the shop's own.
- **SW-63.** Terms are fresh for at most `PAIRS_FRESH_SECS` after the time
  the caller says the reply was read. Older terms, and terms read at a later
  time than now, give no quote and no band.

## 8. Following a swap

- **SW-70.** The reply of `GET /v2/swap/{id}` is read for `status` and, at
  `transaction.mempool` and `transaction.confirmed`, for `transaction.hex`
  and `transaction.id` if they are given; an id that is not the id of the
  bytes is refused. The known states are `swap.created`,
  `transaction.mempool`, `transaction.confirmed`, `invoice.settled`,
  `invoice.expired`, `transaction.failed`, `swap.expired` and
  `transaction.refunded`; any other is reported as unknown, not guessed.
- **SW-71.** One swap is held from the request to the claim by a machine
  that takes the status, the lockup the caller found, the time and the
  height of the chain, and says what to do next: wait and ask again after a
  number of seconds, check the lockup, build the claim, watch the claim, or
  that the swap is paid or has failed, with the reason. Every change makes
  a new value. A failed swap shows no preimage: after a status that says the
  swap failed, or at the timeout, no claim is built. After the claim is
  sent a status changes nothing.
- **SW-72.** The machine does not believe the service about money. A lockup
  is believed only after SW-30 has checked the bytes of the transaction,
  and bytes in a status are always taken as not yet in a block. The swap is
  paid only when the caller reports that the shop's own claim, by its id,
  was seen in a block; another transaction is refused.
- **SW-73.** The machine is saved and restored in a file of one framed
  encoding: the magic `OBM'`, one version byte (1), the stage and its fields,
  and the swap's state file (SW-50) with its length. The decoder reads the
  exact length, refuses an unknown version or stage, a stage that does not
  agree with the phase of the state, and a lockup of less than the promised
  amount, and a file that decodes encodes back to the same bytes.
- **SW-74.** The decoder of the machine's file is fuzzed (`fuzz/fuzz_targets/`
  `swap_follow_decode.rs`): any bytes give a refusal or a machine that
  encodes back to the same bytes.
- **SW-75.** When the claim was built again (SW-46), the claim seen in a
  block may be the last one or any earlier one the caller built and kept:
  a peer may have mined the earlier one before the replacement reached it.
  The caller names those transaction ids. Any other transaction is refused,
  and the state then names the claim that was mined.

## 9. The bytes of the request and the reply

- **SW-80.** A request is written as HTTP/1.1 with `Host` (the onion name),
  `Connection: close` and, for a `POST`, `Content-Type: application/json`
  and `Content-Length`. No other header is sent: no user agent, nothing
  that names the shop. The path is the provider's path prefix and the call's
  path (`/v2/swap/reverse`, `/v2/swap/{id}`); a path with a character outside
  letters, digits and `/ _ . -`, with `..` or `//`, or one that is too long
  is refused.
- **SW-81.** A reply is read from its status line, its headers and its body,
  by `Content-Length` or by `Transfer-Encoding: chunked`, in at most
  `MAX_HTTP_BYTES`. It is refused when it is not HTTP/1.1, when a header is
  not understood, when both lengths or none are given, when a body is cut
  short or has bytes after it, when the content is encoded, and when the
  status is not 2xx to 5xx. A status other than 200 or 201 gives no body to
  read.
- **SW-82.** The provider is a setting of three texts: a name, the pinned
  Tor v3 onion name (56 characters and `.onion`, the checksum and version
  checked, as `openbmt-net` checks them) and the path prefix of its API
  (`/api` on an onion host, or empty). The setting is off by default, and
  the library has no provider in it. No request is written while it is off.
  The copy of the onion check in this crate gives the same answer as the one
  in `openbmt-net` on every name a test tries.

## 10. The pretend service

- **SW-90.** A test helper plays the service: it answers the pairs call,
  the create call and the status call, makes a real lockup to the address
  it promised (it holds the refund key and builds the tree with the code the
  client checks, and has a mode that builds a wrong tree), and broadcasts
  to a regtest Bitcoin Core. A run, ignored by default, goes from the pairs
  to a claim that Core accepts and mines, and to paid.
- **SW-91.** Against the pretend service the refusals hold: a wrong tree, a
  lockup that is too small, a timeout that is too near, stale pairs and an
  amount above the live maximum.

## 11. The numbers

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
| `PAIRS_FRESH_SECS` | 600 | SW-63 |
| `MAX_HTTP_BYTES` | 64 KiB | SW-60, SW-81 |
| `POLL_WAITING_SECS` | 10 | SW-71 |
| `POLL_LOCKED_SECS` | 30 | SW-71 |

**[to tune]**: the service's own minimum and maximum are read live (SW-60);
the numbers above are the shop's floors and caps, not the service's. The
service's timeout in blocks was not read from a swap, so `MIN_CLAIM_BLOCKS`
stays to tune, and so do the age of the terms and the two polling intervals.
