// In-memory domain contract for the illustrative browser checkout.
// This is not a payment engine, durable ledger, or provider integration.
"use strict";

(function (root, factory) {
  const contract = factory();
  if (typeof module !== "undefined" && module.exports) module.exports = contract;
  root.OpenBmtSaleContract = contract;
})(globalThis, function () {

const ITEMS = Object.freeze([
  Object.freeze({ id: "oat-latte", name: "Oat latte", unitMinor: 550 }),
  Object.freeze({ id: "sesame-bagel", name: "Sesame bagel", unitMinor: 450 }),
]);

const METHODS = new Set(["Bitcoin", "Card", "Cash"]);
const WAITING = new Set(["waitingBitcoin", "waitingCard", "waitingCash"]);
const TERMINAL = new Set(["paid", "cancelled"]);

function integer(value, label, minimum = 0) {
  if (!Number.isSafeInteger(value) || value < minimum) throw new RangeError(`${label} must be a safe integer >= ${minimum}`);
  return value;
}

function freezeSale(fields) {
  fields.qty = Object.freeze([...fields.qty]);
  return Object.freeze(fields);
}

function assertSale(sale) {
  if (!sale || typeof sale !== "object" || !Number.isSafeInteger(sale.order) || sale.order < 1 ||
      sale.currency !== "USD" ||
      !Array.isArray(sale.qty) || sale.qty.length !== ITEMS.length ||
      !sale.qty.every((qty) => Number.isSafeInteger(qty) && qty >= 0 && qty <= 99) ||
      !["draft", ...WAITING, "uncertainCard", "checkingCard", ...TERMINAL].includes(sale.status)) {
    throw new TypeError("invalid sale state");
  }
  return sale;
}

function copy(sale, changes) {
  assertSale(sale);
  return freezeSale({ ...sale, ...changes, qty: changes.qty || sale.qty });
}

function create(order, qty = [1, 1]) {
  integer(order, "order", 1);
  if (!Array.isArray(qty) || qty.length !== ITEMS.length) throw new TypeError("qty must contain one quantity per item");
  qty.forEach((n) => { integer(n, "quantity"); if (n > 99) throw new RangeError("quantity must be between 0 and 99"); });
  return freezeSale({ order, currency: "USD", status: "draft", method: null, qty: [...qty], attemptId: null,
    cashReceivedMinor: null, changeMinor: null, refundRequestId: null });
}

function totalMinor(sale) {
  assertSale(sale);
  const total = sale.qty.reduce((sum, qty, i) => sum + qty * ITEMS[i].unitMinor, 0);
  if (!Number.isSafeInteger(total)) throw new RangeError("total must be a safe integer");
  return total;
}

function setQuantity(sale, index, qty) {
  assertSale(sale);
  if (sale.status !== "draft") throw new Error("basket is locked after tender begins");
  integer(index, "item index");
  if (index >= ITEMS.length) throw new RangeError("item index is out of range");
  integer(qty, "quantity");
  if (qty > 99) throw new RangeError("quantity must be between 0 and 99");
  const next = [...sale.qty]; next[index] = qty;
  return copy(sale, { qty: next });
}

function begin(sale, method) {
  assertSale(sale);
  if (sale.status !== "draft") throw new Error("only a draft sale can begin");
  if (!METHODS.has(method)) throw new TypeError("method must be Bitcoin, Card, or Cash");
  if (totalMinor(sale) <= 0) throw new RangeError("sale total must be greater than zero");
  const status = method === "Bitcoin" ? "waitingBitcoin" : method === "Card" ? "waitingCard" : "waitingCash";
  const attemptId = `demo-${sale.order}-${method.toLowerCase()}-attempt-1`;
  return copy(sale, { status, method, attemptId });
}

function transition(sale, from, status) {
  assertSale(sale);
  if (sale.status !== from) throw new Error(`invalid transition from ${sale.status}`);
  return copy(sale, { status });
}

function cardSubmitted(sale) { return transition(sale, "waitingCard", "uncertainCard"); }
function checkCard(sale) { return transition(sale, "uncertainCard", "checkingCard"); }

function confirm(sale) {
  assertSale(sale);
  if (sale.status !== "waitingBitcoin" && sale.status !== "checkingCard") throw new Error("sale is not awaiting a simulated confirmation");
  return copy(sale, { status: "paid" });
}

function recordCash(sale, tenderedMinor) {
  assertSale(sale);
  if (sale.status !== "waitingCash") throw new Error("cash can only be recorded while waiting for cash");
  integer(tenderedMinor, "tendered amount");
  const due = totalMinor(sale);
  if (tenderedMinor < due) throw new RangeError("cash tender is below the full balance");
  return copy(sale, { status: "paid", cashReceivedMinor: tenderedMinor, changeMinor: tenderedMinor - due });
}

function cancel(sale) {
  assertSale(sale);
  if (sale.status !== "draft" && !WAITING.has(sale.status)) throw new Error("sale cannot be canceled in this state");
  return copy(sale, { status: "cancelled" });
}

function requestRefund(sale) {
  assertSale(sale);
  if (sale.status !== "paid") throw new Error("only a paid sale can have a refund request");
  if (sale.refundRequestId) return sale;
  return copy(sale, { refundRequestId: `demo-${sale.order}-refund-1` });
}

function receipt(sale) {
  assertSale(sale);
  if (sale.status !== "paid") throw new Error("receipt requires a paid sale");
  const items = Object.freeze(ITEMS.flatMap((item, i) => sale.qty[i] === 0 ? [] : [Object.freeze({ ...item, quantity: sale.qty[i], lineMinor: item.unitMinor * sale.qty[i] })]));
  return Object.freeze({ order: sale.order, currency: sale.currency, status: "paid", method: sale.method, items,
    qty: Object.freeze([...sale.qty]), totalMinor: totalMinor(sale), attemptId: sale.attemptId,
    cashReceivedMinor: sale.cashReceivedMinor, changeMinor: sale.changeMinor,
    simulated: true, refundRequested: Boolean(sale.refundRequestId), refundRequestId: sale.refundRequestId });
}

const api = { ITEMS, create, setQuantity, totalMinor, begin, cardSubmitted, checkCard, confirm, recordCash, cancel, requestRefund, receipt };
return api;
});
