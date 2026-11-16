// Proposed merchant checkout using the demo shell's walkthrough, device frame, and customer actor.
// All payment outcomes below are simulations; this path has no payment SDK or card processor.
"use strict";
(function (root) {
  const ITEMS = [{ name: "Oat latte", price: 5.5 }, { name: "Sesame bagel", price: 4.5 }];
  const STEPS = ["Start sale", "Review cart", "Choose payment", "Payment", "Receipt"];
  const DEVICE_ID = "merchant";
  // Valid QR modules from crates/openbmt-qr for the explicitly nonpayable text payload "OPENBMT DEMO ONLY - NO PAYMENT".
  const DEMO_CODE = [
    "1111111011110000101111111", "1000001011010001101000001", "1011101001001100101011101", "1011101011011110001011101",
    "1011101000100101101011101", "1000001001110110001000001", "1111111010101010101111111", "0000000011111101000000000",
    "1011011100010100001001011", "1001110001010010110011111", "0010001001011011010110110", "1100100100010000101110010",
    "1111111110011100001101011", "0010100011011010000100000", "0110001100110100000111100", "1001100100010000001110011",
    "0000011010101010111110100", "0000000010000001100010000", "1111111011100110101010011", "1000001011010110100011101",
    "1011101000100111111110011", "1011101011100001111111101", "1011101011100011111110110", "1000001001011110000011101",
    "1111111010110000111100001",
  ].map((row) => Array.from(row, (bit) => bit === "1"));
  let interrupted = null;
  let receipts = [];
  let nextOrder = 1042;

  const money = (n) => "$" + n.toFixed(2);
  const fresh = (order = nextOrder) => ({ order, status: "new", method: null, qty: [1, 1] });
  const amountOf = (sale) => ITEMS.reduce((n, item, i) => n + item.price * sale.qty[i], 0);

  function open(shell, context) {
    const U = context.U, C = context.C;
    const device = { id: DEVICE_ID, caption: "Juniper Coffee", sizes: [{ label: "Tablet", size: C.TABLET }, { label: "Phone", size: C.PHONE }] };
    const previousRestart = shell.onRestart;
    let state = fresh();
    let selectedMethod = null;
    let active = true;

    const total = () => ITEMS.reduce((n, item, i) => n + item.price * state.qty[i], 0);
    const itemSummary = () => ITEMS.map((item, i) => `${state.qty[i]} × ${item.name}`).join(" · ");

    function viewFor(size, screen) {
      const ui = C.deviceUi(context.palette, false, context.assets.logo, size);
      const body = [];
      const moneyLines = (pairs) => ui.facts(pairs);
      if (screen === "start") {
        body.push(ui.text("Illustrative catalog", 20, true));
        body.push(ui.text("Start the walkthrough with a latte and a bagel from this sample catalog.", 14, false, false, ui.pal.secondary));
        body.push(ui.text("Recovery and prior receipts last only for this browser session.", 13, false, false, ui.pal.secondary));
        body.push(moneyLines(ITEMS.map((item) => [item.name, money(item.price)])));
        if (interrupted) body.push(ui.text(`Interrupted order #${interrupted.order} · ${money(amountOf(interrupted))} · ${interrupted.method}`, 14, true));
        if (receipts.length) {
          body.push(ui.text("Recent receipts", 16, true));
          body.push(ui.receipt(receipts.slice(0, 3).map((r) => ({ text: `Order #${r.order} · ${r.total} · ${r.method} · ${r.items}`, style: U.LineStyle.Plain }))));
        }
      } else if (screen === "cart") {
        body.push(ui.text("Your order", 20, true));
        body.push(ui.receipt([
          ...ITEMS.map((item, i) => ({ text: `${state.qty[i]} × ${item.name}  ·  ${money(item.price * state.qty[i])}`, style: U.LineStyle.Plain })),
          { text: `Total  ·  ${money(total())} USD`, style: U.LineStyle.Strong },
        ]));
        body.push(ui.text("One cart and amount stay with this order whichever way the customer pays.", 14, false, false, ui.pal.secondary));
      } else if (screen === "tender") {
        body.push(ui.text("Choose how to pay", 20, true));
        body.push(moneyLines([["Order", `#${state.order}`], ["Total", `${money(total())} USD`]]));
        body.push(ui.text("The merchant checkout keeps the same cart for Bitcoin or card.", 14, false, false, ui.pal.secondary));
      } else if (screen === "payment") {
        if (state.status === "paid") {
          body.push(ui.text("Payment complete", 20, true, false, ui.pal.good.ink));
          body.push(ui.text(`Order #${state.order} · ${money(total())} USD · ${state.method}`, 14, false, false, ui.pal.secondary));
          body.push(ui.text("This order is complete. Start a new sale from the receipt.", 14, false, false, ui.pal.secondary));
        } else if (state.status === "waiting") {
          body.push(ui.text(state.method === "Bitcoin" ? "Waiting for customer payment" : "Card handoff", 20, true));
          body.push(moneyLines([["Order", `#${state.order}`], ["Amount", `${money(total())} USD`], ["Method", state.method]]));
          if (state.method === "Bitcoin") {
            const QR = (context.CV || root.OpenBmtCounterViews).QrView;
            const side = Math.min(ui.px(248), ui.context.width - ui.px(56));
            body.push(ui.codeCard(QR(ui, DEMO_CODE, side)));
            body.push(ui.text("Demo code · no payment", 13, true, false, ui.pal.secondary));
            body.push(ui.text("This sample code cannot pay this order.", 13, false, false, ui.pal.secondary));
          } else {
            body.push(ui.text("Card provider handoff · proposed", 17, true));
            body.push(ui.text("A simulated embedded provider screen on this same merchant device. This demo has no card SDK or processor connected.", 14, false, false, ui.pal.secondary));
          }
        } else if (state.status === "uncertain") {
          body.push(ui.text("Card result not confirmed", 20, true, false, ui.pal.warn.ink));
          body.push(moneyLines([["Order", `#${state.order}`], ["Amount", `${money(total())} USD`]]));
          body.push(ui.text("Do not retry or hand over the order until the merchant checks the attempt.", 14, false, false, ui.pal.secondary));
        } else if (state.status === "checking") {
          body.push(ui.text("Checking card attempt", 20, true));
          body.push(moneyLines([["Order", `#${state.order}`], ["Amount", `${money(total())} USD`]]));
          body.push(ui.text("Waiting for the provider response for this attempt.", 14, false, false, ui.pal.secondary));
        } else if (state.status === "cancelled") {
          body.push(ui.text("Checkout canceled", 20, true, false, ui.pal.stop.ink));
          body.push(ui.text("No payment received. No goods handed over.", 14, false, false, ui.pal.secondary));
        }
      } else if (screen === "receipt") {
        if (state.status === "paid") {
          body.push(ui.text("Payment complete", 20, true, false, ui.pal.good.ink));
          body.push(ui.receipt([
            { text: `Receipt · Order #${state.order}`, style: U.LineStyle.Strong },
            { text: itemSummary(), style: U.LineStyle.Plain },
            { text: `Total · ${money(total())} USD`, style: U.LineStyle.Strong },
            { text: `Payment · ${state.method}`, style: U.LineStyle.Plain },
          ]));
        } else {
          body.push(ui.text("Order canceled", 20, true, false, ui.pal.stop.ink));
          body.push(ui.text("No receipt was issued because payment was not received.", 14, false, false, ui.pal.secondary));
        }
      }

      const actions = screenActions(ui, screen);
      return C.Device(ui, ui.page(ui.bar("Juniper Coffee", null, null), body, actions));
    }

    function screenActions(ui, screen) {
      if (screen === "start") {
        if (interrupted) return [ui.primary("Resume interrupted sale")];
        return [ui.primary("New sale")];
      }
      if (screen === "cart") return [ui.primary("Continue")];
      if (screen === "tender") return [ui.primary("Bitcoin"), ui.secondary("Card"), ui.quiet("Cancel sale")];
      if (screen === "payment") {
        if (state.status === "uncertain") return [ui.primary("Check card status")];
        if (state.status === "cancelled") return [ui.primary("New sale"), ui.secondary("Done")];
        if (state.status === "waiting") return [ui.quiet("Cancel sale")];
        return [];
      }
      if (screen === "receipt") {
        if (state.status === "paid") return [ui.primary("New sale"), ui.secondary("Done")];
        return [ui.primary("New sale"), ui.secondary("Done")];
      }
      return [];
    }

    function show(screen) {
      shell.show(DEVICE_ID, (size) => viewFor(size, screen));
      shell.light(DEVICE_ID);
    }

    function recordPayment() {
      if (!active || !selectedMethod || (state.status !== "waiting" && state.status !== "checking")) return false;
      state.status = "paid";
      const receipt = { order: state.order, total: `${money(total())} USD`, method: state.method, items: itemSummary() };
      receipts = [receipt, ...receipts.filter((r) => r.order !== receipt.order)].slice(0, 12);
      nextOrder = Math.max(nextOrder, Number(state.order) + 1);
      return true;
    }

    function completePayment() {
      if (!recordPayment()) return;
      enter(4);
    }

    function finish() {
      shell.finish({
        heading: state.status === "paid" ? `Order #${state.order} is complete.` : `Order #${state.order} was canceled.`,
        lines: state.status === "paid"
          ? ["The merchant kept one cart and amount.", `The proposed ${state.method} path reached a simulated completion.`, "The receipt is available in this checkout session."]
          : ["No payment was received.", "No goods were handed over.", "Start a fresh sale when ready."],
      });
    }

    function enter(index) {
      if (!active) return;
      shell.actor(null);
      shell.step(index);
      shell.questions([
        { q: "Does this connect to a card processor?", a: "This browser walkthrough simulates the provider screen and a successful response after an interruption. A separate Android Sandbox harness has completed test payments with Square's Mobile Payments SDK. Square's full POS app is not embedded here." },
        { q: "What has actually been built?", a: "The catalog and unified interface are a proposal. The native Android proof has tested amount entry, Bitcoin Regtest receipts, payment recovery, and Square Sandbox card payments. The other counter scenarios run the Counter app's model on a simulated chain." },
        { q: "What happens when I leave or refresh?", a: "Changing scenarios keeps an interrupted example available to resume in this tab. Start over or refreshing resets the illustration. Durable restart recovery is tested separately in the native app." },
      ]);
      if (index === 0) {
        show("start");
        if (interrupted) {
          shell.say("An interrupted payment is waiting. Recovery is available only while this browser session stays open.", ["Tap ", { b: "Resume interrupted sale" }, " to check this attempt."]);
          shell.target(DEVICE_ID, "Resume interrupted sale", () => {
            state = { ...interrupted, qty: [...interrupted.qty] };
            selectedMethod = state.method;
            interrupted = null;
            if (state.status === "checking") state.status = "uncertain";
            nextOrder = Math.max(nextOrder, Number(state.order) + 1);
            enter(3);
          });
        } else {
          shell.say("Start a new order from the sample catalog.", ["You are the cashier. Tap ", { b: "New sale" }, "."]);
          shell.target(DEVICE_ID, "New sale", () => { state = fresh(nextOrder); selectedMethod = null; enter(1); });
        }
      } else if (index === 1) {
        show("cart");
        shell.say(`The cart is ready: ${itemSummary()}, ${money(total())} USD.`, ["Tap ", { b: "Continue" }, " to choose a payment method."]);
        shell.target(DEVICE_ID, "Continue", () => enter(2));
      } else if (index === 2) {
        state.method = null; selectedMethod = null; state.status = "cart";
        show("tender");
        shell.say("The same order can go to Bitcoin or card.", ["Choose ", { b: "Bitcoin" }, ", ", { b: "Card" }, ", or ", { b: "Cancel sale" }, "."]);
        shell.targets(DEVICE_ID, [
          { label: "Bitcoin", onTap() { selectedMethod = "Bitcoin"; state.method = selectedMethod; state.status = "waiting"; enter(3); } },
          { label: "Card", onTap() { selectedMethod = "Card"; state.method = selectedMethod; state.status = "waiting"; enter(3); } },
          { label: "Cancel sale", onTap() { selectedMethod = null; state.status = "cancelled"; nextOrder = Math.max(nextOrder, Number(state.order) + 1); enter(4); } },
        ]);
      } else if (index === 3) {
        const paymentState = state;
        show("payment");
        if (state.status === "paid" || state.status === "cancelled") {
          shell.say(state.status === "paid" ? "This order is already complete." : "This order is canceled.", ["Review its receipt to start another sale or finish."]);
          return;
        }
        if (state.status === "uncertain") {
          shell.say("The card result is unclear. Check the existing attempt before proceeding.", ["Tap ", { b: "Check card status" }, "."]);
          shell.target(DEVICE_ID, "Check card status", () => { state.status = "checking"; enter(3); });
          return;
        }
        if (state.status === "checking") {
          shell.say("The merchant is checking the existing card attempt.", [root.innerWidth < 900 ? "Scroll below the device and tap " : "As the card provider, tap ", { b: "Confirm payment (demo)" }, "."]);
          shell.actor({ who: "Card provider", line: "This walkthrough returns a successful result for the same attempt.", button: "Confirm payment (demo)", onTap() { if (state === paymentState) completePayment(); } });
          return;
        }
        const action = state.method === "Bitcoin" ? "Send Bitcoin payment (demo)" : "Submit card handoff (demo)";
        shell.say(state.method === "Bitcoin"
          ? "The merchant is waiting for the customer's wallet payment."
          : "The customer is at the proposed provider handoff. This demo simulates an unclear response, then checks that same attempt.",
        [root.innerWidth < 900 ? "Scroll below the device. As the customer, tap " : "You are the customer. Tap ", { b: action }, "."]);
        shell.actor({
          who: "Customer",
          line: state.method === "Bitcoin" ? "Pays this sample order from a wallet (simulation)." : "Submits a sample card handoff (simulation).",
          button: action,
          onTap() {
            if (!active || state !== paymentState || state.status !== "waiting") return;
            if (state.method === "Bitcoin") completePayment();
            else { state.status = "uncertain"; enter(3); }
          },
        });
        shell.target(DEVICE_ID, "Cancel sale", () => {
          if (!active || state !== paymentState || state.status !== "waiting") return;
          state.status = "cancelled";
          nextOrder = Math.max(nextOrder, Number(state.order) + 1);
          enter(4);
        });
      } else {
        show("receipt");
        if (state.status === "paid") {
          shell.say(`Payment complete for order #${state.order}.`, ["Review the receipt. Tap ", { b: "New sale" }, " or ", { b: "Done" }, "."]);
        } else {
          shell.say(`Order #${state.order} was canceled.`, ["No payment was received. Start a ", { b: "New sale" }, " or tap ", { b: "Done" }, "."]);
        }
        shell.targets(DEVICE_ID, [
          { label: "New sale", onTap() { state = fresh(nextOrder); selectedMethod = null; enter(1); } },
          { label: "Done", onTap: finish },
        ]);
      }
    }

    function replay(index) {
      const prior = { ...state, qty: [...state.qty] };
      const method = selectedMethod;
      // Navigation only redraws the current order. It must never act as a payment
      // confirmation, especially after an unclear card result.
      if (interrupted) index = 0;
      else if (["paid", "cancelled"].includes(prior.status) && index !== 0) index = 4;
      else if (method && ["waiting", "uncertain", "checking"].includes(prior.status)) index = 3;
      else if (index > 2 && !method && prior.status !== "cancelled") index = 2;
      if (index === 4 && !["paid", "cancelled"].includes(prior.status)) index = 3;
      state = { ...prior, qty: [...prior.qty], method: method || prior.method };
      selectedMethod = method;
      begin();
      enter(index);
    }

    function begin() {
      shell.begin(STEPS, [device], DEVICE_ID, { actorFlow: true });
      shell.scenario("OpenBMT checkout");
      shell.onStep = replay;
    }

    function cleanup(restarting) {
      if (!active) return;
      active = false;
      if (!restarting && ["waiting", "uncertain", "checking"].includes(state.status)) interrupted = { ...state, qty: [...state.qty], amount: total() };
      if (restarting) { interrupted = null; receipts = []; nextOrder = 1042; }
      shell.onLeave = null;
      shell.onStep = null;
      shell.actor(null);
      shell.onRestart = previousRestart;
      shell.begin([], []);
    }

    shell.onLeave = () => cleanup(false);
    shell.onRestart = () => {
      cleanup(true);
      if (previousRestart) previousRestart();
    };
    begin();
    enter(0);
  }

  const api = { open, STEPS, DEVICE_ID };
  root.OpenBmtCheckout = api;
  if (typeof module !== "undefined" && module.exports) module.exports = api;
})(globalThis);
