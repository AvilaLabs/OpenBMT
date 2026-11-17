// The paths of the demo on the counter's own code. The moments, the instruction words, the questions and the
// taps are those of preview.js; what differs is that the counter's screens are drawn by the counter's controller
// (counter-controller.js) from the counter app's own model, whose library runs as WebAssembly on a pretend chain,
// and that a moment ends when the MODEL says so (the sale opened, the sale turned paid), not on a script.
//
// The owner's phone runs on the owner app's own code the same way (owner-controller.js, the owner's model and the
// owner's library as WebAssembly), on the same pretend chain: every chain action goes through one object
// (one-chain.js) that moves both modules together. The customer's wallet is only drawn for the demo; it reads the
// sale's own payment code, or the same request from the counter's tag when the customer taps. The sounds and the
// logo's snap are the apps' own (demo/ui/feedback.js, sound.js).
"use strict";
(async function () {
  const U = OpenBmtUi, C = OpenBmtChrome, Shell = OpenBmtShell;
  const Wl = OpenBmtWallet, QS = OpenBmtQuestions, PC = OpenBmtPaymentCode, CC = OpenBmtCounterController;
  const OC = OpenBmtOwnerController, { OneChain } = OpenBmtOneChain;
  const F = OpenBmtFeedback, Cu = OpenBmtCustomer;
  // Where the files are: the page may say (OpenBmtAssets); the repository's layout otherwise.
  const assets = Object.assign({
    logo: "../ui/logo.png", palette: "../ui/palette.json",
    model: "../../dist/kotlin-js/counter/counter-model.mjs", wasm: "../../dist/demo-counter/counter.wasm", host: "./counter-host.mjs",
    ownerModel: "../../dist/kotlin-js/owner/owner-model.mjs", ownerWasm: "../../dist/demo-owner/owner.wasm", ownerHost: "./owner-host.mjs",
    sounds: "./sounds/", logoPieces: "../ui/logo-pieces/",
  }, globalThis.OpenBmtAssets);
  // Both engines are fetched and compiled once, from the moment the page opens, while the visitor reads the start
  // screen; each run then only instantiates them, which takes moments.
  const compile = (url) => {
    const r = fetch(url);
    return WebAssembly.compileStreaming(r).catch(async () => WebAssembly.compile(await (await fetch(url)).arrayBuffer()));
  };
  const made = (url) => (url ? fetch(url).then((r) => (r.ok ? r.arrayBuffer() : null)).catch(() => null) : Promise.resolve(null));
  const engines = {
    counter: compile(assets.wasm), owner: compile(assets.ownerWasm),
    host: import(assets.host), ownerHost: import(assets.ownerHost),
    model: import(assets.model), ownerModel: import(assets.ownerModel),
    // The pairing record made at build time; without it the counter makes it at each start (about two seconds).
    record: made(assets.record), recordWithBox: made(assets.recordWithBox),
  };
  for (const k in engines) engines[k].catch(() => {});
  const palette = new U.PaletteOf(await (await fetch(assets.palette)).json());
  const shell = new OpenBmtShell(document.getElementById("app"));

  // ---- The sounds and the logo's snap ----
  // Audio starts only after the visitor's first tap; the switch in the bar is remembered.
  const box = OpenBmtSound.forPage(F, assets.sounds);
  for (const kind of ["pointerup", "click", "keydown"]) addEventListener(kind, () => box.unlock(), { capture: true });
  shell.setSound(box.state.soundOn);
  shell.onSound = () => {
    box.state.soundOn = !box.state.soundOn;
    shell.setSound(box.state.soundOn);
  };
  // With reduced motion the logo is still and silent, as the app's is with the animator scale at 0 (Feedback.kt:74 motionOn).
  const motionOn = () => !matchMedia("(prefers-reduced-motion: reduce)").matches;
  const pieces = await F.loadPieces(assets.logoPieces);
  const snap = F.LogoSnapView(pieces, innerWidth < 900 ? 96 : 128, box, motionOn);
  // The sounds are fetched and decoded now, so that the opening screen's first tap finds them ready.
  box.prepare();

  // Filled in when the engines are ready (ready()); nothing below runs a counter before that.
  let wasm, loadCounter, flow, ownerWasm, loadOwner, ownerFlow, record, recordWithBox;
  let readyOnce = null;
  const ready = () => readyOnce || (readyOnce = (async () => {
    wasm = await engines.counter;
    ({ loadCounter } = await engines.host);
    flow = (await engines.model).CounterFlowDemo.getInstance();
    ownerWasm = await engines.owner;
    ({ loadOwner } = await engines.ownerHost);
    ownerFlow = (await engines.ownerModel).OwnerFlowDemo.getInstance();
    record = await engines.record;
    recordWithBox = await engines.recordWithBox;
  })());

  // The page's clock. A visitor who jumps to a later step has the seconds a real counter would have waited skipped.
  // A held clock stands still (the cancelled payment holds it while the visitor reads); letting it go resumes from
  // the held second, so no time is skipped.
  const clock = {
    offset: 0, held: null,
    now() { return this.held !== null ? this.held : Math.floor(Date.now() / 1000) + this.offset; },
    skip(secs) { if (this.held !== null) this.held += secs; else this.offset += secs; },
    hold() { if (this.held === null) this.held = this.now(); },
    release() {
      if (this.held === null) return;
      this.offset = this.held - Math.floor(Date.now() / 1000);
      this.held = null;
    },
  };
  const AMOUNT = "800";
  // The café's second sale, a meal over the counter's instant limit.
  const MEAL = "2000";
  // What a short payment sends of the 8.00 sale (10,877 sats at the demo's price).
  const SHORT = 6000;
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
    clock.release();
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

  /** Lets the browser draw what is on the page before the next long step. */
  const paint = () => new Promise((done) => requestAnimationFrame(() => setTimeout(done, 0)));

  // The Kotlin models call module-global host functions. Keep module installation and node startup together so
  // an older async route cannot replace the hosts of a newer route while it is starting.
  let startTail = Promise.resolve();
  function exclusiveStart(start) {
    const next = startTail.then(start);
    startTail = next.catch(() => {});
    return next;
  }

  function startRun(onEvent, withOwner, current) {
    return exclusiveStart(() => startRunNow(onEvent, withOwner, current));
  }

  async function startRunNow(onEvent, withOwner, current) {
    if (!current()) return null;
    stopRun();
    await ready();
    if (!current()) return null;
    const r = { dead: false, beat: 0, aim: null, onEvent, quiet: false, host: null };
    // Two modules, one chain: both read the page's clock, and every chain action is made on both (one-chain.js).
    const counterChain = await loadCounter(wasm, { now: () => clock.now(), record });
    if (!current()) return null;
    r.host = counterChain;
    const ownerChain = await loadOwner(ownerWasm, { now: () => clock.now() });
    if (!current()) return null;
    r.chain = OneChain(counterChain, ownerChain);
    r.controller = CC.Controller({
      flow, chain: r.chain, clock,
      show(viewOf) {
        if (r.dead || !current()) return;
        shell.show("counter", page(viewOf));
        if (r.aim) shell.target("counter", r.aim, null);
      },
      event(kind, data) {
        if (r.dead || !current()) return;
        // Controller.kt:2498 first: the first time this sale's screen is green or red, its sound plays, once; the model's
        // OutcomePlays (SaleModel.kt:87) keeps count, and the notice of CR-284 shares it. A visitor's skipped taps
        // (r.quiet) are made without it.
        if (kind === "sale" && flow.outcomeFirst(data.number - 1, data.says.colour) && !r.quiet) box.outcome(data.says.colour);
        // Controller.kt:681 onTurns: a sale that turned on another screen plays its colour's sound once, with its notice.
        if (kind === "notice" && !r.quiet) box.outcome(data.colour);
        r.onEvent(kind, data);
      },
    });
    r.owner = OC.Controller({
      flow: ownerFlow, chain: ownerChain, clock,
      show(viewOf) {
        // The owner's phone is on the page only on the merchant's path.
        if (!r.dead && current() && r.ownerShown) shell.show("owner", page(viewOf));
      },
    });
    r.ownerShown = withOwner;
    run = r;
    // The counter first, then a frame for the browser to draw it, then the owner's phone: each start takes a few
    // hundred milliseconds, and the page answers between them.
    r.controller.startNode();
    await paint();
    if (r.dead || !current()) return r;
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

  /** The seconds a real counter waits to see a short payment, run at once. */
  function waitForPartPaid() {
    for (let i = 0; i < 40 && (run.controller.saleData() || {}).state !== "PartPaid"; i++) {
      clock.skip(1);
      pump(run);
    }
  }

  /** The seconds a real counter waits for its own check, run at once: for a visitor who jumped ahead. */
  function waitForPaid() {
    for (let i = 0; i < 40 && !(run.controller.saleData() || {}).isPaid; i++) {
      clock.skip(1);
      pump(run);
    }
  }

  /** The keys of the entry still to type for [keys] (8.00 unless said), and the next thing to tap. */
  function nextTap(minor, keys = AMOUNT) {
    const typed = minor > 0 ? String(minor) : "";
    if (typed === keys) return "Open the sale";
    return keys.startsWith(typed) ? keys[typed.length] : "C";
  }

  /** The Lightning invoice the sale's code carries (the counter adds it a pump or two after the sale opens), or null. */
  function invoiceOf(data) {
    const read = data && data.copyUri && PC.parse(data.copyUri);
    return read && read.lightning ? read.lightning : null;
  }

  /** The seconds a real counter waits for the shop's node to give an invoice, run at once. */
  function waitForInvoice() {
    for (let i = 0; i < 20 && !invoiceOf(run.controller.saleData()); i++) {
      clock.skip(1);
      pump(run);
    }
  }

  /** The wait line while the counter checks a payment the customer sent. */
  const sentLine = (what) => shell.say(what, ["The counter checks it itself. This takes a few seconds."]);

  /** The actor's button, put out of use once tapped. */
  function spendActor() {
    const b = document.querySelector(".actor button");
    if (b) b.disabled = true;
  }

  /** What the register's two ways need of this page. */
  const registerRun = () => ({ assets, palette, clock, flow, ownerFlow, wasm, ownerWasm, loadCounter, loadOwner, stopRun, exclusiveStart, record, recordWithBox });

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
        OpenBmtRegister.play(shell, registerRun());
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
      // The second sale: a meal over the instant limit, paid over Lightning, which is final at once.
      { step: 3, enter() {
        shell.say("Next customer: a meal of 20.00 USD.",
          ["That is over this counter's instant limit of $10. Tap ", { b: "New sale" }, "."]);
        shell.questions(QS.merchant.bigger);
        aim("New sale");
        S.on = { newSale: go };
      }, auto() { c().showNewSale(); } },
      { step: 3, enter() {
        shell.say("The meal comes to 20.00 USD.", ["Type the amount, then tap ", { b: "Open the sale" }, "."]);
        aim(nextTap(flow.entryMinor(), MEAL));
        S.on = { entry: (e) => aim(nextTap(e.minor, MEAL)), sale: go };
      }, auto() { for (const key of MEAL) c().press(key); c().openEntered(); } },
      { step: 3, enter() {
        aim(null);
        // The code carries its Lightning invoice a pump or two after the sale opens.
        const show = (data) => {
          const read = PC.parse(data.copyUri);
          if (read.address) {
            shell.say("The counter shows a code with two ways to pay: on-chain and Lightning.",
              ["This customer pays with Lightning. Tap ", { b: "The customer pays" }, "."]);
          } else {
            // CR-283: over the instant limit the code is the invoice alone, as Lightning is final at once.
            shell.say("Over the instant limit, the counter shows the Lightning code first.",
              ["Lightning is final at once, so nothing waits for a block. Tap ", { b: "The customer pays" }, "."]);
          }
          shell.actor({
            who: "The customer", line: `Pays ${groups(read.sats)} sats over Lightning.`, button: "The customer pays",
            onTap() {
              run.host.payLightning(invoiceOf(run.controller.saleData()));
              spendActor();
              shell.say("The customer paid over Lightning.", ["The counter asks the shop's own Lightning node. This takes a moment."]);
            },
          });
          S.on = { sale: (d) => { if (d.isPaid) go(); } };
        };
        const now = run.controller.saleData();
        if (invoiceOf(now)) show(now);
        else {
          // CR-113: the on-chain code shows at once, and the invoice joins it when the node sends it.
          shell.say("The counter shows a code for this sale.", ["The Lightning code follows as soon as the shop's node sends the invoice."]);
          S.on = { sale: (d) => { if (invoiceOf(d)) show(d); } };
        }
      }, auto() { waitForInvoice(); run.host.payLightning(invoiceOf(run.controller.saleData())); waitForPaid(); } },
      { step: 3, enter() {
        shell.actor(null);
        shell.say("Paid at once. Lightning is final, so the limit does not apply.",
          ["Paid on-chain, a sale over the limit waits for a block, about ten minutes, unless the owner raised the limit. The next sale can start while it waits. Tap ", { b: "Done" }, "."]);
        aim("Done");
        S.on = { home: go };
      }, auto() { c().showHome(); } },
      { step: 4, enter() {
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
        shell.say("A block holds the payment. Your phone shows it in the shop's wallet.", []);
        shell.questions(QS.merchant.books);
      } },
    ];
  }

  // ---- As a customer: the shop's counter (the demo plays the cashier) and your phone with a wallet. ----
  // The moments are customer.js's; this gives them the page's side.
  function customerMoments(go, S) {
    return Cu.moments({
      shell, Wl, QS, PC,
      saleData: () => run.controller.saleData(),
      payCode,
      // The counter's host pays an invoice when it has the call (a pretend Lightning node); false says it has none, and
      // the wallet pays on-chain.
      payLightning: (invoice) => (typeof run.host.payLightning === "function" ? run.host.payLightning(invoice) : false),
      waitForPaid,
      pump: () => pump(run),
    }, go, S);
  }

  // ---- Something goes wrong: a customer's wallet sends too little, then the rest. ----
  function wrongMoments(go, S) {
    const c = () => run.controller;
    return [
      { step: 0, enter() {
        shell.actor(null);
        shell.light("counter", {});
        shell.say("A customer's order comes to 8.00 USD.", ["Type the amount, then tap ", { b: "Open the sale" }, "."]);
        shell.questions(QS.merchant.wrong);
        aim(nextTap(flow.entryMinor()));
        S.on = { entry: (e) => aim(nextTap(e.minor)), sale: go };
      }, auto() { for (const key of AMOUNT) c().press(key); c().openEntered(); } },
      { step: 1, enter() {
        aim(null);
        shell.say("The counter shows a code for this sale.",
          ["This customer's wallet sends too little. Tap ", { b: "The customer pays" }, "."]);
        shell.actor({
          who: "The customer", line: `Pays ${groups(SHORT)} sats.`, button: "The customer pays",
          onTap() {
            run.chain.pay(readCode().address, SHORT);
            spendActor();
            sentLine("The customer sent the payment.");
          },
        });
        S.on = { sale: (d) => { if (d.state === "PartPaid") go(); } };
      }, auto() { run.chain.pay(readCode().address, SHORT); waitForPartPaid(); } },
      { step: 2, enter() {
        shell.say("Part paid: the counter shows how much is missing.",
          ["Do not hand over the goods yet. Tap ", { b: "The customer pays the rest" }, "."]);
        shell.actor({
          who: "The customer", line: "Pays the rest to the same code.", button: "The customer pays the rest",
          onTap() {
            // CR-285: the code of a part paid sale asks for what is missing, so the wallet pays what the code asks.
            payCode(readCode());
            spendActor();
            sentLine("The customer sent the rest.");
          },
        });
        S.on = { sale: (d) => { if (d.isPaid) go(); } };
      }, auto() { payCode(readCode()); waitForPaid(); } },
      { step: 3, enter() {
        shell.actor(null);
        shell.say("Paid. The two payments together cover the sale.", ["Hand over the goods. Tap ", { b: "Done" }, "."]);
        aim("Done");
        S.on = { home: go };
      }, auto() { c().showHome(); } },
      // The last moment says nothing new: the ending card comes over it.
      { step: 3, enter() {
        aim(null);
        shell.actor(null);
        shell.say("Paid. The two payments together cover the sale.", []);
      } },
    ];
  }

  // ---- Something goes wrong, the other way: the customer takes the payment back within the counter's short check. ----
  function cancelledMoments(go, S) {
    const c = () => run.controller;
    // The id of the payment the customer sent: the customer's wallet spends its coin again to take it back.
    let sent = null;
    // The clock is held from the payment until the cancel, so the counter's short check cannot end while the
    // visitor reads; the counter still judges the payment itself.
    const send = () => { const read = readCode(); clock.hold(); sent = run.chain.pay(read.address, read.sats); };
    const cancel = () => { run.chain.cancel(sent); clock.release(); };
    return [
      { step: 0, enter() {
        shell.actor(null);
        shell.light("counter", {});
        shell.say("A customer's order comes to 8.00 USD.", ["Type the amount, then tap ", { b: "Open the sale" }, "."]);
        shell.questions(QS.merchant.wrong);
        aim(nextTap(flow.entryMinor()));
        S.on = { entry: (e) => aim(nextTap(e.minor)), sale: go };
      }, auto() { for (const key of AMOUNT) c().press(key); c().openEntered(); } },
      { step: 1, enter() {
        aim(null);
        const read = readCode();
        shell.say("The counter shows a code for this sale.", ["Tap ", { b: "The customer pays" }, "."]);
        shell.actor({
          who: "The customer", line: `Pays ${groups(read.sats)} sats.`, button: "The customer pays",
          onTap() { send(); spendActor(); go(); },
        });
        S.on = null;
      }, auto: send },
      { step: 1, enter() {
        shell.say("The customer sent the payment. The counter checks for a second payment of the same money (paused here until you tap).",
          ["This customer cancels: tap ", { b: "The customer cancels" }, "."]);
        shell.actor({
          who: "The customer", line: "Sends the same money back to their own wallet.", button: "The customer cancels",
          onTap() { cancel(); spendActor(); },
        });
        S.on = { sale: (d) => { if (d.state === "DoNotHandOver") go(); } };
      }, auto() { cancel(); pump(run); } },
      { step: 2, enter() {
        shell.actor(null);
        shell.say("Red: do not hand over the goods.", []);
      } },
    ];
  }

  // ---- The runner: a moment ends when the model says so; a step tab replays the taps on a fresh model. ----
  const merchant = {
    scenario: "Café", card: "Café",
    steps: ["Ring up", "The customer pays", "Hand over", "A bigger sale", "The owner's phone"],
    devices: [{ id: "counter", caption: "The shop's counter", sizes: COUNTER }, { id: "owner", caption: "The owner's phone", sizes: PHONE }],
    actorAfter: "counter", moments: merchantMoments,
    setup() { run.controller.showHome(); },
  };
  const customer = {
    scenario: "Customer", card: "Customer",
    steps: ["Scan", "Send", "Paid"],
    devices: [{ id: "counter", caption: "The shop's counter", sizes: COUNTER }, { id: "wallet", caption: "Your phone", sizes: PHONE }],
    moments: customerMoments,
    // The cashier's sale of 8.00 USD, opened on the counter's own code.
    setup() { run.controller.openSale(Number(AMOUNT), 0, (words) => console.warn("refused:", words.heading)); },
  };

  const wrong = {
    scenario: "When something goes wrong", sub: "A short payment", card: "Something goes wrong",
    steps: ["Ring up", "A short payment", "The rest", "Paid"],
    devices: [{ id: "counter", caption: "The shop's counter", sizes: COUNTER }],
    actorAfter: "counter", moments: wrongMoments,
    setup() { run.controller.showNewSale(); },
  };

  const cancelled = {
    scenario: "When something goes wrong", sub: "A cancelled payment", card: "Cancelled",
    steps: ["Ring up", "The payment", "Cancelled"],
    devices: [{ id: "counter", caption: "The shop's counter", sizes: COUNTER }],
    actorAfter: "counter", moments: cancelledMoments,
    setup() { run.controller.showNewSale(); },
  };

  let playing = 0;

  /** Plays [path] from its moment [from]: a fresh counter, the earlier moments' taps made for the visitor. */
  async function play(path, from = 0) {
    const S = { index: -1, on: null };
    const mine = ++playing;
    shell.onStep = null;
    shell.begin(path.steps, path.devices, path.actorAfter);
    shell.scenario(path.scenario, path.sub);
    const go = () => enter(S.index + 1);
    const moments = path.moments(go, S);
    function enter(i) {
      if (i >= moments.length) return;
      S.index = i;
      S.on = null;
      shell.step(moments[i].step);
      moments[i].enter();
      // The last moment is the end: its card comes after the instruction has been read.
      if (i === moments.length - 1 && path.card) shell.finish(Shell.CARDS[path.card]);
    }
    // The devices' launch screens are drawn before the counter starts.
    await paint();
    if (mine !== playing) return;
    const started = await startRun((kind, data) => {
      const handler = S.on && S.on[kind];
      if (handler) handler(data);
    }, path.devices.some((d) => d.id === "owner"), () => mine === playing);
    // Another scenario was chosen while this one started: its run is the one to keep.
    if (mine !== playing || !started) { if (run === started) stopRun(); return; }
    path.setup();
    // The visitor's taps before the moment they chose, made on the real model.
    run.quiet = true;
    for (let j = 0; j < from; j++) if (moments[j].auto) moments[j].auto();
    run.quiet = false;
    enter(from);
    shell.onStep = (s) => {
      const target = moments.findIndex((m) => m.step >= s);
      play(path, target < 0 ? 0 : target);
    };
  }

  /** The start screen; [first] leaves its snap to the opening screen, which has played it. */
  const pick = (first) => {
    shell.leave();
    playing++;
    shell.onStep = null;
    stopRun();
    shell.scenario(null);
    shell.start([
      { name: "As a customer", what: "Pay a shop in bitcoin, the way it works at the counter.", go: "Start", onPick: () => shell.pick("Customer") },
      { name: "OpenBMT checkout", what: "Explore an editable sample cart, simulated Bitcoin, card or cash payments, receipts, and order history.", go: "Explore", onPick: () => shell.pick("OpenBMT checkout") },
      { name: "A café", what: "Take a coffee and a meal at the counter, and see them reach the owner's phone.", go: "Start", onPick: () => shell.pick("Café") },
      { name: "With your register", what: "The counter beside the register a shop already has.", go: "Start", onPick: () => shell.pick("With your register") },
      { name: "When something goes wrong", what: "A short payment or a cancelled one, and what the counter does.", go: "Start", onPick: () => shell.pick("A short payment") },
    ], snap, { quiet: first === true });
  };
  shell.onRestart = () => pick();
  // The ending card plays the logo's confirmation sound once, as the box's green outcome does; a red end has none.
  shell.onCard = (leaf) => { if (leaf !== "A cancelled payment") box.outcome("Green"); };
  // A tab, a menu item or a card starts its scenario from the first moment; the register's tab opens its choice.
  shell.onScenario = async (name) => {
    shell.leave();
    const mine = ++playing;
    shell.onStep = null;
    stopRun();
    if (name === "OpenBMT checkout") OpenBmtCheckout.open(shell, { U, C, palette, assets });
    else if (name === "Customer") await play(customer);
    else if (name === "Café") await play(merchant);
    else if (name === "A short payment") await play(wrong);
    else if (name === "A cancelled payment") await play(cancelled);
    else {
      await ready();
      if (mine !== playing) return;
      OpenBmtRegister.open(shell, registerRun(), name === "With your register" ? undefined : name);
    }
  };
  const want = new URLSearchParams(location.search).get("path");
  // The register's ways by name: sends, square, clover ("any", the way's first name, is Square), and register for the choice.
  const registerWay = { sends: "Your register sends the amount", square: "Square", any: "Square", clover: "Clover", register: undefined };
  if (want in registerWay) { await ready(); OpenBmtRegister.open(shell, registerRun(), registerWay[want]); }
  else if (want === "checkout") OpenBmtCheckout.open(shell, { U, C, palette, assets });
  else if (want === "merchant") await play(merchant);
  else if (want === "customer") await play(customer);
  else if (want === "wrong") await play(wrong);
  else if (want === "cancelled") await play(cancelled);
  else {
    // The visitor's first tap unlocks the sounds, and the logo snaps together with them on the opening screen.
    pick(true);
    const apart = F.LogoSnapView(pieces, innerWidth < 900 ? 120 : 160, box, motionOn);
    // The mark's start frame has the pieces off the tile; the pieces are drawn a little apart from their places instead.
    if (motionOn()) {
      const gap = (dy, alpha = 1) => ({ dy, sx: 1, sy: 1, alpha });
      const frame = { ...F.SnapTiming.frame(0), top: gap(-40), b: gap(0), bottom: gap(40), word: gap(34, 0.6) };
      F.onDraw(apart.canvas.getContext("2d"), apart.canvas.width, frame, pieces);
    }
    shell.opening(apart.canvas, {
      play: apart.play, seconds: F.SnapTiming.total,
      get still() { return !motionOn(); },
    });
  }
  document.body.dataset.ready = "1";
})();
