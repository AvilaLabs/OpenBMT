// The page's words, its check-card positions and its inert notice.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import { sha256 } from '@noble/hashes/sha2.js';
import { flipsToBits, flipsToEntropy, fromCoin, fromDie, rollsToValues } from '../src/core.js';
import { cardPrefix, checkPositions, fill, madeUpFlips, madeUpRolls, start } from '../src/page.js';
import { fakeDocument } from './fakedom.js';
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
  assert.equal(fill(WORDS.flipCount, { n: 37, total: 128 }), '37 of 128 bits');
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

test('km_16 rehearsal flips keep 128 bits by the pair rule and repeat for the same start', () => {
  const a = madeUpFlips([]);
  assert.ok(a.every((f) => f === 0 || f === 1));
  assert.equal(flipsToBits(a).length, 128);
  assert.deepEqual(madeUpFlips([]), a);
  const b = madeUpFlips([1, 0, 1]);
  assert.deepEqual(b.slice(0, 3), [1, 0, 1]);
  assert.equal(flipsToBits(b).length, 128);
  assert.notDeepEqual(a, b);
  assert.doesNotThrow(() => fromCoin(a));
});

test('km_16 rehearsal rolls make 50 values by the order-of-three rule and repeat for the same start', () => {
  const a = madeUpRolls([]);
  assert.ok(a.every((r) => Number.isInteger(r) && r >= 1 && r <= 6));
  assert.equal(rollsToValues(a).length, 50);
  assert.deepEqual(madeUpRolls([]), a);
  const b = madeUpRolls([6, 1]);
  assert.deepEqual(b.slice(0, 2), [6, 1]);
  assert.equal(rollsToValues(b).length, 50);
  assert.notDeepEqual(a, b);
  assert.doesNotThrow(() => fromDie(a));
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
  for (const field of ['flips', 'rolls', 'bits', 'values', 'digits', 'entropy', 'wordList', 'positions', 'result']) {
    assert.match(wipe, new RegExp(`s\\.${field}\\b`));
  }
  assert.match(wipe, /\.fill\(0\)/);
  assert.match(wipe, /\.fill\(''\)/);
});

function begin(label) {
  const d = fakeDocument();
  start(d.doc);
  d.click(label);
  return d;
}

test('km_33 after Start for real or Rehearse first comes the choice, then the coin or die screen', () => {
  for (const [label, rehearsal] of [
    [WORDS.realButton, false],
    [WORDS.rehearseButton, true],
  ]) {
    const d = begin(label);
    assert.equal(d.heading(), WORDS.methodHeading);
    const t = d.text();
    for (const k of ['methodText', 'methodCoinNote', 'methodDieNote']) assert.ok(t.includes(WORDS[k]), k);
    assert.equal(d.button(WORDS.heads), null);
    assert.equal(d.button(WORDS.rehearsalFill), null);
    // The coin.
    d.click(WORDS.methodCoin);
    assert.equal(d.heading(), WORDS.flipHeading);
    assert.ok(d.button(WORDS.heads) && d.button(WORDS.tails));
    assert.equal(d.button(WORDS.rehearsalFill) !== null, rehearsal);
    assert.ok(d.text().includes(WORDS.flipPairs));
    // The die, from a fresh choice.
    const e = begin(label);
    e.click(WORDS.methodDie);
    assert.equal(e.heading(), WORDS.dieHeading);
    for (const n of '123456') assert.ok(e.button(n), n);
    assert.equal(e.button(WORDS.heads), null);
    assert.equal(e.button(WORDS.rehearsalFill) !== null, rehearsal);
    for (const k of ['dieTriples', 'dieOrders']) assert.ok(e.text().includes(WORDS[k]), k);
  }
});

test('km_29 the coin screen: buttons and keys H and T, kept bits and flips so far', () => {
  const d = begin(WORDS.realButton);
  d.click(WORDS.methodCoin);
  assert.ok(d.text().includes(fill(WORDS.flipCount, { n: 0, total: 128 })));
  d.click(WORDS.heads); // an open pair
  assert.ok(d.text().includes(fill(WORDS.flipCount, { n: 0, total: 128 })));
  assert.ok(d.text().includes(fill(WORDS.flipsMade, { n: 1 })));
  d.press('T'); // heads then tails: bit 0
  assert.ok(d.text().includes(fill(WORDS.flipCount, { n: 1, total: 128 })));
  d.press('h');
  d.press('h'); // heads-heads: dropped
  assert.ok(d.text().includes(fill(WORDS.flipCount, { n: 1, total: 128 })));
  assert.ok(d.text().includes(fill(WORDS.flipsMade, { n: 4 })));
  d.press('t');
  d.press('h'); // tails then heads: bit 1
  assert.deepEqual(d.cells().slice(0, 3), ['0', '1', '']);
  d.press('1'); // not a coin key
  assert.ok(d.text().includes(fill(WORDS.flipsMade, { n: 6 })));
});

test('km_32 undo removes the last flip, from an open pair or a kept bit, and the bit it formed', () => {
  const d = begin(WORDS.realButton);
  d.click(WORDS.methodCoin);
  assert.equal(d.button(WORDS.undo).disabled, true);
  d.press('h');
  d.press('t'); // bit 0
  d.press('t'); // open pair
  d.click(WORDS.undo); // from the open pair
  assert.ok(d.text().includes(fill(WORDS.flipsMade, { n: 2 })));
  assert.ok(d.text().includes(fill(WORDS.flipCount, { n: 1, total: 128 })));
  d.click(WORDS.undo); // takes the tails: the open pair is heads alone, no bit
  assert.ok(d.text().includes(fill(WORDS.flipsMade, { n: 1 })));
  assert.ok(d.text().includes(fill(WORDS.flipCount, { n: 0, total: 128 })));
  assert.deepEqual(d.cells()[0], '');
  d.press('h'); // heads-heads: dropped pair
  d.click(WORDS.undo);
  d.press('t'); // the pair is heads then tails again
  assert.deepEqual(d.cells()[0], '0');
  d.click(WORDS.undo);
  d.click(WORDS.undo);
  assert.equal(d.button(WORDS.undo).disabled, true);
});

test('km_32 undo on the die screen removes the last roll and the value of its three', () => {
  const d = begin(WORDS.realButton);
  d.click(WORDS.methodDie);
  assert.equal(d.button(WORDS.dieUndo).disabled, true);
  d.press('1');
  d.press('3');
  assert.ok(d.text().includes(fill(WORDS.dieRollsMade, { n: 2 })));
  d.press('6'); // 1 3 6 gives 1
  assert.deepEqual(d.cells()[0], '1');
  assert.ok(d.text().includes(fill(WORDS.dieCount, { n: 1, total: 50 })));
  d.click(WORDS.dieUndo); // from the value back to an open three
  assert.deepEqual(d.cells()[0], '');
  assert.ok(d.text().includes(fill(WORDS.dieCount, { n: 0, total: 50 })));
  assert.ok(d.text().includes(fill(WORDS.dieRollsMade, { n: 2 })));
  d.click(WORDS.dieUndo); // from the open three
  d.click(WORDS.dieUndo);
  assert.ok(d.text().includes(fill(WORDS.dieRollsMade, { n: 0 })));
  assert.equal(d.button(WORDS.dieUndo).disabled, true);
  d.press('7'); // not a die key
  d.press('0');
  assert.ok(d.text().includes(fill(WORDS.dieRollsMade, { n: 0 })));
  d.click('2');
  assert.ok(d.text().includes(fill(WORDS.dieRollsMade, { n: 1 })));
});

test('km_17 the rehearsal fill makes made-up results of the chosen kind and goes on to the words', () => {
  const coin = begin(WORDS.rehearseButton);
  coin.click(WORDS.methodCoin);
  coin.click(WORDS.rehearsalFill);
  assert.ok(coin.text().includes(WORDS.flipsDone));
  assert.ok(coin.text().includes(fill(WORDS.flipCount, { n: 128, total: 128 })));
  coin.click(WORDS.next);
  assert.equal(coin.heading(), WORDS.writeHeading);
  const hand = coin.root.all((n) => n.tag === 'details')[0].textContent;
  assert.ok(hand.includes(WORDS.handCheckCommand));
  assert.match(hand, /printf '%s' '[01]{128}' \| basenc --base2msbf --decode \| sha256sum/);
  assert.doesNotMatch(hand, /echo -n/);
  const die = begin(WORDS.rehearseButton);
  die.click(WORDS.methodDie);
  die.click(WORDS.rehearsalFill);
  assert.ok(die.text().includes(WORDS.dieDone));
  assert.ok(die.text().includes(fill(WORDS.dieCount, { n: 50, total: 50 })));
  die.click(WORDS.next);
  assert.equal(die.heading(), WORDS.writeHeading);
});

test('km_34 the hand check: coin lines and groups, die values, line, hash and groups', () => {
  const coin = begin(WORDS.realButton);
  coin.click(WORDS.methodCoin);
  for (let i = 0; i < 128; i++) {
    coin.press(i % 2 === 0 ? 'h' : 't');
    coin.press(i % 2 === 0 ? 't' : 'h');
  }
  coin.click(WORDS.next);
  const hand = coin.root.all((n) => n.tag === 'details')[0];
  const t = hand.textContent;
  assert.ok(t.includes(WORDS.handText) && t.includes(WORDS.handLast));
  assert.equal(hand.all((n) => n.tag === 'tr').length, 13);
  const blocks = hand.all((n) => n.tag === 'pre').map((n) => n.textContent);
  const bits = [...Array(128).keys()].map((i) => (i % 2 === 0 ? 0 : 1)).join('');
  assert.deepEqual(blocks, [`printf '%s' '${bits}' | basenc --base2msbf --decode | sha256sum`]);
  // The die: a run of the reference's triplets.
  const triplets =
    '136 235 425 512 124 432 523 456 456 652 532 431 651 643 314 361 145 162 145 265 365 641 143 532 531 541 146 614 461 364 126 324 256 216 214 231 126 325 524 541 265 326 436 416 634 631 461 245 356 352';
  const die = begin(WORDS.realButton);
  die.click(WORDS.methodDie);
  for (const n of triplets.replaceAll(' ', '')) die.press(n);
  assert.ok(die.text().includes(fill(WORDS.dieCount, { n: 50, total: 50 })));
  die.click(WORDS.next);
  const dh = die.root.all((n) => n.tag === 'details')[0];
  const dt = dh.textContent;
  for (const k of ['handDieText', 'handDieCommand', 'handDieValues', 'handDieHash', 'handText', 'handLast']) {
    assert.ok(dt.includes(WORDS[k]), k);
  }
  assert.equal(dh.all((n) => n.tag === 'tr').length, 13);
  assert.deepEqual(
    dh.all((n) => n.tag === 'pre').map((n) => n.textContent),
    [
      '11651451144444631212242444153216166316542666543113',
      'echo -n "11651451144444631212242444153216166316542666543113" | sha256sum',
      '375b26bdb3e85c176decb8e398860625',
    ],
  );
  assert.deepEqual(
    die.root.all((n) => (n.attrs.class || '') === 'w').map((n) => n.textContent),
    'dance summer quarter guitar magic arm response novel toe service light enough'.split(' '),
  );
  assert.doesNotMatch(dt, /printf/);
});

// The picture on step 5 is drawn through the browser's parser; a stand-in
// gives the page an element to hang the rectangles on.
globalThis.DOMParser = class {
  parseFromString() {
    const svg = fakeDocument().doc.createElement('svg');
    svg.namespaceURI = 'ns';
    return { querySelector: () => svg };
  }
};

/** Through the write and check screens with the right answers. */
function passCheck(d) {
  const words = d.root.all((n) => (n.attrs.class || '') === 'w').map((n) => n.textContent);
  d.click(WORDS.writeDone);
  const labels = d.root.all((n) => n.tag === 'label');
  const inputs = d.root.all((n) => n.tag === 'input');
  labels.forEach((l, i) => {
    inputs[i].value = words[Number(l.textContent.match(/\d+/)[0]) - 1].slice(0, 4);
  });
  const form = d.root.all((n) => n.tag === 'form')[0];
  for (const fn of form.listeners.submit) fn({ preventDefault() {} });
  return words;
}

test('km_21 Clear and close, after a die run or a coin run, empties the page and drops the key handler', () => {
  for (const method of [WORDS.methodDie, WORDS.methodCoin]) {
    const d = begin(WORDS.rehearseButton);
    d.click(method);
    d.click(WORDS.rehearsalFill);
    d.click(WORDS.next);
    passCheck(d);
    assert.equal(d.heading(), WORDS.phoneHeading);
    d.click(WORDS.phoneDone);
    d.click(WORDS.electrumDone);
    assert.equal(d.keyCount(), 1);
    d.click(WORDS.clearButton);
    assert.equal(d.doc.body.textContent, WORDS.cleared);
    assert.equal(d.keyCount(), 0);
  }
});
