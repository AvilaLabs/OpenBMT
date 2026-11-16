// The shop's Lightning on the counter's own code, in node: the counter's driver asks the pretend node (in the
// WebAssembly module, crates/openbmt-demo-counter/src/lightning.rs) for an invoice, the sale of 8.00 USD shows a code
// with both an address and an invoice for 10,877 sats, the wallet pays the invoice, and the counter's own state turns
// PaidLightning with no block mined. A second sale is paid on-chain as before.
// Usage: node demo/tests/lightning.test.mjs   (after scripts/demo-kotlin-js.sh counter and scripts/demo-build.sh)
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';
import path from 'node:path';
import { loadCounter, invoiceOf } from '../web2/counter-host.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.join(here, '../..');
const require = createRequire(import.meta.url);
const { Controller } = require('../web2/counter-controller.js');
const PC = require('../web2/payment-code.js');

let clock = 1_700_000_000;
const chain = await loadCounter(await readFile(path.join(root, 'dist/demo-counter/counter.wasm')), { now: () => clock });
const flow = (await import(path.join(root, 'dist/kotlin-js/counter/counter-model.mjs'))).CounterFlowDemo.getInstance();

const events = [];
const c = Controller({
  flow, chain, clock: { now: () => clock },
  show() {},
  event(kind, data) { events.push([kind, data]); },
});
const last = (kind) => events.filter((e) => e[0] === kind).at(-1)?.[1];

// One second of the page's pump: the owner's heartbeat, the counter's pump (which serves its Lightning requests too)
// and the open screen's tick.
const second = () => {
  clock += 1;
  flow.heartbeat(clock, chain.heartbeat());
  flow.pump(clock);
  c.onPump(true);
};
const sale = async (amount) => {
  c.showNewSale();
  for (const key of amount) c.press(key);
  c.openEntered();
  return last('sale');
};

c.startNode();
flow.pump(clock);
c.showHome();

// The 8.00 USD sale opens with the on-chain code at once; the counter asks the node for an invoice on the next pumps.
const opened = await sale(['8', '0', '0']);
assert.equal(opened.state, 'NotPaidYet');
assert.equal(opened.amountBig, '8.00 USD');
assert.equal(opened.needsSats, 10_877);
for (let s = 1; s <= 10 && !invoiceOf(last('sale').copyUri); s++) second();
const withInvoice = last('sale');
const invoice = invoiceOf(withInvoice.copyUri);
assert.ok(invoice, 'the counter put an invoice in the sale\'s code');
assert.match(invoice, /^lnbcrt/, 'a regtest invoice');
const read = PC.parse(withInvoice.copyUri);
assert.match(read.address, /^bcrt1q/, 'the code carries the address');
assert.equal(read.sats, 10_877);
// The invoice's amount is the sale's: 10,877 sats is 108,770 nanobitcoin in BOLT 11's own writing.
assert.match(invoice, /^lnbcrt108770n1/);
assert.equal(withInvoice.state, 'NotPaidYet');

// The wallet pays the invoice. No block is mined: the counter's lookups find it settled and it judges the sale itself.
assert.equal(chain.payLightning(invoice), 10_877);
assert.throws(() => chain.payLightning(invoice), /already paid/);
let state = withInvoice.state;
for (let s = 1; s <= 20 && state !== 'PaidLightning'; s++) {
  second();
  state = last('sale').state;
}
assert.equal(state, 'PaidLightning', 'the counter turns the sale paid over Lightning');
const paid = last('sale');
assert.equal(paid.isPaid, true);
assert.equal(paid.code, false);
assert.equal(paid.outcomeLine, '8.00 USD · 10,877 sats');
c.showHome();

// A second sale is paid on-chain as before: its address, its amount, before any block.
const next = await sale(['8', '0', '0']);
assert.equal(next.number, 2);
for (let s = 1; s <= 10 && !invoiceOf(last('sale').copyUri); s++) second();
const code = last('sale').copyUri;
const wallet = PC.parse(code);
chain.pay(wallet.address, wallet.sats);
state = last('sale').state;
for (let s = 1; s <= 25 && state !== 'PaidInstant'; s++) {
  second();
  state = last('sale').state;
}
assert.equal(state, 'PaidInstant', 'the on-chain payment is accepted at once');
// No block was mined for either sale: the first block of the chain is the one mined now.
assert.equal(chain.mine(), 1);
console.log(`lightning: ok (8.00 USD, ${read.sats} sats, PaidLightning with no block mined; the second sale PaidInstant on-chain)`);
c.stopNode();
