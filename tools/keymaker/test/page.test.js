// The page's words, its check-card positions and its inert notice.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import { sha256 } from '@noble/hashes/sha2.js';
import { flipsToEntropy } from '../src/core.js';
import { cardPrefix, checkPositions, fill, madeUpFlips } from '../src/page.js';
import { WORDS } from '../src/words.js';

const read = (name) => readFileSync(new URL(`../${name}`, import.meta.url), 'utf8');
const seedFlips = Array.from({ length: 128 }, (_, i) => (i % 2 === 0 ? 1 : 0));

/** The source with comments blanked, so that only code is searched. */
function code(text) {
  return text.replace(/\/\*[\s\S]*?\*\//g, (m) => m.replace(/[^\n]/g, ' ')).replace(/\/\/[^\n]*/g, '');
}

function stringLiterals(text) {
  return [...code(text).matchAll(/'((?:[^'\\\n]|\\.)*)'|"((?:[^"\\\n]|\\.)*)"|`((?:[^`\\]|\\.)*)`/g)].map(
    (m) => m[1] ?? m[2] ?? m[3],
  );
}

test('km_15 the page shows only words from WORDS: no string of more than two words in page.js', () => {
  const page = read('src/page.js');
  const long = stringLiterals(page).filter((s) => s.trim().split(/\s+/).length > 2);
  assert.deepEqual(long, []);
  // Whatever is assigned to textContent, innerText or innerHTML comes from
  // WORDS or a variable, never from a literal; and nothing builds markup.
  assert.doesNotMatch(code(page), /(textContent|innerText|innerHTML|outerHTML|insertAdjacentHTML)\s*=\s*['"`]/);
  assert.doesNotMatch(code(page), /\b(innerHTML|outerHTML|insertAdjacentHTML|document\.write)\b/);
});

test('km_15 every key of WORDS is used by the page or the build, and every key used exists', () => {
  const used = new Set();
  for (const f of ['src/page.js', 'build.mjs']) {
    for (const m of read(f).matchAll(/WORDS\.(\w+)/g)) used.add(m[1]);
  }
  const keys = Object.keys(WORDS);
  assert.deepEqual(
    [...used].filter((k) => !keys.includes(k)),
    [],
    'keys used but not in WORDS',
  );
  assert.deepEqual(
    keys.filter((k) => !used.has(k)),
    [],
    'keys in WORDS that no screen shows',
  );
  assert.ok(Object.isFrozen(WORDS));
  assert.equal(fill(WORDS.flipCount, { n: 37, total: 128 }), '37 of 128');
  assert.equal(fill(WORDS.checkWordLabel, { pos: 5 }), 'Word 5');
});

test('km_16 the check positions come from the flips alone: three, distinct, 0 to 11, ascending', () => {
  const entropy = flipsToEntropy(seedFlips);
  // Worked outside this code: SHA-256 of the entropy starts bc1443a0d17aab2d;
  // the bytes 0xbc, 0x14, 0x43, 0xa0 ... give 188 % 12 = 8, 20 % 12 = 8
  // (repeat), 67 % 12 = 7, 160 % 12 = 4: positions 8, 7, 4, ascending 4, 7, 8.
  assert.equal(Buffer.from(sha256(entropy)).toString('hex').slice(0, 16), 'bc1443a0d17aab2d');
  assert.deepEqual(checkPositions(entropy), [4, 7, 8]);
  // The same flips always give the same positions.
  assert.deepEqual(checkPositions(flipsToEntropy(seedFlips)), [4, 7, 8]);
  // Many different flips: always valid, and every place gets chosen.
  const seen = new Array(12).fill(0);
  let block = new Uint8Array(32);
  for (let i = 0; i < 3000; i++) {
    block = sha256(block);
    const p = checkPositions(block.slice(0, 16));
    assert.equal(p.length, 3);
    assert.equal(new Set(p).size, 3);
    assert.deepEqual(p, [...p].sort((a, b) => a - b));
    for (const x of p) {
      assert.ok(x >= 0 && x <= 11);
      seen[x]++;
    }
  }
  // 9000 picks over 12 places: about 750 each.
  for (const n of seen) assert.ok(n > 600 && n < 900, `uneven: ${seen}`);
  // No source of chance in the function's file.
  const page = code(read('src/page.js'));
  for (const re of [/Math\.random/, /getRandomValues/, /Date\b/, /performance/]) {
    assert.doesNotMatch(page, re);
  }
});

test('km_16 the card is read by the first four letters, whatever the case', () => {
  assert.equal(cardPrefix('primary'), 'prim');
  assert.equal(cardPrefix('cat'), 'cat');
  assert.equal(cardPrefix('fetch'), 'fetc');
});

test('km_16 rehearsal flips are 128 values of 0 or 1 and repeat for the same start', () => {
  const a = madeUpFlips([]);
  assert.equal(a.length, 128);
  assert.ok(a.every((f) => f === 0 || f === 1));
  assert.deepEqual(madeUpFlips([]), a);
  const b = madeUpFlips([1, 0, 1]);
  assert.deepEqual(b.slice(0, 3), [1, 0, 1]);
  assert.equal(b.length, 128);
  assert.notDeepEqual(a, b);
});

test('km_17 the file holds a visible inert notice the script hides, and a noscript notice', () => {
  const html = read('src/page.html');
  assert.match(html, /<div id="inert"[^>]*>\{\{inert\}\}<\/div>/);
  assert.doesNotMatch(html.match(/<div id="inert"[^>]*>/)[0], /hidden/);
  assert.match(html, /<noscript>[\s\S]*\{\{noScript\}\}[\s\S]*<\/noscript>/);
  assert.match(code(read('src/page.js')), /inert\.hidden\s*=\s*true/);
  assert.match(read('src/page.css'), /\[hidden\]\s*\{\s*display:\s*none\s*!important/);
});

test('km_21 the page uses no storage, clipboard, download, window, timer or sender', () => {
  const page = code(read('src/page.js'));
  const banned = [
    /localStorage|sessionStorage|indexedDB|caches\b|cookie/,
    /clipboard|execCommand|ClipboardEvent|\bcopy\b|\bcut\b/,
    /\bBlob\b|createObjectURL|download|FileReader|showSaveFilePicker/,
    /window\.open|\.open\s*\(|location\s*=|location\.|history\./,
    /setTimeout|setInterval|requestAnimationFrame|requestIdleCallback|queueMicrotask/,
    /fetch|XMLHttpRequest|WebSocket|EventSource|sendBeacon|postMessage|Worker/,
    /\bprint\b|document\.write|console\./,
  ];
  for (const re of banned) assert.doesNotMatch(page, re, `${re}`);
});

test('km_21 Clear and close wipes what the page holds and replaces the body with the cleared text', () => {
  const page = code(read('src/page.js'));
  const clear = page.slice(page.indexOf('function clearAndClose'), page.indexOf('onKey = (e)'));
  assert.match(clear, /wipeState\(state\)/);
  assert.match(clear, /state = null/);
  assert.match(clear, /removeEventListener/);
  assert.match(clear, /input\.value = ''/);
  assert.match(clear, /doc\.body\.replaceChildren\([\s\S]*WORDS\.cleared/);
  const wipe = page.slice(page.indexOf('function wipeState'), page.indexOf('function newState'));
  for (const field of ['flips', 'entropy', 'wordList', 'positions', 'result']) {
    assert.match(wipe, new RegExp(`s\\.${field}\\b`));
  }
  assert.match(wipe, /\.fill\(0\)/);
  assert.match(wipe, /\.fill\(''\)/);
});
