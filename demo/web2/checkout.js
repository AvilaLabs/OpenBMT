// Proposed merchant checkout, with simulated external payment outcomes in a visibly separate demo control area.
// This is not an SDK integration and local browser state is never evidence of a real settlement.
"use strict";
(function (root) {
  const KEY = "openbmt-proposed-checkout-v1";
  const ITEMS = [{ name: "Oat latte", price: 5.5 }, { name: "Sesame bagel", price: 4.5 }];
  const STATUSES = new Set(["ready", "bitcoin-pending", "card-pending", "card-uncertain", "paused", "partial-cancelled", "partial-resolved", "failed", "cancelled", "paid", "refund-requested", "refund-review", "refunding", "refunded"]);
  let storageReadFailed = false;
  const money = (n) => "$" + n.toFixed(2);
  const fresh = (order = "1042", history = [], receipts = []) => ({ order, status: "ready", method: null, paid: 0, qty: [1, 1], history, receipts });
  const validState = (s) => {
    if (!s || ![1, 2].includes(s.version) || typeof s.order !== "string" || !/^\d{4,8}$/.test(s.order) || !STATUSES.has(s.status)) return false;
    if (!Array.isArray(s.qty) || s.qty.length !== 2 || s.qty.some((q) => !Number.isInteger(q) || q < 1 || q > 8)) return false;
    const storedDue = Math.round(ITEMS.reduce((sum, x, i) => sum + x.price * s.qty[i], 0) * 100) / 100;
    if (!Number.isFinite(s.paid) || s.paid < 0 || s.paid > storedDue || Math.round(s.paid * 100) !== s.paid * 100 || !Array.isArray(s.history) || s.history.length > 40) return false;
    if (s.history.some((e) => !e || typeof e.label !== "string" || e.label.length > 90 || typeof e.time !== "string")) return false;
    const receipts = s.receipts === undefined && s.version === 1 ? [] : s.receipts;
    if (!Array.isArray(receipts) || receipts.length > 12 || receipts.some((r) => !r || typeof r.order !== "string" || !/^\d{4,8}$/.test(r.order)
      || typeof r.total !== "string" || !/^\$\d+\.\d{2} USD$/.test(r.total)
      || (r.method !== "Bitcoin" && r.method !== "Card") || typeof r.items !== "string" || r.items.length < 1 || r.items.length > 160)) return false;
    if (s.method !== null && s.method !== "Bitcoin" && s.method !== "Card") return false;
    if (s.paid > 0 && s.method !== "Bitcoin") return false;
    if (s.status === "paid" && !s.history.some((e) => e.label === (s.method === "Bitcoin" ? "Bitcoin test complete" : "Card test complete"))) return false;
    if (s.status === "refunded" && !s.history.some((e) => e.label === "Refund simulation complete")) return false;
    if (["bitcoin-pending", "card-pending", "card-uncertain", "paused", "partial-cancelled", "paid", "refund-requested", "refund-review", "refunding", "refunded", "failed"].includes(s.status) && !s.method) return false;
    if (["bitcoin-pending", "paused", "partial-cancelled", "partial-resolved"].includes(s.status) && s.method !== "Bitcoin") return false;
    if (["card-pending", "card-uncertain", "failed"].includes(s.status) && s.method !== "Card") return false;
    return true;
  };
  const read = () => {
    storageReadFailed = false;
    try {
      const raw = localStorage.getItem(KEY);
      if (!raw) return fresh();
      const value = JSON.parse(raw);
      if (!validState(value)) { localStorage.removeItem(KEY); return fresh(); }
      return { ...value, version: 2, receipts: value.receipts || [] };
    } catch (_) { storageReadFailed = true; return fresh(); }
  };
  const element = (tag, cls, text) => { const e = document.createElement(tag); if (cls) e.className = cls; if (text != null) e.textContent = text; return e; };

  function open(shell) {
    shell.begin([], []);
    shell.scenario("OpenBMT checkout");
    shell.say("Proposed checkout · payment simulation", ["A single merchant checkout holds the cart and order. Demo controls simulate the payment network."]);
    shell.stage.replaceChildren();
    let state = read();
    let storageWarning = storageReadFailed;
    const experience = element("div", "checkout-experience");
    const app = element("section", "checkout-proposal");
    app.setAttribute("aria-label", "Juniper Coffee merchant checkout");
    const top = element("header", "checkout-top");
    const orderLabel = element("span", "checkout-order");
    top.append(element("div", "checkout-brand", "Juniper Coffee"), orderLabel);
    const grid = element("div", "checkout-grid");
    const order = element("section", "checkout-order-card");
    const pay = element("section", "checkout-pay");
    const stateLine = element("p", "checkout-state");
    stateLine.setAttribute("aria-live", "polite"); stateLine.setAttribute("role", "status");
    const paymentSurface = element("div", "checkout-payment-surface");
    const receiptList = element("section", "checkout-history");
    app.append(top, grid, receiptList); grid.append(order, pay); pay.append(element("h2", null, "Payment"), stateLine, paymentSurface);

    const controls = element("aside", "demo-controls");
    controls.setAttribute("aria-label", "Test-only payment simulation controls");
    const controlsHead = element("h2", null, "Demo controls · test only");
    const controlsNote = element("p", "demo-controls-note", "These controls simulate external outcomes. They are outside the merchant app and do not move money.");
    const controlActions = element("div", "checkout-actions");
    const controlLog = element("section", "checkout-history");
    const storageNotice = element("p", "checkout-storage-warning", "This browser could not save the demo session. Recovery after refresh is unavailable.");
    storageNotice.hidden = true;
    controls.append(controlsHead, controlsNote, controlActions, controlLog, storageNotice);
    experience.append(app, controls);

    const button = (label, action, cls = "checkout-button") => {
      const b = element("button", cls, label); b.type = "button"; b.addEventListener("click", action); return b;
    };
    const total = () => ITEMS.reduce((sum, x, i) => sum + x.price * state.qty[i], 0);
    const persist = () => {
      try { localStorage.setItem(KEY, JSON.stringify({ ...state, version: 2 })); storageWarning = false; }
      catch (_) { storageWarning = true; }
    };
    const log = (label) => {
      state.history.unshift({ label, time: new Date().toLocaleString() });
      state.history = state.history.slice(0, 40); persist();
    };
    const completePayment = () => {
      state.status = "paid";
      const receipt = {
        order: state.order,
        total: money(total()) + " USD",
        method: state.method,
        items: ITEMS.map((item, i) => `${state.qty[i]} × ${item.name}`).join(" · "),
      };
      state.receipts = [receipt, ...state.receipts.filter((old) => old.order !== receipt.order)].slice(0, 12);
      log(state.method === "Bitcoin" ? "Bitcoin test complete" : "Card test complete");
      render();
    };
    const startNextOrder = () => {
      state = fresh(String(Number(state.order) + 1).padStart(4, "0"), state.history, state.receipts);
      persist();
      render();
    };
    const locked = () => !["ready", "cancelled"].includes(state.status);
    function render() {
      orderLabel.textContent = "Order #" + state.order;
      order.replaceChildren(element("h2", null, "Your order"));
      ITEMS.forEach((item, i) => {
        const row = element("div", "checkout-row");
        const name = element("span", null, item.name + " · " + money(item.price));
        const qty = element("div", "checkout-qty");
        const down = button("−", () => { if (state.qty[i] > 1) { state.qty[i]--; persist(); render(); } }, "checkout-qty-button");
        down.setAttribute("aria-label", "Remove one " + item.name);
        const count = element("span", null, String(state.qty[i])); count.setAttribute("aria-live", "polite");
        const up = button("+", () => { if (state.qty[i] < 8) { state.qty[i]++; persist(); render(); } }, "checkout-qty-button");
        up.setAttribute("aria-label", "Add one " + item.name);
        down.disabled = locked() || state.qty[i] <= 1; up.disabled = locked() || state.qty[i] >= 8;
        qty.append(down, count, up); row.append(name, qty); order.append(row);
      });
      const due = total();
      const sum = element("div", "checkout-total"); sum.append(element("strong", null, "Total"), element("strong", null, money(due) + " USD")); order.append(sum);
      pay.replaceChildren(element("h2", null, "Payment"), stateLine, paymentSurface);
      const previousReceipts = state.receipts.filter((r) => r.order !== state.order);
      receiptList.replaceChildren();
      if (previousReceipts.length) {
        receiptList.append(element("h3", null, "Previous demo receipts"));
        for (const r of previousReceipts) receiptList.append(element("p", "checkout-history-row", `Order #${r.order} · ${r.total} · ${r.method} · ${r.items}`));
      }
      paymentSurface.replaceChildren(); controlActions.replaceChildren();
      const paymentAllowed = state.status === "ready";
      if (paymentAllowed) {
        stateLine.textContent = "Choose how your customer will pay · " + money(due) + " due";
        paymentSurface.append(button("Pay with Bitcoin", () => { state.method = "Bitcoin"; state.paid = 0; state.status = "bitcoin-pending"; log("Bitcoin request opened"); render(); }, "checkout-button primary"),
          button("Pay with card", () => { state.method = "Card"; state.paid = 0; state.status = "card-pending"; log("Card test started"); render(); }, "checkout-button"));
      } else if (state.status === "bitcoin-pending" || state.status === "paused") {
        stateLine.textContent = state.status === "paused" ? "Payment paused · resume this same order" : (state.paid ? money(state.paid) + " received · " + money(due - state.paid) + " remaining" : "Waiting for Bitcoin · " + money(due));
        paymentSurface.append(qrGraphic(), element("p", "checkout-qr-caption", "Payment request"));
        if (state.status === "paused") controlActions.append(button("Resume payment", () => { state.status = "bitcoin-pending"; log("Bitcoin request resumed"); render(); }, "checkout-button primary"));
      } else if (state.status === "card-pending") {
        stateLine.textContent = "Card payment in progress · awaiting result";
        paymentSurface.append(element("p", "checkout-card-wait", "Card payment in progress"));
      } else if (state.status === "card-uncertain") {
        stateLine.textContent = "Card result not confirmed · do not retry or hand over the order";
        paymentSurface.append(element("p", "checkout-alert", "Payment status unknown. Reconciliation is required before another tender."));
      } else if (state.status === "failed") {
        stateLine.textContent = "Card attempt failed · no payment received";
        paymentSurface.append(element("p", "checkout-alert", "The attempt is resolved as failed. No goods handed over."), button("Try payment again", () => { state.status = "ready"; state.method = null; log("Failed attempt resolved; new attempt allowed"); render(); }, "checkout-button"));
      } else if (state.status === "partial-cancelled") {
        stateLine.textContent = money(state.paid) + " received · checkout cancelled · owner follow-up required";
        paymentSurface.append(element("p", "checkout-alert", "No goods handed over. Keep the received funds matched to this order while the owner follows up."));
      } else if (state.status === "partial-resolved") {
        stateLine.textContent = money(state.paid) + " received · owner follow-up recorded · order closed";
        paymentSurface.append(element("p", "checkout-alert", "No goods handed over. The received funds remain recorded against this order."));
      } else if (state.status === "paid") {
        stateLine.textContent = "Payment complete · " + money(due) + " · " + state.method;
        const receipt = element("article", "checkout-receipt");
        receipt.append(element("h3", null, "Receipt · Juniper Coffee"), element("p", null, "Order #" + state.order + " · " + ITEMS.map((it, i) => state.qty[i] + " × " + it.name).join(" · ") + " · " + money(due) + " USD"), element("p", null, "Payment: " + state.method));
        paymentSurface.append(receipt,
          button("Request refund", () => { state.status = "refund-requested"; log("Refund requested"); render(); }, "checkout-button"),
          button("Start next order", startNextOrder, "checkout-button primary"));
      } else if (["refund-requested", "refund-review", "refunding", "refunded"].includes(state.status)) {
        const labels = {
          "refund-requested": "Refund requested · awaiting owner review",
          "refund-review": "Refund reviewed · awaiting approval",
          refunding: "Refund approved · simulation still pending",
          refunded: "Refund complete",
        };
        stateLine.textContent = labels[state.status];
        const receipt = element("article", "checkout-receipt"); receipt.append(element("h3", null, "Order #" + state.order + " · " + money(due)), element("p", null, "Refund request · awaiting the next step.")); paymentSurface.append(receipt);
        if (state.status === "refund-requested") controlActions.append(button("Review refund request", () => { state.status = "refund-review"; log("Refund reviewed"); render(); }, "checkout-button"));
        else if (state.status === "refund-review") controlActions.append(button("Approve test refund", () => { state.status = "refunding"; log("Test refund approved"); render(); }, "checkout-button primary"));
        else if (state.status === "refunding") controlActions.append(button("Complete refund simulation", () => { state.status = "refunded"; log("Refund simulation complete"); render(); }, "checkout-button primary"));
        else controlActions.append(button("Start next order", startNextOrder, "checkout-button primary"));
      } else if (state.status === "cancelled") {
        stateLine.textContent = "Checkout cancelled · no payment received";
        paymentSurface.append(element("p", "checkout-card-wait", "No goods handed over."));
        paymentSurface.append(button("Start this order again", () => { state.status = "ready"; state.method = null; state.paid = 0; log("Order reopened"); render(); }, "checkout-button"));
      }
      if (state.status === "bitcoin-pending") {
        if (state.paid === 0) controlActions.append(button("Simulate partial receipt · $4.00", () => { state.paid = 4; log("Partial Bitcoin test receipt · $4.00"); render(); }, "checkout-button primary"));
        else controlActions.append(button("Simulate remaining receipt · " + money(due - state.paid), () => { state.paid = due; completePayment(); }, "checkout-button primary"));
        controlActions.append(button("Pause payment request", () => { state.status = "paused"; log("Payment request paused"); render(); }));
        controlActions.append(button(state.paid ? "Cancel with funds received" : "Cancel checkout", () => {
          if (state.paid) { state.status = "partial-cancelled"; log("Cancelled with partial funds · owner follow-up required"); }
          else { state.status = "cancelled"; state.method = null; log("Checkout cancelled"); }
          render();
        }, "checkout-button quiet"));
      } else if (state.status === "card-pending") {
        controlActions.append(button("Complete card test payment", completePayment, "checkout-button primary"),
          button("Lose callback · mark result uncertain", () => { state.status = "card-uncertain"; log("Card result uncertain · reconciliation required"); render(); }, "checkout-button quiet"));
      } else if (state.status === "card-uncertain") {
        controlActions.append(button("Reconcile · confirm payment completed", completePayment, "checkout-button primary"),
          button("Reconcile · confirm attempt failed", () => { state.status = "failed"; log("Card attempt failed"); render(); }, "checkout-button"));
      } else if (state.status === "partial-cancelled") {
        controlActions.append(button("Record owner follow-up complete", () => { state.status = "partial-resolved"; log("Owner follow-up recorded"); render(); }, "checkout-button"));
      } else if (state.status === "partial-resolved") {
        controlActions.append(button("Start next order", startNextOrder, "checkout-button primary"));
      }
      controlLog.replaceChildren(element("h3", null, "Recent activity"));
      if (!state.history.length) controlLog.append(element("p", "checkout-empty", "No payment activity yet."));
      else state.history.forEach((item) => controlLog.append(element("p", "checkout-history-row", item.label + " · " + item.time)));
      storageNotice.hidden = !storageWarning;
    }
    render();
    shell.stage.append(experience);
    const restart = shell.onRestart;
    shell.onRestart = () => { shell.onRestart = restart; if (restart) restart(); else open(shell); };
    shell.onLeave = () => { shell.onRestart = restart; };
  }
  function qrGraphic() {
    const canvas = element("div", "checkout-qr");
    canvas.setAttribute("role", "img"); canvas.setAttribute("aria-label", "Payment request code");
    for (let i = 0; i < 81; i++) canvas.append(element("i", [0, 1, 2, 9, 10, 11, 18, 19, 20, 6, 7, 8, 15, 16, 17, 24, 25, 26, 54, 55, 56, 63, 64, 65, 72, 73, 74].includes(i) || (i * 17 + i * i) % 5 < 2 ? "dark" : ""));
    return canvas;
  }
  root.OpenBmtCheckout = { open, KEY, ITEMS, validState };
  if (typeof module !== "undefined" && module.exports) module.exports = root.OpenBmtCheckout;
})(globalThis);
