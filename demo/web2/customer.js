// The customer's path of the demo: the moments of the visitor's phone, with a wallet that scans the counter's code or
// taps the counter, and pays on-chain or, when the request carries a Lightning invoice, over Lightning. The page
// (live.js) gives it the things that touch the page and the pretend chain (`env`), so demo/tests/customer-path.test.mjs
// runs the same moments in node with a stub of each.
//
// env:
//   shell, Wl, QS         the page's shell, the wallet's screens and the questions
//   PC                    payment-code.js
//   saleData()            the data of the sale on the counter's screen (counter-controller.js), or null
//   payCode(read)         the wallet pays the request's address and amount on the pretend chain
//   payLightning(invoice) the host's `payLightning`, or null when the host has none (feature-detected by the page)
//   waitForPaid()         the seconds a real counter waits for its own check, run at once
//   pump()                the page's pump: the counter takes what happened
"use strict";
(function (root) {
  /** The words of the choice of how to pay: the wallet's own row. */
  const LIGHTNING = "Lightning", ON_CHAIN = "On-chain";

  /** The moments of the customer's path. [go] ends a moment; [S] is the runner's state ({index, on}). */
  function moments(env, go, S) {
    const { shell, Wl, QS, PC } = env;
    // How the visitor's phone read the payment, the request it read, how it pays, and how it paid.
    let via = "scan";
    let read = null;
    let method = LIGHTNING;
    let paidBy = "chain";

    /** The wallet scans the sale's own code: the address and the amount of its payment request. */
    function readCode() {
      const data = env.saleData();
      const code = data && PC.parse(data.copyUri);
      if (!code) throw new Error("the counter shows no payment code");
      return code;
    }

    /** The wallet reads the counter's tag, the same request as an NDEF file (TP-01). */
    function readTag() {
      const data = env.saleData();
      const text = data && PC.fromTag(data.tag);
      const code = text && PC.parse(text);
      if (!code) throw new Error("the counter's tag holds no payment");
      return code;
    }

    const readNow = () => (via === "tap" ? readTag() : readCode());

    /** The wallet pays how the visitor chose: over Lightning when it can, on-chain otherwise. */
    function send() {
      if (read.lightning && method === LIGHTNING && env.payLightning && env.payLightning(read.lightning) !== false) {
        paidBy = "lightning";
        env.pump();
      } else {
        paidBy = "chain";
        env.payCode(read);
      }
    }

    /** The wallet's last screens, once the counter has turned paid. */
    const paidNow = () => {
      shell.say("Your phone sent the payment. The counter saw it in a few seconds.",
        ["The green screen tells the cashier to hand over your order."], { label: "Next", onSkip: go });
      shell.questions(QS.customer.paid);
    };

    /** The check before sending, drawn again whenever the visitor picks the other way to pay. */
    function drawConfirm() {
      const pay = read.lightning ? { method, onPick(m) { method = m; drawConfirm(); } } : null;
      shell.show("wallet", () => Wl.confirm(read.sats, read.address, pay));
      shell.target("wallet", "Send", () => { send(); go(); });
    }

    return [
      // The customer's phone on its home screen: Bitcoin Wallet to scan, or the phone held to the counter to tap.
      { step: 0, enter() {
        via = "scan";
        shell.show("wallet", () => Wl.homeScreen());
        shell.under("wallet", "Hold to the counter");
        shell.light("wallet", { counter: "The cashier's screen. It waits for your payment." });
        shell.say("The cashier rang up your order: 8.00 USD. The counter shows a code.",
          ["You are the customer. Tap ", { b: "Bitcoin Wallet" }, " to scan the code, or ", { b: "Hold to the counter" }, " to pay with a tap."]);
        shell.questions(QS.customer.scan);
        shell.targets("wallet", [
          { label: "Bitcoin Wallet", onTap() { via = "scan"; go(); } },
          { label: "Hold to the counter", onTap() { via = "tap"; go(); } },
        ]);
      } },
      // The wallet opened by the customer, with Scan; or the phone reading the counter's tag for about a second.
      { step: 0, enter() {
        if (via === "tap") {
          shell.show("wallet", () => Wl.tapping());
          shell.say("", ["Your phone reads the payment from the counter, without the camera."]);
          const at = S.index;
          setTimeout(() => { if (S.index === at) go(); }, 1000);
          return;
        }
        shell.show("wallet", () => Wl.home(250000));
        shell.say("", ["Your wallet is open. Tap ", { b: "Scan" }, "."]);
        shell.questions(QS.customer.scan);
        shell.target("wallet", "Scan", go);
      } },
      { step: 0, enter() {
        // A tap has read the payment already: the wallet opens on the check.
        if (via === "tap") { go(); return; }
        shell.show("wallet", () => Wl.scanning());
        shell.say("", ["Your phone reads the code on the counter."]);
        const at = S.index;
        setTimeout(() => { if (S.index === at) go(); }, 1400);
      } },
      { step: 1, enter() {
        read = readNow();
        method = LIGHTNING;
        const opened = via === "tap" ? "Your phone read the payment from the counter and opened your wallet." : null;
        if (read.lightning) {
          shell.say(opened || "Your phone read the payment: the amount and two ways to pay it.",
            ["Pick ", { b: "Lightning" }, " or ", { b: "On-chain" }, ", then tap ", { b: "Send" }, "."]);
        } else {
          shell.say(opened || "Your phone read the code: the amount and the shop's address.",
            ["Check the amount, then tap ", { b: "Send" }, "."]);
        }
        shell.questions(QS.customer.send);
        drawConfirm();
      }, auto() { via = "tap"; read = readNow(); method = LIGHTNING; send(); env.waitForPaid(); } },
      { step: 2, enter() {
        shell.show("wallet", () => Wl.sent(read.sats));
        shell.light("counter", { wallet: "Your payment is sent." });
        if (paidBy === "lightning") {
          // The line waits for the counter's own state: the counter asks the shop's node and turns paid.
          const lnPaid = () => {
            shell.say("Your phone paid over Lightning. The counter knew at once.",
              ["The green screen tells the cashier to hand over your order."], { label: "Next", onSkip: go });
            shell.questions(QS.customer.paidLightning);
          };
          shell.questions([]);
          const now = env.saleData();
          if (now && now.isPaid) lnPaid();
          else S.on = { sale: (d) => { if (d.isPaid) { S.on = null; lnPaid(); } } };
          return;
        }
        shell.say("Your phone sent the payment.", ["Watch the shop's counter: it checks the payment itself."]);
        shell.questions([]);
        const data = env.saleData();
        if (data && data.isPaid) paidNow();
        else S.on = { sale: (d) => { if (d.isPaid) { S.on = null; paidNow(); } } };
      } },
      { step: 2, enter() {
        shell.light("counter", {});
        shell.say("You paid from your own wallet, straight to the shop's own wallet, with no company in between.", []);
      } },
    ];
  }

  const api = { moments, LIGHTNING, ON_CHAIN };
  root.OpenBmtCustomer = api;
  if (typeof module !== "undefined" && module.exports) module.exports = api;
})(globalThis);
