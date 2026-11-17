// Navigation integration checks: actual Shell lifecycle and the Shell-backed checkout walkthrough.
// Usage: node demo/tests/checkout-navigation.test.mjs
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';

class Node_ {
  constructor(tag) {
    this.tag = tag; this.children = []; this.parent = null; this.attrs = {}; this.handlers = {}; this.text = '';
    this.style = {}; this.hidden = false; this.disabled = false; this.inert = false; this.value = ''; this.id = '';
    this.classes = new Set(); this.clientHeight = 700; this.clientWidth = 1100; this.offsetHeight = 0;
    this.classList = {
      add: (...xs) => xs.forEach((x) => this.classes.add(x)),
      remove: (...xs) => xs.forEach((x) => this.classes.delete(x)),
      contains: (x) => this.classes.has(x),
      toggle: (x, on) => { const next = on === undefined ? !this.classes.has(x) : on; if (next) this.classes.add(x); else this.classes.delete(x); return next; },
    };
  }
  set className(v) { this.classes = new Set(String(v).split(/\s+/).filter(Boolean)); }
  get className() { return [...this.classes].join(' '); }
  set textContent(v) { this.children.forEach((c) => { c.parent = null; }); this.children = []; this.text = String(v); }
  get textContent() { return this.text + this.children.map((c) => c.textContent).join(''); }
  set innerHTML(v) { this.html = v; }
  get firstElementChild() { return this.children.find((c) => !c.tag.startsWith('#')) || null; }
  append(...nodes) {
    for (const node of nodes) {
      const n = typeof node === 'string' ? Object.assign(new Node_('#text'), { text: node }) : node;
      if (n.parent) n.remove();
      this.children.push(n); n.parent = this;
    }
  }
  after(node) {
    if (!this.parent) return;
    if (node.parent) node.remove();
    const at = this.parent.children.indexOf(this); this.parent.children.splice(at + 1, 0, node); node.parent = this.parent;
  }
  replaceChildren(...nodes) { this.children.forEach((c) => { c.parent = null; }); this.children = []; this.append(...nodes); }
  remove() { if (this.parent) this.parent.children = this.parent.children.filter((c) => c !== this); this.parent = null; }
  setAttribute(k, v) { this.attrs[k] = String(v); }
  getAttribute(k) { return this.attrs[k] ?? null; }
  removeAttribute(k) { delete this.attrs[k]; }
  addEventListener(k, fn) { (this.handlers[k] ||= []).push(fn); }
  removeEventListener(k, fn) { this.handlers[k] = (this.handlers[k] || []).filter((f) => f !== fn); }
  click() { if (!this.disabled) for (const fn of [...(this.handlers.click || [])]) fn({ stopPropagation() {}, preventDefault() {} }); }
  focus() { globalThis.document.activeElement = this; }
  animate() { return { onfinish: null, cancel() {}, playState: 'finished' }; }
  getBoundingClientRect() { return { width: 0, left: 0, top: 0 }; }
  contains(node) { return node === this || this.children.some((c) => c.contains(node)); }
  all(pred, out = []) { if (pred(this)) out.push(this); for (const c of this.children) c.all(pred, out); return out; }
  querySelectorAll(selector) {
    if (selector.includes(':is(button')) return this.all((n) => n !== this && (n.tag === 'button' || n.attrs.role === 'button'));
    const parts = selector.split(',').map((s) => s.trim());
    const match = (node, part) => {
      if (part.startsWith('.')) {
        const [cls, notCls] = part.slice(1).split(':not(.');
        return node.classList.contains(cls) && (!notCls || !node.classList.contains(notCls.replace(/\)$/, '')));
      }
      const attr = part.match(/^\[([^=\]]+)=?["']?([^"'\]]*)["']?\]$/);
      if (attr) return node.attrs[attr[1]] !== undefined && (!attr[2] || node.attrs[attr[1]] === attr[2]);
      if (part === 'button' || part === 'button, [role=button]') return node.tag === 'button' || node.attrs.role === 'button';
      if (part === '[role=button]') return node.attrs.role === 'button';
      return false;
    };
    return this.all((node) => node !== this && parts.some((p) => match(node, p)));
  }
  querySelector(selector) { return this.querySelectorAll(selector)[0] || null; }
}

const docHandlers = new Map();
globalThis.document = {
  createElement: (tag) => new Node_(tag),
  createTextNode: (text) => Object.assign(new Node_('#text'), { text: String(text) }),
  addEventListener: (kind, fn) => { const list = docHandlers.get(kind) || []; list.push(fn); docHandlers.set(kind, list); },
  removeEventListener: (kind, fn) => docHandlers.set(kind, (docHandlers.get(kind) || []).filter((x) => x !== fn)),
};
globalThis.localStorage = { getItem: () => null, setItem() {}, removeItem() {} };
globalThis.matchMedia = () => ({ matches: true }); // Shell's reduced-motion path avoids animation completion timing.
globalThis.innerWidth = 1100;
globalThis.addEventListener = () => {};
globalThis.getComputedStyle = () => ({ columnGap: '16', paddingTop: '0', paddingBottom: '0', paddingLeft: '0', paddingRight: '0' });
globalThis.location = { assign() {} };

const require = createRequire(import.meta.url);
const Shell = require('../web2/shell.js');
const Checkout = require('../web2/checkout.js');

const queryButtons = (root) => root.querySelectorAll('button');
const button = (root, text) => queryButtons(root).find((b) => b.textContent.trim() === text);
const click = (root, text) => { const b = button(root, text); assert.ok(b, `button exists: ${text}`); b.click(); };
const clickShell = (shell, text) => click(shell.root, text);
const checkoutText = (shell) => shell.slots.get(Checkout.DEVICE_ID)?.phone.firstElementChild?.textContent || '';
const currentStep = (shell) => shell.stepList.children.findIndex((li) => li.classList.contains('now'));

// A small view adapter makes the real checkout route draw into the real Shell without importing the live WASM engine.
const size = (width, height) => ({ width, height, label: width > 500 ? 'Tablet' : 'Phone' });
const uiFor = () => {
  const node = (tag, text = '') => { const n = new Node_(tag); n.textContent = text; return n; };
  const button = (text) => node('button', text);
  return {
    context: { width: 412 },
    pal: { secondary: '#666', warn: { ink: '#765' }, stop: { ink: '#900' }, good: { ink: '#080' } },
    facts: (rows) => node('section', rows.map((r) => r.join(' · ')).join(' ')),
    text: (text) => node('p', text),
    receipt: (lines) => node('section', lines.map((line) => line.text).join(' ')),
    primary: button, secondary: button, quiet: button,
    px: (value) => value,
    codeCard: (code) => code,
    bar: (label) => node('header', label || ''),
    page: (bar, body, actions) => { const p = node('div'); p.append(bar, ...body, ...actions); return p; },
  };
};
const context = {
  U: {
    LineStyle: { Plain: 'plain', Strong: 'strong' },
    WRAP_CONTENT: -2,
    LinearLayout: () => new Node_('div'),
    LayoutParams: class { constructor(width, height) { this.width = width; this.height = height; } },
    addView(parent, child) { parent.append(child); },
  },
  C: {
    TABLET: size(700, 900), PHONE: size(360, 740),
    deviceUi: () => uiFor(),
    Device: (_ui, page) => page,
  },
  CV: { QrView: (_ui, code) => new Node_('div') },
  palette: {}, assets: { logo: 'demo-logo.png' },
};

function merchantLikeRoute(shell) {
  shell.begin(['Sale', 'Receipt'], [{
    id: 'merchant', caption: 'Merchant register',
    sizes: [{ label: 'Tablet', size: context.C.TABLET }, { label: 'Phone', size: context.C.PHONE }],
  }]);
  shell.scenario('Café');
  let screen = 'Sale';
  const show = () => shell.show('merchant', (s) => {
    const page = new Node_('article'); page.textContent = `${screen} · ${s.label}`; return page;
  });
  show(); shell.step(0);
  shell.onStep = (i) => { screen = i ? 'Receipt' : 'Sale'; show(); shell.step(i); };
}

function run() {
  const shell = new Shell(new Node_('div'));
  let staleStepCalled = 0;
  const start = () => {
    shell.scenario(null);
    shell.start([{ name: 'OpenBMT checkout', what: 'test', go: 'Explore', onPick: () => shell.pick('OpenBMT checkout') }]);
  };
  shell.onRestart = start;
  shell.onScenario = (name) => {
    shell.leave();
    if (name === 'OpenBMT checkout') Checkout.open(shell, context);
    else if (name === 'Café') merchantLikeRoute(shell);
  };

  // Enter the checkout through the actual Shell route after a merchant-like route with a live step callback.
  merchantLikeRoute(shell);
  shell.onStep = () => { staleStepCalled++; };
  const staleStep = shell.onStep;
  shell.pick('OpenBMT checkout');
  assert.equal(shell.slots.size, 1, 'checkout uses one Shell-managed merchant device');
  assert.ok(shell.stage.classList.contains('actor-flow'), 'checkout keeps mobile merchant and customer actions in separate layout space');
  assert.ok(shell.slots.has(Checkout.DEVICE_ID));
  assert.deepEqual(shell.stepList.children.map((li) => li.querySelector('button').textContent.slice(1)), Checkout.STEPS);
  assert.notEqual(shell.onStep, staleStep, 'checkout replaces the previous route step callback');
  assert.equal(staleStepCalled, 0);
  assert.equal(currentStep(shell), 0);
  assert.match(checkoutText(shell), /Illustrative sample catalog/);

  // Drive the actual checkout steps using Shell target handlers and role actor; then replay Payment from the step bar.
  clickShell(shell, 'New sale');
  assert.equal(currentStep(shell), 1); assert.match(checkoutText(shell), /Edit your order/);
  clickShell(shell, 'Continue');
  assert.equal(currentStep(shell), 2); assert.match(checkoutText(shell), /Choose how to pay/);
  clickShell(shell, 'Bitcoin');
  assert.equal(currentStep(shell), 3);
  assert.match(checkoutText(shell), /Bitcoin receipt example/);
  // Skipping to Receipt cannot manufacture payment; replay stays on the pending Payment step.
  shell.stepList.children[4].querySelector('button').click();
  assert.equal(currentStep(shell), 3);
  assert.match(checkoutText(shell), /Bitcoin receipt example/);
  assert.doesNotMatch(checkoutText(shell), /Payment complete/);

  // Replaying Payment preserves the pending attempt; the role actor completes it only afterward.
  const paymentStep = shell.stepList.children[3].querySelector('button');
  paymentStep.click();
  assert.equal(currentStep(shell), 3, 'the Payment step invokes checkout replay');
  assert.equal(staleStepCalled, 0, 'old route callback does not fire when a new step target is used');
  assert.match(checkoutText(shell), /Bitcoin receipt example/);
  assert.match(checkoutText(shell), /Order · #1042/, 'replay keeps the same sale ID');
  clickShell(shell, 'Simulate Bitcoin receipt');
  assert.equal(currentStep(shell), 4); assert.match(checkoutText(shell), /Payment complete/);
  assert.match(checkoutText(shell), /Order #1042/);
  // Receipt replay preserves the completed order instead of starting the next order.
  const receiptStep = shell.stepList.children[4].querySelector('button');
  receiptStep.click();
  assert.equal(currentStep(shell), 4); assert.match(checkoutText(shell), /Payment complete/);
  assert.match(checkoutText(shell), /Order #1042/, 'future-step replay does not silently create a new order');

  // The same current screen and sale survive responsive redraws on both device sizes.
  clickShell(shell, 'Phone');
  assert.match(checkoutText(shell), /Payment complete/); assert.match(checkoutText(shell), /Payment · Bitcoin/);
  clickShell(shell, 'Tablet');
  assert.match(checkoutText(shell), /Payment complete/); assert.match(checkoutText(shell), /Payment · Bitcoin/);

  // A new sale clears the previous Bitcoin method before a cashier jumps in the step bar.
  clickShell(shell, 'New sale');
  shell.stepList.children[3].querySelector('button').click();
  assert.equal(currentStep(shell), 2, 'a new sale jumping to Payment first requires a fresh tender choice');
  assert.match(checkoutText(shell), /Choose how to pay/);
  assert.equal(button(shell.root, 'Simulate Bitcoin receipt'), undefined, 'the previous Bitcoin actor is cleared');
  assert.equal(button(shell.root, 'Simulate card submission'), undefined, 'the previous card actor is cleared');
  assert.match(checkoutText(shell), /Order · #1043/, 'the new sale has its own order ID');

  // Cancel the selected Bitcoin tender before payment. A detached actor is inert and the next sale gets a new ID.
  clickShell(shell, 'Bitcoin');
  const lateActor = button(shell.root, 'Simulate Bitcoin receipt');
  assert.ok(lateActor);
  clickShell(shell, 'Cancel sale');
  assert.match(checkoutText(shell), /Order canceled/);
  assert.doesNotMatch(checkoutText(shell), /Payment complete|Receipt · Order/);
  lateActor.click();
  assert.match(checkoutText(shell), /Order canceled/, 'a detached customer action cannot revive a canceled order');
  clickShell(shell, 'New sale'); clickShell(shell, 'Continue');
  assert.match(checkoutText(shell), /Order · #1044/, 'cancellation consumes an order number without reusing it');
  clickShell(shell, 'Card');
  assert.equal(currentStep(shell), 3);
  clickShell(shell, 'Simulate card submission');
  assert.match(checkoutText(shell), /Card result not confirmed/);
  shell.stepList.children[3].querySelector('button').click();
  assert.equal(currentStep(shell), 3);
  assert.match(checkoutText(shell), /Card result not confirmed/, 'Payment replay preserves an unresolved card result');
  assert.equal(button(shell.root, 'Simulate card submission'), undefined, 'uncertain card payment cannot be submitted again');

  // Leaving through another tab invokes checkout cleanup; Shell.begin removes its slot before the next view appears.
  shell.pick('Café');
  assert.equal(shell.stage.classList.contains('actor-flow'), false, 'checkout layout option is cleared on route exit');
  assert.equal(shell.onStep instanceof Function, true, 'new route installs its own replay callback');
  assert.equal(shell.slots.size, 1);
  assert.ok(shell.slots.has('merchant'));
  assert.match(shell.stage.textContent, /Sale · Tablet/);
  assert.doesNotMatch(shell.stage.textContent, /Illustrative sample catalog|Payment complete/);
  shell.stepList.children[1].querySelector('button').click();
  assert.match(shell.stage.textContent, /Receipt · Tablet/);
  shell.pick('OpenBMT checkout');
  assert.match(checkoutText(shell), /Interrupted order/);
  assert.ok(button(shell.root, 'Resume interrupted sale'));
  clickShell(shell, 'Resume interrupted sale');
  assert.equal(currentStep(shell), 3);
  assert.match(checkoutText(shell), /Card result not confirmed/);
  assert.match(checkoutText(shell), /Order · #1044/, 'resuming keeps the interrupted order ID');

  // Start over cleans up callbacks and the interrupted order; re-entry starts a new fresh flow.
  shell.onRestart();
  assert.equal(shell.onLeave, null); assert.equal(shell.onStep, null);
  assert.ok(shell.root.querySelector('.start'));
  shell.pick('OpenBMT checkout');
  assert.equal(currentStep(shell), 0);
  assert.match(checkoutText(shell), /Illustrative sample catalog/);
  assert.doesNotMatch(checkoutText(shell), /Interrupted order|Recent orders/);
  shell.pick('Café');
  assert.equal(shell.slots.size, 1); assert.doesNotMatch(shell.stage.textContent, /Illustrative sample catalog|Interrupted order/);
}

run();
