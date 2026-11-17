// Domain invariants for the illustrative, in-memory checkout contract.
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import vm from 'node:vm';

const require = createRequire(import.meta.url);
const Sale = require('../web2/sale-contract.js');

test('catalog and draft sales are immutable and use exact minor-unit arithmetic', () => {
  const draft = Sale.create(1042);
  assert.equal(draft.currency, 'USD');
  assert.deepEqual(Sale.ITEMS.map(({ id, unitMinor }) => [id, unitMinor]), [['oat-latte', 550], ['sesame-bagel', 450]]);
  assert.equal(Sale.totalMinor(draft), 1000);
  assert.equal(Object.isFrozen(draft), true);
  assert.equal(Object.isFrozen(draft.qty), true);
  assert.equal(Object.isFrozen(Sale.ITEMS[0]), true);
  assert.throws(() => Sale.create(1043, [1]), /qty/);
  assert.throws(() => Sale.create(0), /order/);
  for (const invalid of [NaN, Infinity, -1, 1.25, Number.MAX_SAFE_INTEGER + 1]) {
    assert.throws(() => Sale.create(1, [invalid, 1]));
  }
  assert.throws(() => Sale.create(1, [100, 1]), /0 and 99/);
  assert.throws(() => Sale.setQuantity(draft, 2, 1), /out of range/);
  assert.throws(() => Sale.setQuantity(draft, 0, 1.5));
  assert.throws(() => Sale.begin(Sale.create(1043, [0, 0]), 'Bitcoin'), /greater than zero/);
});

test('the browser script exposes the same contract global without a module loader', () => {
  const source = readFileSync(new URL('../web2/sale-contract.js', import.meta.url), 'utf8');
  const browser = vm.runInNewContext(`${source}\nOpenBmtSaleContract`, {});
  assert.deepEqual(Array.from(browser.ITEMS, (item) => [item.id, item.unitMinor]), [['oat-latte', 550], ['sesame-bagel', 450]]);
  assert.equal(browser.totalMinor(browser.create(1043)), 1000);
});

test('quantity changes return a new state and the basket locks when a payment attempt begins', () => {
  const draft = Sale.create(1044);
  const changed = Sale.setQuantity(draft, 0, 2);
  assert.deepEqual(draft.qty, [1, 1]);
  assert.deepEqual(changed.qty, [2, 1]);
  assert.equal(Sale.totalMinor(changed), 1550);
  const waiting = Sale.begin(changed, 'Bitcoin');
  assert.equal(waiting.status, 'waitingBitcoin');
  assert.equal(waiting.method, 'Bitcoin');
  assert.match(waiting.attemptId, /1044-bitcoin/);
  assert.equal(Sale.begin(changed, 'Bitcoin').attemptId, waiting.attemptId, 'replaying the same draft keeps a stable demo attempt identity');
  assert.throws(() => Sale.setQuantity(waiting, 0, 3), /locked/);
  assert.throws(() => Sale.begin(waiting, 'Card'), /only a draft/);
  assert.throws(() => Sale.confirm(Sale.begin(changed, 'Card')), /awaiting/);
  assert.throws(() => Sale.confirm(Sale.cancel(waiting)), /awaiting/);
});

test('card submission is uncertain until explicitly checked, then can be confirmed', () => {
  const waiting = Sale.begin(Sale.create(1045), 'Card');
  const uncertain = Sale.cardSubmitted(waiting);
  assert.equal(uncertain.status, 'uncertainCard');
  assert.equal(uncertain.attemptId, waiting.attemptId);
  assert.throws(() => Sale.confirm(uncertain), /awaiting/);
  const checking = Sale.checkCard(uncertain);
  assert.equal(checking.status, 'checkingCard');
  const paid = Sale.confirm(checking);
  assert.equal(paid.status, 'paid');
  assert.throws(() => Sale.checkCard(checking), /invalid transition/);
  assert.throws(() => Sale.cardSubmitted(uncertain), /invalid transition/);
});

test('cash must cover the whole balance and records exact change without mutating a rejected attempt', () => {
  const waiting = Sale.begin(Sale.create(1046), 'Cash');
  assert.equal(waiting.status, 'waitingCash');
  assert.throws(() => Sale.recordCash(waiting, 999), /below the full balance/);
  assert.equal(waiting.status, 'waitingCash');
  assert.throws(() => Sale.recordCash(waiting, 1000.5));
  const paid = Sale.recordCash(waiting, 1200);
  assert.equal(paid.status, 'paid');
  assert.equal(paid.cashReceivedMinor, 1200);
  assert.equal(paid.changeMinor, 200);
  const receipt = Sale.receipt(paid);
  assert.equal(receipt.totalMinor, 1000);
  assert.equal(receipt.currency, 'USD');
  assert.equal(receipt.simulated, true);
  assert.equal(Object.isFrozen(receipt), true);
  assert.equal(Object.isFrozen(receipt.items), true);
  assert.equal(Object.isFrozen(receipt.items[0]), true);
  assert.throws(() => Sale.recordCash(paid, 1200), /only be recorded/);
});

test('receipt snapshot omits catalog items with a zero quantity', () => {
  const oneItem = Sale.setQuantity(Sale.create(1048), 1, 0);
  const receipt = Sale.receipt(Sale.confirm(Sale.begin(oneItem, 'Bitcoin')));
  assert.equal(receipt.totalMinor, 550);
  assert.deepEqual(receipt.items.map(({ id, quantity, lineMinor }) => [id, quantity, lineMinor]), [['oat-latte', 1, 550]]);
});

test('cancel is limited to draft or unsubmitted waiting states, and paid receipt history stays a snapshot', () => {
  const draft = Sale.create(1047);
  const canceled = Sale.cancel(draft);
  assert.equal(canceled.status, 'cancelled');
  assert.throws(() => Sale.cancel(canceled), /cannot be canceled/);
  assert.throws(() => Sale.receipt(canceled), /requires a paid/);
  const paid = Sale.confirm(Sale.begin(draft, 'Bitcoin'));
  const before = Sale.receipt(paid);
  const requested = Sale.requestRefund(paid);
  assert.match(requested.refundRequestId, /1047-refund-1/);
  assert.equal(Sale.requestRefund(requested), requested, 'a replay returns the same request state');
  assert.equal(Sale.receipt(paid).refundRequested, false, 'the original paid receipt is unchanged');
  const after = Sale.receipt(requested);
  assert.equal(before.refundRequested, false);
  assert.equal(after.refundRequested, true);
  assert.equal(after.refundRequestId, requested.refundRequestId);
  assert.equal(after.status, 'paid', 'a request does not represent a completed reversal');
  assert.throws(() => Sale.requestRefund(draft), /only a paid/);
});
