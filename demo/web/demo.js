// The demo page's own glue: the three calls into the wasm module, and the
// drawing of what comes back. No library, no network beyond the page's own
// files. In a browser the page starts itself; under Node the same glue is
// loaded by demo.test.js, which drives the shop flow without a page.
"use strict";

// The words of the controls.
const CONTROL_WORDS = {
  amount: "Sale amount, US dollars",
  newSale: "Ring up the sale",
  customerPays: "Customer pays",
  letTimePass: "Let 10 seconds pass",
  mine: "Mine a block",
  amountNotAccepted: "Type an amount from 0.01 to 1000000 dollars, then ring up the sale.",
};

// The seconds "let time pass" moves the clock: more than the counter's
// watch for a conflicting payment (8 seconds).
const TIME_PASS_SECS = 10;

const isNode = typeof module !== "undefined" && module.exports;
// Under Node the screens are required; in the page demo.js loads screens.js
// itself (a plain script that sets window.Screens) before it starts.
let Screens = isNode ? require("./screens.js") : null;

function loadScreens() {
  return new Promise((resolve, reject) => {
    const s = document.createElement("script");
    s.src = "screens.js";
    s.onload = () => {
      Screens = window.Screens;
      resolve();
    };
    s.onerror = () => reject(new Error("screens.js"));
    document.head.append(s);
  });
}

// The step the shell page names for a reply: no sale yet, the code on the
// counter, a payment seen and not yet in a block, or a payment in a block.
// A sale accepted at once is still not in a block: "seenUnconfirmed".
function stepOf(reply) {
  const c = reply.counter;
  if (c.kind !== "sale") return "saleReady";
  switch (c.state) {
    case "NotPaidYet":
      return "showingCode";
    case "PartPaid":
      return "partPaid";
    case "Waiting":
      return "waiting";
    case "PaidInstant":
      return "paidInstant";
    case "Final":
      return "final";
    default:
      return "confirmed";
  }
}

function announce(reply) {
  document.dispatchEvent(new CustomEvent("demo:update", { detail: { step: stepOf(reply), chain: reply.chain } }));
}

// ---- the glue: the three calls of the module -------------------------------

// `instance` is the module's instance. A request is JSON; a reply is JSON.
function createGlue(instance) {
  const ex = instance.exports;
  const enc = new TextEncoder();
  const dec = new TextDecoder();
  return {
    call(request) {
      const bytes = enc.encode(JSON.stringify(request));
      const ptr = ex.demo_alloc(bytes.length);
      // The memory may have grown in the allocation: take the buffer after it.
      new Uint8Array(ex.memory.buffer, ptr, bytes.length).set(bytes);
      const packed = BigInt.asUintN(64, ex.demo_call(ptr, bytes.length));
      const outPtr = Number(packed >> 32n);
      const outLen = Number(packed & 0xffffffffn);
      const text = dec.decode(new Uint8Array(ex.memory.buffer, outPtr, outLen));
      ex.demo_free(outPtr, outLen);
      return JSON.parse(text);
    },
  };
}

// The shop scenario as the page drives it. Every step returns the reply
// of the module, or throws with the module's own reason.
class ShopDemo {
  constructor(glue) {
    this.glue = glue;
  }
  step(request) {
    const reply = this.glue.call(request);
    if (!reply.ok) throw new Error(reply.error);
    return reply;
  }
  setUp(seed) { return this.step({ op: "set_up", seed }); }
  newSale(cents) { return this.step({ op: "new_sale", cents }); }
  customerPays() { return this.step({ op: "customer_pays" }); }
  tick(secs) { return this.step({ op: "tick", secs }); }
  mine() { return this.step({ op: "mine" }); }
  state() { return this.step({ op: "state" }); }
}

// A dollar amount typed as text to whole cents, or null if it is not one.
function centsOf(text) {
  const m = /^(\d{1,7})(?:\.(\d{1,2}))?$/.exec(text.trim());
  if (!m) return null;
  return Number(m[1]) * 100 + Number((m[2] || "").padEnd(2, "0") || 0);
}

// The shell's #demo-controls gets the buttons (each with a data-action) and
// the lines under them; the two rectangles are drawn by screens.js.
function startPage(doc, demo, appWords) {
  const root = doc.getElementById("demo-controls");
  root.classList.add("obd-controls");
  const make = (tag, text, attrs) => {
    const e = doc.createElement(tag);
    if (text) e.textContent = text;
    Object.assign(e, attrs || {});
    return e;
  };
  const label = make("label", CONTROL_WORDS.amount + " ");
  const amount = make("input", "", { type: "number", min: "0.01", max: "1000000", step: "0.01", value: "5.00" });
  label.append(amount);
  const button = (action, text) => {
    const b = make("button", text, { type: "button" });
    b.dataset.action = action;
    return b;
  };
  const newSale = button("new-sale", CONTROL_WORDS.newSale);
  const pays = button("customer-pays", CONTROL_WORDS.customerPays);
  const pass = button("let-time-pass", CONTROL_WORDS.letTimePass);
  const mine = button("mine", CONTROL_WORDS.mine);
  const status = make("p");
  status.className = "obd-status";
  const problem = make("p");
  problem.className = "obd-problem";
  problem.hidden = true;
  root.replaceChildren(label, newSale, pays, pass, mine, status, problem);

  function draw(reply) {
    Screens.renderPos(doc, doc.getElementById("pos-screen"), reply, appWords.counter);
    Screens.renderPhone(doc, doc.getElementById("phone-screen"), reply);
    const sale = reply.counter.sale;
    pays.disabled = !sale || sale.customer_paid;
    pass.disabled = !sale;
    problem.hidden = true;
    announce(reply);
  }
  function fail(text) {
    problem.textContent = text;
    problem.hidden = false;
  }
  function run(f) {
    try {
      draw(f());
    } catch (e) {
      fail(e.message);
    }
  }
  newSale.addEventListener("click", () => {
    const cents = centsOf(amount.value);
    if (cents === null || cents === 0) return fail(CONTROL_WORDS.amountNotAccepted);
    run(() => demo.newSale(cents));
  });
  pays.addEventListener("click", () => run(() => demo.customerPays()));
  pass.addEventListener("click", () => run(() => demo.tick(TIME_PASS_SECS)));
  mine.addEventListener("click", () => run(() => demo.mine()));
  return { draw, fail };
}

function newSeed() {
  // The seed for the customer's coins comes from the browser's random source.
  const bytes = crypto.getRandomValues(new Uint8Array(6));
  return bytes.reduce((a, b) => a * 256 + b, 0);
}

async function boot() {
  const doc = document;
  doc.dispatchEvent(new CustomEvent("demo:update", { detail: { step: "starting" } }));
  try {
    await loadScreens();
    const [wasm, counter, owner] = await Promise.all([
      WebAssembly.instantiateStreaming
        ? WebAssembly.instantiateStreaming(fetch("demo.wasm"), {})
        : fetch("demo.wasm").then((r) => r.arrayBuffer()).then((b) => WebAssembly.instantiate(b, {})),
      fetch("words/counter-app.json").then((r) => r.json()),
      fetch("words/owner-app.json").then((r) => r.json()),
    ]);
    const demo = new ShopDemo(createGlue(wasm.instance));
    const page = startPage(doc, demo, { counter, owner });
    page.draw(demo.setUp(newSeed()));
    // Start over: a fresh shop and a fresh chain.
    doc.addEventListener("demo:restart", () => page.draw(demo.setUp(newSeed())));
    // Only the shop is played so far; another place starts the same scenario.
    doc.addEventListener("demo:place", () => page.draw(demo.setUp(newSeed())));
  } catch (e) {
    doc.dispatchEvent(new CustomEvent("demo:failed", { detail: { message: e.message } }));
  }
}

if (typeof module !== "undefined" && module.exports) {
  module.exports = { stepOf, createGlue, ShopDemo, centsOf, TIME_PASS_SECS, counterWords: Screens.counterWords, qrRows: Screens.qrRows };
} else {
  boot();
}
