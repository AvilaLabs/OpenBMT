// The scenarios of the demo (demo/web2): the tabs and the ending cards of the shell on a small stand-in for the page,
// the short-payment path on the counter's own state (PartPaid, then paid), and the café's meal paid over Lightning
// (PaidLightning). The drawing is checked in a browser.
// Usage: node demo/tests/scenario.test.mjs   (after scripts/demo-kotlin-js.sh counter and scripts/demo-build.sh)
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';
import path from 'node:path';
import { loadCounter, invoiceOf } from '../web2/counter-host.mjs';

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
  assert.deepEqual(Shell.BAR.map((t) => t.name), ['Customer', 'Café', 'With your register', 'When something goes wrong']);
  assert.deepEqual(Shell.BAR[2].menu, ['Your register sends the amount', 'Square, Clover and others']);
  assert.deepEqual(Shell.BAR[3].menu, ['A short payment', 'A cancelled payment']);
  assert.deepEqual(Shell.SCENARIOS, ['Customer', 'Café', 'Your register sends the amount', 'Square, Clover and others', 'A short payment', 'A cancelled payment']);
  assert.equal(Shell.CARDS.Customer.heading, 'You paid a shop in bitcoin.');
  assert.equal(Shell.CARDS['Café'].heading, 'That is a day at the counter.');
  assert.equal(Shell.CARDS['Register sends'].heading, Shell.CARDS['Register any'].heading);
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
  assert.deepEqual(tabs.map((t) => t.label.textContent), ['Customer', 'Café', 'With your register', 'When something goes wrong']);
  // The select: a place for each scenario without a menu, and an optgroup of items for each menu.
  const groups = shell.scenSel.children.filter((c) => c.tag === 'optgroup');
  assert.deepEqual(groups.map((g) => g.label), ['With your register', 'When something goes wrong']);
  assert.deepEqual(groups.map((g) => labels(g.children)), [Shell.BAR[2].menu, Shell.BAR[3].menu]);
  assert.deepEqual(labels(shell.scenSel.children.filter((c) => c.tag === 'option' && c.value)), ['Customer', 'Café']);

  // A tab without a menu names its scenario; it has no menu and no chevron.
  tabs[0].label.click();
  tabs[1].label.click();
  assert.deepEqual(chosen, ['Customer', 'Café']);
  assert.equal(tabs[0].list, undefined);

  // "With your register": the label opens the choice screen; the chevron beside it opens the menu.
  tabs[2].label.click();
  assert.equal(chosen.at(-1), 'With your register');
  assert.ok(tabs[2].list.hidden, 'a click on the label opens no menu');
  const chev = tabs[2].tab.children.find((c) => c.classes.has('tab-chev'));
  assert.ok(chev, 'a chevron of its own');
  chev.click();
  assert.ok(!tabs[2].list.hidden, 'the chevron opens the menu');
  assert.equal(chev.getAttribute('aria-expanded'), 'true');
  assert.deepEqual(labels([...tabs[2].items.values()]), Shell.BAR[2].menu);
  // Picking an item starts that sub-scenario and closes the menu.
  tabs[2].items.get('Square, Clover and others').click();
  assert.equal(chosen.at(-1), 'Square, Clover and others');
  assert.ok(tabs[2].list.hidden);

  // "When something goes wrong": the label opens the menu (a click, or Enter, which is a click), and carries the chevron.
  assert.ok(tabs[3].label.classes.has('chevron'));
  tabs[3].label.click();
  assert.ok(!tabs[3].list.hidden);
  assert.equal(chosen.length, 5 - 1, 'opening a menu starts nothing');
  tabs[3].label.click();
  assert.ok(tabs[3].list.hidden, 'a second click closes it');
  // Arrow down opens it from the keyboard and moves to the first item; arrows move, Esc closes and gives the focus back.
  for (const f of tabs[3].label.handlers.keydown || []) f(key('ArrowDown'));
  assert.ok(!tabs[3].list.hidden);
  assert.equal(document.activeElement, tabs[3].items.get('A short payment'));
  for (const f of tabs[3].items.get('A short payment').handlers.keydown) f(key('ArrowDown'));
  assert.equal(document.activeElement, tabs[3].items.get('A cancelled payment'));
  for (const f of tabs[3].items.get('A cancelled payment').handlers.keydown) f(key('ArrowDown'));
  assert.equal(document.activeElement, tabs[3].items.get('A short payment'), 'the arrows wrap');
  for (const f of tabs[3].items.get('A short payment').handlers.keydown) f(key('Escape'));
  assert.ok(tabs[3].list.hidden);
  assert.equal(document.activeElement, tabs[3].label);
  // A click elsewhere on the page closes an open menu.
  tabs[3].label.click();
  for (const f of docHandlers.click) f({ target: new Node_('div') });
  assert.ok(tabs[3].list.hidden);
  tabs[3].items.get('A cancelled payment').click();
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
  // The register's choice screen: its tab is marked, no item is, and the select shows the tab's name.
  shell.scenario('With your register');
  assert.deepEqual(current(), ['With your register']);
  assert.equal(shell.scenSel.value, '');
  assert.equal(shell.scenHold.textContent, 'With your register');
  shell.scenario(null);
  assert.deepEqual(current(), []);
  // A change of the select names the scenario.
  shell.scenSel.value = 'A short payment';
  for (const f of shell.scenSel.handlers.change) f();
  assert.equal(chosen.at(-1), 'A short payment');
}

test_cards_and_inert_end: {
  const app = new Node_('div');
  const shell = new Shell(app);
  const chosen = [];
  shell.onScenario = (name) => chosen.push(name);
  const device = [{ id: 'counter', caption: 'The shop\'s counter', sizes: [{ size: { width: 100, height: 100 } }] }];
  const cards = {
    Customer: ['Customer', undefined, 'Customer'], 'Café': ['Café', undefined, 'Café'],
    'Your register sends the amount': ['With your register', 'Your register sends the amount', 'Register sends'],
    'Square, Clover and others': ['With your register', 'Square, Clover and others', 'Register any'],
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
    { name: 'With your register', what: 'c', go: 'Start', onPick: () => picked.push(3) },
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
  // The register's choice shows the scenario bar and has no Back.
  shell.choice(roles.slice(0, 2), { title: 'With your register', lede: 'x' });
  assert.equal(shell.demo.hidden, false, 'the bar is shown');
  assert.equal(named(app, 'Back').length, 0);
  assert.equal(app.all((x) => x.classes.has('chooser')).length, 1);
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
  // The rest is exactly what the sale's own data says is missing, to the same address.
  const missing = Math.round(part.needsSats - part.receivedSats);
  assert.equal(missing, part.needsSats - 6000);
  assert.equal(PC.parse(last('sale').copyUri).address, address);
  chain.pay(address, missing);
  until('paid', () => last('sale').isPaid);
  assert.equal(last('sale').state, 'PaidInstant');
  c.showHome();
}

test_meal_over_lightning: {
  const opened = sale(['2', '0', '0', '0']);
  assert.equal(opened.amountBig, '20.00 USD');
  until('the invoice', () => invoiceOf(last('sale').copyUri));
  const read = PC.parse(last('sale').copyUri);
  assert.ok(read.lightning && read.address, 'a code with two ways to pay');
  assert.equal(last('sale').isPaid, false);
  assert.equal(chain.payLightning(invoiceOf(last('sale').copyUri)), read.sats);
  until('PaidLightning', () => last('sale').state === 'PaidLightning');
  assert.equal(last('sale').isPaid, true);
  assert.equal(last('sale').number, 2);
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

test_register_views: {
  const RV = require('../web2/register-views.js');
  const text = (n) => n.textContent;
  const buttons = (n) => n.all((x) => x.tag === 'button');
  const landscape = RV.LANDSCAPE;
  assert.deepEqual([landscape.width, landscape.height], [962, 601], 'the counter tablet, turned landscape');
  // The sale screen: six item tiles, the ticket, and the large Charge button.
  const sale = RV.register({ screen: 'sale' }, landscape);
  assert.deepEqual(labels(buttons(sale)).slice(0, 6), ['Coffee 4.00', 'Bagel 4.00', 'Juice 5.00', 'Muffin 3.50', 'Tea 3.00', 'Cookie 2.50']);
  const body = text(sale);
  for (const w of ['Order 1042', 'Total 8.00 USD']) assert.ok(body.includes(w), w);
  assert.equal(sale.all((x) => x.tag === 'div' && x.textContent === 'Coffee 4.00').length, 1, 'the ticket line');
  assert.equal(sale.all((x) => x.tag === 'div' && x.textContent === 'Bagel 4.00').length, 1);
  assert.deepEqual(labels(buttons(sale)).slice(6), ['Charge 8.00 USD']);
  assert.ok(!body.includes('Paid with Bitcoin'));
  let charged = 0;
  const live = RV.register({ screen: 'sale', taps: { charge: () => { charged++; } } }, landscape);
  buttons(live).find((b) => text(b) === 'Charge 8.00 USD').click();
  assert.equal(charged, 1);
  // The tender screen: three large buttons, only Bitcoin a target, and the line under them.
  let bitcoin = 0;
  const tender = RV.register({ screen: 'tender', taps: { bitcoin: () => { bitcoin++; } } }, landscape);
  assert.ok(text(tender).includes('How is it paid?'));
  assert.deepEqual(labels(buttons(tender)), ['Cash', 'Card', 'Bitcoin']);
  assert.ok(text(tender).includes('Bitcoin is a payment type the shop added.'));
  for (const b of buttons(tender)) b.click();
  assert.equal(bitcoin, 1, 'only Bitcoin does anything');
  // Once the sale is paid the ticket says so and the tender screen has closed.
  const paid = RV.register({ screen: 'paid' }, landscape);
  assert.ok(text(paid).includes('Paid with Bitcoin'));
  assert.ok(!text(paid).includes('How is it paid?'));
  assert.deepEqual(labels(buttons(paid)).filter((x) => x.startsWith('Charge')), [], 'no Charge once it is paid');
  // The tablet's own size is the fallback for a narrow window.
  assert.ok(text(RV.register({ screen: 'sale' }, { width: 601, height: 962, density: 4 / 3 })).includes('Total 8.00 USD'));
}

c.stopNode();
console.log('scenario: ok (bar and menus, opening screen, inert end and summary, cards; short payment; cancelled payment red; meal over Lightning; register screens)');
