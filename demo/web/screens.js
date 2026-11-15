// What is drawn inside the two rectangles: the counter's screen and the
// owner phone's. The words come from the module (the library's) and from the
// word files the apps' tests write (the apps' own); the layout copies the
// Android apps roughly. Loaded by the page before demo.js, and by Node.
"use strict";

// Words of the owner phone's lines.
const OWNER_WORDS = {
  nothing: "No sales yet. A sale shows here as soon as the counter sends it.",
  heading: "Latest sale",
  id: "Sale",
  amount: "Amount",
  state: "State",
  counting: "In the books as",
  sats: "Received",
  notYet: "nothing counted yet",
};

// The words of a counter reply: a sale's come with the reply (the library's
// own), the app's own states are looked up in the file the app's tests write.
function counterWords(reply, appWords) {
  const c = reply.counter;
  if (c.kind === "sale") return { tone: c.words.tone, heading: c.words.heading, what: c.words.what, next: c.words.next };
  const w = appWords[c.state];
  if (!w) throw new Error("no words for the state " + c.state);
  return { tone: w.tone, heading: w.heading, what: w.what, next: w.next };
}

// The modules of the QR code as rows of booleans, from the module's reply.
function qrRows(payment) {
  const n = payment.qr_size;
  const rows = [];
  for (let y = 0; y < n; y++) {
    const row = [];
    for (let x = 0; x < n; x++) row.push(payment.qr[y * n + x] === "1");
    rows.push(row);
  }
  return rows;
}

function satsText(n) {
  return String(n).replace(/\B(?=(\d{3})+(?!\d))/g, ",");
}

// The code is on the screen while the customer can still pay with it.
function showsCode(reply) {
  const c = reply.counter;
  return !!reply.payment && c.kind === "sale" && (c.state === "NotPaidYet" || c.state === "PartPaid");
}

function el(doc, tag, cls, text) {
  const e = doc.createElement(tag);
  if (cls) e.className = cls;
  if (text !== undefined) e.textContent = text;
  return e;
}

function drawQr(doc, rows) {
  const canvas = el(doc, "canvas", "obd-qr");
  const quiet = 4;
  const scale = 6;
  const side = (rows.length + 2 * quiet) * scale;
  canvas.width = side;
  canvas.height = side;
  const g = canvas.getContext("2d");
  g.fillStyle = "#fff";
  g.fillRect(0, 0, side, side);
  g.fillStyle = "#000";
  rows.forEach((row, y) => row.forEach((dark, x) => {
    if (dark) g.fillRect((x + quiet) * scale, (y + quiet) * scale, scale, scale);
  }));
  return canvas;
}

// The counter's screen inside `root` (#pos-screen).
function renderPos(doc, root, reply, appWords) {
  const words = counterWords(reply, appWords);
  const screen = el(doc, "div", "obd-screen");
  screen.append(el(doc, "div", "obd-band tone-" + words.tone));
  const body = el(doc, "div", "obd-body");
  body.append(el(doc, "p", "obd-heading", words.heading), el(doc, "p", "obd-what", words.what), el(doc, "p", "obd-next", words.next));
  const sale = reply.counter.sale;
  if (sale) body.append(el(doc, "p", "obd-amount", "$" + sale.amount + "  (" + satsText(sale.needs_sats) + " sats)"));
  if (showsCode(reply)) {
    body.append(drawQr(doc, qrRows(reply.payment)), el(doc, "p", "obd-address", reply.payment.address));
  }
  screen.append(body);
  root.replaceChildren(screen);
}

// The owner phone's screen inside `root` (#phone-screen): the sale as the
// owner's books have it.
function renderPhone(doc, root, reply) {
  const screen = el(doc, "div", "obd-screen");
  const body = el(doc, "div", "obd-body");
  const o = reply.owner;
  if (!o) {
    body.append(el(doc, "p", "obd-heading", OWNER_WORDS.nothing));
  } else {
    body.append(el(doc, "p", "obd-heading", OWNER_WORDS.heading));
    const dl = el(doc, "dl", "obd-record");
    const sats = o.sats_received === null ? OWNER_WORDS.notYet : satsText(o.sats_received) + " sats";
    const rows = [
      [OWNER_WORDS.id, o.id],
      [OWNER_WORDS.amount, o.amount + " " + o.currency],
      [OWNER_WORDS.state, o.state],
      [OWNER_WORDS.counting, o.counting],
      [OWNER_WORDS.sats, sats],
    ];
    for (const [k, v] of rows) dl.append(el(doc, "dt", "", k), el(doc, "dd", "", v));
    body.append(dl);
  }
  screen.append(body);
  root.replaceChildren(screen);
}

const Screens = { counterWords, qrRows, satsText, showsCode, renderPos, renderPhone, OWNER_WORDS };
if (typeof module !== "undefined" && module.exports) module.exports = Screens;
else if (typeof window !== "undefined") window.Screens = Screens;
