# Fuzz targets

Fuzz targets for the decoders and parsers in `openbmt-core` and for the Key
app's signer (`openbmt-signer`), built with
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
| `pair_offer_decode` | `PairOffer::from_bytes` |
| `pair_request_decode` | `PairRequest::from_bytes` |
| `signed_pairing_decode` | `SignedPairing::from_bytes` |
| `wallet_parse` | `WalletDescription::parse` for mainnet and testnet on UTF-8 input; on success, parsing the canonical string again must give the same canonical string |

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
