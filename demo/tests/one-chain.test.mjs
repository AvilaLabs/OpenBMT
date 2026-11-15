// The page's one pretend chain (demo/web2/one-chain.js) over the counter's module and the owner's module, with the
// two controllers on their own models, in node: a sale is opened on the counter, the customer's payment goes through
// the one function, and the two modules hold the same chain (the same tip hash) after the payment and after a block.
// The owner's home is drawn from the owner model's state: its balance words before and after the block.
// Usage: node demo/tests/one-chain.test.mjs   (after scripts/demo-kotlin-js.sh counter and owner, and scripts/demo-build.sh)
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';
import path from 'node:path';
import { loadCounter } from '../web2/counter-host.mjs';
import { loadOwner } from '../web2/owner-host.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.join(here, '../..');
const require = createRequire(import.meta.url);
const { OneChain } = require('../web2/one-chain.js');
const CC = require('../web2/counter-controller.js');
const OC = require('../web2/owner-controller.js');
const PC = require('../web2/payment-code.js');

// The page's one clock, which a test moves by hand; both modules read it.
let clock = 1_700_000_000;
const now = () => clock;
const counterChain = await loadCounter(await readFile(path.join(root, 'dist/demo-counter/counter.wasm')), { now });
const ownerChain = await loadOwner(await readFile(path.join(root, 'dist/demo-owner/owner.wasm')), { now });
const counterFlow = (await import(path.join(root, 'dist/kotlin-js/counter/counter-model.mjs'))).CounterFlowDemo.getInstance();
const ownerFlow = (await import(path.join(root, 'dist/kotlin-js/owner/owner-model.mjs'))).OwnerFlowDemo.getInstance();
const chain = OneChain(counterChain, ownerChain);

const events = [];
const counter = CC.Controller({ flow: counterFlow, chain, clock: { now }, show() {}, event(kind, data) { events.push([kind, data]); } });
const homes = [];
const owner = OC.Controller({ flow: ownerFlow, chain: ownerChain, clock: { now }, show(view) { homes.push(view); } });

counter.startNode();
owner.startNode();
owner.render();
const pump = () => { counterFlow.pump(clock); ownerFlow.pump(clock); counter.onPump(true); owner.onPump(true); };
pump();
assert.equal(homes.length, 1, 'the owner home is drawn once from the model');

// Both modules start on the same chain.
assert.match(chain.tip(), /^[0-9a-f]{64}$/);
const before = owner.balanceWords();
assert.equal(before, 'The wallet can spend 0 sats. Checked to block 0 of 0.');

// The counter opens the sale of 8.00 USD; the customer's wallet reads its code and pays through the one chain.
counter.showNewSale();
for (const key of '800') counter.press(key);
counter.openEntered();
const sale = events.filter((e) => e[0] === 'sale').at(-1)[1];
const read = PC.parse(sale.copyUri);
const tipBefore = chain.tip();
chain.pay(read.address, read.sats);
clock += 1;
pump();
assert.equal(counterChain.tip(), ownerChain.tip(), 'the same tip after a payment');
assert.equal(chain.tip(), tipBefore, 'a payment alone does not move the tip');

// The counter's own check turns the sale paid before any block.
for (let s = 0; s < 20 && !events.filter((e) => e[0] === 'sale').at(-1)[1].isPaid; s++) {
  clock += 1;
  counterFlow.heartbeat(clock, chain.heartbeat());
  pump();
}
assert.equal(events.filter((e) => e[0] === 'sale').at(-1)[1].isPaid, true, 'the counter saw the payment');

// A block on the one chain: both modules mine it at the same time and hold the same tip.
assert.equal(chain.mine(), 1);
clock += 1;
pump();
owner.takeEvents(clock);
assert.equal(counterChain.tip(), ownerChain.tip(), 'the same tip after a block');
assert.notEqual(chain.tip(), tipBefore, 'the block moved the tip');

// The owner's model has taken the block: its home says so, drawn again from the model.
const after = owner.balanceWords();
assert.equal(after, `The wallet can spend ${read.sats.toLocaleString('en-US')} sats. Checked to block 1 of 1.`);
assert.ok(homes.length >= 2, 'the home was drawn again');

// Two chains that differ are named, not hidden.
const odd = OneChain({ mine: () => 1, tip: () => 'a' }, { mine: () => 2, tip: () => 'b' });
assert.throws(() => odd.mine(), /the two chains differ/);
assert.throws(() => odd.tip(), /the two chains differ/);

console.log(`one-chain: ok (tip ${chain.tip().slice(0, 12)}, owner: "${before}" -> "${after}")`);
counter.stopNode();
owner.stopNode();
