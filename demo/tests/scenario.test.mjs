// The scenarios of the demo (demo/web2): the tabs and the ending cards of the shell on a small stand-in for the page,
// the short-payment path on the counter's own state (PartPaid, then paid). The drawing is checked in a browser.
// Usage: node demo/tests/scenario.test.mjs   (after scripts/demo-kotlin-js.sh counter and scripts/demo-build.sh)
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';
import path from 'node:path';
import { loadCounter } from '../web2/counter-host.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.join(here, '../..');
const require = createRequire(import.meta.url);

// ---- A stand-in for the page: just enough of the DOM for the shell's bar, tabs, stage and card. ----
class Node_ {
  constructor(tag) {
    this.tag = tag;
    this.children = [];
    this.parent = null;
    this.attrs = {};
    this.handlers = {};
    this.text = '';
    this.style = {};
    this.hidden = false;
    this.value = '';
    this.id = '';
    this.disabled = false;
    this.inert = false;
    this.tabIndex = 0;
    const set = new Set();
    this.classes = set;
    this.classList = {
      add: (c) => set.add(c),
      remove: (c) => set.delete(c),
      toggle: (c, on) => (on === undefined ? (set.has(c) ? set.delete(c) : set.add(c)) : on ? set.add(c) : set.delete(c)),
      contains: (c) => set.has(c),
    };
  }
  set className(v) { this.classes.clear(); for (const c of String(v).split(/\s+/).filter(Boolean)) this.classes.add(c); }
  get className() { return [...this.classes].join(' '); }
  set textContent(v) { this.children = []; this.text = String(v); }
  get textContent() { return this.text + this.children.map((c) => c.textContent).join(''); }
  set innerHTML(v) { this.html = v; }
  append(...items) { for (const i of items) { const n = typeof i === 'string' ? Object.assign(new Node_('#text'), { text: i }) : i; n.parent = this; this.children.push(n); } }
  after() {}
  replaceChildren(...items) { this.children = []; this.append(...items); }
  remove() { if (this.parent) this.parent.children = this.parent.children.filter((c) => c !== this); this.parent = null; }
  setAttribute(k, v) { this.attrs[k] = String(v); }
  getAttribute(k) { return this.attrs[k] ?? null; }
  removeAttribute(k) { delete this.attrs[k]; }
  addEventListener(k, f) { (this.handlers[k] ||= []).push(f); }
  removeEventListener(k, f) { this.handlers[k] = (this.handlers[k] || []).filter((x) => x !== f); }
  click() { for (const f of this.handlers.click || []) f({ stopPropagation() {} }); }
  focus() { globalThis.document.activeElement = this; }
  animate() { return { onfinish: null }; }
  contains(n) { return n === this || this.children.some((c) => c.contains && c.contains(n)); }
  querySelector() { return null; }
  querySelectorAll() { return []; }
  all(pred, out = []) { if (pred(this)) out.push(this); for (const c of this.children) c.all(pred, out); return out; }
}
const timers = [];
globalThis.setTimeout = (f, ms) => { timers.push({ f, ms }); return timers.length; };
globalThis.clearTimeout = (id) => { if (timers[id - 1]) timers[id - 1].f = null; };
const docHandlers = {};
globalThis.document = {
  createElement: (t) => new Node_(t), activeElement: null,
  addEventListener: (k, f) => { (docHandlers[k] ||= []).push(f); },
  removeEventListener: (k, f) => { docHandlers[k] = (docHandlers[k] || []).filter((x) => x !== f); },
  createTextNode: (t) => Object.assign(new Node_('#text'), { text: t }),
};
globalThis.matchMedia = () => ({ matches: true });
globalThis.localStorage = { getItem: () => null, setItem() {} };
globalThis.innerWidth = 1440;
globalThis.addEventListener = () => {};
globalThis.getComputedStyle = () => ({ columnGap: '0', paddingLeft: '0', paddingRight: '0' });
globalThis.location = { assign(url) { this.went = url; } };

const Shell = require('../web2/shell.js');
const named = (n, label) => n.all((x) => x.tag === 'button' && x.textContent === label);

const key = (k) => ({ key: k, preventDefault() {} });
const labels = (list) => list.map((x) => x.textContent);

test_words: {
  assert.deepEqual(Shell.BAR.map((t) => t.name), ['Customer', 'Café', 'When something goes wrong']);
  assert.deepEqual(Shell.BAR[2].menu, ['A short payment', 'A cancelled payment']);
  assert.deepEqual(Shell.SCENARIOS, ['Customer', 'Café', 'A short payment', 'A cancelled payment']);
  assert.equal(Shell.CARDS.Customer.heading, 'You paid a shop in bitcoin.');
  assert.equal(Shell.CARDS['Café'].heading, 'That is a day at the counter.');
  assert.equal(Shell.CARDS['Something goes wrong'].heading, 'A short payment never turns green.');
  assert.equal(Shell.CARDS.Cancelled.heading, 'A cancelled payment turns red.');
  assert.deepEqual(Shell.CARDS.Cancelled.lines, [
    'The counter watches for a second payment of the same money.',
    'Caught in time, the sale turns red, and nothing is handed over.',
    'The cashier asks for another payment.',
  ]);
  for (const card of Object.values(Shell.CARDS)) assert.equal(card.lines.length, 3);
}

test_bar: {
  const app = new Node_('div');
  const shell = new Shell(app);
  const chosen = [];
  shell.onScenario = (name) => chosen.push(name);
  const tabs = [...shell.tabs.values()];
  assert.deepEqual(tabs.map((t) => t.label.textContent), ['Customer', 'Café', 'When something goes wrong']);
  // The select: a place for each scenario without a menu, and an optgroup of items for each menu.
  const groups = shell.scenSel.children.filter((c) => c.tag === 'optgroup');
  assert.deepEqual(groups.map((g) => g.label), ['When something goes wrong']);
  assert.deepEqual(groups.map((g) => labels(g.children)), [Shell.BAR[2].menu]);
  assert.deepEqual(labels(shell.scenSel.children.filter((c) => c.tag === 'option' && c.value)), ['Customer', 'Café']);

  // A tab without a menu names its scenario; it has no menu and no chevron.
  tabs[0].label.click();
  tabs[1].label.click();
  assert.deepEqual(chosen, ['Customer', 'Café']);
  assert.equal(tabs[0].list, undefined);

  // "When something goes wrong": the label opens the menu (a click, or Enter, which is a click), and carries the chevron.
  assert.ok(tabs[2].label.classes.has('chevron'));
  tabs[2].label.click();
  assert.ok(!tabs[2].list.hidden);
  assert.equal(chosen.length, 2, 'opening a menu starts nothing');
  tabs[2].label.click();
  assert.ok(tabs[2].list.hidden, 'a second click closes it');
  // Arrow down opens it from the keyboard and moves to the first item; arrows move, Esc closes and gives the focus back.
  for (const f of tabs[2].label.handlers.keydown || []) f(key('ArrowDown'));
  assert.ok(!tabs[2].list.hidden);
  assert.equal(document.activeElement, tabs[2].items.get('A short payment'));
  for (const f of tabs[2].items.get('A short payment').handlers.keydown) f(key('ArrowDown'));
  assert.equal(document.activeElement, tabs[2].items.get('A cancelled payment'));
  for (const f of tabs[2].items.get('A cancelled payment').handlers.keydown) f(key('ArrowDown'));
  assert.equal(document.activeElement, tabs[2].items.get('A short payment'), 'the arrows wrap');
  for (const f of tabs[2].items.get('A short payment').handlers.keydown) f(key('Escape'));
  assert.ok(tabs[2].list.hidden);
  assert.equal(document.activeElement, tabs[2].label);
  // A click elsewhere on the page closes an open menu.
  tabs[2].label.click();
  for (const f of docHandlers.click) f({ target: new Node_('div') });
  assert.ok(tabs[2].list.hidden);
  tabs[2].items.get('A cancelled payment').click();
  assert.equal(chosen.at(-1), 'A cancelled payment');

  // The current scenario and sub-scenario are marked, and only those.
  const current = () => [...shell.tabs.values()].flatMap((t) => [t.label, ...t.items.values()]).filter((x) => x.getAttribute('aria-current')).map((x) => x.textContent);
  shell.scenario('Café');
  assert.deepEqual(current(), ['Café']);
  assert.equal(shell.scenSel.value, 'Café');
  shell.scenario('When something goes wrong', 'A cancelled payment');
  assert.deepEqual(current(), ['When something goes wrong', 'A cancelled payment']);
  assert.equal(shell.scenSel.value, 'A cancelled payment');
  assert.equal(shell.leaf, 'A cancelled payment');
  // A change of the select names the scenario.
  shell.scenSel.value = 'A short payment';
  for (const f of shell.scenSel.handlers.change) f();
  assert.equal(chosen.at(-1), 'A short payment');
  tabs[1].label.click();
  assert.equal(chosen.at(-1), 'Café');
}

test_cards_and_inert_end: {
  const app = new Node_('div');
  const shell = new Shell(app);
  const chosen = [];
  shell.onScenario = (name) => chosen.push(name);
  const device = [{ id: 'counter', caption: 'The shop\'s counter', sizes: [{ size: { width: 100, height: 100 } }] }];
  const cards = {
    Customer: ['Customer', undefined, 'Customer'], 'Café': ['Café', undefined, 'Café'],
    'A short payment': ['When something goes wrong', 'A short payment', 'Something goes wrong'],
    'A cancelled payment': ['When something goes wrong', 'A cancelled payment', 'Cancelled'],
  };
  for (const [leaf, [tab, sub, key]] of Object.entries(cards)) {
    shell.scenario(tab, sub);
    shell.begin(['One'], device, 'counter');
    shell.actor({ who: 'The customer', line: 'x', button: 'Pays', onTap() {} });
    shell.say('It happened.', []);
    shell.finish(Shell.CARDS[key]);
    // At the last moment every device and the customer are out of use at once, before the card comes.
    assert.equal(shell.cardEl, null, 'the card is not there at once');
    assert.ok(shell.slots.get('counter').wrap.inert, 'the device is inert');
    assert.ok(shell.actorEl.inert, 'the customer is inert');
    assert.ok(shell.stage.classes.has('finished'), 'the devices are dimmed');
    const t = timers.at(-1);
    assert.equal(t.ms, 1200);
    t.f();
    const card = shell.cardEl;
    assert.ok(card, 'the card opens');
    assert.equal(card.all((x) => x.tag === 'h2')[0].textContent, Shell.CARDS[key].heading);
    assert.deepEqual(card.all((x) => x.tag === 'li').map((x) => x.textContent), Shell.CARDS[key].lines);
    const next = card.all((x) => x.classes.has('end-next'))[0].children.filter((x) => x.tag === 'button').map((x) => x.textContent);
    assert.deepEqual(next, Shell.SCENARIOS.filter((n) => n !== leaf), 'one button for each other scenario, by its bar name');
    assert.equal(document.activeElement, card.all((x) => x.tag === 'h2')[0], 'focus moves to the heading');
    assert.equal(card.all((x) => x.attrs['aria-label'] === 'Close').length, 1);
    // A Try next button chooses that scenario.
    named(card, next[0])[0].click();
    assert.equal(chosen.at(-1), next[0]);
    // Esc puts the card away; the devices stay inert and dimmed, and one button in the instruction line brings it back.
    for (const f of docHandlers.keydown) f({ key: 'Escape' });
    assert.equal(shell.cardEl, null);
    assert.ok(shell.slots.get('counter').wrap.inert && shell.actorEl.inert);
    assert.ok(shell.stage.classes.has('finished'));
    const summary = shell.doEl.all((x) => x.tag === 'button');
    assert.deepEqual(labels(summary), ['See the summary']);
    summary[0].click();
    assert.ok(shell.cardEl, 'the summary button opens the card again');
    assert.equal(shell.doEl.all((x) => x.tag === 'button').length, 0, 'and goes while the card is open');
    assert.ok(shell.slots.get('counter').wrap.inert);
  }

  // The close control does the same as Esc; Start over and the handbook are plain buttons.
  shell.scenario('Customer');
  shell.begin(['One'], device);
  shell.finish(Shell.CARDS.Customer);
  timers.at(-1).f();
  let again = 0;
  shell.onRestart = () => { again++; };
  const card = shell.cardEl;
  named(card, 'Start over')[0].click();
  assert.equal(again, 1);
  named(card, 'Read the handbook')[0].click();
  assert.equal(location.went, '/handbook/');
  card.all((x) => x.attrs['aria-label'] === 'Close')[0].click();
  assert.equal(shell.cardEl, null);
  assert.ok(shell.slots.get('counter').wrap.inert, 'closing the card leaves the devices inert');
  assert.equal(labels(shell.doEl.all((x) => x.tag === 'button'))[0], 'See the summary');

  // A new start brings everything back: no inert device, no dimming, no button.
  shell.begin(['One'], device, 'counter');
  assert.ok(!shell.slots.get('counter').wrap.inert);
  assert.ok(!shell.stage.classes.has('finished'));
  assert.equal(shell.summaryEl, null);

  // A card that has not come yet is stopped by a new start; the devices were inert meanwhile.
  shell.finish(Shell.CARDS.Customer);
  const waiting = timers.at(-1);
  assert.ok(shell.slots.get('counter').wrap.inert);
  shell.begin(['One'], device);
  assert.equal(waiting.f, null, 'the timer of the card is cleared');
  assert.equal(shell.cardEl, null);
  assert.ok(!shell.slots.get('counter').wrap.inert);
}

test_opening_screen: {
  const app = new Node_('div');
  const shell = new Shell(app);
  const canvas = new Node_('canvas');
  const log = [];
  const spec = { play() { log.push('play'); }, seconds: 2, still: false, done() { log.push('done'); } };
  globalThis.matchMedia = () => ({ matches: false });
  const o = shell.opening(canvas, spec);
  assert.equal(o.tag, 'button');
  assert.equal(o.getAttribute('aria-label'), 'Start the demo');
  assert.deepEqual(labels(o.all((x) => x.classes.has('open-name') || x.classes.has('open-line'))), ['OpenBMT', 'Tap to start the demo.']);
  assert.ok(o.all((x) => x === canvas).length, 'the logo is on it');
  assert.equal(document.activeElement, o, 'Enter and Space reach the button at once');
  const before = timers.length;
  o.click();
  // The tap plays the snap; the screen goes 600 ms after the snap's own length, over 300 ms.
  assert.deepEqual(log, ['play']);
  assert.equal(timers[before].ms, 2600);
  o.click();
  assert.deepEqual(log, ['play'], 'a second tap does nothing');
  timers[before].f();
  assert.equal(timers[before + 1].ms, 300);
  assert.deepEqual(log, ['play']);
  timers[before + 1].f();
  assert.deepEqual(log, ['play', 'done']);
  assert.equal(o.parent, null, 'the screen is gone');

  globalThis.matchMedia = () => ({ matches: true });
  // Reduced motion: the tap goes straight to the start screen, no sound.
  const still = [];
  const o2 = shell.opening(canvas, { play() { still.push('play'); }, seconds: 2, still: true, done() { still.push('done'); } });
  o2.click();
  assert.deepEqual(still, ['done']);
  assert.equal(o2.parent, null);
}

test_choice_and_start: {
  const app = new Node_('div');
  const shell = new Shell(app);
  const picked = [];
  const roles = [
    { name: 'As a customer', what: 'a', go: 'Start', onPick: () => picked.push(1) },
    { name: 'A café', what: 'b', go: 'Start', onPick: () => picked.push(2) },
    { name: 'A third choice', what: 'c', go: 'Start', onPick: () => picked.push(3) },
    { name: 'When something goes wrong', what: 'd', go: 'Start', onPick: () => picked.push(4) },
  ];
  const plays = [];
  const snap = { canvas: new Node_('canvas'), play() { plays.push('snap'); } };
  shell.start(roles, snap, { quiet: true });
  assert.deepEqual(plays, [], 'the first showing plays no snap');
  assert.equal(app.all((x) => x.classes.has('role')).length, 4);
  app.all((x) => x.classes.has('role'))[2].click();
  assert.deepEqual(picked, [3]);
  shell.start(roles, snap);
  assert.deepEqual(plays, ['snap'], 'it plays after Start over');
}

// ---- The counter's own state on the real modules. ----
const { Controller } = require('../web2/counter-controller.js');
const PC = require('../web2/payment-code.js');
let clock = 1_700_000_000;
const chain = await loadCounter(await readFile(path.join(root, 'dist/demo-counter/counter.wasm')), { now: () => clock });
const flow = (await import(path.join(root, 'dist/kotlin-js/counter/counter-model.mjs'))).CounterFlowDemo.getInstance();
const events = [];
const c = Controller({ flow, chain, clock: { now: () => clock }, show() {}, event(kind, data) { events.push([kind, data]); } });
const last = (kind) => events.filter((e) => e[0] === kind).at(-1)?.[1];
const second = () => { clock += 1; flow.heartbeat(clock, chain.heartbeat()); flow.pump(clock); c.onPump(true); };
const sale = (keys) => { c.showNewSale(); for (const k of keys) c.press(k); c.openEntered(); return last('sale'); };
const until = (what, test) => { for (let s = 0; s < 40 && !test(); s++) second(); assert.ok(test(), what); };
c.startNode();
flow.pump(clock);
c.showHome();

test_short_payment: {
  const opened = sale(['8', '0', '0']);
  assert.equal(opened.state, 'NotPaidYet');
  const address = PC.parse(opened.copyUri).address;
  // The customer's wallet sends 6,000 sats of the sale's sats: the counter's own state turns PartPaid, never green.
  chain.pay(address, 6000);
  until('PartPaid', () => last('sale').state === 'PartPaid');
  const part = last('sale');
  assert.equal(part.isPaid, false);
  assert.equal(part.receivedSats, 6000);
  assert.ok(part.needsSats > 10_000 && part.needsSats < 11_500, 'the sale of 8.00 USD needs about 10,880 sats');
  assert.equal(part.code, true, 'the code stays for the rest');
  for (let s = 0; s < 10; s++) second();
  assert.equal(last('sale').isPaid, false, 'a short payment stays short');
  // CR-285: the band leads with what is missing, and the code asks for exactly that, to the same address.
  const missing = Math.round(part.needsSats - part.receivedSats);
  assert.equal(missing, part.needsSats - 6000);
  assert.equal(part.stillHeading, 'Still to pay');
  assert.equal(part.amountSmall, `${missing.toLocaleString('en-US')} sats to send`);
  assert.match(part.amountBig, /^\d+\.\d\d USD$/, 'the large amount is in the shop\'s currency');
  assert.equal(part.receivedLine, `Paid 6,000 sats of ${part.needsSats.toLocaleString('en-US')}. The sale is 8.00 USD.`);
  const rest = PC.parse(last('sale').copyUri);
  assert.equal(rest.address, address);
  assert.equal(rest.sats, missing, 'a wallet that scans the code again is asked for the rest, not the whole sale');
  chain.pay(rest.address, rest.sats);
  until('paid', () => last('sale').isPaid);
  assert.equal(last('sale').state, 'PaidInstant');
  c.showHome();
}

// CR-286: a payment that waits shows that it is alive: the short check has its seconds left in a ring and the wait for a
// block says how long ago the payment was seen, in the page's own clock, so a skipped wait shows its minutes truly.
test_wait_shows_it_is_alive: {
  c.showHome();
  sale(['8', '0', '0']);
  const read = PC.parse(last('sale').copyUri);
  chain.pay(read.address, read.sats);
  until('the short check', () => last('sale').ringSecs != null);
  const check = last('sale');
  assert.equal(check.state, 'Waiting');
  assert.ok(check.ringSecs >= 1 && check.ringSecs <= check.ringTotal, 'seconds left of the whole');
  assert.equal(check.ringLabel, `${check.ringSecs} s`);
  assert.equal(check.seen, null, 'a ring has no seen line');
  assert.equal(check.pulse, true, 'the mark pulses while the check is healthy');
  const sig = check.signature;
  second();
  if (last('sale').ringSecs != null) assert.equal(last('sale').signature, sig, 'the seconds move in place: the screen is not drawn again');
  until('paid', () => last('sale').isPaid);
  assert.equal(last('sale').ringSecs, null, 'the ring is gone when the wait is over');
  c.showHome();

  // Over the instant limit on-chain: the wait is for a block, with how long ago the payment was seen.
  sale(['2', '0', '0', '0']);
  second();
  const big = PC.parse(last('sale').copyUri);
  chain.pay(big.address, big.sats);
  until('seen', () => last('sale').seen != null);
  const waiting = last('sale');
  assert.equal(waiting.state, 'Waiting');
  assert.equal(waiting.ringSecs, null, 'a wait for a block has no countdown');
  assert.match(waiting.seen, /^Seen \d+ s ago$/);
  clock += 185;
  flow.heartbeat(clock, chain.heartbeat());
  flow.pump(clock);
  c.onPump(true);
  second();
  assert.equal(last('sale').seen, 'Seen 3 min ago', 'the page\'s clock shows its minutes');
  c.showHome();
}

// CR-284: a sale that turns green while another screen shows gives a notice at the top of that screen; a tap on it opens
// the sale, and the sale's own screen takes it away.
test_notice_of_a_sale_that_turns: {
  const shown = [];
  const view = Controller({ flow, chain, clock: { now: () => clock }, show(viewOf) { shown.push(viewOf); }, event(kind, data) { events.push([kind, data]); } });
  view.showNewSale();
  for (const k of '800') view.press(k);
  view.openEntered();
  const read = PC.parse(view.saleData().copyUri);
  view.showHome();
  assert.deepEqual(view.notices(), { shown: [], more: null }, 'no notice yet');
  chain.pay(read.address, read.sats);
  for (let s = 0; s < 40 && !view.notices().shown.length; s++) { clock += 1; flow.heartbeat(clock, chain.heartbeat()); flow.pump(clock); view.onPump(true); }
  const n = view.notices();
  assert.equal(n.shown.length, 1);
  assert.equal(n.shown[0].green, true);
  assert.match(n.shown[0].heading, /^Sale \d+ is paid$/);
  assert.equal(n.shown[0].next, 'Hand over the goods. Tap to see the sale.');
  assert.equal(events.filter((e) => e[0] === 'notice').at(-1)[1].colour, 'Green', 'its sound plays once');
  const before = events.filter((e) => e[0] === 'notice').length;
  for (let s = 0; s < 5; s++) { clock += 1; flow.pump(clock); view.onPump(true); }
  assert.equal(events.filter((e) => e[0] === 'notice').length, before, 'once');
  view.tapNotice(n.shown[0].index);
  assert.deepEqual(view.notices(), { shown: [], more: null }, 'the sale shows, so its notice is gone');
  assert.equal(view.saleData().isPaid, true);
  assert.equal(events.filter((e) => e[0] === 'sale').at(-1)[1].isPaid, true);
  view.showHome();
}
test_cancelled_payment: {
  const opened = sale(['8', '0', '0']);
  const read = PC.parse(opened.copyUri);
  const txid = chain.pay(read.address, read.sats);
  assert.match(txid, /^[0-9a-f]{64}$/);
  // The counter's short check runs: the sale is waiting, not green.
  for (let s = 0; s < 3; s++) second();
  assert.equal(last('sale').state, 'Waiting');
  assert.equal(last('sale').isPaid, false);
  // The customer spends the same coin to an address of their own: the counter's own state turns red.
  // Through the page's one chain, as the scenario does it (the owner's module is not on the page there).
  const { OneChain } = require('../web2/one-chain.js');
  const back = OneChain(chain, null).cancel(txid);
  assert.match(back, /^[0-9a-f]{64}$/);
  assert.notEqual(back, txid);
  until('the red state', () => last('sale').state === 'DoNotHandOver');
  assert.equal(last('sale').isPaid, false);
  assert.equal(last('sale').says.colour, 'Red');
  for (let s = 0; s < 12; s++) second();
  assert.equal(last('sale').state, 'DoNotHandOver', 'the red state stays');
  // The first payment is gone from the pool: it cannot be taken back twice.
  assert.throws(() => chain.cancel(txid), /no longer waiting/);
  c.showHome();
}

c.stopNode();
console.log('scenario: ok (bar and menus, opening screen, inert end and summary, cards; short payment; cancelled payment red)');
