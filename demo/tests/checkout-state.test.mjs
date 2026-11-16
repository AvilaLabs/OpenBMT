// Behavior and accessibility checks for the proposed checkout; external outcomes remain demo controls only.
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';

class Node_ {
  constructor(tag) { this.tag = tag; this.children = []; this.handlers = {}; this.attrs = {}; this.text = ''; this.className = ''; this.type = ''; this.hidden = false; this.disabled = false; }
  set textContent(v) { for (const c of this.children) c.parent = null; this.children = []; this.text = String(v); }
  get textContent() { return this.text + this.children.map((n) => n.textContent).join(''); }
  append(...nodes) { for (const node of nodes) { if (node.parent) node.parent.children = node.parent.children.filter((child) => child !== node); this.children.push(node); node.parent = this; } }
  replaceChildren(...nodes) { for (const child of this.children) child.parent = null; this.children = []; this.append(...nodes); }
  setAttribute(k, v) { this.attrs[k] = String(v); }
  addEventListener(k, f) { (this.handlers[k] ||= []).push(f); }
  click() { if (!this.disabled) for (const f of this.handlers.click || []) f({}); }
  all(pred, out = []) { if (pred(this)) out.push(this); for (const c of this.children) c.all(pred, out); return out; }
}
const memory = new Map();
globalThis.localStorage = { getItem: (k) => memory.get(k) || null, setItem: (k, v) => memory.set(k, v), removeItem: (k) => memory.delete(k) };
globalThis.document = { createElement: (tag) => new Node_(tag) };
const require = createRequire(import.meta.url);
const Checkout = require('../web2/checkout.js');
const makeShell = () => ({ stage: new Node_('main'), say() {}, begin() {}, scenario(name) { this.selectedScenario = name; }, onRestart: null, onLeave: null });
const stageButtons = (shell) => shell.stage.all((x) => x.tag === 'button');
const find = (shell, label) => stageButtons(shell).find((b) => b.textContent === label);
const click = (shell, label) => { const b = find(shell, label); assert.ok(b, `button exists: ${label}`); b.click(); };
const appText = (shell) => shell.stage.all((x) => x.attrs['aria-label'] === 'Juniper Coffee merchant checkout')[0].textContent;
const controlsText = (shell) => shell.stage.all((x) => x.attrs['aria-label'] === 'Test-only payment simulation controls')[0].textContent;
const reset = () => { memory.clear(); };

// Merchant sees a single cart and real checkout choices; payment outcomes are outside its app frame.
{
  reset(); const shell = makeShell(); Checkout.open(shell);
  assert.equal(shell.selectedScenario, 'OpenBMT checkout');
  assert.match(appText(shell), /Oat latte/); assert.match(appText(shell), /Sesame bagel/);
  assert.match(appText(shell), /\$10\.00 USD/);
  assert.doesNotMatch(appText(shell), /Simulate partial|Complete card test|Reconcile/);
  assert.match(controlsText(shell), /test only/);
  click(shell, 'Pay with Bitcoin');
  assert.match(appText(shell), /Waiting for Bitcoin/); assert.match(appText(shell), /Payment request/);
  click(shell, 'Simulate partial receipt · $4.00');
  assert.match(appText(shell), /\$4\.00 received/);
  click(shell, 'Pause payment request');
  assert.match(appText(shell), /Payment paused/);
  const resumed = makeShell(); Checkout.open(resumed);
  click(resumed, 'Resume payment');
  assert.match(appText(resumed), /\$4\.00 received/);
  click(resumed, 'Simulate remaining receipt · $6.00');
  assert.match(appText(resumed), /Payment complete/);
  assert.match(appText(resumed), /Receipt · Juniper Coffee/);
  click(resumed, 'Request refund');
  assert.match(appText(resumed), /awaiting owner review/);
  click(resumed, 'Review refund request');
  assert.match(appText(resumed), /awaiting approval/);
  click(resumed, 'Approve test refund');
  assert.match(appText(resumed), /simulation still pending/);
  assert.doesNotMatch(appText(resumed), /Refund simulation complete/);
  click(resumed, 'Complete refund simulation');
  assert.match(appText(resumed), /Refund complete/);
}

// A lost card callback blocks a fresh tender until reconciliation resolves the outcome.
{
  reset(); const shell = makeShell(); Checkout.open(shell);
  click(shell, 'Pay with card');
  assert.match(appText(shell), /awaiting result/);
  click(shell, 'Lose callback · mark result uncertain');
  assert.match(appText(shell), /do not retry/);
  assert.doesNotMatch(appText(shell), /Pay with Bitcoin|Pay with card/);
  click(shell, 'Reconcile · confirm attempt failed');
  assert.match(appText(shell), /Card attempt failed · no payment received/);
  click(shell, 'Try payment again');
  click(shell, 'Pay with card');
  click(shell, 'Complete card test payment');
  assert.match(appText(shell), /Payment: Card/);
}

// Cancelling after partial funds keeps the amount visible and requires owner follow-up before another tender.
{
  reset(); const shell = makeShell(); Checkout.open(shell);
  click(shell, 'Pay with Bitcoin');
  click(shell, 'Simulate partial receipt · $4.00');
  click(shell, 'Cancel with funds received');
  assert.match(appText(shell), /\$4\.00 received/);
  assert.match(appText(shell), /owner follow-up required/);
  assert.doesNotMatch(appText(shell), /Pay with Bitcoin|Pay with card/);
  click(shell, 'Record owner follow-up complete');
  assert.match(appText(shell), /\$4\.00 received · owner follow-up recorded/);
  assert.match(appText(shell), /funds remain recorded against this order/);
  assert.match(controlsText(shell), /Owner follow-up recorded/);
  assert.doesNotMatch(appText(shell), /Pay with Bitcoin|Pay with card/);
}

// Cart can be adjusted before tender; corrupt persisted state cannot restore a paid screen.
{
  reset(); const shell = makeShell(); Checkout.open(shell);
  const inc = stageButtons(shell).find((b) => b.attrs['aria-label'] === 'Add one Oat latte');
  assert.ok(inc); inc.click();
  assert.match(appText(shell), /\$15\.50 USD/);
  memory.set(Checkout.KEY, JSON.stringify({ version: 1, order: '1042', status: 'paid', method: 'Bitcoin', paid: 10, qty: [999, 1], history: [] }));
  const recovered = makeShell(); Checkout.open(recovered);
  assert.match(appText(recovered), /Choose how your customer will pay/);
  assert.doesNotMatch(appText(recovered), /Payment complete/);
  assert.equal(memory.has(Checkout.KEY), false, 'corrupt checkout data is discarded');
}

// Paid orders can continue without a refund; a bounded receipt snapshot survives the next order and a reload.
{
  reset(); const shell = makeShell(); Checkout.open(shell);
  click(shell, 'Pay with Bitcoin');
  click(shell, 'Simulate partial receipt · $4.00');
  click(shell, 'Simulate remaining receipt · $6.00');
  assert.match(appText(shell), /Receipt · Juniper Coffee/);
  click(shell, 'Start next order');
  assert.match(appText(shell), /Order #1043/);
  assert.match(appText(shell), /Choose how your customer will pay/);
  assert.match(appText(shell), /Previous demo receipts/);
  assert.match(appText(shell), /Order #1042 · \$10\.00 USD · Bitcoin · 1 × Oat latte · 1 × Sesame bagel/);
  assert.doesNotMatch(appText(shell), /Request refund/);

  const reloaded = makeShell(); Checkout.open(reloaded);
  assert.match(appText(reloaded), /Order #1043/);
  assert.match(appText(reloaded), /Order #1042 · \$10\.00 USD · Bitcoin · 1 × Oat latte · 1 × Sesame bagel/);
  assert.doesNotMatch(appText(reloaded), /Refund complete/);
}

// Version 1 sessions without receipts migrate safely to version 2 with an empty receipt list.
{
  reset();
  memory.set(Checkout.KEY, JSON.stringify({ version: 1, order: '1042', status: 'ready', method: null, paid: 0, qty: [1, 1], history: [] }));
  const shell = makeShell(); Checkout.open(shell);
  assert.doesNotMatch(appText(shell), /Previous demo receipts/);
  click(shell, 'Pay with Bitcoin');
  const saved = JSON.parse(memory.get(Checkout.KEY));
  assert.equal(saved.version, 2);
  assert.deepEqual(saved.receipts, []);
  const malformedReceipt = { version: 2, order: '1042', status: 'ready', method: null, paid: 0, qty: [1, 1], history: [], receipts: [
    { order: '1041', total: 'ten dollars', method: 'Bitcoin', items: '1 × Oat latte' },
  ] };
  assert.equal(Checkout.validState(malformedReceipt), false, 'receipt snapshots are shape-validated');
}

// All actions are native keyboard buttons, and dynamic payment state is announced to assistive technology.
{
  reset(); const shell = makeShell(); Checkout.open(shell); click(shell, 'Pay with card');
  for (const b of stageButtons(shell)) {
    assert.equal(b.type, 'button');
    assert.notEqual(b.attrs.tabindex, '-1');
  }
  const live = shell.stage.all((x) => x.attrs['aria-live'] === 'polite');
  assert.ok(live.length >= 1);
}

// A browser that refuses persistence says recovery is unavailable instead of claiming the session was saved.
{
  reset();
  const oldSet = globalThis.localStorage.setItem;
  globalThis.localStorage.setItem = () => { throw new Error('storage unavailable'); };
  const shell = makeShell(); Checkout.open(shell);
  click(shell, 'Pay with Bitcoin');
  assert.match(controlsText(shell), /could not save the demo session/);
  globalThis.localStorage.setItem = oldSet;
}
