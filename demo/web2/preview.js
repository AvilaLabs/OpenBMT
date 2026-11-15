// The two paths of the demo over the apps' screens with fixed sample data (demo/ui/gallery-data.js), to check the
// page, its words and its questions before the apps' own code drives the screens. The order of moments, the
// instructions, the questions and the taps are the ones the live demo uses. The sale is 8.00 USD: under the
// first instant limit of $10, so the counter turns green on its own check (docs/handbook/getting-paid.md).
"use strict";
(async function () {
  const U = OpenBmtUi, C = OpenBmtChrome, CV = OpenBmtCounterViews, OV = OpenBmtOwnerViews, D = OpenBmtGalleryData;
  const Wl = OpenBmtWallet, QS = OpenBmtQuestions;
  const palette = new U.PaletteOf(await (await fetch("../ui/palette.json")).json());
  const shell = new OpenBmtShell(document.getElementById("app"));
  const ADDRESS = "bcrt1q9p0hpmws4pupj85nrw7d8uxna83wuyp0wnmg50";
  const SATS = 10877;

  // The sample screens at the demo's amount.
  const sale = {
    newSale: Object.assign({}, D.counter.newSale, { entry: "8.00 USD" }),
    unpaid: Object.assign({}, D.counter.unpaid, {
      amountBig: "8.00 USD", amountSmall: "10,877 sats to send", receivedLine: "Received 0 sats of 10,877 sats",
    }),
    paid: Object.assign({}, D.counter.paid, { outcomeLine: "8.00 USD · 10,877 sats" }),
  };

  /** A device page: [view] draws it with the Ui of the device's size. */
  const page = (view) => (size) => {
    const ui = C.deviceUi(palette, false, "../ui/logo.png", size);
    return C.Device(ui, view(ui));
  };
  const COUNTER = [{ label: "Tablet", size: C.TABLET }, { label: "Phone", size: C.PHONE }];
  const PHONE = [{ size: C.PHONE }];

  // ---- As a merchant: the counter and the owner's phone; the demo plays the customer. ----
  const merchant = {
    steps: ["Ring up", "The customer pays", "Hand over", "The owner's phone"],
    devices: [{ id: "counter", caption: "The shop's counter", sizes: COUNTER }, { id: "owner", caption: "The owner's phone", sizes: PHONE }],
    actorAfter: "counter",
    moments: [
      { step: 0, run(go) {
        shell.show("counter", page((ui) => CV.showHome(ui, D.counter.home)));
        shell.show("owner", page((ui) => OV.showHome(ui, D.owner.home)));
        shell.actor(null);
        shell.light("counter", { owner: "Nothing to do here yet." });
        shell.say("", ["You are the cashier. Tap ", { b: "New sale" }, "."]);
        shell.questions(QS.merchant.ringUp);
        shell.target("counter", "New sale", go);
      } },
      { step: 0, run(go) {
        shell.show("counter", page((ui) => CV.showNewSale(ui, sale.newSale)));
        shell.say("The customer's order comes to 8.00 USD.", ["Type the amount, then tap ", { b: "Open the sale" }, "."]);
        shell.target("counter", "Open the sale", go);
      } },
      { step: 1, run(go) {
        shell.show("counter", page((ui) => CV.showSale(ui, sale.unpaid)));
        shell.say("The counter shows a code for this sale.",
          ["The customer scans it with their own wallet. Tap ", { b: "The customer pays" }, "."]);
        shell.questions(QS.merchant.code);
        shell.actor({ who: "The customer", line: "Scans the code and sends 10,877 sats.", button: "The customer pays", onTap: go });
      } },
      { step: 2, run(go) {
        shell.actor(null);
        shell.show("counter", page((ui) => CV.showSale(ui, sale.paid)));
        shell.say("Paid. The counter saw the payment itself, in a few seconds.",
          ["Read the green screen: hand over the goods. Then tap ", { b: "Done" }, "."]);
        shell.questions(QS.merchant.paid);
        shell.target("counter", "Done", go);
      } },
      { step: 3, run(go) {
        shell.show("counter", page((ui) => CV.showHome(ui, D.counter.home)));
        shell.light("owner", { counter: "Ready for the next sale." });
        shell.say("The sale is done at the counter.", ["Now you are the owner. On your phone, tap ", { b: "Money" }, "."]);
        shell.questions(QS.merchant.owner);
        shell.target("owner", "Money", go);
      } },
      { step: 3, run(go) {
        shell.show("owner", page((ui) => OV.showMoney(ui, D.owner.money)));
        shell.say("", ["Tap ", { b: "Books" }, "."]);
        shell.target("owner", "Books", go);
      } },
      { step: 3, run(go) {
        shell.show("owner", page((ui) => OV.showBooks(ui, D.owner.books)));
        shell.say("Today's sales are in your books.",
          ["In a real shop the payment is settled when a block holds it, in about ten minutes."],
          { label: "Skip ahead", onSkip: go });
        shell.questions(QS.merchant.books);
      } },
      { step: 4, run() {
        shell.say("A block holds the payment: the sale is settled.",
          ["That is a whole sale. Tap ", { b: "Start over" }, " to try it as the customer."]);
      } },
    ],
  };

  // ---- As a customer: the shop's counter (the demo plays the cashier) and your phone with a wallet. ----
  const customer = {
    steps: ["Scan", "Send", "Paid"],
    devices: [{ id: "counter", caption: "The shop's counter", sizes: COUNTER }, { id: "wallet", caption: "Your phone", sizes: PHONE }],
    moments: [
      { step: 0, run(go) {
        shell.show("counter", page((ui) => CV.showSale(ui, sale.unpaid)));
        shell.show("wallet", () => Wl.home(250000));
        shell.light("wallet", { counter: "The cashier's screen. It waits for your payment." });
        shell.say("The cashier rang up your order: 8.00 USD. The counter shows a code.",
          ["You are the customer. On your phone, tap ", { b: "Scan" }, "."]);
        shell.questions(QS.customer.scan);
        shell.target("wallet", "Scan", go);
      } },
      { step: 0, run(go) {
        shell.show("wallet", () => Wl.scanning());
        shell.say("", ["Your phone reads the code on the counter."]);
        setTimeout(go, 1400);
      } },
      { step: 1, run(go) {
        shell.show("wallet", () => Wl.confirm(SATS, ADDRESS));
        shell.say("Your phone read the code: the amount and the shop's address.",
          ["Check the amount, then tap ", { b: "Send" }, "."]);
        shell.questions(QS.customer.send);
        shell.target("wallet", "Send", go);
      } },
      { step: 2, run(go) {
        shell.show("wallet", () => Wl.sent(SATS));
        shell.show("counter", page((ui) => CV.showSale(ui, sale.paid)));
        shell.light("counter", { wallet: "Your payment is sent." });
        shell.say("Your phone sent the payment. The counter saw it in a few seconds.",
          ["The green screen tells the cashier to hand over your order."], { label: "Next", onSkip: go });
        shell.questions(QS.customer.paid);
      } },
      { step: 3, run() {
        shell.light("counter", {});
        shell.say("You paid from your own wallet, straight to the shop's own wallet, with no company in between.",
          ["That is all a customer does. Tap ", { b: "Start over" }, " to try it as the merchant."]);
      } },
    ],
  };

  function play(path) {
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
    run();
  }

  const pick = () => shell.start([
    { name: "As a customer", what: "Pay a shop in bitcoin, the way it works at the counter.", go: "Start", onPick: () => play(customer) },
    { name: "As a merchant", what: "Take a payment at the counter and see it reach the owner's phone.", go: "Start", onPick: () => play(merchant) },
  ]);
  shell.onRestart = pick;
  const want = new URLSearchParams(location.search).get("path");
  if (want === "merchant") play(merchant); else if (want === "customer") play(customer); else pick();
  document.body.dataset.ready = "1";
})();
