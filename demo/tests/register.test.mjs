// "With your register" (demo/web2/register.js) on the real modules, in node: the first way, where the register sends
// the amount through the Box to the counter, and the other way, where any register's closed order is matched to the
// counter's sale by the owner's matcher. The drawing is not run here: the views are checked in a browser.
// Usage: node demo/tests/register.test.mjs   (after scripts/demo-kotlin-js.sh counter and owner, and scripts/demo-build.sh)
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
const RG = require('../web2/register.js');

let clock = 1_700_000_000;
const now = () => clock;
const counterWasm = await readFile(path.join(root, 'dist/demo-counter/counter.wasm'));
const ownerWasm = await readFile(path.join(root, 'dist/demo-owner/owner.wasm'));
const flow = (await import(path.join(root, 'dist/kotlin-js/counter/counter-model.mjs'))).CounterFlowDemo.getInstance();
const ownerFlow = (await import(path.join(root, 'dist/kotlin-js/owner/owner-model.mjs'))).OwnerFlowDemo.getInstance();

/** A fresh counter and owner on one chain, for a shop with or without a Box. */
async function start(withBox) {
  const counterChain = await loadCounter(counterWasm, { now });
  const ownerChain = await loadOwner(ownerWasm, { now });
  const chain = OneChain(counterChain, ownerChain);
  const events = [];
  const counter = CC.Controller({ flow, chain, clock: { now }, withBox, show() {}, event(kind, data) { events.push([kind, data]); } });
  const owner = OC.Controller({ flow: ownerFlow, chain: ownerChain, clock: { now }, show() {} });
  counter.startNode();
  owner.startNode();
  const pump = () => { flow.pump(clock); ownerFlow.pump(clock); counter.onPump(true); owner.onPump(true); };
  pump();
  const last = (kind) => events.filter((e) => e[0] === kind).at(-1)?.[1];
  /** Pays the open sale's own code and lets the counter check it, a second at a time, until it is paid. */
  const payAndWait = () => {
    const read = PC.parse(counter.saleData().copyUri);
    chain.pay(read.address, read.sats);
    for (let s = 0; s < 40 && !counter.saleData().isPaid; s++) {
      clock += 1;
      flow.heartbeat(clock, chain.heartbeat());
      pump();
    }
  };
  const stop = () => { counter.stopNode(); owner.stopNode(); };
  return { chain, counter, owner, pump, last, payAndWait, stop, events };
}

test_first_way: {
  const t = await start(true);
  t.counter.showHome();
  assert.deepEqual(t.counter.tillCards(), [], 'no card before the register sends anything');

  // The register sends the amount: the Box tells the counter, and the card waits on the counter's home.
  const till = t.chain.tillRequest(RG.MINOR);
  assert.match(till, /^[0-9a-f]{32}$/);
  const cards = t.counter.tillCards();
  assert.equal(cards.length, 1);
  assert.deepEqual(cards[0].lines.map((l) => l.label), ['Amount', 'Till reference', 'Minutes left']);
  assert.equal(cards[0].lines[0].value, '8.00 USD');
  assert.equal(cards[0].sale, till);
  assert.equal(CC.WORDS.till.card, 'From the till');
  assert.equal(CC.WORDS.till.start, 'Start');

  // The counter's own Start opens the sale for the till's sale, and its screen is the sale's.
  cards[0].start();
  const opened = t.last('sale');
  assert.equal(opened.state, 'NotPaidYet');
  assert.equal(opened.amountBig, '8.00 USD');
  assert.equal(t.counter.tillOfSale(), till, 'the sale names the till\'s sale');
  assert.deepEqual(t.counter.tillCards(), [], 'the card is gone once the sale is started');

  // Paid on the pretend chain: the counter's own state says so, and that is what the register closes its order on.
  t.payAndWait();
  const paid = t.counter.saleData();
  assert.equal(paid.isPaid, true);
  assert.equal(t.counter.tillOfSale(), till);
  t.stop();
}

test_first_way_not_now: {
  const t = await start(true);
  t.counter.showHome();
  const till = t.chain.tillRequest(RG.MINOR);
  const [card] = t.counter.tillCards();
  assert.equal(card.sale, till);
  card.notNow();
  assert.deepEqual(t.counter.tillCards(), [], 'Not now hides the card and nothing is sent');
  t.stop();
}

test_first_way_needs_a_box: {
  const t = await start(false);
  assert.throws(() => t.chain.tillRequest(RG.MINOR), /running|Box|CR-266/, 'a counter whose settings name no Box drops it');
  t.stop();
}

test_other_way_matches: {
  const t = await start(false);
  const facts = { totalMinor: RG.MINOR, orderOpened: clock, closedAt: 0, sale: null };
  t.counter.showNewSale();
  for (const key of RG.KEYS) t.counter.press(key);
  t.counter.openEntered();
  const open = t.last('sale');
  facts.sale = { number: open.number, amountMinor: RG.minorOf(open.amountBig), opened: clock, paid: 0 };
  assert.equal(facts.sale.amountMinor, 800);
  t.payAndWait();
  facts.sale.paid = clock;
  // The cashier closes the order on the register with the Bitcoin tender.
  clock += 5;
  facts.closedAt = clock;
  const verdict = RG.parseVerdict(t.chain.registerVerdict(RG.requestText(facts, clock + 1)));
  assert.deepEqual(verdict, { matches: 1, byOrder: 0, alerts: [] }, 'the owner\'s phone matches the order to the sale');
  const lines = RG.tillLines(ownerFlow, verdict, clock + 1);
  assert.equal(lines.lastRead, 'Last read just now.');
  assert.equal(lines.counts, '1 sale matched. No alerts.');
  t.stop();
}

test_other_way_alert: {
  const t = await start(false);
  // A Bitcoin order closed on the register and no counter sale: an alert once the grace has passed.
  const facts = { totalMinor: RG.MINOR, orderOpened: clock, closedAt: clock + 5, sale: null };
  const verdict = RG.parseVerdict(t.chain.registerVerdict(RG.requestText(facts, clock + 5 + 700)));
  assert.equal(verdict.matches, 0);
  assert.equal(verdict.alerts.length, 1);
  const lines = RG.tillLines(ownerFlow, verdict, clock + 5 + 700);
  assert.equal(lines.counts, '0 sales matched. 1 alert.');
  t.stop();
}

console.log('register: ok');
