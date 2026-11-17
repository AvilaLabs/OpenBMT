// Session-only sample checkout. Every actor is a simulation; no payment service is connected.
"use strict";
(function (root) {
  const Contract = (typeof module !== "undefined" && module.exports)
    ? require("./sale-contract.js")
    : root.OpenBmtSaleContract;
  if (!Contract) throw new Error("sale-contract.js must load before checkout.js");

  const STEPS = ["Start sale", "Review cart", "Choose payment", "Payment", "Receipt", "History"];
  const DEVICE_ID = "merchant";
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
  let history = [];
  let nextOrder = 1042;
  let nextCashInput = "";
  let error = "";

  const money = (minor) => "$" + Math.floor(minor / 100) + "." + String(minor % 100).padStart(2, "0");

  function parseMoneyMinor(input) {
    const match = String(input).trim().replace(/^\$/, "").match(/^(\d+)(?:\.(\d{1,2}))?$/);
    if (!match) return NaN;
    const value = Number(match[1]) * 100 + Number((match[2] || "").padEnd(2, "0"));
    return Number.isSafeInteger(value) ? value : NaN;
  }

  const amountOf = (sale) => Contract.totalMinor(sale);

  function open(shell, context) {
    const U = context.U;
    const C = context.C;
    const device = {
      id: DEVICE_ID,
      caption: "Juniper Coffee",
      sizes: [
        { label: "Tablet", size: C.TABLET },
        { label: "Phone", size: C.PHONE },
      ],
    };
    const previousRestart = shell.onRestart;
    let sale = Contract.create(nextOrder, [1, 1]);
    let active = true;
    let viewedOrder = null;
    let saleStarted = false;

    const total = () => amountOf(sale);
    const summary = (current = sale) => Contract.ITEMS
      .map((item, i) => ({ item, quantity: current.qty[i] }))
      .filter((line) => line.quantity > 0)
      .map((line) => `${line.quantity} × ${line.item.name}`)
      .join(" · ");

    function asReceipt(current) {
      const receipt = current.status === "paid"
        ? Contract.receipt(current)
        : Object.freeze({
          order: current.order,
          status: current.status,
          method: current.method,
          qty: current.qty,
          totalMinor: Contract.totalMinor(current),
          attemptId: current.attemptId,
          simulated: true,
        });
      return { sale: current, receipt };
    }

    function remember(current) {
      const record = asReceipt(current);
      history = [record, ...history.filter((item) => item.sale.order !== record.sale.order)].slice(0, 20);
      return record;
    }

    function orderLabel(record) {
      const current = record.sale;
      return `Order #${current.order} · ${current.status} · ${money(amountOf(current))} · ${current.method || "none"}${current.refundRequestId ? " · refund requested" : ""}`;
    }

    function viewFor(size, view) {
      const ui = C.deviceUi(context.palette, false, context.assets.logo, size);
      const body = [];
      const text = (value, sizeSp = 14, bold = false, color = ui.pal.secondary) =>
        ui.text(value, sizeSp, bold, false, color);
      const facts = (rows) => ui.facts(rows);

      if (view === "start") {
        body.push(text("Illustrative sample catalog", 20, true, ui.pal.text));
        body.push(text("Adjust the two sample items, then choose Bitcoin, card, or cash."));
        body.push(text("This example keeps orders and receipts in memory for this browser session only."));
        body.push(facts(Contract.ITEMS.map((item) => [item.name, money(item.unitMinor)])));
        if (interrupted) {
          body.push(text(`Interrupted order #${interrupted.order} · ${money(amountOf(interrupted))} · ${interrupted.method}`, 14, true, ui.pal.warn.ink));
        }
        body.push(text("Simulation · all payment outcomes in this walkthrough are examples."));
        if (history.length) {
          body.push(text("Recent orders", 16, true, ui.pal.text));
          body.push(ui.receipt(history.slice(0, 4).map((record) => ({
            text: orderLabel(record), style: U.LineStyle.Plain,
          }))));
        }
      } else if (view === "cart") {
        body.push(text("Edit your order", 20, true, ui.pal.text));
        Contract.ITEMS.forEach((item, i) => {
          body.push(ui.receipt([{
            text: `${item.name} · ${money(item.unitMinor)} each · quantity ${sale.qty[i]} · ${money(item.unitMinor * sale.qty[i])}`,
            style: U.LineStyle.Plain,
          }]));
          const controls = U.LinearLayout("h");
          const minus = i === 0 ? "Oat latte −" : "Sesame bagel −";
          const plus = i === 0 ? "Oat latte +" : "Sesame bagel +";
          U.addView(controls, ui.quiet(minus), new U.LayoutParams(U.WRAP_CONTENT, U.WRAP_CONTENT));
          const plusParams = new U.LayoutParams(U.WRAP_CONTENT, U.WRAP_CONTENT);
          plusParams.leftMargin = ui.px(8);
          U.addView(controls, ui.secondary(plus), plusParams);
          body.push(controls);
        });
        body.push(facts([["Order", `#${sale.order}`], ["Total", `${money(total())} USD`]]));
        body.push(text("Quantity must be between 0 and 99. An empty cart cannot be paid."));
        if (error) body.push(text(error, 14, true, ui.pal.warn.ink));
      } else if (view === "tender") {
        body.push(text("Choose how to pay", 20, true, ui.pal.text));
        body.push(facts([["Order", `#${sale.order}`], ["Total", `${money(total())} USD`]]));
      } else if (view === "payment") {
        const status = sale.status;
        const heading = {
          waitingBitcoin: "Bitcoin receipt example",
          waitingCard: "Card handoff example",
          uncertainCard: "Card result not confirmed",
          checkingCard: "Checking card attempt",
          waitingCash: "Cash tender",
        }[status] || "Payment handoff";
        const headingColor = status === "uncertainCard" ? ui.pal.warn.ink : ui.pal.text;
        body.push(text(heading, 20, true, headingColor));
        body.push(facts([
          ["Order", `#${sale.order}`],
          ["Amount", `${money(total())} USD`],
          ["Method", sale.method || "—"],
        ]));

        if (sale.method === "Bitcoin") {
          const QR = (context.CV || root.OpenBmtCounterViews).QrView;
          const side = Math.min(ui.px(248), ui.context.width - ui.px(56));
          body.push(ui.codeCard(QR(ui, DEMO_CODE, side)));
          body.push(text("Sample QR · nonpayable", 13, true));
          body.push(text("This example models a confirmed Bitcoin receipt after a simulated customer action. The sample QR cannot pay this order."));
        } else if (sale.method === "Card") {
          if (status === "uncertainCard") {
            body.push(text("Check this existing attempt before continuing. Another method or payment cannot start while its result is unknown."));
          } else if (status === "checkingCard") {
            body.push(text("The result for this same card attempt is simulated."));
          } else {
            body.push(text("Simulated card provider handoff · no processor connected."));
          }
        } else if (sale.method === "Cash") {
          body.push(text("Cash received", 15, true, ui.pal.text));
          if (typeof ui.field === "function") {
            const input = ui.field("Amount in dollars", 1, false);
            input.value = nextCashInput;
            input.inputMode = "decimal";
            input.setAttribute("aria-label", "Cash received in dollars");
            input.addEventListener("input", () => {
              nextCashInput = input.value;
              error = "";
            });
            body.push(input);
          } else {
            body.push(text(`Cash received input value: ${nextCashInput || ""}`));
          }
          if (error) body.push(text(error, 14, true, ui.pal.warn.ink));
          body.push(text("Simulation · enter the amount the cashier received, then explicitly record cash."));
        }
      } else if (view === "receipt") {
        const current = viewedOrder ? viewedOrder.sale : sale;
        if (current.status === "paid") body.push(text("Payment complete · simulation", 20, true, ui.pal.good.ink));
        else body.push(text("Order canceled", 20, true, ui.pal.stop.ink));
        const lines = [
          { text: current.status === "paid" ? `Receipt · Order #${current.order}` : `Order detail · #${current.order}`, style: U.LineStyle.Strong },
          { text: summary(current), style: U.LineStyle.Plain },
          { text: `Total · ${money(amountOf(current))} USD`, style: U.LineStyle.Strong },
          { text: `Status · ${current.status}`, style: U.LineStyle.Plain },
          { text: `Payment · ${current.method || "none"}`, style: U.LineStyle.Plain },
        ];
        if (current.cashReceivedMinor != null) {
          lines.push({ text: `Cash received · ${money(current.cashReceivedMinor)} USD`, style: U.LineStyle.Plain });
          lines.push({ text: `Change · ${money(current.changeMinor)} USD`, style: U.LineStyle.Plain });
        }
        if (current.refundRequestId) {
          lines.push({ text: "Refund request recorded for this order · not paid", style: U.LineStyle.Plain });
        }
        body.push(ui.receipt(lines));
        body.push(text(current.status === "paid"
          ? current.refundRequestId
            ? "Refund request recorded for this order · no money was returned."
            : "Payment was simulated in this example."
          : "No payment was recorded for this order."));
      } else if (view === "history") {
        body.push(text("Order history", 20, true, ui.pal.text));
        body.push(text("Select an order to open its saved detail. History lasts for this tab session."));
        for (const record of history) body.push(ui.secondary(orderLabel(record)));
        if (viewedOrder) body.push(text(`Selected order #${viewedOrder.sale.order} · ${viewedOrder.sale.status}`));
      }

      return C.Device(ui, ui.page(ui.bar("Juniper Coffee", null, null), body, screenActions(ui, view)));
    }

    function screenActions(ui, view) {
      if (view === "start") {
        return [
          ui.primary(interrupted ? "Resume interrupted sale" : "New sale"),
          ...(history.length ? [ui.secondary("Order history")] : []),
        ];
      }
      if (view === "cart") {
        return [ui.primary("Continue"), ui.quiet("Cancel sale")];
      }
      if (view === "tender") {
        return [ui.primary("Bitcoin"), ui.secondary("Card"), ui.secondary("Cash"), ui.quiet("Back to cart"), ui.quiet("Cancel sale")];
      }
      if (view === "payment") {
        if (sale.status === "uncertainCard") return [ui.primary("Check card status")];
        if (sale.status === "checkingCard") return [];
        if (sale.status === "waitingCash") return [ui.primary("Record cash"), ui.quiet("Cancel sale")];
        if (sale.status === "waitingBitcoin" || sale.status === "waitingCard") return [ui.quiet("Cancel sale")];
        return [];
      }
      if (view === "receipt") {
        const current = viewedOrder && viewedOrder.sale;
        return [
          ...(current?.status === "paid" && !current.refundRequestId ? [ui.secondary("Request refund")] : []),
          ui.secondary("Order history"), ui.primary("New sale"), ui.quiet("Done"),
        ];
      }
      if (view === "history") {
        return [ui.primary("New sale"), ui.quiet("Done")];
      }
      return [];
    }

    function show(view) {
      shell.show(DEVICE_ID, (size) => viewFor(size, view));
      shell.light(DEVICE_ID);
    }

    function finish() {
      const finishedSale = viewedOrder ? viewedOrder.sale : sale;
      shell.finish({
        heading: finishedSale.status === "paid" ? `Order #${finishedSale.order} is complete.` : `Order #${finishedSale.order} is closed.`,
        lines: ["All payment outcomes are simulated.", "Orders and receipts are kept only in this browser session."],
      });
    }

    function setSale(next) {
      sale = next;
      error = "";
    }

    function closeSale() {
      setSale(Contract.cancel(sale));
      remember(sale);
      nextOrder = Math.max(nextOrder, sale.order + 1);
      enter(4);
    }

    function enter(index) {
      if (!active) return;
      shell.actor(null);
      shell.step(index);
      shell.questions([
        { q: "Are payments connected?", a: "No. Bitcoin, card, and cash outcomes in this browser walkthrough are simulations. The QR is sample data and cannot pay." },
        { q: "How long is order history kept?", a: "Only in memory for this browser tab session. Refreshing or starting over clears the example." },
        { q: "Does a refund request issue money?", a: "No. This demo records one request linked to the original receipt. It does not pay a refund or reverse Bitcoin." },
      ]);

      if (index === 0) {
        show("start");
        shell.say("Start a sample sale or review an order from this session.", ["Tap ", { b: interrupted ? "Resume interrupted sale" : "New sale" }, " or open Order history."]);
        if (interrupted) {
          shell.target(DEVICE_ID, "Resume interrupted sale", () => {
            sale = interrupted;
            interrupted = null;
            enter(3);
          });
        } else {
          shell.target(DEVICE_ID, "New sale", () => {
            sale = Contract.create(nextOrder, [1, 1]);
            saleStarted = true;
            viewedOrder = null;
            nextCashInput = "";
            enter(1);
          });
        }
        if (history.length) {
          shell.target(DEVICE_ID, "Order history", () => {
            viewedOrder = history[0] || null;
            enter(5);
          });
        }
      } else if (index === 1) {
        show("cart");
        shell.say(`Cart total ${money(total())} USD. Adjust each quantity from zero to 99.`, ["Continue when the cart is ready."]);
        shell.targets(DEVICE_ID, [
          { label: "Oat latte −", onTap: () => { setSale(Contract.setQuantity(sale, 0, Math.max(0, sale.qty[0] - 1))); enter(1); } },
          { label: "Oat latte +", onTap: () => { setSale(Contract.setQuantity(sale, 0, Math.min(99, sale.qty[0] + 1))); enter(1); } },
          { label: "Sesame bagel −", onTap: () => { setSale(Contract.setQuantity(sale, 1, Math.max(0, sale.qty[1] - 1))); enter(1); } },
          { label: "Sesame bagel +", onTap: () => { setSale(Contract.setQuantity(sale, 1, Math.min(99, sale.qty[1] + 1))); enter(1); } },
          { label: "Continue", onTap: () => {
            if (total() <= 0) {
              error = "Add at least one item before choosing payment.";
              enter(1);
              return;
            }
            enter(2);
          } },
          { label: "Cancel sale", onTap: closeSale },
        ]);
      } else if (index === 2) {
        show("tender");
        shell.say("Choose Bitcoin, card, or cash for this same order.", ["The selected method creates one payment attempt."]);
        shell.targets(DEVICE_ID, [
          ...[["Bitcoin", "Bitcoin"], ["Card", "Card"], ["Cash", "Cash"]].map(([label, method]) => ({
            label,
            onTap: () => {
              if (total() <= 0) return;
              setSale(Contract.begin(sale, method));
              nextCashInput = "";
              enter(3);
            },
          })),
          { label: "Back to cart", onTap: () => enter(1) },
          { label: "Cancel sale", onTap: closeSale },
        ]);
      } else if (index === 3) {
        show("payment");
        if (sale.status === "waitingBitcoin" || sale.status === "waitingCard") {
          const attempt = sale;
          const isBitcoin = sale.method === "Bitcoin";
          const label = isBitcoin ? "Simulate Bitcoin receipt" : "Simulate card submission";
          shell.say(isBitcoin
            ? "This sample shows a confirmed receipt after a simulation."
            : "This sample simulates an unclear card response, followed by checking the same attempt.",
          ["Customer action: ", { b: label }, "."]);
          shell.actor({
            who: "Customer · simulation",
            line: isBitcoin
              ? "The example models a confirmed Bitcoin receipt. No live wallet payment occurs."
              : "Submitting this card handoff is simulated.",
            button: label,
            onTap() {
              if (!active || sale !== attempt) return;
              if (isBitcoin) {
                setSale(Contract.confirm(sale));
                remember(sale);
                nextOrder = Math.max(nextOrder, sale.order + 1);
                enter(4);
              } else {
                setSale(Contract.cardSubmitted(sale));
                enter(3);
              }
            },
          });
          shell.target(DEVICE_ID, "Cancel sale", () => {
            if (sale === attempt) closeSale();
          });
        } else if (sale.status === "uncertainCard") {
          shell.say("Check the existing card attempt before proceeding.", ["Unknown card result locks other methods and new payment."]);
          shell.target(DEVICE_ID, "Check card status", () => {
            setSale(Contract.checkCard(sale));
            enter(3);
          });
        } else if (sale.status === "checkingCard") {
          const attempt = sale;
          shell.say("The result for this same card attempt is simulated.", ["As the provider, confirm the demo result."]);
          shell.actor({
            who: "Card provider · simulation",
            line: "A simulated successful response for the existing attempt.",
            button: "Confirm card payment (demo)",
            onTap() {
              if (!active || sale !== attempt) return;
              setSale(Contract.confirm(sale));
              remember(sale);
              nextOrder = Math.max(nextOrder, sale.order + 1);
              enter(4);
            },
          });
        } else if (sale.status === "waitingCash") {
          const attempt = sale;
          show("payment");
          shell.say("Enter cash received, then explicitly record it.", ["Underpayment leaves this order unpaid."]);
          shell.targets(DEVICE_ID, [
            { label: "Record cash", onTap: () => {
              if (sale !== attempt) return;
              const tenderedMinor = parseMoneyMinor(nextCashInput);
              if (!Number.isSafeInteger(tenderedMinor)) {
                error = "Enter cash in dollars with at most two decimal places.";
                enter(3);
                return;
              }
              try {
                setSale(Contract.recordCash(sale, tenderedMinor));
                remember(sale);
                nextOrder = Math.max(nextOrder, sale.order + 1);
                enter(4);
              } catch (err) {
                error = err.message.includes("below the full balance")
                  ? "Cash is below the amount due. Change the received amount before recording cash."
                  : (err.message || "Cash received must cover the total.");
                enter(3);
              }
            } },
            { label: "Cancel sale", onTap: () => { if (sale === attempt) closeSale(); } },
          ]);
        }
      } else if (index === 4) {
        viewedOrder = viewedOrder || history.find((record) => record.sale.order === sale.order) || remember(sale);
        show("receipt");
        const viewedSale = viewedOrder.sale;
        if (viewedSale.status === "paid" && viewedSale.refundRequestId) {
          shell.say(`Order #${viewedSale.order} · refund request recorded.`, ["Refund request recorded for this order; no money was returned."]);
        } else if (viewedSale.status === "paid") {
          shell.say(`Order #${viewedSale.order} · ${viewedSale.method} · ${money(amountOf(viewedSale))} USD.`, ["Payment was simulated. Start a new sale or open order history."]);
        } else {
          shell.say(`Order #${viewedSale.order} was canceled.`, ["No payment was recorded. Start a new sale or open order history."]);
        }
        const actions = [];
        if (viewedOrder.sale.status === "paid" && !viewedOrder.sale.refundRequestId) {
          const refundTarget = viewedOrder;
          actions.push({ label: "Request refund", onTap: () => {
            if (!active || viewedOrder !== refundTarget || refundTarget.sale.status !== "paid" || refundTarget.sale.refundRequestId) return;
            const updated = Contract.requestRefund(refundTarget.sale);
            viewedOrder = remember(updated);
            sale = updated;
            enter(4);
          } });
        }
        shell.targets(DEVICE_ID, [
          ...actions,
          { label: "Order history", onTap: () => enter(5) },
          { label: "New sale", onTap: () => {
            sale = Contract.create(nextOrder, [1, 1]);
            saleStarted = true;
            viewedOrder = null;
            nextCashInput = "";
            enter(1);
          } },
          { label: "Done", onTap: finish },
        ]);
      } else {
        show("history");
        shell.say("Select an order from the list and open its detail.", ["History is session-only."]);
        shell.targets(DEVICE_ID, [
          ...history.map((record) => ({ label: orderLabel(record), onTap: () => { viewedOrder = record; enter(4); } })),
          { label: "New sale", onTap: () => {
            sale = Contract.create(nextOrder, [1, 1]);
            saleStarted = true;
            viewedOrder = null;
            enter(1);
          } },
          { label: "Done", onTap: finish },
        ]);
      }
    }

    function replay(index) {
      const current = sale;
      if (interrupted) {
        index = 0;
      } else if (current.status === "paid" || current.status === "cancelled") {
        if (index !== 0 && index !== 4 && index !== 5) index = 4;
      } else if (["waitingBitcoin", "waitingCard", "waitingCash", "uncertainCard", "checkingCard"].includes(current.status)) {
        if (index !== 3) index = 3;
      } else if (current.status === "draft" && saleStarted && index === 0) {
        index = 1;
      } else if (index > 2 && current.status === "draft") {
        index = 2;
      }
      if (index === 4 && current.status !== "paid" && current.status !== "cancelled") index = 3;
      if (index === 5 && !history.length) index = 0;
      if (index !== 4) viewedOrder = null;
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
      if (!restarting && saleStarted && sale.status === "draft") {
        setSale(Contract.cancel(sale));
        remember(sale);
        nextOrder = Math.max(nextOrder, sale.order + 1);
      }
      if (!restarting && ["waitingBitcoin", "waitingCard", "waitingCash", "uncertainCard", "checkingCard"].includes(sale.status)) {
        interrupted = sale;
      }
      if (restarting) {
        interrupted = null;
        history = [];
        nextOrder = 1042;
      }
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
