// The page's sound: nothing before the first tap, nothing when off, the app's levels, the switch remembered.
//   node --test demo/tests/sound.test.mjs
import test from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';

const require = createRequire(import.meta.url);
const F = require('../ui/feedback.js');
const { SoundBox, KEY } = require('../web2/sound.js');

/** A pretend audio context that records what plays. */
function audio() {
  const played = [];
  const fetched = [];
  const ctx = {
    state: 'running',
    decodeAudioData: async (data) => ({ data }),
    createGain() { return { gain: { value: 1 }, connect() {} }; },
    createBufferSource() {
      const src = { buffer: null, connect(g) { src.gain = g; }, start() { played.push({ buffer: src.buffer, volume: src.gain.gain.value }); } };
      return src;
    },
    destination: {},
  };
  return { played, fetched, env: {
    base: 'sounds/', Feedback: F,
    fetchFile: async (url) => { fetched.push(url); return url; },
    makeContext: () => ctx,
  } };
}
const store = (initial) => {
  const m = new Map(initial ? [[KEY, initial]] : []);
  return { getItem: (k) => (m.has(k) ? m.get(k) : null), setItem: (k, v) => m.set(k, v), m };
};
const settle = () => new Promise((r) => setTimeout(r, 5));

test('nothing plays before the first tap, and the files load only after it', async () => {
  const a = audio();
  const box = SoundBox({ ...a.env, storage: store() });
  assert.equal(box.play(F.Sound.Tick), false);
  assert.equal(a.fetched.length, 0);
  box.unlock();
  await settle();
  assert.equal(a.fetched.length, 5);
  assert.ok(a.fetched.includes('sounds/confirmation_002.ogg'));
});

test('after the first tap a sound plays at its level times the master volume', async () => {
  const a = audio();
  const box = SoundBox({ ...a.env, storage: store() });
  box.unlock();
  await settle();
  assert.equal(box.play(F.Sound.Drop), true);
  assert.ok(Math.abs(a.played[0].volume - 0.64) < 1e-6);
});

test('a sale\'s outcome plays green and red, and nothing for the other colours', async () => {
  const a = audio();
  const box = SoundBox({ ...a.env, storage: store() });
  box.unlock();
  await settle();
  assert.equal(box.outcome('Green'), true);
  assert.equal(box.outcome('Red'), true);
  assert.equal(box.outcome('Blue'), false);
  assert.equal(box.outcome('Neutral'), false);
  assert.equal(a.played.length, 2);
});

test('the switch silences it, and the choice is remembered', async () => {
  const a = audio();
  const s = store();
  const box = SoundBox({ ...a.env, storage: s });
  box.unlock();
  await settle();
  box.state.soundOn = false;
  assert.equal(s.m.get(KEY), 'off');
  assert.equal(box.play(F.Sound.Tick), false);
  assert.equal(SoundBox({ ...a.env, storage: s }).state.soundOn, false);
  box.state.soundOn = true;
  assert.equal(SoundBox({ ...a.env, storage: s }).state.soundOn, true);
  assert.equal(box.play(F.Sound.Tick), true);
});

test('storage that is off leaves the sound on, and a file that does not load stays silent', async () => {
  const a = audio();
  const broken = { getItem() { throw new Error('off'); }, setItem() { throw new Error('off'); } };
  const box = SoundBox({ ...a.env, storage: broken, fetchFile: async () => { throw new Error('gone'); } });
  assert.equal(box.state.soundOn, true);
  box.state.soundOn = false;
  box.state.soundOn = true;
  box.unlock();
  await settle();
  assert.equal(box.play(F.Sound.Tick), false);
});
