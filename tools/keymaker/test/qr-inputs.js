// The inputs of the QR fixtures: what the Rust encoder is asked to encode.
// Written to test/vectors/qr-inputs.txt (one hex line each, `-` for empty);
// the Rust test reads that file and writes qr-fixtures.txt.
import { readFileSync } from 'node:fs';
import { bytesToHex } from '@noble/hashes/utils.js';
import { fromFlips } from '../src/core.js';

// Data codewords at level M, versions 1 to 10 (ISO/IEC 18004, Table 7).
const M_DATA_CODEWORDS = [16, 28, 44, 64, 86, 108, 124, 154, 182, 216];

export const seedFlips = Array.from({ length: 128 }, (_, i) => (i % 2 === 0 ? 1 : 0));

function filler(n) {
  return Uint8Array.from({ length: n }, (_, i) => (i * 37 + 11) & 0xff);
}

/** Largest byte count that still fits each version 1 to 10 at level M. */
export function boundaryLengths() {
  return M_DATA_CODEWORDS.map((cw, i) => Math.floor((cw * 8 - 4 - (i + 1 <= 9 ? 8 : 16)) / 8));
}

export function qrInputs() {
  const enc = new TextEncoder();
  const out = [];
  const seed = fromFlips(seedFlips).wallet;
  for (const d of [seed.descriptor, seed.receiveDescriptor, seed.changeDescriptor]) {
    out.push(enc.encode(d));
  }
  const flipLines = readFileSync(new URL('electrum/flips.txt', import.meta.url), 'utf8')
    .trim()
    .split('\n');
  for (const line of flipLines) {
    out.push(enc.encode(fromFlips([...line].map(Number)).wallet.descriptor));
  }
  out.push(new Uint8Array(0), Uint8Array.of(0x41));
  for (const cap of boundaryLengths()) {
    out.push(filler(cap), filler(cap + 1));
  }
  return out;
}

export function inputsText(inputs) {
  return inputs.map((b) => (b.length === 0 ? '-' : bytesToHex(b))).join('\n') + '\n';
}
