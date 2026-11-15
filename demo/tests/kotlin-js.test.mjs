// Runs the apps' own screen model, compiled for the browser by
// scripts/demo-kotlin-js.sh, in node: the counter's amounts and the owner's
// books, dates and limits, and the one native call of each served by a
// function the page would provide.
// Usage: node demo/tests/kotlin-js.test.mjs
//   (after scripts/demo-kotlin-js.sh counter and scripts/demo-kotlin-js.sh owner;
//   a part whose module is not built is left out and said so)
import assert from 'node:assert/strict';
import fs from 'node:fs';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

const here = path.dirname(fileURLToPath(import.meta.url));
const file = path.join(here, '../../dist/kotlin-js/counter/counter-model.mjs');
const ownerFile = path.join(here, '../../dist/kotlin-js/owner/owner-model.mjs');
const ran = [];

if (fs.existsSync(file)) {
  // The page's side of the seam: the request frame in, a frame out.

  let seen = null;
  globalThis.openbmtCounterCall = (request) => {
    seen = Array.from(request);
    return new Int8Array([1, 2, 3, 4, 5]);
  };

  const mod = await import(file);
  const demo = mod.CounterModelDemo.getInstance();

  assert.equal(demo.showAmount('USD', 1234), '12.34 USD');
  assert.equal(demo.showAmount('JPY', 1500), '1,500 JPY');
  assert.equal(demo.showAmount('SAT', 7500), '7,500 sats');
  assert.equal(demo.showAmount('XXX', 1), null);
  assert.equal(demo.parseAmount('USD', '12.3'), 1230);
  assert.equal(demo.parseAmount('USD', '1,234.50'), 123450);
  assert.equal(demo.parseAmount('USD', '12.345'), -1);
  assert.equal(demo.parseAmount('JPY', '12.5'), -1);

  assert.equal(demo.nativeAnswerLength(new Int8Array([79, 66, 77, 1])), 5);
  assert.deepEqual(seen, [79, 66, 77, 1]);
  ran.push('counter');
} else console.log('kotlin-js: counter module not built, left out');

if (fs.existsSync(ownerFile)) {
  let ownerSeen = null;
  globalThis.openbmtOwnerCall = (request) => {
    ownerSeen = Array.from(request);
    return new Int8Array([9, 8, 7]);
  };
  const owner = (await import(ownerFile)).OwnerModelDemo.getInstance();
  const utc = (y, m, d) => Date.UTC(y, m - 1, d) / 1000;

  // The books page over sample days (the model's own words and totals).
  assert.deepEqual(
    owner.booksRows('USD', ['2026-11-14', '2026-11-13'], [1, 3], ['12.50', '40.00']),
    ['2026-11-14|1 sale, 12.50 USD', '2026-11-13|3 sales, 40.00 USD'],
  );
  assert.deepEqual(owner.booksRows('USD', [], [], []), []);

  // The limit in satoshis: whole numbers past 64 bits in between (the big-number shim), against BigInt.
  assert.equal(owner.limitSats('1000', 100000), 1000000);
  assert.equal(owner.limitSats('1,000', 100000), 1000000);
  assert.equal(owner.limitSats('1000000000000', 1000000), 100000000000000);
  assert.equal(owner.limitSats('92233720368', 1), -1);
  assert.equal(owner.limitSats('92233720369', 1), -1);
  assert.equal(owner.limitSats('0', 100000), -1);
  assert.equal(owner.limitSats('12.5', 100000), -1);
  assert.equal(owner.limitSats('5', 0), -1);
  for (let i = 0; i < 300; i++) {
    const whole = BigInt(Math.floor(Math.random() * 2 ** 40) + 1);
    const price = BigInt(Math.floor(Math.random() * 2 ** 33) + 1);
    const sats = (whole * 100000000n) / price;
    const want = sats > 0n && sats < 2n ** 53n ? Number(sats) : -1;
    if (sats >= 2n ** 53n) continue;
    assert.equal(owner.limitSats(String(whole), Number(price)), want, `${whole} at ${price}`);
  }

  // Dates: the calendar shim (days, months, the offset, the ISO text).
  assert.equal(owner.suggestedDate(0), '1970-01-31');
  assert.equal(owner.suggestedDate(1700000000), '2023-12-14');
  assert.equal(owner.moveBy('2026-12-01', 0), utc(2026, 12, 1));
  assert.equal(owner.moveBy('2026-12-01', utc(2026, 12, 1)), -1);
  assert.equal(owner.moveBy('2026-02-30', 0), -1);
  assert.equal(owner.moveBy('soon', 0), -1);
  assert.equal(owner.patchIsOld('2026-01-15', '2026-07-16'), true);
  assert.equal(owner.patchIsOld('2026-01-15', '2026-07-15'), false);
  assert.equal(owner.patchIsOld('2026-08-31', '2027-02-28'), false);
  assert.equal(owner.patchIsOld('2026-08-31', '2027-03-01'), true);
  assert.equal(owner.patchIsOld('garbage', '2027-03-01'), false);
  assert.equal(owner.shopDay(0, -300), '1969-12-31');
  assert.equal(owner.shopDay(utc(2026, 11, 15) + 3600, 120), '2026-11-15');
  assert.equal(owner.shopDay(utc(2026, 11, 15) - 1, 120), '2026-11-15');
  assert.equal(owner.shopDay(utc(2026, 11, 15) - 7201, 120), '2026-11-14');
  assert.equal(owner.dueDay('2026-02-30', '2026-12-01'), 'bad');
  assert.equal(owner.dueDay('2028-02-29', '2028-02-01'), 'yes');
  assert.equal(owner.dueDay('2026-12-01', '2026-12-01'), 'yes');
  assert.equal(owner.dueDay('2026-11-30', '2026-12-01'), 'no');
  assert.equal(owner.dueDay('2027-12-02', '2026-12-01'), 'yes');
  assert.equal(owner.dueDay('2027-12-03', '2026-12-01'), 'no');
  assert.equal(owner.homeDate(86400 * 365), '1971-01-01');
  assert.equal(owner.homeDate(utc(2024, 2, 29) + 5), '2024-02-29');
  for (let i = 0; i < 300; i++) {
    const secs = Math.floor(Math.random() * 4e9);
    assert.equal(owner.homeDate(secs), new Date(secs * 1000).toISOString().slice(0, 10));
  }

  // The one native call, through the page's function.
  assert.equal(owner.nativeAnswerLength(new Int8Array([79, 66, 77, 2])), 3);
  assert.deepEqual(ownerSeen, [79, 66, 77, 2]);
  ran.push('owner');
}

console.log(`kotlin-js: ok (${ran.join(', ')})`);
