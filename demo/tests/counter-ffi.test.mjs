// The counter's own Rust answering the counter's own Kotlin model, in node: the model compiled for the
// browser (scripts/demo-kotlin-js.sh counter) calls its one native function, and the WebAssembly build of the
// counter's library (scripts/demo-build.sh) answers it, on a chain simulated in memory.
// Usage: node demo/tests/counter-ffi.test.mjs   (after scripts/demo-kotlin-js.sh counter and scripts/demo-build.sh)
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import path from 'node:path';
import { loadCounter } from '../web2/counter-host.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.join(here, '../..');
const wasm = await readFile(path.join(root, 'dist/demo-counter/counter.wasm'));

// The page's clock, which a test moves by hand.
let clock = 1_700_000_000;
const chain = await loadCounter(wasm, { now: () => clock });

const model = await import(path.join(root, 'dist/kotlin-js/counter/counter-model.mjs'));
const flow = model.CounterFlowDemo.getInstance();

// The node starts on the pretend chain from the demo pairing record.
flow.start('/demo/counter', clock, chain.record());

// A sale of 24.50 USD at 100,000 USD per bitcoin: 24,500 sats to send.
const address = flow.openSale(clock, 2450, 'USD', 10_000_000);
assert.match(address, /^bcrt1/, 'a regtest address');
assert.equal(flow.satsNeeded(), 24_500);
assert.equal(flow.saleState(clock), 'NotPaidYet');
assert.equal(flow.salePaid(clock), false);

// The customer pays it on the pretend chain; the node reads its events.
const txid = chain.pay(address, 24_500);
assert.match(txid, /^[0-9a-f]{64}$/);
flow.pump(clock);
assert.notEqual(flow.saleState(clock), 'NotPaidYet', 'the node saw the payment');

// Blocks follow until the model counts the sale paid.
let paid = flow.salePaid(clock);
for (let n = 1; n <= 6 && !paid; n++) {
  clock += 600;
  assert.equal(chain.mine(), n);
  flow.pump(clock);
  paid = flow.salePaid(clock);
}
assert.equal(paid, true, `the sale is paid, its state is ${flow.saleState(clock)}`);
console.log(`counter-ffi: ok (the sale's state is ${flow.saleState(clock)})`);
flow.stop();
