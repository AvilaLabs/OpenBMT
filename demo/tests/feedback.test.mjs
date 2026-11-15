// The sounds' levels and the logo's snap, ported from FeedbackModel.kt: node --test demo/tests/feedback.test.mjs
// The Kotlin side's own numbers are checked against the source text, so a change there fails here.
import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import { createRequire } from 'node:module';

const F = createRequire(import.meta.url)('../ui/feedback.js');
const kt = fs.readFileSync(new URL('../../apps/android-common/src/model/FeedbackModel.kt', import.meta.url), 'utf8');

test('the five sounds are the app\'s, at the app\'s levels', () => {
  for (const [name, sound] of Object.entries(F.Sound)) {
    const m = new RegExp(`${name}\\("([a-z_0-9]+)", ([0-9.]+)f\\)`).exec(kt);
    assert.ok(m, `${name} is in FeedbackModel.kt`);
    assert.equal(sound.file, m[1]);
    assert.equal(sound.level, Number(m[2]));
  }
  assert.equal(F.SoundLevels.MASTER, Number(/MASTER = ([0-9.]+)f/.exec(kt)[1]));
  assert.ok(Math.abs(F.volume(F.Sound.Confirmation) - 0.6) < 1e-6);
  assert.ok(Math.abs(F.volume(F.Sound.Tick) - 0.24) < 1e-6);
});

test('a sale\'s outcome: green plays the confirmation, red the error, the others nothing', () => {
  assert.equal(F.Outcomes.of('Green'), F.Sound.Confirmation);
  assert.equal(F.Outcomes.of('Red'), F.Sound.Error);
  assert.equal(F.Outcomes.of('Blue'), null);
  assert.equal(F.Outcomes.of('Neutral'), null);
});

test('the snap\'s numbers are the Kotlin ones', () => {
  const T = F.SnapTiming;
  assert.equal(T.GAP, Number(/const val GAP = ([0-9.]+)/.exec(kt)[1]));
  assert.equal(T.OVERSHOOT, Number(/const val OVERSHOOT = ([0-9.]+)\n    const val IMPACT/.exec(kt)[1]));
  assert.equal(T.IMPACT, Number(/const val IMPACT = ([0-9.]+)/.exec(kt)[1]));
  for (const p of T.pieces) {
    const m = new RegExp(`LogoPiece\\("${p.name}", (\\d+), (\\d+), (\\d+), (\\d+)\\)`).exec(kt);
    assert.deepEqual([p.x, p.y, p.w, p.h], m.slice(1).map(Number));
  }
});

test('the springs settle, the bars land in order and the cues follow them', () => {
  const T = F.SnapTiming;
  assert.ok(T.topStart > 0 && T.bottomStart > T.topStart);
  assert.ok(T.topLands > T.topStart && T.impact > T.bottomStart && T.wordStart > T.impact);
  assert.ok(T.total > T.wordStart && T.total < 3, `the snap lasts ${T.total} s`);
  const times = T.cues.map((c) => c.atSeconds);
  assert.deepEqual(times, [...times].sort((a, b) => a - b));
  assert.deepEqual(T.cues.map((c) => c.sound), [F.Sound.Tick, F.Sound.Tick, F.Sound.Drop, F.Sound.Maximize]);
});

test('at the start only the mark is faint; at the end every piece is in place', () => {
  const T = F.SnapTiming;
  const first = T.frame(0);
  assert.equal(first.b.alpha, 0);
  assert.equal(first.word.alpha, 0);
  assert.ok(first.top.dy < 0 && first.bottom.dy > 0, 'the bars start away from the tile');
  for (const f of [T.end(), T.frame(T.total + 5)]) {
    assert.equal(f.b.alpha, 1);
    assert.equal(f.word.alpha, 1);
    // (+ 0 turns the minus zero of a bar that has arrived into zero)
    assert.equal(f.top.dy + 0, 0);
    assert.equal(f.bottom.dy + 0, 0);
    assert.equal(f.word.dy + 0, 0);
    assert.equal(f.tileDy, 0);
    assert.equal(f.tileScale, 1);
    assert.equal(f.topInner, 0);
    assert.equal(f.bInnerSx, 1);
  }
});

test('the tile jolts once, at the impact', () => {
  const T = F.SnapTiming;
  assert.equal(T.frame(T.impact - 0.01).tileDy, 0);
  assert.ok(T.frame(T.impact + 0.04).tileDy > 0);
  assert.ok(T.frame(T.impact + 0.04).tileScale < 1);
  assert.ok(T.frame(T.impact + 0.31).tileDy === 0);
});

test('the easing and the spring are what CSS and the bench page give', () => {
  const ease = new F.Bezier(0.25, 0.1, 0.25, 1.0);
  assert.equal(ease.at(0), 0);
  assert.equal(ease.at(1), 1);
  assert.ok(Math.abs(ease.at(0.5) - 0.8024) < 0.001, 'CSS ease at the middle');
  const s = F.Spring.make(0.12, 2 * Math.PI * 2.6);
  assert.equal(s.at(-1), 0);
  assert.equal(s.at(10), 1);
  assert.ok(Math.max(...s.xs) > 1.05 && Math.max(...s.xs) < 1.2, 'an overshoot near 0.12');
});
