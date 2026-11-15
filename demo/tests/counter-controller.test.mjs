// The page's controller for the shop sale (demo/web2/counter-controller.js) on the counter's own model and library,
// in node: the keypad types 8.00, the sale opens, the wallet reads the sale's own payment code and pays the address
// it names on the pretend chain, and the counter's own judgment turns the sale paid before any block (8.00 USD is
// under the first instant limit). The drawing is not run here: the screens are checked in a browser.
// Usage: node demo/tests/counter-controller.test.mjs   (after scripts/demo-kotlin-js.sh counter and scripts/demo-build.sh)
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';
import path from 'node:path';
import { loadCounter } from '../web2/counter-host.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.join(here, '../..');
const require = createRequire(import.meta.url);
const { Controller } = require('../web2/counter-controller.js');
const PC = require('../web2/payment-code.js');

let clock = 1_700_000_000;
const chain = await loadCounter(await readFile(path.join(root, 'dist/demo-counter/counter.wasm')), { now: () => clock });
const flow = (await import(path.join(root, 'dist/kotlin-js/counter/counter-model.mjs'))).CounterFlowDemo.getInstance();

const events = [];
let drawn = 0;
const c = Controller({
  flow, chain, clock: { now: () => clock },
  show() { drawn++; },
  event(kind, data) { events.push([kind, data]); },
});
const last = (kind) => events.filter((e) => e[0] === kind).at(-1)?.[1];

// The node starts on the demo record and takes its first events.
c.startNode();
flow.pump(clock);
c.showHome();
assert.equal(events.at(-1)[0], 'home');

// New sale: the keypad's entry is the model's, key by key.
c.showNewSale();
assert.equal(events.some((e) => e[0] === 'newSale'), true);
assert.equal(last('entry').text, '0.00 USD');
for (const [key, shown] of [['8', '0.08 USD'], ['0', '0.80 USD'], ['0', '8.00 USD']]) {
  c.press(key);
  assert.equal(last('entry').text, shown);
}
assert.equal(last('entry').minor, 800);

// Open the sale: the model opens it and its screen comes from the library's judgment.
c.openEntered();
const opened = last('sale');
assert.equal(opened.state, 'NotPaidYet');
assert.equal(opened.code, true);
assert.equal(opened.amountBig, '8.00 USD');
assert.equal(opened.number, 1);
assert.equal(opened.needsSats, Math.ceil(800 / 7_355_418 * 1e8));
assert.ok(opened.qrSize > 0 && opened.qrBits.length > 0, 'a code to scan');

// The wallet reads the sale's own payment code: the address and the amount, not constants.
const read = PC.parse(opened.copyUri);
assert.equal(read.address, opened.address);
assert.match(read.address, /^bcrt1q/);
assert.equal(read.sats, opened.needsSats);
chain.pay(read.address, read.sats);

// The owner's heartbeats, the network's events and a few seconds: the counter's own check turns it paid.
let state = opened.state;
for (let s = 1; s <= 20 && state !== 'PaidInstant'; s++) {
  clock += 1;
  flow.heartbeat(clock, chain.heartbeat());
  flow.pump(clock);
  c.onPump(true);
  state = last('sale').state;
}
assert.equal(state, 'PaidInstant', 'the counter accepts 8.00 USD at once');
const paid = last('sale');
assert.equal(paid.isPaid, true);
assert.equal(paid.code, false);
assert.equal(paid.says.heading, 'Paid');
assert.equal(paid.outcomeLine, `8.00 USD · ${read.sats.toLocaleString('en-US')} sats`);
assert.deepEqual(Array.from(paid.paidLabels), ['Sale number']);

// Done goes back to home; a block then reaches the node without trouble.
c.showHome();
assert.equal(events.at(-1)[0], 'home');
assert.equal(chain.mine(), 1);
flow.pump(clock += 600);
c.onPump(true);
assert.ok(drawn > 4, 'the screens were drawn');
console.log(`counter-controller: ok (the sale of 8.00 USD, ${read.sats} sats, is ${state} before a block)`);
c.stopNode();
