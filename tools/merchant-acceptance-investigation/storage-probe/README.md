# Storage and recovery probe

Run with `cargo test --locked --manifest-path tools/merchant-acceptance-investigation/storage-probe/Cargo.toml`
from the repository root. The standalone package imports `openbmt-lightning-node` from this checkout
using a relative path. Its lockfile pins the Rust dependencies used for the
investigation.

The tests cover DualStore behavior with a fault-injected second device, synthetic
update-map choices, AEAD mutation and same-name replay, and a fake device that
acknowledges without retaining data. They do not exercise actual Box storage,
VSS, full Lightning monitor restoration, production keys, channels, mainnet, or
real funds. A same-name replay observation describes the limits of AEAD freshness;
it is not an allegation of an AEAD defect.
