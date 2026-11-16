# Merchant acceptance research tools

Isolated prototypes for local payment routing, persistence limits, and Lightning
funding publication. These tools are research artifacts, not production components.
They are not wired into repository builds or CI.

| Tool | Purpose | Run |
| --- | --- | --- |
| `ledger-model/` | SQLite ledger routing and state prototype | `cd ledger-model && python3 -m unittest -v` |
| `storage-probe/` | Rust checks of selected persistence and recovery limits | `cargo test --manifest-path storage-probe/Cargo.toml` |
| `core-api-probe/` | Host-side exercise of the real counter API with synthetic transaction facts | `cargo run --locked --manifest-path core-api-probe/Cargo.toml` |
| `funding-probe/` | Same-host Bitcoin Core regtest probe of LSPS2 funding publication | See `funding-probe/README.md` |
| `money-model/` | Synthetic replay of capacity, online-funds limits, settlement delay, and output economics | See `money-model/README.md` |
| `batch-claim-probe/` | Actual Core regtest checks of unilateral claim aggregation and a limited cold-input authorization | See `batch-claim-probe/README.md` |
| `onchain-batch-probe/` | Ordinary confirmed address payments combined into one savings output, with fees deducted from receipts | See `onchain-batch-probe/README.md` |

The ledger and storage tools use synthetic fixtures and local state. The funding
probe uses a same-host regtest fixture and requires explicit paths to the built
binaries; it is not a real payment or production-provider test. Read each tool's
README before running it. Nothing in this directory tests VSS or a native SDK.
