// The customer's wallet reads the counter's payment request: node --test demo/tests/payment-code.test.mjs
import test from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';

const { parse, satsOf, fromTag } = createRequire(import.meta.url)('../web2/payment-code.js');

test('the amount is read to the satoshi, without floating point', () => {
  assert.equal(satsOf('0.00010877'), 10877);
  assert.equal(satsOf('1'), 100000000);
  assert.equal(satsOf('0.1'), 10000000);
  assert.equal(satsOf('20999999.9769'), 2099999997690000);
  assert.equal(satsOf('0.000000001'), null);
  assert.equal(satsOf('abc'), null);
});

test('the counter writes the request in capitals and the wallet reads it', () => {
  const r = parse('BITCOIN:BCRT1QVVTMSFMNAVXH63QGXRRFTJJ685TSMD8WEF89VFY8CPQR3F0MVFNS60Q2MD?amount=0.00010877');
  assert.deepEqual(r, { address: 'bcrt1qvvtmsfmnavxh63qgxrrftjj685tsmd8wef89vfy8cpqr3f0mvfns60q2md', sats: 10877 });
});

test('a request in lower case, with other parameters, reads the same', () => {
  const r = parse('bitcoin:bcrt1qabc?label=shop&amount=0.0002');
  assert.deepEqual(r, { address: 'bcrt1qabc', sats: 20000 });
});

test('text that is not a request with an address and an amount is refused', () => {
  assert.equal(parse('hello'), null);
  assert.equal(parse('bitcoin:bcrt1qabc'), null);
  assert.equal(parse('bitcoin:bcrt1qabc?amount=0'), null);
  assert.equal(parse('bitcoin:bcrt1qabc?amount=x'), null);
});

/** The NDEF file of a tag for [text]: two bytes of length, then one short URI record with identifier code 0 (TP-02). */
function tagOf(text) {
  const t = new TextEncoder().encode(text);
  const record = [0xd1, 0x01, t.length + 1, 0x55, 0x00, ...t];
  return Int8Array.from([0, record.length, ...record]);
}

test('a request that carries a Lightning invoice gives it back in lower case', () => {
  const r = parse('BITCOIN:BCRT1QABC?AMOUNT=0.0002&LIGHTNING=LNBCRT1PQQQ');
  assert.deepEqual(r, { address: 'bcrt1qabc', sats: 20000, lightning: 'lnbcrt1pqqq' });
  assert.equal(parse('bitcoin:bcrt1qabc?amount=0.0002&lightning=').lightning, undefined);
});

test('the tag holds the same request, read back from its NDEF file', () => {
  const text = 'bitcoin:bcrt1qabc?amount=0.0002&lightning=lnbcrt1pqqq';
  assert.equal(fromTag(tagOf(text)), text);
  assert.deepEqual(parse(fromTag(tagOf(text))), parse(text));
});

test('a long record (more than 255 bytes of payload) is read too', () => {
  const text = 'bitcoin:bcrt1qabc?amount=0.0002&lightning=lnbcrt1' + 'q'.repeat(300);
  const t = new TextEncoder().encode(text);
  const record = [0xc1, 0x01, 0, 0, (t.length + 1) >> 8, (t.length + 1) & 0xff, 0x55, 0x00, ...t];
  assert.equal(fromTag(Uint8Array.from([record.length >> 8, record.length & 0xff, ...record])), text);
});

test('a file that is not one plain URI record is refused', () => {
  const good = Array.from(tagOf('bitcoin:bcrt1qabc?amount=1'), (b) => b & 0xff);
  assert.equal(fromTag(null), null);
  assert.equal(fromTag(new Int8Array(0)), null);
  assert.equal(fromTag(Uint8Array.from(good.slice(0, -1))), null, 'a length that disagrees');
  assert.equal(fromTag(Uint8Array.from(good.map((b, i) => (i === 2 ? 0xd0 : b)))), null, 'not the last record');
  assert.equal(fromTag(Uint8Array.from(good.map((b, i) => (i === 5 ? 0x54 : b)))), null, 'not type U');
  assert.equal(fromTag(Uint8Array.from(good.map((b, i) => (i === 6 ? 0x01 : b)))), null, 'a prefix code');
});
