// Checkout model integration through the shell-facing actor and target handlers.
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';

const require = createRequire(import.meta.url);
const Checkout = require('../web2/checkout.js');
let lastInput = null;

function button(label) {
  return { role: 'button', label, onTap: null };
}

function uiFor() {
  const ui = {
    pal: {
      secondary: 'secondary', text: 'text',
      good: { ink: 'good' }, warn: { ink: 'warn' }, stop: { ink: 'stop' },
    },
    context: { width: 600 },
    px: (n) => n,
    text: (text) => ({ text }),
    facts: (rows) => ({ text: rows.map((row) => row.join(': ')).join(' | ') }),
    receipt: (rows) => ({ text: rows.map((row) => row.text).join(' | ') }),
    codeCard: () => ({ text: 'sample QR' }),
    bar: (title) => ({ text: title }),
    page: (_bar, body, actions) => ({ text: body.map(textOf).join(' | '), actions }),
    field: () => {
      lastInput = {
        value: '',
        setAttribute() {},
        addEventListener(_kind, fn) { this.onInput = fn; },
      };
      return lastInput;
    },
  };
  for (const kind of ['primary', 'secondary', 'quiet']) ui[kind] = button;
  return ui;
}

function textOf(item) {
  return item.text || item.label || (item.children || []).map(textOf).join(' ');
}

function makeShell() {
  return {
    onStep: null,
    onLeave: null,
    onRestart: null,
    targetsByLabel: new Map(),
    actorState: null,
    begin(steps, devices, actorAfter) {
      this.steps = steps;
      this.devices = devices;
      this.actorAfter = actorAfter;
      this.targetsByLabel.clear();
    },
    scenario(name) { this.selectedScenario = name; },
    show(_id, build) { this.targetsByLabel.clear(); this.screen = build(700); },
    light() {},
    step(index) { this.currentStep = index; },
    say(_happened, parts) {
      this.instruction = parts.map((part) => typeof part === 'string' ? part : part.b).join('');
    },
    questions(list) { this.questionList = list; },
    actor(spec) { this.actorState = spec; },
    target(_id, label, onTap) { this.targetsByLabel.set(label, onTap); },
    targets(_id, list) {
      for (const item of list) this.targetsByLabel.set(item.label, item.onTap);
    },
    finish(spec) { this.finished = spec; },
    tap(label) {
      const action = this.targetsByLabel.get(label);
      assert.equal(typeof action, 'function', `target: ${label}`);
      action();
    },
    chooseStep(index) { this.onStep(index); },
  };
}

const U = {
  LineStyle: { Plain: 'plain', Strong: 'strong' },
  WRAP_CONTENT: -2,
  LinearLayout: () => ({ children: [] }),
  LayoutParams: class { constructor(width, height) { this.width = width; this.height = height; } },
  addView(parent, child) { parent.children.push(child); },
};
const C = {
  TABLET: 'tablet', PHONE: 'phone',
  deviceUi: () => uiFor(),
  Device: (_ui, page) => page,
};
const context = { U, C, CV: { QrView: () => ({ text: 'QR' }) }, palette: {}, assets: { logo: 'logo' } };

// Editable quantity cart uses exact cents and prevents an empty payment.
{
  const shell = makeShell();
  Checkout.open(shell, context);
  assert.match(shell.screen.text, /sample catalog/);
  shell.tap('New sale');
  shell.tap('Oat latte −');
  assert.match(shell.screen.text, /\$4\.50 USD/);
  shell.tap('Sesame bagel −');
  shell.tap('Oat latte −');
  assert.match(shell.screen.text, /empty cart cannot be paid/);
  shell.tap('Continue');
  assert.match(shell.screen.text, /Add at least one item/);
  shell.tap('Oat latte +');
  shell.tap('Continue');
  assert.match(shell.screen.text, /\$5\.50 USD/);
  shell.tap('Bitcoin');
  shell.actorState.onTap();
  assert.match(shell.screen.text, /Receipt/);
  assert.match(shell.screen.text, /simulated in this example/i);

  shell.tap('Request refund');
  assert.match(shell.screen.text, /Refund request recorded for this order/);
  assert.equal(shell.targetsByLabel.has('Request refund'), false, 'a completed request is no longer offered');
  shell.tap('Order history');
  assert.match(shell.screen.text, /refund requested/);
  shell.tap('Order #1042 · paid · $5.50 · Bitcoin · refund requested');
  assert.match(shell.screen.text, /Receipt · Order #1042/);
  assert.match(shell.screen.text, /not paid/);
  shell.tap('Order history');
  shell.tap('New sale');
  assert.match(shell.screen.text, /Order: #1043/);
  shell.tap('Cancel sale');
  shell.tap('Order history');
  shell.tap('Order #1042 · paid · $5.50 · Bitcoin · refund requested');
  shell.tap('Done');
  assert.equal(shell.finished.heading, 'Order #1042 is complete.', 'Done describes the historical order being viewed');
  shell.onRestart();
}

// Cash must be fully tendered; the receipt captures both cash and change.
{
  const shell = makeShell();
  Checkout.open(shell, context);
  shell.tap('New sale');
  shell.tap('Continue');
  shell.tap('Cash');
  lastInput.value = '9.99';
  lastInput.onInput();
  shell.tap('Record cash');
  assert.match(shell.screen.text, /below the amount due/);
  lastInput.value = '12.00';
  lastInput.onInput();
  shell.tap('Record cash');
  assert.match(shell.screen.text, /Cash received · \$12\.00 USD/);
  assert.match(shell.screen.text, /Change · \$2\.00 USD/);
  shell.onRestart();
}

// Unknown card results preserve one attempt and prevent another payment route.
{
  const shell = makeShell();
  Checkout.open(shell, context);
  shell.tap('New sale');
  shell.tap('Continue');
  shell.tap('Card');
  assert.doesNotMatch(shell.screen.text, /attempt-1/, 'raw attempt IDs stay out of the cashier-facing screen');
  shell.actorState.onTap();
  assert.match(shell.screen.text, /Card result not confirmed/);
  assert.equal(shell.targetsByLabel.has('Bitcoin'), false);
  assert.equal(shell.targetsByLabel.has('Simulate card submission'), false);
  shell.tap('Check card status');
  const actor = shell.actorState;
  assert.equal(actor.button, 'Confirm card payment (demo)');
  actor.onTap();
  assert.match(shell.screen.text, /Status · paid/);
  assert.match(shell.screen.text, /Payment · Card/);
  shell.onRestart();
}

// Canceled order IDs are consumed, and history lets the cashier inspect the old order.
{
  const shell = makeShell();
  Checkout.open(shell, context);
  shell.tap('New sale');
  shell.tap('Cancel sale');
  assert.match(shell.screen.text, /Status · cancelled/);
  shell.tap('Order history');
  shell.tap('Order #1042 · cancelled · $10.00 · none');
  assert.match(shell.screen.text, /Order canceled/);
  shell.tap('New sale');
  assert.match(shell.screen.text, /Order: #1043/);
  shell.onRestart();
}
