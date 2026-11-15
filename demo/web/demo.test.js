// Drives the shop flow through the page's own glue, against the built wasm.
// Run from the repository root after scripts/demo-build.sh:
//   node --test demo/web/demo.test.js
// DEMO_DIST names another folder than dist/demo.
"use strict";
const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const { stepOf, createGlue, ShopDemo, counterWords, qrRows, centsOf, TIME_PASS_SECS } = require("./demo.js");

const dist = process.env.DEMO_DIST || path.join(__dirname, "..", "..", "dist", "demo");

function load() {
  const bytes = fs.readFileSync(path.join(dist, "demo.wasm"));
  const module = new WebAssembly.Module(bytes);
  // No imports: the module needs nothing from the page.
  assert.deepEqual(WebAssembly.Module.imports(module), []);
  return new ShopDemo(createGlue(new WebAssembly.Instance(module, {})));
}

function words(name) {
  return JSON.parse(fs.readFileSync(path.join(dist, "words", name + ".json"), "utf8"));
}

test("the module exports the three calls and its memory", () => {
  const module = new WebAssembly.Module(fs.readFileSync(path.join(dist, "demo.wasm")));
  const names = WebAssembly.Module.exports(module).map((e) => e.name).sort();
  // The secp256k1 package leaves a few C symbols exported; they are not used.
  for (const n of ["demo_alloc", "demo_call", "demo_free", "memory"]) assert.ok(names.includes(n), n);
});

test("the shop flow: set up, a sale, the customer pays, a block, final", () => {
  const counterApp = words("counter-app");
  const demo = load();
  const seen = [];
  const steps = [];
  const note = (r) => {
    steps.push(stepOf(r));
    seen.push(r.counter.state);
    return r;
  };

  // The shop is set up: a state of the app, with the app's own words.
  let r = note(demo.setUp(123456789));
  assert.equal(r.counter.kind, "app");
  assert.equal(counterWords(r, counterApp).heading, counterApp.pairingSaved.heading);
  assert.equal(r.owner, null);

  // A sale of $5.00: nothing paid, a regtest address and a code.
  r = note(demo.newSale(centsOf("5.00")));
  assert.equal(r.counter.sale.needs_sats, 5000);
  assert.equal(r.counter.words.heading, "Not paid yet");
  assert.match(r.payment.address, /^bcrt1[0-9a-z]+$/);
  assert.equal(r.payment.uri, "BITCOIN:" + r.payment.address.toUpperCase() + "?amount=0.00005");
  const rows = qrRows(r.payment);
  assert.equal(rows.length, r.payment.qr_size);
  assert.ok(rows.every((row) => row.length === rows.length));
  // The finder pattern: dark corners 7 modules across, in three corners.
  const n = rows.length;
  for (const [x, y] of [[0, 0], [n - 7, 0], [0, n - 7]]) {
    assert.ok(rows[y].slice(x, x + 7).every(Boolean));
    assert.ok(rows[y + 6].slice(x, x + 7).every(Boolean));
  }
  assert.equal(r.owner.state, "open");

  // The customer pays: seen, not confirmed.
  r = note(demo.customerPays());
  assert.equal(r.mempool, 1);
  assert.deepEqual(r.chain.pool, [{ sats: 5000, ours: true }]);
  assert.equal(r.counter.sale.customer_paid, true);
  assert.throws(() => demo.customerPays(), /paid/);

  // The watch for a conflicting payment runs out.
  r = note(demo.tick(TIME_PASS_SECS));
  assert.equal(r.owner.state, "paid at once");

  // A block, then five more.
  r = note(demo.mine());
  assert.equal(r.mempool, 0);
  assert.equal(r.owner.sats_received, 5000);
  assert.equal(r.owner.counting, "pending");
  for (let i = 0; i < 5; i++) r = note(demo.mine());
  assert.equal(r.owner.state, "final");
  assert.equal(r.owner.counting, "counted");
  assert.equal(r.height, 106);

  assert.deepEqual(steps, [
    "saleReady", "showingCode", "seenUnconfirmed", "seenUnconfirmed", "paid",
    "paid", "paid", "paid", "paid", "paid",
  ]);
  assert.equal(r.chain.blocks.length, 6);
  assert.deepEqual(r.chain.blocks.map((b) => b.ours), [true, false, false, false, false, false]);
  assert.deepEqual(r.chain.pool, []);
  assert.deepEqual(seen, [
    "pairingSaved", "NotPaidYet", "Waiting", "PaidInstant", "Confirmed",
    "Confirmed", "Confirmed", "Confirmed", "Confirmed", "Final",
  ]);
});

test("the app's word files hold every state the page asks for", () => {
  for (const file of ["counter-app", "owner-app"]) {
    const w = words(file);
    for (const [name, v] of Object.entries(w)) {
      for (const key of ["tone", "heading", "what", "next", "who"]) {
        assert.equal(typeof v[key], "string", file + " " + name + " " + key);
      }
    }
  }
  assert.ok(words("counter-app").pairingSaved);
});

test("two sessions of the same seed make the same replies", () => {
  const run = (seed) => {
    const d = load();
    d.setUp(seed);
    d.newSale(700);
    return JSON.stringify([d.customerPays(), d.mine()]);
  };
  assert.equal(run(1), run(1));
});

test("refusals come back as errors with a reason", () => {
  const d = load();
  assert.throws(() => d.newSale(500), /not set up/);
  d.setUp(1);
  assert.throws(() => d.newSale(0), /amount/);
  assert.throws(() => d.customerPays(), /no sale/);
});

test("the amount typed is read as whole cents", () => {
  assert.equal(centsOf("5"), 500);
  assert.equal(centsOf("5.5"), 550);
  assert.equal(centsOf("0.01"), 1);
  assert.equal(centsOf("12.345"), null);
  assert.equal(centsOf("abc"), null);
  assert.equal(centsOf(""), null);
});
