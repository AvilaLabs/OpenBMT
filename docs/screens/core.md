# Core: what a person is told

Made by `screens_markdown` in `crates/openbmt-core/src/say/import.rs`. Do
not edit it by hand: run the test `sc_12_the_list_is_on_disk` with the
environment variable `OPENBMT_WRITE_SCREENS=1` to write it again.

| State | Heading | What happened | What to do next | Who | Code |
|---|---|---|---|---|---|
| ImportRefused::HasPrivateKey | This holds a private key | What you scanned can spend the money, not just watch it. Anyone who has seen it can take the bitcoin. | Do not use this wallet for the shop. Move any bitcoin in it to a new wallet, then scan the new wallet's public description. | the owner | WL-08 |
| ImportRefused::Damaged | This description is damaged | Its checksum does not match, so part of it was misread or changed. | Scan or export it again from your wallet. | the owner | WL-10 |
| ImportRefused::TestNetwork | This is a test-network wallet | It is for testnet or signet, not real bitcoin. | Export the description of your real bitcoin account and scan that. | the owner | WL-11 |
| ImportRefused::Unsupported | This wallet type is not supported | OpenBMT can check native SegWit single-key wallets and its own 2-of-3 multisig. This is a different kind. | In your wallet, export the public description of a native SegWit account (it starts with wpkh) and scan that. | the owner | WL-12 |
