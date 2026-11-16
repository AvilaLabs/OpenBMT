// The customer's path (demo/web2/customer.js) with a stub page, wallet and host: from the home screen, Bitcoin Wallet
// then Scan, or Hold to the counter; Lightning or on-chain.
//   node --test demo/tests/customer-path.test.mjs
import test from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';

const require = createRequire(import.meta.url);
const PC = require('../web2/payment-code.js');
const QS = require('../web2/questions.js');
const { moments } = require('../web2/customer.js');

const ONCHAIN = 'bitcoin:bcrt1qabc?amount=0.0001';
const WITH_INVOICE = 'bitcoin:bcrt1qabc?amount=0.0001&lightning=lnbcrt1pqqq';

/** The NDEF file of a tag for [text] (TP-02). */
function tagOf(text) {
  const t = new TextEncoder().encode(text);
  const record = [0xd1, 0x01, t.length + 1, 0x55, 0x00, ...t];
  return Int8Array.from([0, record.length, ...record]);
}

/** A page: the shell records what is said, shown and aimed at; the wallet's screens are named values. */
function page(text, { host = true, paid = false } = {}) {
  const log = { says: [], shown: [], targets: null, questions: null, paid: [], lightning: [], pumps: 0, waits: 0 };
  const shell = {
    say(happened, parts, skip) { log.says.push({ happened, parts, skip }); },
    show(id, build) { log.shown.push(build()); },
    light() {},
    questions(list) { log.questions = list; },
    targets(id, list) { log.targets = list; },
    target(id, label, onTap) { log.targets = [{ label, onTap }]; },
    under(id, label) { log.under = label; },
  };
  const Wl = {
    homeScreen: () => ({ screen: 'homeScreen' }),
    home: () => ({ screen: 'home' }),
    scanning: () => ({ screen: 'scanning' }),
    tapping: () => ({ screen: 'tapping' }),
    confirm: (sats, address, pay) => ({ screen: 'confirm', sats, address, pay }),
    sent: (sats) => ({ screen: 'sent', sats }),
  };
  const data = { copyUri: text, tag: tagOf(text), isPaid: paid };
  const env = {
    shell, Wl, QS, PC,
    saleData: () => data,
    payCode: (read) => log.paid.push(read),
    payLightning: host ? (invoice) => { log.lightning.push(invoice); data.isPaid = true; } : () => false,
    waitForPaid: () => { log.waits++; },
    pump: () => { log.pumps++; },
  };
  const S = { index: -1, on: null };
  let go = null;
  const list = moments(env, () => go(), S);
  go = () => enter(S.index + 1);
  function enter(i) { S.index = i; S.on = null; list[i].enter(); }
  /** The words of the last instruction line, with the bold pieces in brackets. */
  const line = () => log.says.at(-1).parts.map((p) => (typeof p === 'string' ? p : `[${p.b}]`)).join('');
  const tap = (label) => log.targets.find((t) => t.label === label).onTap();
  return { log, enter, line, tap, S, list, data, shell };
}

/** The visitor taps the Bitcoin Wallet icon, then Scan, and the scanning moment ends: the check is next. */
function viaIcon(p) {
  p.enter(0);
  p.tap('Bitcoin Wallet');
  p.tap('Scan');
  p.enter(3);
}

/** The visitor taps Hold to the counter; the second moment's own timer would end it, so it is ended here. */
function viaHold(p) {
  p.enter(0);
  p.tap('Hold to the counter');
  p.enter(2);
}

test('the first moment is the phone\'s home screen with Bitcoin Wallet and Hold to the counter, both lit', () => {
  const p = page(ONCHAIN);
  p.enter(0);
  assert.deepEqual(p.log.says[0].happened, 'The cashier rang up your order: 8.00 USD. The counter shows a code.');
  assert.equal(p.line(), 'You are the customer. Tap [Bitcoin Wallet] to scan the code, or [Hold to the counter] to pay with a tap.');
  assert.deepEqual(p.log.targets.map((t) => t.label), ['Bitcoin Wallet', 'Hold to the counter']);
  assert.equal(p.log.shown[0].screen, 'homeScreen');
  assert.equal(p.log.under, 'Hold to the counter');
  assert.deepEqual(p.log.questions.map((q) => q.q), ['Do you need a special app to pay?', 'What is in the code?', 'What is Tap?', 'Can you pay with the wallet app your cards are in?']);
});

test('Bitcoin Wallet opens the wallet with one button, Scan; Scan reads the code; the check shows the address', () => {
  const p = page(ONCHAIN);
  p.enter(0);
  p.tap('Bitcoin Wallet');
  assert.equal(p.S.index, 1);
  assert.equal(p.log.shown.at(-1).screen, 'home');
  assert.equal(p.line(), 'Your wallet is open. Tap [Scan].');
  assert.deepEqual(p.log.targets.map((t) => t.label), ['Scan']);
  p.tap('Scan');
  assert.equal(p.S.index, 2);
  assert.equal(p.log.shown.at(-1).screen, 'scanning');
  assert.equal(p.log.says.at(-1).happened, '');
  assert.equal(p.line(), 'Your phone reads the code on the counter.');
  p.enter(3);
  assert.equal(p.log.shown.at(-1).pay, null);
  assert.equal(p.log.shown.at(-1).address, 'bcrt1qabc');
  assert.equal(p.log.says.at(-1).happened, "Your phone read the code: the amount and the shop's address.");
  assert.equal(p.line(), 'Check the amount, then tap [Send].');
  assert.deepEqual(p.log.targets.map((t) => t.label), ['Send']);
});

test('Hold to the counter shows the reading screen, then the wallet opens on the check from the tag', () => {
  const p = page(ONCHAIN);
  p.enter(0);
  p.tap('Hold to the counter');
  assert.equal(p.S.index, 1);
  assert.equal(p.log.shown.at(-1).screen, 'tapping');
  assert.equal(p.log.says.at(-1).happened, '');
  assert.equal(p.line(), 'Your phone reads the payment from the counter, without the camera.');
  p.data.copyUri = 'not the tag';
  p.enter(2);
  assert.equal(p.S.index, 3, 'the scanning moment is skipped');
  assert.equal(p.log.shown.at(-1).screen, 'confirm');
  assert.equal(p.log.shown.at(-1).sats, 10000, 'the amount is the tag\'s');
  assert.equal(p.log.shown.at(-1).address, 'bcrt1qabc');
  assert.equal(p.log.says.at(-1).happened, 'Your phone read the payment from the counter and opened your wallet.');
  assert.equal(p.line(), 'Check the amount, then tap [Send].');
  assert.ok(!p.log.shown.some((x) => x.screen === 'scanning'));
});

test('a tap that finds no payment on the tag says so', () => {
  const p = page(ONCHAIN);
  p.enter(0);
  p.tap('Hold to the counter');
  p.data.tag = new Int8Array(0);
  assert.throws(() => p.enter(3), /tag holds no payment/);
});

test('without an invoice, Send pays on-chain and waits for the counter', () => {
  const p = page(ONCHAIN);
  viaIcon(p);
  p.tap('Send');
  assert.equal(p.S.index, 4);
  assert.deepEqual(p.log.paid, [{ address: 'bcrt1qabc', sats: 10000 }]);
  assert.deepEqual(p.log.lightning, []);
  assert.equal(p.log.says.at(-1).happened, 'Your phone sent the payment.');
  assert.equal(p.line(), "Watch the shop's counter: it checks the payment itself.");
  // The counter turns paid on its own state: the next line comes with it.
  p.S.on.sale({ isPaid: true });
  assert.equal(p.log.says.at(-1).happened, 'Your phone sent the payment. The counter saw it in a few seconds.');
  assert.equal(p.log.says.at(-1).skip.label, 'Next');
});

test('with an invoice, the check offers Lightning and On-chain, Lightning first', () => {
  const p = page(WITH_INVOICE);
  viaIcon(p);
  const shown = p.log.shown.at(-1);
  assert.equal(shown.pay.method, 'Lightning');
  assert.equal(p.log.says.at(-1).happened, 'Your phone read the payment: the amount and two ways to pay it.');
  assert.equal(p.line(), 'Pick [Lightning] or [On-chain], then tap [Send].');
  assert.deepEqual(p.log.targets.map((t) => t.label), ['Send']);
});

test('the same from Hold to the counter: the tag carries the invoice too', () => {
  const p = page(WITH_INVOICE);
  viaHold(p);
  assert.equal(p.log.shown.at(-1).pay.method, 'Lightning');
  assert.equal(p.log.says.at(-1).happened, 'Your phone read the payment from the counter and opened your wallet.');
  assert.equal(p.line(), 'Pick [Lightning] or [On-chain], then tap [Send].');
});

test('after Hold to the counter, Send pays over Lightning or on-chain as the visitor picks', () => {
  const p = page(WITH_INVOICE);
  viaHold(p);
  p.tap('Send');
  assert.deepEqual(p.log.lightning, ['lnbcrt1pqqq']);
  assert.deepEqual(p.log.paid, []);
  assert.equal(p.S.index, 4);
  const q = page(WITH_INVOICE);
  viaHold(q);
  q.log.shown.at(-1).pay.onPick('On-chain');
  q.tap('Send');
  assert.deepEqual(q.log.lightning, []);
  assert.equal(q.log.paid.length, 1);
  const r = page(ONCHAIN);
  viaHold(r);
  assert.equal(r.line(), 'Check the amount, then tap [Send].');
  r.tap('Send');
  assert.deepEqual(r.log.paid, [{ address: 'bcrt1qabc', sats: 10000 }]);
});

test('Send over Lightning pays the invoice through the host and the counter is paid at once', () => {
  const p = page(WITH_INVOICE);
  viaIcon(p);
  p.tap('Send');
  assert.deepEqual(p.log.lightning, ['lnbcrt1pqqq']);
  assert.deepEqual(p.log.paid, [], 'nothing is sent on-chain');
  assert.ok(p.log.pumps >= 1, 'the counter takes the payment');
  assert.equal(p.S.index, 4);
  assert.equal(p.log.shown.at(-1).screen, 'sent');
  const said = p.log.says.at(-1);
  assert.equal(said.happened, 'Your phone paid over Lightning. The counter knew at once.');
  assert.equal(p.line(), 'The green screen tells the cashier to hand over your order.');
  assert.equal(said.skip.label, 'Next');
  assert.equal(p.S.on, null, 'there is no wait line');
  assert.equal(p.log.questions, QS.customer.paidLightning);
  said.skip.onSkip();
  assert.equal(p.S.index, 5);
});

test('picking On-chain draws the check again and Send pays on-chain', () => {
  const p = page(WITH_INVOICE);
  viaIcon(p);
  p.log.shown.at(-1).pay.onPick('On-chain');
  assert.equal(p.log.shown.at(-1).pay.method, 'On-chain');
  assert.deepEqual(p.log.targets.map((t) => t.label), ['Send'], 'Send is lit again');
  p.tap('Send');
  assert.deepEqual(p.log.paid, [{ address: 'bcrt1qabc', sats: 10000, lightning: 'lnbcrt1pqqq' }]);
  assert.deepEqual(p.log.lightning, []);
  assert.equal(p.log.says.at(-1).happened, 'Your phone sent the payment.');
});

test('a host without payLightning pays the request on-chain', () => {
  const p = page(WITH_INVOICE, { host: false });
  viaIcon(p);
  p.tap('Send');
  assert.equal(p.log.paid.length, 1);
  assert.equal(p.log.says.at(-1).happened, 'Your phone sent the payment.');
});

test('a visitor who jumps to Paid has the taps made for them', () => {
  const p = page(WITH_INVOICE);
  p.list[3].auto();
  assert.deepEqual(p.log.lightning, ['lnbcrt1pqqq']);
  assert.equal(p.log.waits, 1);
  const q = page(ONCHAIN);
  q.list[3].auto();
  assert.equal(q.log.paid.length, 1);
  assert.equal(q.log.waits, 1);
});

test('the last moment is the same on every way to pay', () => {
  const p = page(ONCHAIN);
  p.enter(5);
  assert.equal(p.log.says.at(-1).happened, 'You paid from your own wallet, straight to the shop\'s own wallet, with no company in between.');
  assert.equal(p.line(), 'That is all a customer does. Tap [Start over] to try it as the merchant.');
});

test('the customer\'s questions carry the new ones with their handbook links', () => {
  const find = (list, q) => list.find((x) => x.q === q);
  const tap = find(QS.customer.scan, 'What is Tap?');
  assert.equal(`${tap.page}#${tap.anchor}`, 'getting-paid.html#enter-the-sale-and-show-the-code');
  assert.match(tap.a, /^Hold an unlocked Android phone to the counter\./);
  const card = find(QS.customer.scan, 'Can you pay with the wallet app your cards are in?');
  assert.equal(`${card.page}#${card.anchor}`, 'getting-paid.html#enter-the-sale-and-show-the-code');
  assert.equal(QS.customer.scan.indexOf(card), QS.customer.scan.indexOf(tap) + 1);
  const pick = find(QS.customer.send, 'Lightning or on-chain: which should you pick?');
  assert.equal(`${pick.page}#${pick.anchor}`, 'getting-paid.html#small-sales-and-lightning');
  const fee = find(QS.customer.send, 'Who pays the network fee?');
  assert.equal(`${fee.page}#${fee.anchor}`, 'cost-reference.html#processing-and-network-fees');
});
