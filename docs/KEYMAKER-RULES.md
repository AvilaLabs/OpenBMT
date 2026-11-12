# Key maker rules: coin flips to a wallet, offline

Status: implemented. The core is `tools/keymaker/src/core.js`, the QR encoder
`src/qr.js`, the page `src/page.js` with `src/words.js`, and the bundler
`build.mjs`. Not yet done: the checksum and signature of a release, the
handbook page and a run in a Tails virtual machine. Each rule has an ID so
that a test can name the rule it proves (`km_NN_...`, in
`tools/keymaker/test/`, run with `npm test` inside `tools/keymaker/`). The
rules serve the key maker design:
the owner makes the wallet's words from their own coin flips, offline, and
Electrum must show the same wallet before the shop goes live.

The core turns flips into words and words into the public description of a
wallet. It has no screen, no storage and no network, and it has no source of
chance: the only bits that enter are the flips the owner passes in.

**First build. No independent security audit has been done. Not for real
funds.**

## 1. What is fixed

- **KM-01. The fixed choices.** There are 128 flips, 12 BIP39 English words,
  no passphrase, native SegWit with a single key (BIP84), the account
  `m/84'/0'/0'` and real bitcoin only. Nothing chooses between options.
- **KM-02. The order of the flips.** A flip is 1 for heads and 0 for tails.
  The first flip is the most significant bit of the first byte, and the
  flips fill 16 bytes of entropy in that order.
- **KM-03. No source of chance.** Nothing in `src/` asks for random bytes,
  reads the clock or calls the platform's crypto object. The wallet is a
  function of the flips alone.
- **KM-04. Wrong flips are refused.** Anything other than exactly 128 values,
  each the number 1 or the number 0, is refused with `KM-04`, as is entropy
  that is not 16 bytes. No value is converted or padded.
- **KM-05. No network.** Nothing in `src/` fetches, opens a socket, loads a
  module from a URL or by a computed name, or names a web address. The same
  holds for the built file, libraries included: no `fetch(`,
  `XMLHttpRequest`, `WebSocket`, `EventSource`, `sendBeacon`, `import(`, no
  `http:` or `https:` outside licence comments, no storage, no
  `navigator.clipboard`, no `getRandomValues`, no `Math.random`.

## 2. What comes out

- **KM-06. The outputs.** From the 12 words the core gives the master
  fingerprint (8 hex characters), the account key at `m/84'/0'/0'` as a plain
  `xpub`, the first receive address at `m/84'/0'/0'/0/0` as a `bc1q` address,
  and three descriptors: `wpkh([fp/84h/0h/0h]xpub/<0;1>/*)` and the two
  single-path forms ending `/0/*` and `/1/*`, each with its checksum.
- **KM-07. The working.** For "Check it by hand" the core gives eleven groups
  of 11 flips with each group's number and word, then the last 7 flips with
  the 4 check bits (the first 4 bits of the SHA-256 of the entropy), their
  number and their word. The twelve words so found equal the wallet's words,
  or the working is refused with `KM-07`.
- **KM-08. The checksum on every descriptor.** Each descriptor ends in `#` and
  the 8-character BIP380 checksum of the text before it, and passes the
  BIP380 check.
- **KM-09. Characters outside the descriptor set.** The checksum is written
  from the BIP380 text. A character outside its input set is refused with
  `KM-09`.

## 3. What it is checked against

- **KM-10. BIP380.** The checksum gives `89f8spxm` for `raw(deadbeef)`, and
  the published bad cases (no checksum, an empty, a longer or a shorter one,
  an error in the payload or in the checksum, a character outside the set)
  all fail the check.
- **KM-11. BIP39.** Every English vector of the trezor `vectors.json`
  (24 of them) gives its words from its entropy, its seed with the passphrase
  `TREZOR`, and its root key.
- **KM-12. BIP32.** Test vectors 1 and 2 give every published key, public and
  private, along their chains.
- **KM-13. BIP84.** The BIP84 mnemonic gives the published account key (as a
  plain `xpub`), root key and first receive address.
- **KM-14. Electrum.** For 20 fixed flip strings (`test/electrum/flips.txt`),
  the words, fingerprint, `xpub` and first receive address equal what the
  library of Electrum 4.8.4 prints (`test/electrum/expected.txt`).
  `test/electrum/crosscheck.sh` runs that Electrum again, without a network,
  and compares. The test seed (the flips `10` repeated 64 times) gives
  `primary fetch primary fetch primary fetch primary fetch primary fetch
  primary fever`, fingerprint `818bc9b6` and the first address
  `bc1qyjassxtmzl02s8de8jw9365qmuvwg74udtp3vj`.

## 4. Libraries

The libraries are `@noble/hashes`, `@noble/curves`, `@scure/base`,
`@scure/bip39` and `@scure/bip32`, each pinned to an exact version in
`tools/keymaker/package.json`, with the integrity hashes in
`package-lock.json`. The code of the core outside those libraries is the
checksum, the flip packing, the address encoding and the working.

## 5. The page

The page is `src/page.js`, `src/page.css` and `src/page.html`. One step is on
screen at a time, with its number from 0 to 7 (step 1, the rehearsal, runs
through the same screens as the real run and has no screen of its own).

- **KM-15. Only the words in `words.js`.** Every sentence, label and button
  the page shows comes from `WORDS` in `src/words.js`. `page.js` holds no
  string of more than two words, assigns no literal to `textContent`,
  `innerText` or `innerHTML`, and builds no markup. Every key of `WORDS` is
  shown by some screen, and every key the page names exists. Word positions
  are shown from 1.
- **KM-16. The check card.** Three distinct positions among the 12 are asked,
  in ascending order. They are taken from the flips alone: the bytes of the
  SHA-256 of the entropy, in order, each byte below 252 taken modulo 12,
  skipping repeats (the lowest free places if the digest runs out). No source
  of chance is used, so the same flips always ask the same words. The owner
  types the first four letters of each word (the whole word if it is shorter),
  in any letter case. A wrong answer returns to the check screen with the
  first wrong position named, and a button shows the words again.
- **KM-17. The inert notice.** The file holds a visible notice that the
  script hides when it starts, and a `noscript` notice. The rehearsal fill
  (`rehearsalFill`) is offered only in a rehearsal, and made-up flips come
  from a chain of SHA-256 hashes, not from a source of chance.
- **KM-21. Nothing kept, nothing sent.** The page uses no storage, no
  clipboard, no download, no new window, no timer and no sender. Clear and
  close overwrites the flips, the entropy, the word list and the positions,
  empties the answers, drops its references and replaces the body with the
  `cleared` text.
- **KM-23. The whole run in a browser.** The built file, opened from
  `file://` in a browser with no network, runs a rehearsal through every step
  (a wrong card answer, then a right one), then the test seed's flips for
  real, and shows on step 5 the fingerprint `818bc9b6` and the first address
  `bc1qyjassxtmzl02s8de8jw9365qmuvwg74udtp3vj`; the picture is the QR code of
  the multipath description; a different Electrum offers only Clear and
  close. The test needs a browser, so it runs only where
  `KEYMAKER_BROWSER=1` is set and never in CI. It saves a screenshot of each
  step.

## 6. The QR code

- **KM-20. The port equals the Rust encoder.** `src/qr.js` is the byte-mode,
  level M encoder of `crates/openbmt-qr`, with the same automatic version and
  mask. `test/vectors/qr-fixtures.txt` holds the version, mask and matrix the
  Rust encoder gives for 45 inputs (the three descriptors of the test seed,
  the 20 Electrum descriptors, empty, one byte, and the largest length that
  fits and the next length for each of versions 1 to 10), written by the Rust
  test `keymaker_qr_fixtures` (`OPENBMT_WRITE_QR_FIXTURES=1` writes it, a plain
  run checks it). Every matrix must equal the JavaScript one exactly. The
  reader is checked in the other direction: the JavaScript drawing (black on
  white, quiet zone 4, 6 pixels a module) of three codes is committed as PGM
  files, and the Rust test `keymaker_pgm_pictures_decode_to_their_bytes` reads
  them with `decode_luma` and finds the original bytes. The JavaScript test
  checks that the committed PGM files are exactly what its drawing makes.

## 7. The built file

`node build.mjs` writes `dist/openbmt-keymaker.html` (not committed).

- **KM-18. The bundler and the policy.** The bundler starts at `src/page.js`,
  follows static imports (relative paths, and the five pinned libraries
  through each package's `package.json`: the `exports` map with the `import`
  condition, or Node's own rule for a package with no map), and wraps each
  module in a function of a small registry. It refuses a dynamic `import()`,
  `require()`, `import.meta`, `eval()`, an import it cannot resolve or a name
  the target does not export, a package path outside the exports map, and any
  cycle. Comments are dropped except licence comments. One library function,
  the random-bytes helper of `@noble/hashes`, is replaced by one that
  refuses with `KM-03`; the build fails if the text it replaces is not found.
  The file holds one style block and one script block, and a policy
  `default-src 'none'; script-src 'sha256-...'; style-src 'sha256-...'` with
  the hash of each block, no `unsafe-inline`, no inline style attribute, and
  no other package than the five.
- **KM-19. Same input, same bytes.** Building twice gives byte-identical
  files: sorted modules, no date, `\n` line ends. The test builds twice and
  prints the SHA-256.
- **KM-22. The bundle's core is the core.** Run with `node:vm`, the bundle's
  modules give the same words, wallet, working and QR matrix as `core.js` and
  `qr.js` for the test seed and three Electrum flip strings, and its
  random-bytes helper refuses.
