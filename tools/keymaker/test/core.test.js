import assert from 'node:assert/strict';
import { readFileSync, readdirSync } from 'node:fs';
import test from 'node:test';
import {
  ACCOUNT_PATH,
  arithmetic,
  checkDescriptor,
  descriptorChecksum,
  entropyToWords,
  flipsToEntropy,
  fromFlips,
  wordsToWallet,
} from '../src/core.js';

const seedFlips = Array.from({ length: 128 }, (_, i) => (i % 2 === 0 ? 1 : 0));
const seedWords =
  'primary fetch primary fetch primary fetch primary fetch primary fetch primary fever';

test('km_01 fixed choices: 12 English words, no passphrase, BIP84 account', () => {
  const r = fromFlips(seedFlips);
  assert.equal(r.words, seedWords);
  assert.equal(r.words.split(' ').length, 12);
  assert.equal(ACCOUNT_PATH, "m/84'/0'/0'");
  // A passphrase would change the wallet: the test seed must give the
  // fingerprint Electrum shows for no passphrase.
  assert.equal(r.wallet.fingerprint, '818bc9b6');
  assert.match(r.wallet.descriptor, /^wpkh\(\[818bc9b6\/84h\/0h\/0h\]xpub/);
  assert.equal(r.wallet.address.startsWith('bc1q'), true);
});

test('km_02 flips: heads is 1, first flip is the most significant bit', () => {
  const heads = new Array(128).fill(0);
  heads[0] = 1;
  assert.equal(flipsToEntropy(heads)[0], 0x80);
  heads[0] = 0;
  heads[7] = 1;
  assert.equal(flipsToEntropy(heads)[0], 0x01);
  heads[7] = 0;
  heads[127] = 1;
  assert.equal(flipsToEntropy(heads)[15], 0x01);
  assert.equal(flipsToEntropy(new Array(128).fill(1)).every((b) => b === 0xff), true);
  assert.equal(entropyToWords(flipsToEntropy(new Array(128).fill(0))).split(' ')[11], 'about');
});

function sourceFiles() {
  const dir = new URL('../src/', import.meta.url);
  return readdirSync(dir).map((f) => [f, readFileSync(new URL(f, dir), 'utf8')]);
}

test('km_03 no random source anywhere in src', () => {
  const banned = [/random/i, /\bcrypto\.(subtle|get)/, /\bcrypto\b\s*\./, /\bperformance\.now/, /Date\.now/];
  for (const [f, text] of sourceFiles()) {
    for (const re of banned) assert.doesNotMatch(text, re, `${f}: ${re}`);
  }
});

test('km_04 wrong flips are refused', () => {
  const refuse = (x) => assert.throws(() => flipsToEntropy(x), /KM-04/);
  refuse([]);
  refuse(seedFlips.slice(1));
  refuse([...seedFlips, 1]);
  refuse(seedFlips.map((f, i) => (i === 5 ? 2 : f)));
  refuse(seedFlips.map((f, i) => (i === 5 ? '1' : f)));
  refuse(seedFlips.map((f, i) => (i === 5 ? true : f)));
  refuse(seedFlips.map((f, i) => (i === 5 ? null : f)));
  refuse('1'.repeat(128));
  refuse(undefined);
  assert.throws(() => entropyToWords(new Uint8Array(15)), /KM-04/);
  assert.throws(() => entropyToWords(new Uint8Array(32)), /KM-04/);
  assert.throws(() => entropyToWords([...new Uint8Array(16)]), /KM-04/);
});

test('km_05 no network call anywhere in src', () => {
  const banned = [
    /\bfetch\b/,
    /XMLHttpRequest/,
    /WebSocket/,
    /EventSource/,
    /sendBeacon/,
    /\bimport\s*\(/,
    /\brequire\s*\(/,
    /from\s+['"]\s*(https?:|\/\/|data:|node:(net|http|https|dns|tls|dgram))/,
    /https?:\/\//,
  ];
  const files = sourceFiles();
  assert.ok(files.length > 0);
  for (const [f, text] of files) {
    for (const re of banned) assert.doesNotMatch(text, re, `${f}: ${re}`);
  }
});

test('km_06 outputs for the test seed', () => {
  const w = wordsToWallet(seedWords);
  assert.equal(w.fingerprint, '818bc9b6');
  assert.equal(
    w.xpub,
    'xpub6CgURS1GrKZkAoRGBTKYX9FhLgkKnQVZ6vdNBUbiu24xtRJ6RzLKpdg4CqsVyBLzimcJsviP59hSoGJEpTJiXE8bdBLw1qPC1DXRatKehb8',
  );
  assert.equal(w.address, 'bc1qyjassxtmzl02s8de8jw9365qmuvwg74udtp3vj');
  const origin = `[818bc9b6/84h/0h/0h]${w.xpub}`;
  assert.equal(w.descriptor, `wpkh(${origin}/<0;1>/*)#96yh9yhx`);
  assert.equal(w.receiveDescriptor, `wpkh(${origin}/0/*)#epd8sdec`);
  assert.equal(w.changeDescriptor, `wpkh(${origin}/1/*)#g4gxdcfq`);
  assert.equal(w.fingerprint.length, 8);
});

test('km_07 the working: groups, words and check bits', () => {
  const a = arithmetic(seedFlips);
  assert.equal(a.groups.length, 11);
  assert.equal(a.groups[0].bits, '10101010101');
  assert.equal(a.groups[0].number, 1365);
  assert.equal(a.groups[0].word, 'primary');
  assert.equal(a.groups[1].bits, '01010101010');
  assert.equal(a.groups[1].number, 682);
  assert.equal(a.groups[1].word, 'fetch');
  assert.deepEqual(a.last, { bits: '0101010', checkBits: '1011', number: 683, word: 'fever' });
  assert.equal(a.groups.map((g) => g.word).concat(a.last.word).join(' '), seedWords);
  assert.equal(a.entropyHex, 'aa'.repeat(16));
  // the all-heads case: last 7 flips are ones, check bits from SHA-256
  const ones = arithmetic(new Array(128).fill(1));
  assert.equal(ones.last.bits, '1111111');
  assert.equal(ones.last.checkBits.length, 4);
  assert.throws(() => arithmetic(seedFlips.slice(1)), /KM-04/);
});

test('km_08 descriptors carry correct checksums', () => {
  const w = wordsToWallet(seedWords);
  for (const d of [w.descriptor, w.receiveDescriptor, w.changeDescriptor]) {
    assert.equal(checkDescriptor(d), true, d);
    assert.equal(d.at(-9), '#');
    assert.equal(d.slice(-8), descriptorChecksum(d.slice(0, -9)));
  }
  assert.equal(checkDescriptor(w.descriptor.replace('84h', '49h')), false);
});

test('km_09 characters outside the descriptor set are refused', () => {
  assert.throws(() => descriptorChecksum('raw(Ü)'), /KM-09/);
  assert.throws(() => descriptorChecksum('wpkh(€)'), /KM-09/);
});
