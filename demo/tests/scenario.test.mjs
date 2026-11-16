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

test_words: {
  assert.deepEqual(Shell.SCENARIOS, ['Customer', 'Café', 'Register', 'Something goes wrong']);
  assert.equal(Shell.CARDS.Customer.heading, 'You paid a shop in bitcoin.');
  assert.equal(Shell.CARDS['Café'].heading, 'That is a day at the counter.');
  assert.equal(Shell.CARDS['Register sends'].heading, Shell.CARDS['Register any'].heading);
  assert.equal(Shell.CARDS['Something goes wrong'].heading, 'A short payment never turns green.');
  for (const card of Object.values(Shell.CARDS)) assert.equal(card.lines.length, 3);
}

test_tabs_and_cards: {
  const app = new Node_('div');
  const shell = new Shell(app);
  const chosen = [];
  shell.onScenario = (name) => chosen.push(name);
  // One tab and one select place for each scenario, in order.
  const tabs = shell.scenEl.children;
  assert.deepEqual(tabs.map((b) => b.textContent), Shell.SCENARIOS);
  assert.deepEqual(shell.scenSel.children.map((o) => o.textContent), Shell.SCENARIOS);
  // The current scenario's tab is marked, and only that one.
  shell.scenario('Café');
  assert.deepEqual(tabs.map((b) => b.getAttribute('aria-current')), [null, 'true', null, null]);
  assert.equal(shell.scenSel.value, 'Café');
  // A tap on a tab, and a change of the select, name the scenario.
  tabs[3].click();
  shell.scenSel.value = 'Register';
  for (const f of shell.scenSel.handlers.change) f();
  assert.deepEqual(chosen, ['Something goes wrong', 'Register']);

  // Every scenario's card comes 1.2 s after the last moment, with its words and the other three scenarios.
  const cards = { Customer: 'Customer', 'Café': 'Café', Register: 'Register sends', 'Something goes wrong': 'Something goes wrong' };
  for (const [scenario, key] of Object.entries(cards)) {
    shell.scenario(scenario);
    shell.begin(['One'], [{ id: 'counter', caption: 'The shop\'s counter', sizes: [{ size: { width: 100, height: 100 } }] }]);
    shell.finish(Shell.CARDS[key]);
    assert.equal(shell.cardEl, null, 'the card is not there at once');
    const t = timers.at(-1);
    assert.equal(t.ms, 1200);
    t.f();
    const card = shell.cardEl;
    assert.ok(card, 'the card opens');
    assert.ok(shell.stage.classes.has('finished'), 'the devices are dimmed');
    assert.equal(card.all((x) => x.tag === 'h2')[0].textContent, Shell.CARDS[key].heading);
    assert.deepEqual(card.all((x) => x.tag === 'li').map((x) => x.textContent), Shell.CARDS[key].lines);
    const next = card.all((x) => x.classes.has('end-next'))[0].children.filter((x) => x.tag === 'button').map((x) => x.textContent);
    assert.deepEqual(next, Shell.SCENARIOS.filter((n) => n !== scenario), 'one button for each other scenario');
    assert.equal(document.activeElement, card.all((x) => x.tag === 'h2')[0], 'focus moves to the heading');
    assert.equal(card.all((x) => x.attrs['aria-label'] === 'Close').length, 1);
    // A Try next button chooses that scenario.
    named(card, next[0])[0].click();
    assert.equal(chosen.at(-1), next[0]);
    // Esc closes the card and the devices are back.
    for (const f of docHandlers.keydown) f({ key: 'Escape' });
    assert.equal(shell.cardEl, null);
    assert.ok(!shell.stage.classes.has('finished'));
  }

  // The close control hides it; Start over and the handbook are plain buttons.
  shell.scenario('Customer');
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

  // A card that has not come yet is stopped by a new start.
  shell.finish(Shell.CARDS.Customer);
  const waiting = timers.at(-1);
  shell.begin(['One'], [{ id: 'counter', caption: 'x', sizes: [{ size: { width: 100, height: 100 } }] }]);
  assert.equal(waiting.f, null, 'the timer of the card is cleared');
  assert.equal(shell.cardEl, null);
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
c.stopNode();
console.log('scenario: ok (tabs and cards; short payment PartPaid then PaidInstant; meal PaidLightning)');
