// KM-14: what Electrum's own library printed for 20 fixed flip strings
// (test/electrum/expected.txt, made by test/electrum/crosscheck.sh) equals
// what the core gives. crosscheck.sh re-runs Electrum itself.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import { fromCoin, fromFlips } from '../src/core.js';

test('km_14 core equals the recorded Electrum 4.8.4 output on 20 flip strings', () => {
  const lines = readFileSync(new URL('electrum/expected.txt', import.meta.url), 'utf8')
    .trim()
    .split('\n');
  assert.equal(lines.length, 20);
  const flipLines = readFileSync(new URL('electrum/flips.txt', import.meta.url), 'utf8')
    .trim()
    .split('\n');
  assert.deepEqual(
    lines.map((l) => l.split(' ')[0]),
    flipLines,
  );
  for (const line of lines) {
    const parts = line.split(' ');
    const flips = [...parts[0]].map(Number);
    const [fp, xpub, address] = parts.slice(-3);
    const words = parts.slice(1, 13).join(' ');
    const r = fromFlips(flips);
    assert.equal(r.words, words);
    assert.equal(r.wallet.fingerprint, fp);
    assert.equal(r.wallet.xpub, xpub);
    assert.equal(r.wallet.address, address);
    // The same bits as coin flips through the pair rule (1 is tails-heads).
    const coin = fromCoin(flips.flatMap((b) => (b === 0 ? [1, 0] : [0, 1])));
    assert.equal(coin.words, words);
    assert.equal(coin.wallet.address, address);
  }
});
