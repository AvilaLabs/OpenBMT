// KM-20: the QR port gives the same code as the Rust encoder.
import assert from 'node:assert/strict';
import { readFileSync, writeFileSync } from 'node:fs';
import test from 'node:test';
import { bytesToHex, hexToBytes } from '@noble/hashes/utils.js';
import { darkRuns, encodeBytes, toLuma } from '../src/qr.js';
import { boundaryLengths, inputsText, qrInputs } from './qr-inputs.js';

const vec = (name) => new URL(`vectors/${name}`, import.meta.url);
const writing = process.env.OPENBMT_WRITE_QR_FIXTURES === '1';

const PICTURES = [
  ['one-byte', 24],
  ['seed-receive', 1],
  ['seed-multipath', 0],
];

function pgm(code) {
  const { width, height, pixels } = toLuma(code, 6);
  return Buffer.concat([Buffer.from(`P5\n${width} ${height}\n255\n`), Buffer.from(pixels)]);
}

function parseFixtures() {
  const out = [];
  let cur = null;
  for (const line of readFileSync(vec('qr-fixtures.txt'), 'utf8').split('\n')) {
    if (line.startsWith('input ')) {
      const hex = line.slice(6);
      cur = { bytes: hex === '-' ? new Uint8Array(0) : hexToBytes(hex), rows: [] };
    } else if (line.startsWith('version ')) cur.version = Number(line.slice(8));
    else if (line.startsWith('mask ')) cur.mask = Number(line.slice(5));
    else if (line === 'end') out.push(cur);
    else if (line !== '') cur.rows.push(line);
  }
  return out;
}

test('km_20 the inputs file lists what the core makes, at least 30 of them', () => {
  const inputs = qrInputs();
  assert.ok(inputs.length >= 30);
  if (writing) writeFileSync(vec('qr-inputs.txt'), inputsText(inputs));
  assert.equal(readFileSync(vec('qr-inputs.txt'), 'utf8'), inputsText(inputs));
});

test('km_20 every matrix equals the Rust encoder (version, mask, modules)', () => {
  const inputs = qrInputs();
  const fixtures = parseFixtures();
  assert.equal(fixtures.length, inputs.length);
  for (let n = 0; n < inputs.length; n++) {
    const f = fixtures[n];
    assert.equal(bytesToHex(f.bytes), bytesToHex(inputs[n]), `input ${n}`);
    const code = encodeBytes(inputs[n]);
    assert.equal(code.version, f.version, `input ${n} version`);
    assert.equal(code.mask, f.mask, `input ${n} mask`);
    const rows = code.modules.map((r) => r.map((m) => (m ? '1' : '0')).join(''));
    assert.deepEqual(rows, f.rows, `input ${n} matrix`);
  }
});

test('km_20 the fixtures cover each version boundary up to version 10', () => {
  const fixtures = parseFixtures();
  const versions = new Set();
  boundaryLengths().forEach((cap, i) => {
    const at = fixtures.find((f) => f.bytes.length === cap);
    const over = fixtures.find((f) => f.bytes.length === cap + 1);
    assert.equal(at.version, i + 1, `${cap} bytes`);
    assert.equal(over.version, i + 2, `${cap + 1} bytes`);
    versions.add(at.version);
  });
  assert.equal(versions.size, 10);
});

test('km_20 pictures drawn from the matrices are the PGM files the Rust reader decodes', () => {
  const inputs = qrInputs();
  for (const [name, n] of PICTURES) {
    const bytes = pgm(encodeBytes(inputs[n]));
    if (writing) writeFileSync(vec(`qr-${name}.pgm`), bytes);
    assert.ok(readFileSync(vec(`qr-${name}.pgm`)).equals(bytes), name);
  }
});

test('km_20 the drawing has a quiet zone of 4 and at least 6 pixels a module', () => {
  const code = encodeBytes(new Uint8Array([0x41]));
  const { width, pixels } = toLuma(code, 6);
  assert.equal(width, (21 + 8) * 6);
  assert.ok(pixels.slice(0, 4 * 6 * width).every((p) => p === 255));
  // The top-left finder corner is dark.
  assert.equal(pixels[4 * 6 * width + 4 * 6], 0);
  // The runs cover exactly the dark modules.
  let dark = 0;
  for (const [, , len] of darkRuns(code)) dark += len;
  assert.equal(dark, code.modules.flat().filter(Boolean).length);
});
