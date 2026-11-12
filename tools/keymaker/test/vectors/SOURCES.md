# Test vector sources

- `bip39-english.json`: the `english` list of `vectors.json` in
  https://github.com/trezor/python-mnemonic at commit
  b57a5ad77a981e743f4167ab2f7927a55c1e82a8. Each entry is entropy (hex),
  mnemonic, seed (hex, passphrase `TREZOR`) and extended private key.
- BIP32 vectors 1 and 2, the BIP84 vectors and the BIP380 vectors are written
  into `test/vectors.test.js` from `bip-0032.mediawiki`, `bip-0084.mediawiki`
  and `bip-0380.mediawiki` of https://github.com/bitcoin/bips at commit
  b34d9f751791d4feb22feff4f2ed00bcac83961b.
