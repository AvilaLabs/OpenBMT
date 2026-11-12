import assert from 'node:assert/strict';
import { readFileSync, readdirSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import test from 'node:test';
import {
  ACCOUNT_PATH,
  arithmetic,
  checkDescriptor,
  coinCheckCommand,
  descriptorChecksum,
  dieCheckCommand,
  entropyToBits,
  entropyToWords,
  flipsToBits,
  flipsToEntropy,
  fromCoin,
  fromDie,
  fromFlips,
  rollsToValues,
  valuesToDigits,
  valuesToEntropy,
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

test('km_02 bits: the first kept bit is the most significant bit of the first byte', () => {
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

// words.js holds the sentences the page shows (one of them says the flips
// are "still random"). Its string contents are blanked for the code checks,
// so its code is still scanned and a call could not hide there.
function blankStrings(text) {
  return text.replace(/'(?:[^'\\\n]|\\.)*'/g, "''");
}

function sourceFiles() {
  const dir = new URL('../src/', import.meta.url);
  return readdirSync(dir).map((f) => {
    const text = readFileSync(new URL(f, dir), 'utf8');
    return [f, f === 'words.js' ? blankStrings(text) : text];
  });
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
  assert.throws(() => flipsToEntropy([...seedFlips.slice(1), 2]), /KM-04/);
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

// The pair rule: heads then tails is 0, tails then heads is 1, two the same
// are dropped (heads is 1 and tails is 0 in the list the page holds).
const H = 1;
const T = 0;

/** A flip list that gives `bits` through the pair rule, with dropped pairs mixed in. */
function pairFlips(bits) {
  const out = [];
  bits.forEach((b, i) => {
    if (i % 3 === 0) out.push(H, H);
    if (i % 5 === 1) out.push(T, T);
    out.push(...(b === 0 ? [H, T] : [T, H]));
  });
  out.push(T, T);
  return out;
}

test('km_29 the pair rule: heads-tails is 0, tails-heads is 1, equal pairs are dropped', () => {
  assert.deepEqual(flipsToBits([H, T]), [0]);
  assert.deepEqual(flipsToBits([T, H]), [1]);
  assert.deepEqual(flipsToBits([H, H, T, T]), []);
  assert.deepEqual(flipsToBits([H, T, H, H, T, H, T, T, T, H]), [0, 1, 1]);
  // An odd flip waits for its partner.
  assert.deepEqual(flipsToBits([H, T, H]), [0]);
  assert.deepEqual(flipsToBits([H, T, H, H]), [0]);
  assert.deepEqual(flipsToBits([H, T, H, T]), [0, 0]);
  // The pairs start at the first flip, not after a dropped pair.
  assert.deepEqual(flipsToBits([H, H, H, T]), [0]);
  assert.throws(() => flipsToBits([1, 2]), /KM-29/);
  assert.throws(() => flipsToBits('10'), /KM-29/);
});

test('km_29 the reference walkthrough: the bits give check bits 1000 and the twelve words', () => {
  const bits = [...'11010000100010001110100100010110011100100101111101010110110111101001010001010110011010110110101101110010101000000100011111111100'].map(Number);
  assert.equal(bits.length, 128);
  const flips = pairFlips(bits);
  assert.ok(flips.length > 256);
  assert.deepEqual(flipsToBits(flips), bits);
  const r = fromCoin(flips);
  assert.equal(r.working.last.checkBits, '1000');
  assert.equal(r.words, 'spatial elder easily tool voice hungry eyebrow sniff hidden never ball weekend');
  // The same words from the bits directly.
  assert.equal(fromFlips(bits).words, r.words);
});

const triplets =
  '136 235 425 512 124 432 523 456 456 652 532 431 651 643 314 361 145 162 145 265 365 641 143 532 531 541 146 614 461 364 126 324 256 216 214 231 126 325 524 541 265 326 436 416 634 631 461 245 356 352';
const tripletValues =
  '1 1 6 5 1 4 5 1 1 4 4 4 4 4 6 3 1 2 1 2 2 4 2 4 4 4 1 5 3 2 1 6 1 6 6 3 1 6 5 4 2 6 6 6 5 4 3 1 1 3';

test('km_30 the order of three: each value follows the order of low, middle and high', () => {
  const rolls = [...triplets.replaceAll(' ', '')].map(Number);
  const values = rollsToValues(rolls);
  assert.equal(values.join(' '), tripletValues);
  // Every one of the six orders, by hand.
  const table = { 123: 1, 132: 2, 231: 3, 321: 4, 312: 5, 213: 6 };
  for (const [t, v] of Object.entries(table)) {
    assert.deepEqual(rollsToValues([...t].map(Number)), [v], t);
    assert.deepEqual(rollsToValues([...t].map((d) => Number(d) + (Number(d) < 4 ? 2 : 0))), [v], t);
  }
  // The small example: the three with a repeat is dropped.
  const small = '123 463 244 152 236 654 411 614 614 336';
  assert.deepEqual(rollsToValues([...small.replaceAll(' ', '')].map(Number)), [1, 3, 2, 1, 4, 5, 5]);
  // A repeat anywhere in a three drops it; an open three gives nothing.
  assert.deepEqual(rollsToValues([1, 1, 2, 2, 3, 3, 3, 3, 3]), []);
  assert.deepEqual(rollsToValues([1, 2, 3, 4, 5]), [1]);
  assert.throws(() => rollsToValues([1, 2, 7]), /KM-30/);
  assert.throws(() => rollsToValues([0, 2, 3]), /KM-30/);
  assert.throws(() => rollsToValues([1.5, 2, 3]), /KM-30/);
  assert.throws(() => rollsToValues('123'), /KM-30/);
});

test('km_31 the die hash: 50 values, their digits, the first 16 bytes of SHA-256', () => {
  const values = tripletValues.split(' ').map(Number);
  assert.equal(values.length, 50);
  const digits = '11651451144444631212242444153216166316542666543113';
  assert.equal(valuesToDigits(values), digits);
  const entropy = valuesToEntropy(values);
  assert.equal(Buffer.from(entropy).toString('hex'), '375b26bdb3e85c176decb8e398860625');
  const rolls = [...triplets.replaceAll(' ', '')].map(Number);
  const r = fromDie(rolls);
  assert.equal(r.words, 'dance summer quarter guitar magic arm response novel toe service light enough');
  assert.equal(r.die.digits, digits);
  assert.equal(r.die.hash, '375b26bdb3e85c176decb8e398860625');
  assert.equal(r.working.entropyHex, r.die.hash);
  assert.deepEqual(r.die.values, values);
  // Exactly 50 values, each 1 to 6.
  assert.throws(() => valuesToDigits(values.slice(1)), /KM-31/);
  assert.throws(() => valuesToDigits([...values, 1]), /KM-31/);
  assert.throws(() => valuesToDigits(values.map((v, i) => (i === 3 ? 7 : v))), /KM-31/);
  assert.throws(() => fromDie(rolls.slice(0, 120)), /KM-31/);
  // Dropped threes in the middle do not change the result.
  const withDrops = [];
  for (let i = 0; i < rolls.length; i += 3) {
    withDrops.push(...rolls.slice(i, i + 3));
    if (i % 9 === 0) withDrops.push(2, 2, 5);
  }
  assert.equal(fromDie(withDrops).words, r.words);
});

test('km_34 the hand check lines match the real tools', () => {
  const bits = [...'11010000100010001110100100010110011100100101111101010110110111101001010001010110011010110110101101110010101000000100011111111100'].map(Number);
  const cmd = coinCheckCommand(bits);
  assert.equal(cmd, `printf '%s' '${bits.join('')}' | basenc --base2msbf --decode | sha256sum`);
  assert.deepEqual(entropyToBits(flipsToEntropy(bits)), bits);
  const values = tripletValues.split(' ').map(Number);
  assert.equal(
    dieCheckCommand(values),
    'echo -n "11651451144444631212242444153216166316542666543113" | sha256sum',
  );
  // Run them: the first hex character is the check bits of the coin run, the
  // first 32 are the entropy of the die run.
  const sh = (c) => execFileSync('bash', ['-c', c], { encoding: 'utf8' });
  const coinHash = sh(cmd).split(' ')[0];
  assert.equal(parseInt(coinHash[0], 16).toString(2).padStart(4, '0'), '1000');
  assert.equal(sh(dieCheckCommand(values)).split(' ')[0].slice(0, 32), '375b26bdb3e85c176decb8e398860625');
  assert.throws(() => coinCheckCommand(bits.slice(1)), /KM-04/);
  assert.throws(() => dieCheckCommand(values.slice(1)), /KM-31/);
});
