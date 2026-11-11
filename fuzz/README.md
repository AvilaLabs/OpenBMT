# Fuzz targets

Fuzz targets for the decoders and parsers in `openbmt-core`, for the Key
app's signer (`openbmt-signer`), for the spend path (`openbmt-spend`), for
the books export (`openbmt-books`) and for the payment watcher
(`openbmt-watch`), built with
[cargo-fuzz](https://github.com/rust-fuzz/cargo-fuzz). This directory is its
own cargo project and is not a member of the main workspace, so
`cargo test --workspace` at the repository root does not touch it.

Each decoding target feeds arbitrary bytes to one `from_bytes` and checks two
things: it never panics, and when decoding succeeds the value encodes back to
exactly the input bytes.

| Target | What it feeds |
|---|---|
| `signed_batch_decode` | `SignedBatch::from_bytes` |
| `record_decode` | `Record::from_bytes` |
| `signed_settings_decode` | `SignedSettings::from_bytes` |
| `high_water_decode` | `HighWater::from_bytes` |
| `counter_batches_decode` | `CounterBatches::from_bytes`, with fixed roots and policy; the counter and wallet checksum are read from the input's own header |
| `exposure_decode` | `Exposure::from_bytes` |
| `pause_tracker_decode` | `PauseTracker::from_bytes` |
| `signer_state_decode` | `SignerState::from_bytes` with a fixed network and key, on the input as it is and on the input placed after the wallet text of a valid snapshot (so that it gets past the header); on success the state must encode back to exactly those bytes |
| `signer_review` | `SignerState::review` on a fixed registered state with arbitrary bytes as the PSBT file; it must never panic. A file that passes is signed on a copy of the state: the stored bytes must be the new state and read back unchanged, no total may go down, and the returned file must be reported as already signed. Seed files that pass every rule come from the `fuzz_seeds` example (below) |
| `watch_box_proof` | `verify_box_proof` with a chain that knows every block, so that only the proof checks can refuse; the input is split into a proof, a coinbase and a payment by two 16-bit lengths at its start. It must never panic; a proof that passes must prove a non-coinbase transaction that is not 64 bytes long without its witness |
| `fee_history_decode` | `FeeHistory::from_bytes`; on success the history must encode back to exactly those bytes, and `recent_rate`, `is_low` and `forget_from` must never panic |
| `watch_block` | `payments_in_block` with a chain that knows every block; the first byte gives the length (modulo 40) of the shop's script, the rest is the block. It must never panic; every payment returned must be after the coinbase, in block order, pay the script and not be 64 bytes long without its witness |
| `net_manager` | the peer manager on regtest with 3 peers, driven by a script of steps in the input: connect, receive bytes (optionally after a good handshake, and optionally framed as a chosen command with a right checksum), tick, close. It must never panic, and the bytes waiting for a peer never pass the session cap |
| `pair_offer_decode` | `PairOffer::from_bytes` |
| `pair_request_decode` | `PairRequest::from_bytes` |
| `counter_bundle_decode` | `CounterBundle::from_bytes`, with re-encode equality |
| `counter_pairing_record_decode` | `PairingRecord::from_bytes` (the counter's stored pairing record), with re-encode equality |
| `counter_lightning_record_decode` | `LightningRecord::from_bytes` (the counter's sealed Lightning connection), with re-encode stability |
| `signed_pairing_decode` | `SignedPairing::from_bytes` |
| `owner_command_decode` | `SignedCommand::from_bytes` |
| `owner_pos_setup_decode` | `PosSetup::from_bytes` of `openbmt-owner-node` (the owner's till settings, magic `OBMr`); on success the settings must encode back to exactly those bytes |
| `watch_sale_decode` | `SaleWatch::from_bytes` |
| `net_v2_bytes` | a session on the encrypted transport after a good handshake with a fixed responder: the input is the raw stream, or the contents of valid packets (genuine or decoys) that the session decrypts and parses. It must never panic, a drop names its rule, and the bytes waiting never pass the session cap |
| `net_book_decode` | `AddressBook::from_bytes` |
| `entropy_dice` | `words_from_dice_hashed`, `face_counts` and `bits_from_dice` on UTF-8 input; success must mean the input held only the digits 1 to 6 |
| `key_deposit_code` | `parse_deposit_code` of `openbmt-key-ffi` on UTF-8 input, the first byte choosing the network; it must never panic, refuse only with a `deposit-` or `address` rule, and a code that is read must give an amount in range, a label without control characters, no `req-` or understood name in `ignored`, and an address that reads again as itself |
| `key_words` | `KeyApp::create_from_entropy` with `TypedWords`, from lines of text or from indices into the English list; it must never panic, refuse only with `words-count`, `words-unknown` or `words-checksum` (and then leave the Key app empty, with no long word echoed), and an accepted phrase must leave no words waiting |
| `key_dispatch` | `dispatch` of `openbmt-key-ffi` on arbitrary request bytes (or, when the first byte says so, the fields of a frame addressed to a Key app that has its wallet, to an open session, or to no object), against an in-memory store; it must never panic, always answer with a response frame that is not an internal error, and the bytes it stores must always restore a Key app |
| `key_lock_frame_decode` | `lock::decode` of `openbmt-key-ffi` (the frame the Key app stores its state in, magic `OBMx`), on the input as it is and behind a good magic and version; a frame that is read must have parameters inside the limit, be exactly as long as its header says and encode back to the input |
| `counter_sale_decode` | `Sale::from_bytes` |
| `counter_desk_decode` | `Desk::from_bytes` |
| `counter_event_decode` | `Event::decode` |
| `counter_ops` | a real counter driven by a script of operations in the input (open, requote, sighting, block, reorg, time, judge, override, cancel, owner command, a Lightning payment, restart) against a memory store that fails the writes the input chooses; see the target for the invariants it asserts after every operation |
| `spend_combine` | `openbmt_spend::combine` and `finalize` against the file of one fixed plan; the input is two "signed" files split at the first byte. It must never panic, and no transaction may come out, because nothing in the input is a signature by a wallet key |
| `books_export_decode` | `Books::from_bundle` (what is read must encode back to the input) and `decode_export_header` (the header must encode back to the input's prefix); a file whose header asks for tiny Argon2 parameters is also given to `open_export` with a fixed passphrase, and must always be refused without a panic |
| `session_request_decode` | `Request::from_bytes` (the approval session request); on success the value must encode back to exactly the input |
| `session_response_decode` | `Response::from_bytes` (the approval session response); on success the value must encode back to exactly the input |
| `approver_state_decode` | `Approver::restore` (the Key app's stored state), with and without key 2; on success the snapshot must equal the input |
| `owner_queue_decode` | `Owner::restore` (the owner phone's stored queue); on success the snapshot must equal the input |
| `wallet_parse` | `WalletDescription::parse` for mainnet and testnet on UTF-8 input; on success, parsing the canonical string again must give the same canonical string |
| `courier_records_decode` | `decode_records`, the content of a Records message; on success each record encoded again behind its length must give the input back |
| `box_claim_decode` | `Claim::from_bytes` and `Welcome::from_bytes` (each must encode back to the input) and `Invitation::from_text` on UTF-8 input (one that reads must read back from its own text to the same invitation) |
| `box_notice_decode` | `PaidNotice::from_bytes` (a notice that reads is the pinned 141 bytes and encodes back to the input) and `PosNotice::from_bytes` (encodes back to the input); checking a signature and writing the text of a notice never panic |
| `box_status_request` | `parse_request`, the reader of the Box's status page, on arbitrary bytes, and `form_field` and `same_origin` on the bytes and on the request they give; it must never panic, a request that is passed must be inside the head and body limits with a host of printable characters, a `GET` has no body, the same bytes read the same way, and a bytes-so-far that is not whole is not over the head limit |
| `qr_encode` | `openbmt_qr` on arbitrary bytes: the first byte gives the level and how to encode (bytes, text, or a forced version and mask from the next two bytes, which may be out of range); it must never panic, refuse a bad version or mask only with those errors, and a code that is made must have the level asked for, every module readable (and none outside the square dark), and the format bits in both copies in the matrix equal to those of its level and mask |
| `qr_decode` | `decode_luma` and `decode_luma_strided` of `openbmt_qr`, four ways by the first byte: raw pixels of a small frame (with or without row padding); a code drawn from the input and damaged by pixel writes from the rest of it; and any sides, including ones out of range, over a buffer of any length. It must never panic and stay in its work budget; sides out of range must be refused as `QR-10` or `QR-11`; a result must have a version from 1 to 40 and a mask under 8; an undamaged drawing must read back exactly |
| `rates_parse` | `parse_response` on a source (the first byte, modulo the table) and arbitrary bytes, and `parse_price_cents` on the same bytes as text; it must never panic and give the same answer twice, a refusal for length or encoding must be true, and a price that is read must be in the band, make a sample, combine with an equal one into the same rate, and read back from its own dollars and cents |
| `ur_decode` | `Decoder::receive` of `openbmt_ur` on the input as text, one line at a time into one decoder; every refusal must start with its `UR-` code and an estimate must not pass 1. Then the input's bytes as a message (the first byte gives the fragment length) through `Encoder` and back: a decoder that completes must give the same bytes |
| `ur_key_text` | `ur_to_key_text` of `openbmt_ur` on the input as the message of a UR of each type it reads and of some it refuses (the first byte chooses one of them), and on the input as the content of the byte string around a `bytes` message. It must never panic; a refusal must start with its `UR-` code and have words; a key text must be `[<8 hex>/48h/<0 or 1>h/0h/2h]` and an extended key of the right length |
| `pos_square_parse` | The Square reader of `openbmt-pos-readers`: a cursor, a page size and a reply (and a payments reply) from the input; the request body must be JSON, the same bytes must give the same answer, a page that is read has at most 500 orders each within its bounds and a cursor the next request takes, and a refusal for length or encoding must be true |
| `qr_jni_call` | The pure-Rust dispatch of the apps' framed call (`openbmt_qr_jni::call`, not the JNI layer): the input as a request, and behind a good head of each operation, to a fresh registry; every reply must be framed and a refusal a five-byte rule code (for the key operation, a code and three texts). Then a well-formed `qr_encode` and `ur_encode` from the input must give well-formed replies, and the parts of a full turn fed to a decoder must complete the message |
| `box_pos_config` | `PosConfig::from_bytes` of `openbmt-box`, the Box's own POS vendor file: it must never panic, bytes that are read must come back out unchanged by `to_bytes`, and what was read must validate and give POS settings |
| `pos_clover_parse` | The Clover reader: a cursor, a page size and a reply from the input; the query must be ASCII with only unreserved characters and the separators, the same bytes must give the same answer, a page that is read is no longer than the limit, has more exactly when it is as long, and gives a cursor the next request takes |
| `lightning_decode` | `openbmt-lightning`, by the first byte: a BOLT 11 invoice (`decode`, the same text in lower case reads the same, and an invoice that passes `check` names the amount, the hash and the signer asked for), a Core Lightning rune (`check_rune`), an `lndconnect` address, or a node reply read as the answer to a lookup and to an invoice request on both kinds of node; none may panic, the same bytes give the same answer, and a payment is only paid for at least the amount asked |

## Running

Fuzzing needs a nightly toolchain and `cargo install cargo-fuzz`.

```sh
cargo +nightly fuzz list
cargo +nightly fuzz run signed_batch_decode -- -max_total_time=60 -rss_limit_mb=2048 -max_len=4096
```

The `signer_review` target needs seed files to get past parsing:

```sh
cargo run -p openbmt-signer --example fuzz_seeds -- fuzz/corpus/signer_review
```

Corpora and crash inputs are written to `fuzz/corpus` and `fuzz/artifacts`,
which are not committed. A crash input can be replayed with
`cargo +nightly fuzz run <target> fuzz/artifacts/<target>/<file>`. When a
target finds a crash, fix the decoder and add the input as a regression test
in the crate it belongs to.
