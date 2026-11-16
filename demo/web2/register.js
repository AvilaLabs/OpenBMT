// "With your register": the counter beside the shop's existing register, in two ways. The page plays the register
// (a plain mock, register-views.js) and, in the first way, the shop's Box; everything else is the apps' own code.
//
// The first way: a register that can send the amount. The Box tells the counter that the till asks for a sale
// (CR-266), which is what the Box's till listener does for the real register; the counter shows the card "From the
// till" and its own Start opens the sale (CR-267, CR-268); the register closes its order when the counter's own state
// says the till's sale is paid, which is what the paid notice that names the till's sale says to the Box (BX-77).
// The page does not make the Box's seal: the request goes to the node as the link hands it, after the seal is read.
//
// The other way: any register. The cashier types the amount on the counter, closes the order on the register with the
// Bitcoin tender, and the owner's phone matches the register's order to the counter's sale with the matcher of
// crates/openbmt-pos, the library the owner app calls. The sale's facts are read by the page from the counter's state;
// the owner app reads them from the counter's records, which this demo's phone does not hold.
//
// The page calls `OpenBmtRegister.play(shell, run)` at the keypad of the merchant's path (live.js): it adds a link
// under the instruction line that opens this view. [run] is what live.js has: the assets, the palette, the clock, the
// two models, the two modules' bytes and loaders, and how to stop its own run.
"use strict";
(function (root) {
  const MINOR = 800;
  const KEYS = "800";
  const groups = (n) => Number(n).toLocaleString("en-US");

  // ---- What does not need a page: the facts the matcher is given, and its verdict. ----

  /** "8.00 USD" as minor units: 800. */
  function minorOf(shown) {
    return Number(String(shown).split(" ")[0].replace(/[^0-9]/g, ""));
  }

  /**
   * The request of the owner module's matcher: the register's order and the counter's sale as numbers (see
   * crates/openbmt-demo-owner/src/register.rs). A time that is not yet is 0; [sale].index is the sale's number.
   */
  function requestText(f, now) {
    const s = f.sale || {};
    return [f.totalMinor, f.orderOpened, f.closedAt || 0, s.number || 0, s.amountMinor || 0, s.opened || 0, s.paid || 0, now].join(" ");
  }

  /** The matcher's verdict as text (`matches=1;by_order=0;alerts=0;alert=...`) read: {matches, byOrder, alerts}. */
  function parseVerdict(text) {
    const out = { matches: 0, byOrder: 0, alerts: [] };
    for (const part of text.split(";")) {
      const [k, ...v] = part.split("=");
      const value = v.join("=");
      if (k === "matches") out.matches = Number(value);
      else if (k === "by_order") out.byOrder = Number(value);
      else if (k === "alert") out.alerts.push(value);
    }
    return out;
  }

  /** The lines of the owner's till screen for a verdict at [now], as the owner app's PosLines writes them. */
  function tillLines(ownerFlow, verdict, now) {
    const [lastRead, counts] = Array.from(ownerFlow.tillLines(now, now, verdict.matches, verdict.byOrder, verdict.alerts.length));
    return { lastRead, counts, alerts: verdict.alerts.length };
  }

  // ---- The view. ----

  /** The link under the instruction line of the keypad moment, and what it opens. */
  function play(shell, run) {
    const link = document.createElement("button");
    link.className = "register-link";
    link.textContent = "Already have a register? See how it connects.";
    const st = link.style;
    st.background = "none";
    st.border = "0";
    st.color = "var(--blue)";
    st.font = "inherit";
    st.fontSize = "15px";
    st.textDecoration = "underline";
    st.cursor = "pointer";
    link.addEventListener("click", () => open(shell, run));
    shell.doEl.append(link);
    shell.fit();
  }

  function open(shell, ctx) {
    const C = OpenBmtChrome, RV = OpenBmtRegisterViews;
    const CC = OpenBmtCounterController, OC = OpenBmtOwnerController, { OneChain } = OpenBmtOneChain;
    const clock = ctx.clock, flow = ctx.flow, ownerFlow = ctx.ownerFlow;
    ctx.stopRun();

    const COUNTER = [{ label: "Tablet", size: C.TABLET }, { label: "Phone", size: C.PHONE }];
    const PHONE = [{ size: C.PHONE }];
    const pageOf = (view) => (size) => {
      const ui = C.deviceUi(ctx.palette, false, ctx.assets.logo, size);
      return C.Device(ui, view(ui));
    };

    let run = null;
    const previousRestart = shell.onRestart;

    function stopMine() {
      if (!run) return;
      run.dead = true;
      clearInterval(run.timer);
      run.controller.stopNode();
      run.owner.stopNode();
      run = null;
    }

    function leave() {
      stopMine();
      const chooser = shell.root.querySelector(".start");
      if (chooser) chooser.remove();
      shell.onRestart = previousRestart;
      shell.onStep = null;
      shell.actor(null);
    }
    shell.onRestart = () => { leave(); if (previousRestart) previousRestart(); };

    /** What the page does for the counter, as live.js does: the owner's heartbeat, the network's events, the tick. */
    function pump(r) {
      if (r.dead) return;
      const now = clock.now();
      if (now - r.beat >= 3) {
        r.beat = now;
        flow.heartbeat(now, r.chain.heartbeat());
      }
      flow.pump(now);
      ownerFlow.pump(now);
      r.controller.onPump(true);
      r.owner.onPump(true);
    }

    async function startRun(onEvent, withBox, ownerShown) {
      stopMine();
      const r = { dead: false, beat: 0, aim: null, ownerShown, registerPaid: false };
      const counterChain = await ctx.loadCounter(ctx.wasm, { now: () => clock.now() });
      const ownerChain = await ctx.loadOwner(ctx.ownerWasm, { now: () => clock.now() });
      r.chain = OneChain(counterChain, ownerChain);
      r.controller = CC.Controller({
        flow, chain: r.chain, clock, withBox,
        show(viewOf) {
          if (r.dead) return;
          shell.show("counter", pageOf(viewOf));
          if (r.aim) shell.target("counter", r.aim, null);
        },
        event(kind, data) { if (!r.dead) onEvent(kind, data); },
      });
      r.owner = OC.Controller({
        flow: ownerFlow, chain: ownerChain, clock,
        show(viewOf) { if (!r.dead && r.ownerShown) shell.show("owner", pageOf(viewOf)); },
      });
      run = r;
      r.controller.startNode();
      r.owner.startNode();
      r.owner.render();
      pump(r);
      r.timer = setInterval(() => pump(r), 1000);
      return r;
    }

    function aim(label) {
      run.aim = label;
      if (label) shell.target("counter", label, null);
    }

    /** The register, drawn on its device: closed or not. */
    function showRegister(paid) {
      shell.show("register", () => RV.register({ paid }));
    }

    function readCode() {
      const data = run.controller.saleData();
      const read = data && OpenBmtPaymentCode.parse(data.copyUri);
      if (!read) throw new Error("the counter shows no payment code");
      return read;
    }

    function waitForPaid() {
      for (let i = 0; i < 40 && !(run.controller.saleData() || {}).isPaid; i++) {
        clock.skip(1);
        pump(run);
      }
    }

    function nextTap(minor) {
      const typed = minor > 0 ? String(minor) : "";
      if (typed === KEYS) return "Open the sale";
      return KEYS.startsWith(typed) ? KEYS[typed.length] : "C";
    }

    /** The customer's moment, in both ways: the code, the customer who pays, and the wait while the counter checks. */
    function customerMoment(go, S, instruction) {
      aim(null);
      const read = readCode();
      shell.say("The counter shows a code for this sale.", instruction);
      shell.actor({
        who: "The customer", line: `Scans the code and sends ${groups(read.sats)} sats.`, button: "The customer pays",
        onTap() {
          run.chain.pay(read.address, read.sats);
          const b = document.querySelector(".actor button");
          if (b) b.disabled = true;
          shell.say("The customer sent the payment.", ["The counter checks it itself. This takes a few seconds."]);
        },
      });
      S.on = { sale: (d) => { if (d.isPaid) go(); } };
    }
    const customerAuto = () => { run.chain.pay(readCode().address, readCode().sats); waitForPaid(); };

    // ---- The first way: the register sends the amount. ----
    function sendsMoments(go, S, facts) {
      const c = () => run.controller;
      const tillRequest = () => { facts.till = run.chain.tillRequest(MINOR); c().tick(); };
      return [
        { step: 0, enter() {
          shell.actor(null);
          shell.light("register");
          showRegister(false);
          shell.say("A customer's order comes to 8.00 USD on your register.", ["You are the cashier. On the register, tap ", { b: "Bitcoin" }, "."]);
          shell.target("register", "Bitcoin", () => { tillRequest(); go(); });
          S.on = null;
        }, auto() { tillRequest(); } },
        { step: 0, enter() {
          shell.light("counter");
          shell.say("The register sent the amount to the counter.", ["On the counter, tap ", { b: "Start" }, "."]);
          aim("Start");
          S.on = { sale: go };
        }, auto() { c().tillCards()[0].start(); } },
        { step: 1, enter() {
          customerMoment(go, S, ["Nothing was typed twice. Tap ", { b: "The customer pays" }, "."]);
        }, auto: customerAuto },
        { step: 2, enter() {
          shell.actor(null);
          // The register closes its own order when the counter's own state says the till's sale is paid (BX-77).
          const paid = c().tillOfSale() === facts.till && (c().saleData() || {}).isPaid === true;
          showRegister(paid);
          shell.light("counter");
          shell.say("Paid. The register closes the order by itself.", ["Read the green screen: hand over the goods. Then tap ", { b: "Done" }, "."]);
          aim("Done");
          S.on = { home: go };
        }, auto() { c().showHome(); } },
        { step: 2, enter() {
          aim(null);
          shell.say("That is a sale with your register.", ["Tap ", { b: "Start over" }, " to try another way."]);
        } },
      ];
    }

    // ---- The other way: any register; the owner's phone matches the register's order to the counter's sale. ----
    function anyMoments(go, S, facts) {
      const c = () => run.controller;
      const closeOrder = () => { if (!facts.closedAt) facts.closedAt = clock.now(); };
      const verdictNow = () => {
        const now = clock.now();
        return parseVerdict(run.chain.registerVerdict(requestText(facts, now)));
      };
      return [
        { step: 0, enter() {
          shell.actor(null);
          shell.light("counter");
          showRegister(false);
          shell.say("A customer's order comes to 8.00 USD on your register.", ["You are the cashier. Type the same amount on the counter: tap ", { b: "New sale" }, "."]);
          aim("New sale");
          S.on = { newSale: go };
        }, auto() { c().showNewSale(); } },
        { step: 0, enter() {
          shell.say("", ["Type 8.00, then tap ", { b: "Open the sale" }, "."]);
          aim(nextTap(flow.entryMinor()));
          S.on = { entry: (e) => aim(nextTap(e.minor)), sale: go };
        }, auto() { for (const key of KEYS) c().press(key); c().openEntered(); } },
        { step: 1, enter() {
          customerMoment(go, S, ["The customer scans it with their own wallet. Tap ", { b: "The customer pays" }, "."]);
        }, auto: customerAuto },
        { step: 2, enter() {
          shell.actor(null);
          shell.light("register");
          shell.say("Paid. Now close the order on your register.", ["On the register, tap ", { b: "Bitcoin" }, "."]);
          shell.target("register", "Bitcoin", () => { closeOrder(); showRegister(true); go(); });
        }, auto() { closeOrder(); showRegister(true); } },
        { step: 3, enter() {
          shell.light("owner");
          showRegister(true);
          shell.say("The owner's phone reads the register's orders and matches each Bitcoin payment to a counter sale.",
            ["This one matches. A Bitcoin order with no counter sale would raise an alert. Tap ", { b: "Start over" }, " to try another way."]);
          // The owner's phone shows its till screen from here on, not its home.
          run.ownerShown = false;
          const lines = tillLines(ownerFlow, verdictNow(), clock.now());
          shell.show("owner", pageOf((ui) => RV.tillScreen(ui, lines)));
        } },
      ];
    }

    const ways = {
      sends: { steps: ["Ring up", "The customer pays", "Done"], withBox: true, withOwner: false, moments: sendsMoments,
        devices: [{ id: "register", caption: "Your register", sizes: PHONE }, { id: "counter", caption: "The shop's counter", sizes: COUNTER }] },
      any: { steps: ["Ring up", "The customer pays", "Close the order", "The owner's phone"], withBox: false, withOwner: true, moments: anyMoments,
        devices: [{ id: "register", caption: "Your register", sizes: PHONE }, { id: "counter", caption: "The shop's counter", sizes: COUNTER },
          { id: "owner", caption: "The owner's phone", sizes: PHONE }] },
    };

    /** Plays [way] from its moment [from]: a fresh counter, the earlier moments' taps made for the visitor. */
    async function playWay(way, from = 0) {
      const S = { index: -1, on: null };
      const facts = { totalMinor: MINOR, orderOpened: clock.now(), closedAt: 0, sale: null, till: null };
      shell.begin(way.steps, way.devices, "counter");
      const go = () => enter(S.index + 1);
      const moments = way.moments(go, S, facts);
      function enter(i) {
        if (i >= moments.length) return;
        S.index = i;
        S.on = null;
        shell.step(moments[i].step);
        moments[i].enter();
      }
      await startRun((kind, data) => {
        if (kind === "sale") {
          if (!facts.sale) facts.sale = { number: data.number, amountMinor: minorOf(data.amountBig), opened: clock.now(), paid: 0 };
          if (data.isPaid && !facts.sale.paid) facts.sale.paid = clock.now();
        }
        const handler = S.on && S.on[kind];
        if (handler) handler(data);
      }, way.withBox, way.withOwner);
      run.controller.showHome();
      for (let j = 0; j < from; j++) if (moments[j].auto) moments[j].auto();
      enter(from);
      shell.onStep = (s) => {
        const target = moments.findIndex((m) => m.step >= s);
        playWay(way, target < 0 ? 0 : target);
      };
    }

    // The two ways, on the shell's start look.
    const chooser = shell.root.querySelector(".start");
    if (chooser) chooser.remove();
    shell.start([
      { name: "Your register sends the amount", what: "Registers that can send the amount to the counter. Nothing is typed twice.", go: "", onPick: () => playWay(ways.sends) },
      { name: "Square, Clover and others", what: "Ring up on your register, type the amount on the counter, then close the order as Bitcoin. The owner's phone checks that they match.", go: "", onPick: () => playWay(ways.any) },
    ]);
  }

  const api = { play, minorOf, requestText, parseVerdict, tillLines, MINOR, KEYS };
  root.OpenBmtRegister = api;
  if (typeof module !== "undefined" && module.exports) module.exports = api;
})(globalThis);
