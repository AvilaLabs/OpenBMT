# Vendored lightning-liquidity 0.2.8

This directory is the published `lightning-liquidity` 0.2.8 crate with a
focused datetime backend patch. The source originated from crates.io at
`https://crates.io/crates/lightning-liquidity/0.2.8`; its archive SHA-256 is
`edf2ddc875e7a915fa31e66a427a67ff909b866c53cc9c2496e656f08a4dfd3b` (also the
crate checksum recorded in the original `Cargo.lock`). The crate's embedded
`.cargo_vcs_info.json` identifies upstream revision
`254bc9687dd190375956ad790f87239b97ebd058`, path `lightning-liquidity`, in
`https://github.com/lightningdevkit/rust-lightning`.

The upstream package declares `MIT OR Apache-2.0`; both license texts are
included here from that upstream revision. All files from the published crate
are retained. The exact source and manifest changes are recorded in
`chrono-to-time.patch`. The normalized Cargo manifest also carries forward the
upstream lint declaration for its four custom cfg names, including `ldk_bench`
and `lsps1_service`, so Cargo recognizes those documented cfgs without lowering
or suppressing the lint.

The patch changes only the datetime backend used by `LSPSDateTime` from chrono
to the exact `time` 0.3.55 release, with default features disabled and only
`large-dates` and `parsing` enabled. Formatting is implemented with `alloc`
and the component accessors, so the upstream crate's no_std feature mode stays
available. It retains the public LSPSDateTime wrapper,
RFC3339 input validation, UTC normalization, pre-Unix-epoch rejection,
whole-second comparisons, the UTC `Z` spelling in serde, the `+00:00` spelling
in Display/`to_rfc3339`/Lightning `Writeable`, and Chrono's AutoSi fraction
widths (3/6/9 digits). The tuple field's inner type changes from chrono's
`DateTime<Utc>` to `time::OffsetDateTime`, and a private leap-second marker is
added as a second tuple field. This changes `.0`'s type and makes the tuple
constructor private to the crate; downstream code using either form must
migrate. A public `unix_timestamp()` accessor preserves whole-second timestamp
reads without exposing the backend type. OpenBMT's two previous direct
`.0.timestamp()` call sites now use that accessor.

`time` 0.3.55 was released 2026-08-01, declares Rust 1.88, and is MIT OR
Apache-2.0 licensed. The current RustSec database lists `time` advisories for
pre-0.2.23 local-offset functions and for RFC 2822 parsing before 0.3.47;
this patch pins 0.3.55 and uses RFC3339 parsing only.
