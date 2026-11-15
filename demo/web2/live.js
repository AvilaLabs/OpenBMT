// The two paths of the demo on the counter's own code. The moments, the instruction words, the questions and the
// taps are those of preview.js; what differs is that the counter's screens are drawn by the counter's controller
// (counter-controller.js) from the counter app's own model, whose library runs as WebAssembly on a pretend chain,
// and that a moment ends when the MODEL says so (the sale opened, the sale turned paid), not on a script.
//
// The owner's phone runs on the owner app's own code the same way (owner-controller.js, the owner's model and the
// owner's library as WebAssembly), on the same pretend chain: every chain action goes through one object
// (one-chain.js) that moves both modules together. The customer's wallet is only drawn for the demo; it reads the
// sale's own payment code.
"use strict";
(async function () {
  const U = OpenBmtUi, C = OpenBmtChrome;
  const Wl = OpenBmtWallet, QS = OpenBmtQuestions, PC = OpenBmtPaymentCode, CC = OpenBmtCounterController;
  const OC = OpenBmtOwnerController, { OneChain } = OpenBmtOneChain;
  // Where the files are: the page may say (OpenBmtAssets); the repository's layout otherwise.
  const assets = Object.assign({
    logo: "../ui/logo.png", palette: "../ui/palette.json",
    model: "../../dist/kotlin-js/counter/counter-model.mjs", wasm: "../../dist/demo-counter/counter.wasm", host: "./counter-host.mjs",
    ownerModel: "../../dist/kotlin-js/owner/owner-model.mjs", ownerWasm: "../../dist/demo-owner/owner.wasm", ownerHost: "./owner-host.mjs",
  }, globalThis.OpenBmtAssets);
  const palette = new U.PaletteOf(await (await fetch(assets.palette)).json());
  const shell = new OpenBmtShell(document.getElementById("app"));
  const wasm = await (await fetch(assets.wasm)).arrayBuffer();
  const { loadCounter } = await import(assets.host);
  const flow = (await import(assets.model)).CounterFlowDemo.getInstance();
  const ownerWasm = await (await fetch(assets.ownerWasm)).arrayBuffer();
  const { loadOwner } = await import(assets.ownerHost);
  const ownerFlow = (await import(assets.ownerModel)).OwnerFlowDemo.getInstance();

  // The page's clock. A visitor who jumps to a later step has the seconds a real counter would have waited skipped.
  const clock = { offset: 0, now() { return Math.floor(Date.now() / 1000) + this.offset; }, skip(secs) { this.offset += secs; } };
  const AMOUNT = "800";
  const groups = (n) => Number(n).toLocaleString("en-US");

  /** A device page: [view] draws it with the Ui of the device's size. */
  const page = (view) => (size) => {
    const ui = C.deviceUi(palette, false, assets.logo, size);
    return C.Device(ui, view(ui));
  };
  const COUNTER = [{ label: "Tablet", size: C.TABLET }, { label: "Phone", size: C.PHONE }];
  const PHONE = [{ size: C.PHONE }];

  // ---- A run of the counter: a fresh WebAssembly instance, a fresh node, the controller and the pump. ----
  let run = null;

  function stopRun() {
    if (!run) return;
    run.dead = true;
    clearInterval(run.timer);
    run.controller.stopNode();
    run.owner.stopNode();
    run = null;
  }

  /** What the page does for the counter: the owner's heartbeat, the network's events, and the screen's own tick. */
  function pump(r) {
    if (r.dead) return;
    const now = clock.now();
    // The demo plays the owner's phone, which tells the counter now and then that it is watching (CR-50).
    if (now - r.beat >= 3) {
      r.beat = now;
      flow.heartbeat(now, r.chain.heartbeat());
    }
    flow.pump(now);
    ownerFlow.pump(now);
    r.controller.onPump(true);
    r.owner.onPump(true);
  }

  async function startRun(onEvent, withOwner) {
    stopRun();
    const r = { dead: false, beat: 0, aim: null, onEvent };
    // Two modules, one chain: both read the page's clock, and every chain action is made on both (one-chain.js).
    const counterChain = await loadCounter(wasm, { now: () => clock.now() });
    const ownerChain = await loadOwner(ownerWasm, { now: () => clock.now() });
    r.chain = OneChain(counterChain, ownerChain);
    r.controller = CC.Controller({
      flow, chain: r.chain, clock,
      show(viewOf) {
        if (r.dead) return;
        shell.show("counter", page(viewOf));
        if (r.aim) shell.target("counter", r.aim, null);
      },
      event(kind, data) { if (!r.dead) r.onEvent(kind, data); },
    });
    r.owner = OC.Controller({
      flow: ownerFlow, chain: ownerChain, clock,
      show(viewOf) {
        // The owner's phone is on the page only on the merchant's path.
        if (!r.dead && r.ownerShown) shell.show("owner", page(viewOf));
      },
    });
    r.ownerShown = withOwner;
    run = r;
    r.controller.startNode();
    r.owner.startNode();
    r.owner.render();
    pump(r);
    r.timer = setInterval(() => pump(r), 1000);
    return r;
  }

  /** The counter's tap on [label], lit for the visitor; null puts the light out. */
  function aim(label) {
    run.aim = label;
    if (label) shell.target("counter", label, null);
  }

  /** The customer's wallet scans the sale's own code: the address and the amount of its payment request. */
  function readCode() {
    const data = run.controller.saleData();
    const read = data && PC.parse(data.copyUri);
    if (!read) throw new Error("the counter shows no payment code");
    return read;
  }

  /** The customer's wallet pays what the code asks, on the pretend chain. */
  function payCode(read) {
    run.chain.pay(read.address, read.sats);
  }

  /** The seconds a real counter waits for its own check, run at once: for a visitor who jumped ahead. */
  function waitForPaid() {
    for (let i = 0; i < 40 && !(run.controller.saleData() || {}).isPaid; i++) {
      clock.skip(1);
      pump(run);
    }
  }

  /** The keys of the entry still to type for 8.00, and the next thing to tap. */
  function nextTap(minor) {
    const typed = minor > 0 ? String(minor) : "";
    if (typed === AMOUNT) return "Open the sale";
    return AMOUNT.startsWith(typed) ? AMOUNT[typed.length] : "C";
  }

  // ---- As a merchant: the counter and the owner's phone; the demo plays the customer. ----
  function merchantMoments(go, S) {
    const c = () => run.controller;
    return [
      { step: 0, enter() {
        shell.actor(null);
        shell.light("counter", { owner: "Nothing to do here yet." });
        shell.say("", ["You are the cashier. Tap ", { b: "New sale" }, "."]);
        shell.questions(QS.merchant.ringUp);
        aim("New sale");
        S.on = { newSale: go };
      }, auto() { c().showNewSale(); } },
      { step: 0, enter() {
        shell.say("The customer's order comes to 8.00 USD.", ["Type the amount, then tap ", { b: "Open the sale" }, "."]);
        aim(nextTap(flow.entryMinor()));
        S.on = { entry: (e) => aim(nextTap(e.minor)), sale: go };
      }, auto() { for (const key of AMOUNT) c().press(key); c().openEntered(); } },
      { step: 1, enter() {
        aim(null);
        const read = readCode();
        shell.say("The counter shows a code for this sale.",
          ["The customer scans it with their own wallet. Tap ", { b: "The customer pays" }, "."]);
        shell.questions(QS.merchant.code);
        shell.actor({
          who: "The customer", line: `Scans the code and sends ${groups(read.sats)} sats.`, button: "The customer pays",
          onTap() {
            payCode(readCode());
            const b = document.querySelector(".actor button");
            if (b) b.disabled = true;
            shell.say("The customer sent the payment.", ["The counter checks it itself. This takes a few seconds."]);
          },
        });
        S.on = { sale: (d) => { if (d.isPaid) go(); } };
      }, auto() { payCode(readCode()); waitForPaid(); } },
      { step: 2, enter() {
        shell.actor(null);
        shell.say("Paid. The counter saw the payment itself, in a few seconds.",
          ["Read the green screen: hand over the goods. Then tap ", { b: "Done" }, "."]);
        shell.questions(QS.merchant.paid);
        aim("Done");
        S.on = { home: go };
      }, auto() { c().showHome(); } },
      { step: 3, enter() {
        aim(null);
        // The owner's phone is drawn from the owner model's own state (owner-controller.js); the counter rests.
        shell.light("owner", { counter: "Ready for the next sale." });
        shell.say("The sale is done at the counter.",
          ["Now you are the owner. In a real shop the payment is settled when a block holds it, in about ten minutes."],
          { label: "Skip ahead", onSkip: go });
        shell.questions(QS.merchant.owner);
        S.on = null;
      } },
      { step: 4, enter() {
        // A block holds the payment, on the one pretend chain: both modules take it, and the owner's model has it
        // before the home is drawn again from the model.
        run.chain.mine();
        pump(run);
        run.owner.takeEvents(clock.now());
        shell.say("A block holds the payment. Your phone shows it in the shop's wallet.",
          ["That is a whole sale. Tap ", { b: "Start over" }, " to try it as the customer."]);
        shell.questions(QS.merchant.books);
      } },
    ];
  }

  // ---- As a customer: the shop's counter (the demo plays the cashier) and your phone with a wallet. ----
  function customerMoments(go, S) {
    let read = null;
    /** The wallet's last screens, once the counter has turned paid. */
    const paidNow = () => {
      shell.say("Your phone sent the payment. The counter saw it in a few seconds.",
        ["The green screen tells the cashier to hand over your order."], { label: "Next", onSkip: go });
      shell.questions(QS.customer.paid);
    };
    return [
      { step: 0, enter() {
        shell.show("wallet", () => Wl.home(250000));
        shell.light("wallet", { counter: "The cashier's screen. It waits for your payment." });
        shell.say("The cashier rang up your order: 8.00 USD. The counter shows a code.",
          ["You are the customer. On your phone, tap ", { b: "Scan" }, "."]);
        shell.questions(QS.customer.scan);
        shell.target("wallet", "Scan", go);
      } },
      { step: 0, enter() {
        shell.show("wallet", () => Wl.scanning());
        shell.say("", ["Your phone reads the code on the counter."]);
        const at = S.index;
        setTimeout(() => { if (S.index === at) go(); }, 1400);
      } },
      { step: 1, enter() {
        read = readCode();
        shell.show("wallet", () => Wl.confirm(read.sats, read.address));
        shell.say("Your phone read the code: the amount and the shop's address.",
          ["Check the amount, then tap ", { b: "Send" }, "."]);
        shell.questions(QS.customer.send);
        shell.target("wallet", "Send", () => { payCode(read); go(); });
      }, auto() { read = readCode(); payCode(read); waitForPaid(); } },
      { step: 2, enter() {
        shell.show("wallet", () => Wl.sent(read.sats));
        shell.light("counter", { wallet: "Your payment is sent." });
        shell.say("Your phone sent the payment.", ["Watch the shop's counter: it checks the payment itself."]);
        shell.questions([]);
        const data = run.controller.saleData();
        if (data && data.isPaid) paidNow();
        else S.on = { sale: (d) => { if (d.isPaid) { S.on = null; paidNow(); } } };
      } },
      { step: 3, enter() {
        shell.light("counter", {});
        shell.say("You paid from your own wallet, straight to the shop's own wallet, with no company in between.",
          ["That is all a customer does. Tap ", { b: "Start over" }, " to try it as the merchant."]);
      } },
    ];
  }

  // ---- The runner: a moment ends when the model says so; a step tab replays the taps on a fresh model. ----
  const merchant = {
    steps: ["Ring up", "The customer pays", "Hand over", "The owner's phone"],
    devices: [{ id: "counter", caption: "The shop's counter", sizes: COUNTER }, { id: "owner", caption: "The owner's phone", sizes: PHONE }],
    actorAfter: "counter", moments: merchantMoments,
    setup() { run.controller.showHome(); },
  };
  const customer = {
    steps: ["Scan", "Send", "Paid"],
    devices: [{ id: "counter", caption: "The shop's counter", sizes: COUNTER }, { id: "wallet", caption: "Your phone", sizes: PHONE }],
    moments: customerMoments,
    // The cashier's sale of 8.00 USD, opened on the counter's own code.
    setup() { run.controller.openSale(Number(AMOUNT), 0, (words) => console.warn("refused:", words.heading)); },
  };

  /** Plays [path] from its moment [from]: a fresh counter, the earlier moments' taps made for the visitor. */
  async function play(path, from = 0) {
    const S = { index: -1, on: null };
    shell.begin(path.steps, path.devices, path.actorAfter);
    const go = () => enter(S.index + 1);
    const moments = path.moments(go, S);
    function enter(i) {
      if (i >= moments.length) return;
      S.index = i;
      S.on = null;
      shell.step(moments[i].step);
      moments[i].enter();
    }
    await startRun((kind, data) => {
      const handler = S.on && S.on[kind];
      if (handler) handler(data);
    }, path.devices.some((d) => d.id === "owner"));
    path.setup();
    // The visitor's taps before the moment they chose, made on the real model.
    for (let j = 0; j < from; j++) if (moments[j].auto) moments[j].auto();
    enter(from);
    shell.onStep = (s) => {
      const target = moments.findIndex((m) => m.step >= s);
      play(path, target < 0 ? 0 : target);
    };
  }

  const pick = () => {
    stopRun();
    shell.start([
      { name: "As a customer", what: "Pay a shop in bitcoin, the way it works at the counter.", go: "Start", onPick: () => play(customer) },
      { name: "As a merchant", what: "Take a payment at the counter and see it reach the owner's phone.", go: "Start", onPick: () => play(merchant) },
    ]);
  };
  shell.onRestart = pick;
  const want = new URLSearchParams(location.search).get("path");
  if (want === "merchant") await play(merchant); else if (want === "customer") await play(customer); else pick();
  document.body.dataset.ready = "1";
})();
