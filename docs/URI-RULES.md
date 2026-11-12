# Bitcoin payment URI rules

`openbmt-core::bip321::PaymentRequest` reads the mainnet payment subset of
[BIP 321](https://github.com/bitcoin/bips/blob/master/bip-0321.mediawiki).
The scheme and query keys are ASCII case-insensitive. The address is checked
by the Bitcoin address parser and must be a checksummed mainnet P2PKH, P2SH,
P2WPKH, P2WSH or P2TR address. A URI with no address needs at least one
recognized, nonempty payment instruction (`lightning`, `lno` or `sp`). A
label or amount alone does not provide a payment target.

Amounts use decimal BTC notation only, with digits on both sides of a decimal
point when present, at most eight fractional digits, and no more than 21
million BTC. Values are converted directly to integer satoshis. Exponents,
signs, commas, excess precision and percent-encoded amount digits are refused.
Labels and messages are percent-decoded once as UTF-8; malformed escapes,
invalid UTF-8, control characters and decoded strings over 512 bytes are
refused. A literal `+` remains a plus, as required by RFC 3986.

`lightning`, `lno` and `sp` values are returned exactly as URI-encoded text.
The parser checks that their percent escapes are well formed, but does not
interpret their contents. A `req-` prefix is retained as `required: true` for
recognized payment methods. Unsupported required parameters are refused.
Required `amount`, `label` and `message` fields are recorded in
`PaymentRequest::required_fields`, so callers can distinguish them from
optional values.
Unknown optional parameters are retained in bounded form for callers to
ignore or inspect. Every query key is unique after percent-decoding, ASCII
case-folding and removal of a `req-` prefix, so forms such as `amount` and
`req-amount` cannot create ambiguous duplicate fields.

This is a deliberately stricter profile than BIP 321 in several ways: it
refuses all duplicate canonical keys (BIP 321 permits duplicates for some
payment and unknown optional keys), limits the full URI to 2048 bytes and
label/message values to 512 decoded bytes, and accepts only mainnet on-chain
addresses. Testnet-specific `tb` parameters are refused too. It also refuses
empty query fields, fragments, authority syntax, userinfo, percent-encoded
address paths and non-ASCII URI bytes. Callers must still obtain user
authorization before acting on a parsed request.

## BIP 321 reader rules

- **BU-01 — Mainnet address.** Accept only checksum-valid mainnet P2PKH, P2SH,
  P2WPKH, P2WSH and P2TR addresses. Refuse testnet, mixed-case witness,
  bad-checksum and other witness-version addresses. Covered by
  `bu_01_mainnet_address_classes_and_network_are_checked`.
- **BU-02 — Amount.** Convert decimal BTC directly to satoshis, with no
  exponent, sign, excess precision, encoded digits or value above 21 million
  BTC. Covered by `bu_02_amount_converts_exact_decimal_to_satoshis`.
- **BU-03 — Duplicates and required keys.** Refuse duplicate query keys and
  unsupported `req-` parameters. Covered by
  `bu_03_duplicate_keys_and_unknown_required_are_refused`.
- **BU-04 — Label and message.** Decode percent escapes once as UTF-8, reject
  controls and invalid UTF-8, and cap decoded text at 512 bytes. Covered by
  `bu_04_amount_and_decoded_text_bounds_are_strict`.
- **BU-05 — URI syntax.** Refuse malformed URI shape, fragments, authority,
  address path escapes, malformed percent escapes and characters outside the
  RFC 3986 query set. Covered by
  `bu_05_uri_shape_and_percent_escapes_are_validated`.
- **BU-06 — Required marker and size.** Keep the `req-` marker for supported
  payment methods and refuse URIs over 2048 bytes. Covered by
  `bu_06_required_payment_flag_and_uri_size_are_preserved`.
- **BU-07 — Query key identity.** Decode and case-fold keys before matching,
  reject aliases and duplicates, and refuse control bytes in keys. Covered by
  `bu_07_encoded_keys_cannot_bypass_canonicalization`.
- **BU-08 — Payment instructions and required fields.** Preserve raw
  `lightning`, `lno` and `sp` values and their required flags, and record the
  required marker for `amount`, `label` and `message`. An empty address needs
  a nonempty known payment instruction. Covered by
  `bu_08_supported_methods_keep_raw_values_and_required_flags`.

`bu_00_required_rule_ids_are_complete_and_unique` checks that this list stays
in sync with the rule tests.
