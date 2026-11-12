// KM-23: the built file runs in a real browser, offline, through every step.
// Local only: it needs a Chromium-family browser and bwrap, and it is off
// unless KEYMAKER_BROWSER=1 (CI does not set it). Run it under the memory
// guard:  KEYMAKER_BROWSER=1 npm test   (after `node build.mjs`)
//
// The browser runs inside `bwrap --unshare-net` (no network at all) and is
// driven over the DevTools protocol on a pipe, so no port is opened.
// KEYMAKER_BROWSER_BIN names the browser binary (the real one, not a launcher
// script), KEYMAKER_SHOTS the folder for the screenshots.
import assert from 'node:assert/strict';
import { mkdirSync, writeFileSync } from 'node:fs';
import { homedir } from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';
import { wordsToWallet } from '../src/core.js';
import { darkRuns, encodeBytes } from '../src/qr.js';
import { WORDS } from '../src/words.js';
import { launch, openPage } from './browser.mjs';

const enabled = process.env.KEYMAKER_BROWSER === '1';
const browserBin = process.env.KEYMAKER_BROWSER_BIN || '/opt/brave.com/brave/brave';
const shots = process.env.KEYMAKER_SHOTS || path.join(homedir(), '.cache/openbmt/tmp/ui/keymaker');
const profileRoot = process.env.KEYMAKER_PROFILE || path.join(homedir(), '.cache/openbmt/tmp/keymaker/profile');
const file = path.join(path.dirname(fileURLToPath(import.meta.url)), '../dist/openbmt-keymaker.html');

const seedWords =
  'primary fetch primary fetch primary fetch primary fetch primary fetch primary fever';
const seedAddress = 'bc1qyjassxtmzl02s8de8jw9365qmuvwg74udtp3vj';

const J = JSON.stringify;

async function session(width, height, run) {
  const profile = `${profileRoot}-${width}`;
  const b = launch({ browser: browserBin, profile, width, height });
  try {
    const page = await openPage(b, { width, height });
    await run(page);
    assert.deepEqual(page.errors, [], 'no errors in the page');
  } finally {
    try {
      await Promise.race([b.send('Browser.close'), new Promise((r) => setTimeout(r, 3000))]);
    } catch {
      // the browser is already gone
    }
    const gone = await Promise.race([b.exited, new Promise((r) => setTimeout(() => r('late'), 5000))]);
    if (gone === 'late') b.child.kill();
  }
}

function helpers(page, width) {
  mkdirSync(shots, { recursive: true });
  let n = 0;
  return {
    async shot(name) {
      n += 1;
      const png = await page.shot();
      writeFileSync(path.join(shots, `${width}-${String(n).padStart(2, '0')}-${name}.png`), png);
    },
    click: (label) =>
      page.eval(`(() => {
        const b = [...document.querySelectorAll('button')].find((x) => x.textContent === ${J(label)});
        if (!b) return false;
        b.click();
        return true;
      })()`),
    has: (label) =>
      page.eval(`[...document.querySelectorAll('button')].some((x) => x.textContent === ${J(label)})`),
    text: () => page.eval('document.body.innerText'),
    heading: () => page.eval("document.querySelector('h1') && document.querySelector('h1').textContent"),
    step: () => page.eval("document.querySelector('.step') && document.querySelector('.step').textContent"),
    banner: () => page.eval("!!document.querySelector('.banner')"),
    words: () => page.eval("[...document.querySelectorAll('.words .w')].map((x) => x.textContent)"),
    count: () => page.eval("document.querySelector('.count').textContent"),
    qrBox: () =>
      page.eval(`(() => {
        const r = document.querySelector('.qr svg').getBoundingClientRect();
        return { top: r.top, bottom: r.bottom, left: r.left, right: r.right,
          w: window.innerWidth, h: window.innerHeight };
      })()`),
    async qrInside() {
      const b = await this.qrBox();
      assert.ok(b.top >= 0 && b.left >= 0 && b.bottom <= b.h && b.right <= b.w, JSON.stringify(b));
    },
    async noOverflow() {
      assert.equal(await page.eval('document.documentElement.scrollWidth <= window.innerWidth'), true);
    },
    /** `right` is true or false for all three boxes, or one flag per box. */
    async answer(words, right) {
      const labels = await page.eval("[...document.querySelectorAll('label.field span')].map((x) => x.textContent)");
      assert.equal(labels.length, 3);
      const pos = labels.map((l) => Number(l.match(/\d+/)[0]));
      assert.equal(new Set(pos).size, 3);
      assert.deepEqual(pos, [...pos].sort((a, b) => a - b));
      const typed = pos.map((p, k) =>
        (Array.isArray(right) ? right[k] : right) ? words[p - 1].slice(0, 4).toUpperCase() : 'zzzz',
      );
      await page.eval(`(() => {
        const t = ${J(typed)};
        [...document.querySelectorAll('input.prefix')].forEach((i, k) => { i.value = t[k]; });
        document.querySelector('form.check').requestSubmit();
      })()`);
      return pos;
    },
    async phone(words, expectFingerprint, expectAddress) {
      const wallet = wordsToWallet(words.join(' '));
      const fp = await page.eval("document.querySelector('.fp').textContent");
      const address = await page.eval("[...document.querySelectorAll('.address .g')].map((x) => x.textContent)");
      assert.ok(address.every((g) => g.length <= 4));
      assert.equal(fp, wallet.fingerprint);
      assert.equal(address.join(''), wallet.address);
      if (expectFingerprint) assert.equal(fp, expectFingerprint);
      if (expectAddress) assert.equal(address.join(''), expectAddress);
      // The picture is the code of the multipath description.
      const code = encodeBytes(new TextEncoder().encode(wallet.descriptor));
      const runs = darkRuns(code);
      const drawn = await page.eval(`(() => {
        const svg = document.querySelector('.qr svg');
        const rects = [...svg.querySelectorAll('rect')].slice(1);
        return {
          width: Number(svg.getAttribute('width')),
          total: Number(svg.getAttribute('viewBox').split(' ')[2]),
          rects: rects.map((r) => [+r.getAttribute('x'), +r.getAttribute('y'), +r.getAttribute('width')]),
          ns: svg.namespaceURI === document.querySelector('.qr svg rect').namespaceURI,
        };
      })()`);
      assert.equal(drawn.total, code.size + 8);
      assert.ok(drawn.width / drawn.total >= 4);
      assert.equal(drawn.ns, true);
      assert.deepEqual(
        drawn.rects,
        runs.map(([x, y, len]) => [x + 4, y + 4, len]),
      );
    },
  };
}

async function realRun(page, h) {
  assert.equal(await h.click(WORDS.realButton), true);
  assert.equal(await h.banner(), false);
  assert.equal(await h.heading(), WORDS.methodHeading);
  assert.equal(await h.click(WORDS.methodCoin), true);
  assert.equal(await h.heading(), WORDS.flipHeading);
  assert.equal(await h.has(WORDS.rehearsalFill), false, 'no fill outside the rehearsal');
  assert.equal(await h.count(), '0 of 128 bits');
  // The test seed: the bits 1, 0, 1, ... as pairs by the keyboard (a dropped
  // pair first, then tails-heads for a 1 and heads-tails for a 0).
  await page.key('h');
  await page.key('h');
  for (let i = 0; i < 128; i++) {
    await page.key(i % 2 === 0 ? 't' : 'h');
    await page.key(i % 2 === 0 ? 'h' : 't');
  }
  assert.equal(await h.count(), '128 of 128 bits');
  await h.shot('flip-real-full');
  assert.equal(await h.click(WORDS.next), true);
  assert.deepEqual(await h.words(), seedWords.split(' '));
  assert.equal(await h.banner(), false);
  await h.shot('write-real');
  assert.equal(await h.click(WORDS.writeDone), true);
  await h.answer(seedWords.split(' '), true);
  assert.equal(await h.heading(), WORDS.phoneHeading);
  assert.equal(await h.step(), 'Step 5 of 7');
  await h.phone(seedWords.split(' '), '818bc9b6', seedAddress);
  await h.noOverflow();
  await h.shot('phone-real');
}

for (const [width, height] of [
  [1280, 800],
  [1024, 768],
]) {
  test(
    `km_23 the built page runs every step in a browser, offline (${width} x ${height})`,
    { skip: enabled ? false : 'set KEYMAKER_BROWSER=1 (local only, needs a browser and bwrap)' },
    async () => {
      await session(width, height, async (page) => {
        const h = helpers(page, width);
        await page.goto(`file://${file}`);
        // Step 0
        assert.equal(await page.eval("document.getElementById('inert').hidden"), true);
        assert.equal(await h.heading(), WORDS.title);
        assert.equal(await h.step(), 'Step 0 of 7');
        assert.equal(await h.banner(), false);
        await h.noOverflow();
        await h.shot('start');
        // The rehearsal, every step
        assert.equal(await h.click(WORDS.rehearseButton), true);
        assert.equal(await h.banner(), true);
        assert.equal(await h.step(), 'Step 2 of 7');
        assert.equal(await h.heading(), WORDS.methodHeading);
        await h.shot('method-rehearsal');
        assert.equal(await h.click(WORDS.methodCoin), true);
        assert.equal(await h.has(WORDS.rehearsalFill), true);
        await page.key('h');
        await page.key('t');
        await page.key('h');
        assert.equal(await h.count(), '1 of 128 bits');
        assert.equal(await h.click(WORDS.undo), true);
        assert.equal(await h.count(), '1 of 128 bits');
        assert.equal(await h.click(WORDS.undo), true);
        assert.equal(await h.count(), '0 of 128 bits');
        await page.key('h');
        await page.key('t');
        await h.shot('flip-rehearsal');
        assert.equal(await h.click(WORDS.rehearsalFill), true);
        assert.equal(await h.count(), '128 of 128 bits');
        await h.noOverflow();
        await h.shot('flip-rehearsal-full');
        assert.equal(await h.click(WORDS.next), true);
        assert.equal(await h.step(), 'Step 3 of 7');
        assert.equal(await h.banner(), true);
        const words = await h.words();
        assert.equal(words.length, 12);
        assert.equal(await page.eval("document.querySelector('details.hand').open"), false);
        await h.noOverflow();
        await h.shot('write-rehearsal');
        await page.eval("document.querySelector('details.hand').open = true");
        assert.equal(await page.eval("document.querySelectorAll('table.working tr').length"), 13);
        assert.equal(await page.eval("document.querySelectorAll('details.hand pre.cmd').length"), 1);
        await h.shot('write-rehearsal-hand-open');
        await page.eval("document.querySelector('details.hand').open = false");
        assert.equal(await h.click(WORDS.writeDone), true);
        assert.equal(await h.step(), 'Step 4 of 7');
        await h.shot('check-rehearsal');
        const pos = await h.answer(words, false);
        assert.ok((await h.text()).includes(WORDS.checkWrong.replace('{pos}', String(pos[0]))));
        assert.equal(await h.has(WORDS.checkBack), true);
        assert.equal(await h.heading(), WORDS.checkHeading);
        await h.shot('check-wrong');
        assert.equal(await h.click(WORDS.checkBack), true);
        assert.deepEqual(await h.words(), words);
        assert.equal(await h.click(WORDS.writeDone), true);
        assert.deepEqual(await h.answer(words, true), pos);
        assert.equal(await h.heading(), WORDS.phoneHeading);
        await h.phone(words);
        await h.noOverflow();
        await h.shot('phone-rehearsal');
        assert.equal(await h.click(WORDS.phoneDone), true);
        assert.equal(await h.step(), 'Step 6 of 7');
        await h.shot('electrum');
        assert.equal(await h.click(WORDS.electrumDone), true);
        assert.equal(await h.step(), 'Step 7 of 7');
        assert.ok((await h.text()).includes(WORDS.rehearsalDoneHeading));
        await h.shot('finish-rehearsal');
        // For real, with the test seed
        await realRun(page, h);
        assert.equal(await h.click(WORDS.phoneDone), true);
        assert.equal(await h.click(WORDS.electrumDone), true);
        assert.equal(await h.heading(), WORDS.finishHeading);
        assert.equal(await h.has(WORDS.clearButton), true);
        await h.shot('finish-real');
        assert.equal(await h.click(WORDS.clearButton), true);
        assert.equal((await h.text()).trim(), WORDS.cleared);
        assert.equal(await page.eval("document.querySelectorAll('input, button').length"), 0);
        await h.shot('cleared');

        // Electrum shows something different: stop, and only clear is offered.
        await page.goto(`file://${file}`);
        assert.equal(await h.click(WORDS.rehearseButton), true);
        assert.equal(await h.click(WORDS.methodCoin), true);
        assert.equal(await h.click(WORDS.rehearsalFill), true);
        assert.equal(await h.click(WORDS.next), true);
        const w2 = await h.words();
        assert.equal(await h.click(WORDS.writeDone), true);
        await h.answer(w2, true);
        assert.equal(await h.click(WORDS.phoneDone), true);
        assert.equal(await h.click(WORDS.electrumDifferent), true);
        assert.ok((await h.text()).includes(WORDS.electrumStop));
        assert.equal(await h.has(WORDS.electrumDone), false);
        assert.equal(await h.has(WORDS.electrumDifferent), false);
        assert.equal(await page.eval("document.querySelectorAll('button').length"), 1);
        await h.shot('electrum-stop');
        assert.equal(await h.click(WORDS.clearButton), true);
        assert.equal((await h.text()).trim(), WORDS.cleared);
      });
    },
  );
}

test(
  'km_24 step 6 lists the Electrum steps beside the fingerprint and address; the whole QR is on screen',
  { skip: enabled ? false : 'set KEYMAKER_BROWSER=1 (local only, needs a browser and bwrap)' },
  async () => {
    for (const [width, height] of [
      [1280, 800],
      [1024, 768],
    ]) {
      await session(width, height, async (page) => {
        const h = helpers(page, `km24-${width}`);
        await page.goto(`file://${file}`);
        // Rehearsal: the banner shares the badge row and the QR is whole.
        assert.equal(await h.click(WORDS.rehearseButton), true);
        assert.equal(await h.click(WORDS.methodCoin), true);
        assert.equal(await page.eval("document.querySelector('.banner').closest('header.top') !== null"), true);
        assert.equal(await h.click(WORDS.rehearsalFill), true);
        assert.equal(await h.click(WORDS.next), true);
        const words = await h.words();
        assert.equal(await h.click(WORDS.writeDone), true);
        await h.answer(words, true);
        await h.qrInside();
        const fp = await page.eval("document.querySelector('.fp').textContent");
        const addr = await page.eval("[...document.querySelectorAll('.address .g')].map((x) => x.textContent).join('')");
        await h.shot('phone-rehearsal-fit');
        assert.equal(await h.click(WORDS.phoneDone), true);
        // Step 6: eight steps, and the same fingerprint and address as step 5.
        assert.equal(await h.step(), 'Step 6 of 7');
        assert.deepEqual(
          await page.eval("[...document.querySelectorAll('ol.steps li')].map((x) => x.textContent)"),
          WORDS.electrumSteps,
        );
        assert.equal(WORDS.electrumSteps.length, 8);
        assert.equal(await page.eval("document.querySelector('.fp').textContent"), fp);
        assert.equal(
          await page.eval("[...document.querySelectorAll('.address .g')].map((x) => x.textContent).join('')"),
          addr,
        );
        const t = await h.text();
        assert.ok(t.includes(WORDS.electrumText) && t.includes(WORDS.electrumClose));
        await h.shot('electrum-steps');
        // For real: no banner, whole QR.
        assert.equal(await h.click(WORDS.electrumDone), true);
        await realRun(page, h);
        await h.qrInside();
      });
    }
  },
);

test(
  'km_16 a wrong card answer clears only the wrong boxes and focuses the first of them',
  { skip: enabled ? false : 'set KEYMAKER_BROWSER=1 (local only, needs a browser and bwrap)' },
  async () => {
    await session(1280, 800, async (page) => {
      const h = helpers(page, 'km16');
      await page.goto(`file://${file}`);
      assert.equal(await h.click(WORDS.rehearseButton), true);
      assert.equal(await h.click(WORDS.methodCoin), true);
      assert.equal(await h.click(WORDS.rehearsalFill), true);
      assert.equal(await h.click(WORDS.next), true);
      const words = await h.words();
      assert.equal(await h.click(WORDS.writeDone), true);
      const state = () =>
        page.eval(`({
          values: [...document.querySelectorAll('input.prefix')].map((i) => i.value),
          focus: [...document.querySelectorAll('input.prefix')].findIndex((i) => i === document.activeElement),
        })`);
      // The middle and last boxes wrong: the first stays, focus on the middle.
      const pos = await h.answer(words, [true, false, false]);
      let s = await state();
      assert.deepEqual(s.values, [words[pos[0] - 1].slice(0, 4).toUpperCase(), '', '']);
      assert.equal(s.focus, 1);
      assert.ok((await h.text()).includes(WORDS.checkWrong.replace('{pos}', String(pos[1]))));
      // Only the first wrong: the last right box is kept, focus on the first.
      await h.answer(words, [false, true, true]);
      s = await state();
      assert.deepEqual(s.values, ['', words[pos[1] - 1].slice(0, 4).toUpperCase(), words[pos[2] - 1].slice(0, 4).toUpperCase()]);
      assert.equal(s.focus, 0);
      // All three right: on to step 5.
      await h.answer(words, true);
      assert.equal(await h.heading(), WORDS.phoneHeading);
    });
  },
);

test(
  'km_33 the die: the reference rolls, by the keyboard, give the reference words; check by hand shows the digits',
  { skip: enabled ? false : 'set KEYMAKER_BROWSER=1 (local only, needs a browser and bwrap)' },
  async () => {
    const rolls =
      '136235425512124432523456456652532431651643314361145162145265365641143532531541146614461364126324256216214231126325524541265326436416634631461245356352';
    const words = 'dance summer quarter guitar magic arm response novel toe service light enough'.split(' ');
    await session(1280, 800, async (page) => {
      const h = helpers(page, 'km33');
      await page.goto(`file://${file}`);
      assert.equal(await h.click(WORDS.realButton), true);
      assert.equal(await h.click(WORDS.methodDie), true);
      assert.equal(await h.heading(), WORDS.dieHeading);
      assert.equal(await h.count(), '0 of 50 values');
      for (const n of rolls.slice(0, 5)) await page.key(n);
      assert.equal(await h.count(), '1 of 50 values');
      assert.equal(await h.click(WORDS.dieUndo), true);
      assert.equal(await h.click(WORDS.dieUndo), true);
      assert.equal(await h.count(), '1 of 50 values');
      for (const n of rolls.slice(3)) await page.key(n);
      assert.equal(await h.count(), '50 of 50 values');
      await h.shot('die-full');
      assert.equal(await h.click(WORDS.next), true);
      assert.deepEqual(await h.words(), words);
      await page.eval("document.querySelector('details.hand').open = true");
      assert.deepEqual(
        await page.eval("[...document.querySelectorAll('details.hand pre.cmd')].map((x) => x.textContent)"),
        [
          '11651451144444631212242444153216166316542666543113',
          'echo -n "11651451144444631212242444153216166316542666543113" | sha256sum',
          '375b26bdb3e85c176decb8e398860625',
        ],
      );
      await h.shot('die-hand-open');
    });
  },
);
