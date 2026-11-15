// The two paths of the demo over the apps' screens with fixed sample data (demo/ui/gallery-data.js), to check the
// page and its words before the apps' own model drives the screens. The order of moments, the instructions and
// the taps are the ones the live demo uses.
"use strict";
(async function () {
  const U = OpenBmtUi, C = OpenBmtChrome, CV = OpenBmtCounterViews, OV = OpenBmtOwnerViews, D = OpenBmtGalleryData;
  const Wl = OpenBmtWallet;
  const palette = new U.PaletteOf(await (await fetch("../ui/palette.json")).json());
  const shell = new OpenBmtShell(document.getElementById("app"));
  const ADDRESS = "bcrt1q9p0hpmws4pupj85nrw7d8uxna83wuyp0wnmg50";
  const SATS = 33309;

  const counter = (build) => { const ui = C.deviceUi(palette, false, "../ui/logo.png"); return C.Device(ui, build(ui)); };
  const owner = counter;

  // ---- As a merchant: the counter and the owner's phone; the demo plays the customer. ----
  const merchant = {
    steps: ["Ring up", "The customer pays", "Hand over", "The owner's phone"],
    devices: [{ id: "counter", caption: "The shop's counter" }, { id: "owner", caption: "The owner's phone" }],
    actorAfter: "counter",
    moments: [
      { step: 0, run(go) {
        shell.show("counter", counter((ui) => CV.showHome(ui, D.counter.home)));
        shell.show("owner", owner((ui) => OV.showHome(ui, D.owner.home)));
        shell.actor(null);
        shell.light("counter", { owner: "Nothing to do here yet." });
        shell.say("", ["You are the cashier. Tap ", { b: "New sale" }, "."]);
        shell.target("counter", "New sale", go);
      } },
      { step: 0, run(go) {
        shell.show("counter", counter((ui) => CV.showNewSale(ui, D.counter.newSale)));
        shell.say("The customer's order comes to 24.50 USD.", ["Type the amount, then tap ", { b: "Open the sale" }, "."]);
        shell.target("counter", "Open the sale", go);
      } },
      { step: 1, run(go) {
        shell.show("counter", counter((ui) => CV.showSale(ui, D.counter.unpaid)));
        shell.say("The counter shows a code for this sale.",
          ["The customer scans it with their own wallet. Tap ", { b: "The customer pays" }, "."]);
        shell.actor({ who: "The customer", line: "Scans the code and sends 33,309 sats.", button: "The customer pays", onTap: go });
      } },
      { step: 2, run(go) {
        shell.actor(null);
        shell.show("counter", counter((ui) => CV.showSale(ui, D.counter.paid)));
        shell.say("Paid. The counter saw the payment itself, in a few seconds.",
          ["Read the green screen: hand over the goods. Then tap ", { b: "Done" }, "."]);
        shell.target("counter", "Done", go);
      } },
      { step: 3, run(go) {
        shell.show("counter", counter((ui) => CV.showHome(ui, D.counter.home)));
        shell.light("owner", { counter: "Ready for the next sale." });
        shell.say("The sale is done at the counter.", ["Now you are the owner. On your phone, tap ", { b: "Money" }, "."]);
        shell.target("owner", "Money", go);
      } },
      { step: 3, run(go) {
        shell.show("owner", owner((ui) => OV.showMoney(ui, D.owner.money)));
        shell.say("", ["Tap ", { b: "Books" }, "."]);
        shell.target("owner", "Books", go);
      } },
      { step: 3, run(go) {
        shell.show("owner", owner((ui) => OV.showBooks(ui, D.owner.books)));
        shell.say("Today's sales are in your books.",
          ["In a real shop the payment is settled when a block holds it, in about ten minutes."],
          { label: "Skip ahead", onSkip: go });
      } },
      { step: 4, run() {
        shell.say("A block holds the payment: the sale is settled.",
          ["That is a whole sale. Tap ", { b: "Start over" }, " to try it as the customer."]);
      } },
    ],
  };

  // ---- As a customer: the shop's counter (the demo plays the cashier) and your phone with a wallet. ----
  const wallet = (screen) => screen;
  const customer = {
    steps: ["Scan", "Send", "Paid"],
    devices: [{ id: "counter", caption: "The shop's counter" }, { id: "wallet", caption: "Your phone" }],
    moments: [
      { step: 0, run(go) {
        shell.show("counter", counter((ui) => CV.showSale(ui, D.counter.unpaid)));
        shell.show("wallet", wallet(Wl.home(250000)));
        shell.light("wallet", { counter: "The cashier's screen. It waits for your payment." });
        shell.say("The cashier rang up your order: 24.50 USD. The counter shows a code.",
          ["You are the customer. On your phone, tap ", { b: "Scan" }, "."]);
        shell.target("wallet", "Scan", go);
      } },
      { step: 0, run(go) {
        shell.show("wallet", wallet(Wl.scanning()));
        shell.say("", ["Your phone reads the code on the counter."]);
        setTimeout(go, 1400);
      } },
      { step: 1, run(go) {
        shell.show("wallet", wallet(Wl.confirm(SATS, ADDRESS)));
        shell.say("Your phone read the code: the amount and the shop's address.",
          ["Check the amount, then tap ", { b: "Send" }, "."]);
        shell.target("wallet", "Send", go);
      } },
      { step: 2, run(go) {
        shell.show("wallet", wallet(Wl.sent(SATS)));
        shell.show("counter", counter((ui) => CV.showSale(ui, D.counter.paid)));
        shell.light("counter", { wallet: "Your payment is sent." });
        shell.say("Your phone sent the payment. The counter saw it in a few seconds.",
          ["The green screen tells the cashier to hand over your order."], { label: "Next", onSkip: go });
      } },
      { step: 3, run() {
        shell.light("counter", {});
        shell.say("You paid from your own wallet, straight to the shop's own wallet, with no company in between.",
          ["That is all a customer does. Tap ", { b: "Start over" }, " to try it as the merchant."]);
      } },
    ],
  };

  function play(path, from = 0) {
    shell.begin(path.steps, path.devices, path.actorAfter);
    let i = 0;
    const run = () => {
      const m = path.moments[i];
      shell.step(m.step);
      m.run(() => { i += 1; if (i < path.moments.length) run(); });
    };
    // A step tab plays every moment before that step with the taps made for the visitor.
    shell.onStep = (s) => {
      shell.begin(path.steps, path.devices, path.actorAfter);
      i = path.moments.findIndex((m) => m.step >= s);
      for (let j = 0; j < i; j++) path.moments[j].run(() => {});
      run();
    };
    i = from;
    run();
  }

  const pick = () => shell.start([
    { name: "As a customer", what: "Pay a shop in bitcoin, the way it works at the counter.", go: "Start", onPick: () => play(customer) },
    { name: "As a merchant", what: "Take a payment at the counter and see it reach the owner's phone.", go: "Start", onPick: () => play(merchant) },
  ]);
  shell.onRestart = pick;
  const want = new URLSearchParams(location.search).get("path");
  if (want === "merchant") play(merchant); else if (want === "customer") play(customer); else pick();
  const at = Number(new URLSearchParams(location.search).get("step"));
  if (want && at) shell.onStep(at);
  document.body.dataset.ready = "1";
})();
