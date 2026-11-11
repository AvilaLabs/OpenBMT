# Lightning rules: the counter's invoices from the merchant's own node

Status: specification for `crates/openbmt-lightning`, with the one change to
`openbmt-core` that it needs (LN-03). Each rule has an ID so that a test can
name the rule it proves. The rules implement `docs/DESIGN.md` section 8,
"Option: connect your own Lightning node": the counter shows a BIP 321 code
with an on-chain address and a Lightning invoice made by the node the
merchant already runs, and a Lightning payment is final at once.

This library has no network, no clock, no storage and no TLS. It builds the
text of each request, reads each reply strictly, checks the invoice and
builds the code. The app sends every request over HTTPS to the node, with
the pinned certificate (LN-12), and hands the status and the bytes of the
answer back. A reply that cannot be read is a refusal, never a guess.

**What the facts rest on.** The fixtures of the tests are written from the
projects' own documents, not captured from a running node, and no node was
run. Every point that needs a real node to confirm is marked
**[verify, E7]**. The documents are cited where each fact is used; the pages
of Core Lightning (docs.corelightning.org) do not show the version they
describe.

## 1. What is supported

- **LN-01.** Two kinds of node are supported: LND, over its REST interface,
  and Core Lightning, over clnrest. A credential is one of the two
  (`Credential::Lnd`, `Credential::Cln`). The crate builds the same calls
  for both: make an invoice, ask whether it is paid, and the tests of a
  connection (LN-20).
- **LN-02.** Eclair is not supported, and the setup says so plainly: Eclair
  has one password with the full power of its API, which includes paying and
  closing channels, and no restricted credential; its API has no TLS setting
  in `reference.conf`. A counter that held the password would hold a key
  that can spend (the Eclair documentation and its `reference.conf`, under
  Sources).
  A merchant who runs Eclair puts a filtering proxy of their own in front
  of it, or does not connect it.

## 2. Where the money goes, and what the counter holds

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
- **LN-04.** The counter holds one credential for the node, which is not
  part of the settings and is never sent to another device. It is limited to
  invoices: LND `invoices:read` and `invoices:write`, Core Lightning the
  methods `invoice` and `listinvoices`. The node software enforces that
  limit, not this crate. Core Lightning has shipped security releases whose
  notes list fixes in "runes and commando": 26.06.7 (2026-08-28), 26.06.8
  (2026-09-22), 26.06.9 (2026-10-07) and 26.06.10 (2026-11-04)
  (https://github.com/ElementsProject/lightning/releases). So the node must
  run a current release: Core Lightning 26.06.10 or later, and LND's current
  release, which was v0.21.6-beta on 2026-10-30
  (https://github.com/lightningnetwork/lnd/releases). The setup words say so,
  and say that the counter holds a key to the node that is limited to
  invoices. Reading a credential is never enough to trust it, so every setup
  probes it (LN-06, LN-09).

## 3. LND

- **LN-05.** The credential for LND is a macaroon baked with the permissions
  `invoices:read` and `invoices:write` only. The words give the command:
  `lncli bakemacaroon invoices:read invoices:write`. The stock
  `invoice.macaroon` is refused, because it also has `address` and
  `onchain:read` (the LND macaroons page, under Sources): it is caught by
  the probe of LN-06, since the balance of the wallet can be read with it.
  The macaroon's own bytes do not say what it may do in a form
  LND documents for clients, so only their shape is checked: 1 to 2,048
  bytes, the first byte 2 (the binary form of version 2 of macaroons).
- **LN-06.** At connect time two read-only calls that a macaroon limited to
  invoices must be refused are sent: `GET /v1/getinfo` (needs `info:read`) and
  `GET /v1/balance/blockchain` (needs `onchain:read`). Each must be answered
  with a failure whose message begins `permission denied`, which is what LND
  answers a call the macaroon does not allow (`lncli` shows `rpc error: code =
  Unknown desc = permission denied`), whatever HTTP status the gateway puts on
  it **[verify, E7]**. A success means the macaroon can do more than invoices
  and the setup is refused with the command of LN-05. Any other failure
  proves nothing, and the setup is refused too.
- **LN-07.** Requests to LND carry the macaroon in hex in the header
  `Grpc-Metadata-macaroon`. An invoice is made with `POST /v1/invoices` and
  the body `{"memo":..,"value_msat":"..","expiry":".."}` (64-bit integers are
  texts in REST); the reply gives `r_hash` (base64) and `payment_request`
  (https://lightning.engineering/api-docs/api/lnd/lightning/add-invoice/).
  Whether it is paid is asked with `GET /v1/invoice/{r_hash in hex}`, the
  first version of the lookup: the second, `/v2/invoices/lookup`, needs
  `invoices:write` in LND's source (invoices_server.go, v0.21.6-beta), so it
  would make the macaroon wider than it needs to be (the LND REST swagger
  file, under Sources).
  Streams are not used: the payment is polled.

## 4. Core Lightning

- **LN-08.** The credential for Core Lightning is a rune, read in the clear
  by the counter. A rune is the base64url of a 32-byte hash and then its
  restrictions as text, joined by `&`; a restriction is alternatives joined
  by `|`; an alternative is a field name, one operator of `= / ^ $ ~ < > { }
  # !` and a value in which `\` escapes `\`, `|` and `&`
  (https://docs.corelightning.org/reference/createrune,
  https://github.com/rustyrussell/runes). A rune with no restriction,
  with a restriction that is not in this grammar (an unknown operator, an
  unknown escape, no operator) or whose restrictions are not text is
  refused. Of the restrictions, those that name the field `method` are
  the ones that count: each must be made only of `method=` alternatives, each
  naming `invoice` or `listinvoices`; one that names another method, names
  the method with another operator or names it together with another field
  is refused whole, because `|` makes the rune as wide as its widest
  alternative. What the `method` restrictions allow together must be exactly
  `invoice` and `listinvoices`. Other restrictions (time, rate, id, the
  unique id of the rune) only narrow a rune and are accepted when they are
  well formed **[verify, E7]** (the form of the unique id is not described on
  the pages read).
- **LN-09.** At connect time two read-only calls that a rune limited to
  invoices must be refused are sent: `getinfo` and `listfunds`. Each must be
  answered with a failure that has the status 401 or 403, or whose message
  begins `Not permitted` or `Not authorized` **[verify, E7]**. A success
  refuses the setup with words; any other failure proves nothing and refuses
  it too. Nothing the crate calls moves money.
- **LN-10.** Requests to Core Lightning carry the rune in the header `Rune`
  and are `POST https://host:port/v1/<method>` with a JSON body
  (https://docs.corelightning.org/docs/rest). An invoice is made with
  `invoice` (`amount_msat`, `label`, `description`, `expiry`); whether it is
  paid is asked with `listinvoices` by `payment_hash`
  (https://docs.corelightning.org/reference/invoice,
  https://docs.corelightning.org/reference/listinvoices). `waitinvoice` is
  not used: it blocks until the invoice is paid or ends, which over Tor or
  through a proxy needs timeouts of minutes. An error of the node is a
  refusal that carries its status and its own text, cleaned and cut short,
  for the details only.

## 5. The connection

- **LN-11.** The connect string of LND is the `lndconnect://` address that
  Umbrel and Start9 show: `lndconnect://host:port?cert=<base64url of the DER
  certificate>&macaroon=<base64url of the macaroon>`. Both parameters are
  needed, no other and none twice. For Core Lightning the setup takes
  `host:port`, the rune and the certificate of clnrest, one by one: the
  crate defines no connect string of its own, because Core Lightning
  documents none. A host is a name (letters, digits, hyphens, dots, at most
  253 characters, each part of 1 to 63) or an address in square brackets,
  and the port is 1 to 65535.
- **LN-12.** The certificate of the node is pinned: the DER bytes or their
  SHA-256 are given at setup (a PEM file, as the node writes it, is read
  into the DER bytes; a fingerprint may be written in pairs with colons). The
  crate does no TLS. The app connects only to a node that shows a certificate
  for which `CertPin::accepts` is true, and checks nothing else about the
  certificate: a pin replaces the certificate authorities, and a node's own
  certificate is not signed by one. Plain HTTP is never used, and the crate
  has no way to ask for it. A connect string with no certificate is refused,
  because nothing could be pinned.

## 6. The invoice

- **LN-13.** An invoice is asked for with the sale's amount in
  millisatoshis (the sale's satoshis times 1,000; 1 to all the bitcoin there
  is), a lifetime in seconds (1 to 86,400) and a description. The lifetime is
  the lifetime of the counter's quote, `openbmt_counter::money::QUOTE_SECS`,
  300 seconds (CR-05). That constant is in the counter crate, which this one
  does not depend on, so the lifetime is a parameter that the counter passes.
  The description is `Sale <number>` and nothing else: the payer's wallet
  shows it, so it holds no shop name and no item.
- **LN-14.** A payment is paid only when the node says so and the amount it
  received is at least the amount asked: LND `state` is `SETTLED` and
  `amt_paid_msat` is at least the amount; Core Lightning `status` is `paid`
  and `amount_received_msat` is at least the amount. `OPEN`, `unpaid`,
  `expired`, `CANCELED` and `ACCEPTED` (LND holds a payment it has not
  settled) are not paid. A paid reply for less is "too little", a reply
  for no such invoice is "not found", and any other or unknown state is not
  paid, with the node's word for it quoted. A reply about another payment
  hash than the one asked is refused. A reply that cannot be read is not a
  payment either. The replies are read with the bounds of `openbmt-json`
  (256 KiB, 16 levels, 10,000 values); a number may be a JSON number or, as
  LND writes 64-bit integers, a text of digits **[verify, E7]**.
- **LN-15.** The label of an invoice at Core Lightning is made, not drawn:
  `obmt-<first four bytes of the counter's id in hex>-<sale number>-<attempt>`.
  If a request has no answer (a timeout) the app sends it again, and the
  node answers error 900 (the label is used) when the first one got through.
  Then the invoice is asked for by label, and it is taken up again only if
  the label, the amount, the description and the state `unpaid` are this
  sale's; its BOLT 11 text then passes LN-17 like any other. If they are not,
  the counter makes the next attempt, which has a new label. LND has no
  labels, so a repeated request makes a second invoice, and only the one
  shown is waited for. An invoice taken up again keeps the time it was made,
  so the app takes it up only while the quote it belongs to lasts.
- **LN-16.** The invoice the node returns is read by this crate. It is
  bech32 with a good checksum in one case, at most 1,023 characters (the
  length the checksum covers), starting `ln`, then the network (`bc`, `tb`,
  `tbs` or `bcrt`) and an amount written as BOLT 11 writes it (digits with no
  leading zero, one of `m u n p`, `p` only for whole tens of picobitcoin).
  After the 7 groups of the time come the fields, then the 104 groups of the
  signature. The payment hash, the description (as text), the expiry and the
  node key are read, each at most once, and wrong in length means refused
  (where BOLT 11 would skip some); other fields are skipped. The signature
  is a 64-byte signature and a recovery number over the SHA-256 of the prefix
  and the data before the signature, and gives the public key of the signer.
  Format: https://github.com/lightning/bolts/blob/master/11-payment-encoding.md.
- **LN-17.** An invoice is shown only if it passes, in this order: the network
  is the shop's; the signer is the node key of the settings (LN-03), and so
  is the `n` field if the invoice has one; the amount is exactly the amount
  asked; the payment hash is the one the node reported; the expiry is
  the lifetime asked (3,600 seconds when the invoice has no expiry field,
  as BOLT 11 says); the description is the sale's text and not a hash of one.
  Any mismatch refuses the invoice with words. The signature check is the
  part that proves the invoice belongs to the shop's node.

## 7. The code

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

## 8. Setup, payment and what a person is told

- **LN-20.** The test of a connection is a chain of three requests: the first
  probe, the second probe and a test invoice of 1 satoshi, one after the
  other; each reply decides whether the next request is made. The test
  invoice must be made, must pass LN-17 with the node key of the settings, and
  is never paid; it runs out in 300 seconds. A failure says why and what to
  do next: the key can do more than invoices (and the exact command or kind
  of rune to make), the probe proved nothing (update the node), no invoice,
  the label of the test is used, the invoice is signed by another node than
  the settings name, or the invoice fails a check.
- **LN-21.** A Lightning payment is final at once (DESIGN section 8): there
  is no waiting for a block and no refund by the chain. The crate only
  says paid or not paid, with the facts: the payment hash, the amount received
  in millisatoshis and, when the node says, the time it settled in seconds
  since 1970. The counter keeps them with the sale; the fields it needs are
  listed in the report of the change that adds this crate.
- **LN-22.** Every state and refusal has words under `docs/SCREEN-RULES.md`
  (SC-01 to SC-12): at a sale for the cashier, who is told to take another
  kind of payment and what to tell the owner; at setup for the owner. The rule
  number and the node's own text are in the details only. The list is
  `say::all_words`, and `docs/screens/lightning.md` is made from it.
- **LN-23.** The crate has no unsafe code and no `unwrap`, `expect`, `panic`
  or indexing that can panic outside tests. It depends on workspace crates by
  path (`openbmt-core`, `openbmt-json`) and on `bitcoin` 0.32.8, which is
  already in every list of `deps/`, with its `secp-recovery` feature: the same
  `secp256k1` package, with the module that gives the signer of an invoice.
  `scripts/deps-check.sh` passes with the lists unchanged. The credential
  of a connection is never in a `Debug` text.

## 9. Sources

```
Core Lightning releases (26.06.7 to 26.06.10, security notes):
  https://github.com/ElementsProject/lightning/releases
Core Lightning pages (clnrest, invoice, listinvoices, createrune):
  https://docs.corelightning.org/docs/rest
  https://docs.corelightning.org/reference/invoice
  https://docs.corelightning.org/reference/listinvoices
  https://docs.corelightning.org/reference/createrune
Runes (the form of a rune):
  https://github.com/rustyrussell/runes
LND releases (v0.21.6-beta):
  https://github.com/lightningnetwork/lnd/releases
LND macaroons page (the stock invoice.macaroon, the header):
  https://docs.lightning.engineering/lightning-network-tools/lnd/macaroons
LND REST swagger file (the lookup by hash):
  https://github.com/lightningnetwork/lnd/blob/master/lnrpc/lightning.swagger.json
LND add-invoice page:
  https://lightning.engineering/api-docs/api/lnd/lightning/add-invoice/
Eclair documentation and its reference.conf (no restricted credential):
  https://acinq.github.io/eclair/
  https://github.com/ACINQ/eclair/blob/master/eclair-core/src/main/resources/reference.conf
BIP 321:
  https://github.com/bitcoin/bips/blob/master/bip-0321.mediawiki
BOLT 11:
  https://github.com/lightning/bolts/blob/master/11-payment-encoding.md
```
