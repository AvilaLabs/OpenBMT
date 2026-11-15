// The customer's wallet reads the counter's payment request: node --test demo/tests/payment-code.test.mjs
import test from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';

const { parse, satsOf } = createRequire(import.meta.url)('../web2/payment-code.js');

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
