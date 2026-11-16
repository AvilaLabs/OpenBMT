// The proposed checkout stays inside the shared walkthrough and device frame.
// All payment actors are illustrative; no external payment service is involved.
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';

const require = createRequire(import.meta.url);
const Checkout = require('../web2/checkout.js');

function button(label) { return { role: 'button', label, onTap: null }; }
function uiFor(size) {
  const ui = {
    size, pal: { secondary: 'secondary', good: { ink: 'good' }, warn: { ink: 'warn' }, stop: { ink: 'stop' } },
    context: { width: 600 }, px: (n) => n,
    text: (text) => ({ text }),
    facts: (items) => ({ text: items.map((x) => x.join(': ')).join(' | ') }),
    receipt: (items) => ({ text: items.map((x) => x.text).join(' | ') }),
    codeCard: (qr) => ({ text: 'sample QR', qr }),
    bar: (title) => ({ text: title }),
    page: (_bar, body, actions) => ({ text: body.map((x) => x.text || '').join(' | '), actions }),
  };
  for (const kind of ['primary', 'secondary', 'quiet']) ui[kind] = (label) => button(label);
  return ui;
}

function makeShell() {
  const shell = {
    onStep: null, onLeave: null, onRestart: null, screen: null, actorState: null,
    targetsByLabel: new Map(), steps: [], devices: [],
    begin(steps, devices, actorAfter) {
      this.steps = steps; this.devices = devices; this.actorAfter = actorAfter;
      this.targetsByLabel.clear(); this.screen = null;
    },
    scenario(name) { this.selectedScenario = name; },
    show(_id, build) { this.screen = build(700); },
    light(id) { this.lit = id; }, step(i) { this.currentStep = i; },
    say(_happened, parts) { this.instruction = parts.map((x) => typeof x === 'string' ? x : x.b).join(''); },
    questions(q) { this.questionList = q; },
    actor(spec) { this.actorState = spec; },
    target(_id, label, onTap) { this.targetsByLabel.set(label, onTap); },
    targets(_id, list) { for (const x of list) this.targetsByLabel.set(x.label, x.onTap); },
    finish(spec) { this.finished = spec; },
    tap(label) { const action = this.targetsByLabel.get(label); assert.equal(typeof action, 'function', `target: ${label}`); action(); },
    chooseStep(i) { assert.equal(typeof this.onStep, 'function'); this.onStep(i); },
    leave() { this.onLeave?.(); },
  };
  return shell;
}

const U = { LineStyle: { Plain: 'plain', Strong: 'strong' } };
const C = {
  TABLET: 'tablet', PHONE: 'phone',
  deviceUi: (_palette, _dark, _logo, size) => uiFor(size),
  Device: (_ui, page) => page,
};
const CV = { QrView: (_ui, matrix) => ({ text: `QR ${matrix.length}x${matrix[0].length}` }) };
const context = { U, C, CV, palette: {}, assets: { logo: 'logo' } };

// Bitcoin order: one merchant device, cart, payment view, customer actor and receipt.
{
  const shell = makeShell(); Checkout.open(shell, context);
  assert.equal(shell.selectedScenario, 'OpenBMT checkout');
  assert.deepEqual(shell.steps, Checkout.STEPS);
  assert.deepEqual(shell.devices.map((x) => x.id), ['merchant']);
  assert.equal(shell.actorAfter, 'merchant');
  assert.match(shell.screen.text, /Oat latte/);
  assert.match(shell.instruction, /New sale/);

  shell.tap('New sale');
  assert.equal(shell.currentStep, 1);
  assert.match(shell.screen.text, /\$10\.00 USD/);
  shell.tap('Continue');
  shell.tap('Bitcoin');
  assert.equal(shell.currentStep, 3);
  assert.match(shell.screen.text, /Demo code · no payment/);
  assert.match(shell.screen.text, /cannot pay this order/);
  assert.equal(shell.actorState.who, 'Customer');
  shell.actorState.onTap();
  assert.equal(shell.currentStep, 4);
  assert.match(shell.screen.text, /Payment complete/);
  assert.match(shell.screen.text, /Receipt · Order #1042/);
  shell.chooseStep(1);
  assert.equal(shell.currentStep, 4, 'completed order cannot be reopened for a second tender');
  shell.chooseStep(0);
  assert.match(shell.screen.text, /Recent receipts/);
  assert.match(shell.screen.text, /Order #1042/);
  shell.tap('New sale');
  assert.match(shell.screen.text, /\$10\.00 USD/);
  shell.chooseStep(3);
  assert.equal(shell.currentStep, 2, 'a new sale cannot inherit the previous Bitcoin method');
  assert.equal(shell.actorState, null, 'a fresh sale does not expose a stale payment actor');
  assert.match(shell.screen.text, /Choose how to pay/);
  shell.onRestart();
}

// An unclear card result stays on the same attempt during navigation and recovery.
{
  const shell = makeShell(); Checkout.open(shell, context);
  shell.tap('New sale'); shell.tap('Continue'); shell.tap('Card');
  shell.actorState.onTap();
  assert.equal(shell.currentStep, 3);
  assert.match(shell.screen.text, /Card result not confirmed/);
  shell.chooseStep(3);
  assert.match(shell.screen.text, /Card result not confirmed/);
  assert.equal(shell.targetsByLabel.has('Submit card handoff (demo)'), false);
  shell.tap('Check card status');
  assert.match(shell.screen.text, /Checking card attempt/);
  shell.chooseStep(4);
  assert.equal(shell.currentStep, 3, 'Receipt cannot resolve an unknown card result');
  assert.match(shell.screen.text, /Checking card attempt/);
  shell.actorState.onTap();
  assert.equal(shell.currentStep, 4);
  assert.match(shell.screen.text, /Payment · Card/);
  shell.onRestart();
}

// A waiting sale can be canceled before payment is submitted; uncertain card state cannot.
{
  const shell = makeShell(); Checkout.open(shell, context);
  shell.tap('New sale'); shell.tap('Continue'); shell.tap('Card');
  shell.tap('Cancel sale');
  assert.equal(shell.currentStep, 4);
  assert.match(shell.screen.text, /Order canceled/);
  assert.match(shell.screen.text, /No receipt was issued/);
  shell.onRestart();
}

// Navigation away parks a pending attempt for explicit same-session recovery; Start over discards it.
{
  const first = makeShell(); Checkout.open(first, context);
  first.tap('New sale'); first.tap('Continue'); first.tap('Card'); first.leave();
  const reopened = makeShell(); Checkout.open(reopened, context);
  assert.match(reopened.screen.text, /Interrupted order #1042/);
  reopened.tap('Resume interrupted sale');
  assert.equal(reopened.currentStep, 3);
  assert.match(reopened.screen.text, /Card handoff/);
  reopened.onRestart();
  const fresh = makeShell(); Checkout.open(fresh, context);
  assert.match(fresh.screen.text, /Oat latte/);
  assert.doesNotMatch(fresh.screen.text, /Interrupted order/);
}
