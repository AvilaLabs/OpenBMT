// Runs the apps' own screen model, compiled for the browser by
// scripts/demo-kotlin-js.sh, in node: the counter's amounts, and the one native
// call served by a function the page would provide.
// Usage: node demo/tests/kotlin-js.test.mjs   (after scripts/demo-kotlin-js.sh counter)
import assert from 'node:assert/strict';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

const here = path.dirname(fileURLToPath(import.meta.url));
const file = path.join(here, '../../dist/kotlin-js/counter/counter-model.mjs');

// The page's side of the seam: the request frame in, a frame out.
let seen = null;
globalThis.openbmtCounterCall = (request) => {
  seen = Array.from(request);
  return new Int8Array([1, 2, 3, 4, 5]);
};

const mod = await import(file);
const demo = mod.CounterModelDemo.getInstance();

assert.equal(demo.showAmount('USD', 1234), '12.34 USD');
assert.equal(demo.showAmount('JPY', 1500), '1,500 JPY');
assert.equal(demo.showAmount('SAT', 7500), '7,500 sats');
assert.equal(demo.showAmount('XXX', 1), null);
assert.equal(demo.parseAmount('USD', '12.3'), 1230);
assert.equal(demo.parseAmount('USD', '1,234.50'), 123450);
assert.equal(demo.parseAmount('USD', '12.345'), -1);
assert.equal(demo.parseAmount('JPY', '12.5'), -1);

assert.equal(demo.nativeAnswerLength(new Int8Array([79, 66, 77, 1])), 5);
assert.deepEqual(seen, [79, 66, 77, 1]);

console.log('kotlin-js: ok');
