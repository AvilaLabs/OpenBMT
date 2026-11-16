// The customer's wallet: a plain bitcoin wallet on the customer's phone, no brand, drawn only for the demo.
// It is not an OpenBMT app; any wallet that scans a payment code works the same way at the counter.
// Five screens: home, scanning, tapping, the check before sending, and sent.
"use strict";
(function (root) {
  const C = root.OpenBmtChrome;
  const W = C.WIDTH_DP, H = C.HEIGHT_DP;

  function el(tag, style, text) {
    const e = document.createElement(tag);
    if (style) e.style.cssText = style;
    if (text != null) e.textContent = text;
    return e;
  }

  const FONT = '"Roboto", system-ui, sans-serif';
  const INK = "#16181d", MUTED = "#5f6470", LINE = "#e3e5ea", BG = "#ffffff", SOFT = "#f2f3f6";

  function frame(children, buttons) {
    const dev = el("div", `width:${W}px;height:${H}px;position:relative;background:${BG};font-family:${FONT};color:${INK};display:flex;flex-direction:column;`);
    dev.className = "device";
    const page = el("div", `flex:1;display:flex;flex-direction:column;padding:${C.STATUS_DP}px 0 ${C.NAV_DP}px;`);
    const head = el("div", `height:64px;display:flex;align-items:center;padding:0 20px;font-size:22px;font-weight:500;`, "Wallet");
    const body = el("div", "flex:1;display:flex;flex-direction:column;padding:8px 20px;gap:16px;");
    body.append(...children);
    const foot = el("div", "display:flex;flex-direction:column;gap:12px;padding:16px 20px 20px;");
    for (const b of buttons || []) foot.append(b);
    page.append(head, body, foot);
    dev.append(page, C.statusBar(true), C.navBar(true));
    return dev;
  }

  function button(text, primary) {
    const b = el("button", `height:56px;border-radius:28px;font:500 17px ${FONT};border:${primary ? "0" : "1px solid " + LINE};background:${primary ? INK : BG};color:${primary ? "#fff" : INK};`, text);
    return b;
  }

  function card(rows) {
    const c = el("div", `background:${SOFT};border-radius:20px;padding:20px;display:flex;flex-direction:column;gap:6px;`);
    c.append(...rows);
    return c;
  }

  const sats = (n) => n.toLocaleString("en-US") + " sats";

  /** Home: the balance, and Scan and Tap side by side to pay. */
  function home(balance) {
    const row = el("div", "display:flex;gap:12px;");
    const scan = button("Scan", true), tap = button("Tap", false);
    scan.style.flex = tap.style.flex = "1";
    row.append(scan, tap);
    return frame([
      card([el("div", `font-size:15px;color:${MUTED}`, "Balance"), el("div", "font-size:36px;font-weight:500", sats(balance))]),
      el("div", `font-size:15px;color:${MUTED};padding:4px 2px`, "To pay a shop, scan the code on its screen or tap your phone to the counter."),
    ], [row]);
  }

  /** Scanning: the camera pointed at the counter's code. */
  function scanning() {
    const view = el("div", "flex:1;border-radius:24px;background:#1b1d22;position:relative;overflow:hidden;");
    const box = el("div", "position:absolute;left:50%;top:44%;width:230px;height:230px;transform:translate(-50%,-50%);border:3px solid #fff;border-radius:18px;box-shadow:0 0 0 2000px rgba(0,0,0,0.35);");
    const tip = el("div", "position:absolute;left:0;right:0;bottom:28px;text-align:center;color:#fff;font-size:16px;", "Point the camera at the code");
    view.append(box, tip);
    return frame([view], [button("Cancel", false)]);
  }

  /** Tapping: the phone held to the counter, which gives the payment over NFC. */
  function tapping() {
    const view = el("div", "flex:1;border-radius:24px;background:#1b1d22;position:relative;overflow:hidden;display:grid;place-items:center;");
    const rings = el("div", "position:relative;width:200px;height:200px;");
    for (const r of [200, 140, 80]) {
      rings.append(el("div", `position:absolute;left:50%;top:50%;width:${r}px;height:${r}px;transform:translate(-50%,-50%);border:3px solid rgba(255,255,255,${r === 80 ? 0.95 : r === 140 ? 0.6 : 0.3});border-radius:50%;`));
    }
    const tip = el("div", "position:absolute;left:0;right:0;bottom:28px;text-align:center;color:#fff;font-size:16px;", "Hold near the counter");
    view.append(rings, tip);
    return frame([view], [button("Cancel", false)]);
  }

  /**
   * The check before sending: who it goes to and how much, from the code. When the request carries a Lightning
   * invoice, [pay] is {method, onPick(method)}: a row "Pay with" offers Lightning and On-chain, and the line below
   * says "Lightning invoice" for Lightning and the address for On-chain.
   */
  function confirm(amountSats, address, pay) {
    const lightning = !!pay && pay.method === "Lightning";
    const rows = [
      el("div", "font-size:22px;font-weight:500;padding:4px 2px", "Send this payment?"),
      card([
        el("div", `font-size:15px;color:${MUTED}`, "Amount"),
        el("div", "font-size:34px;font-weight:500", sats(amountSats)),
      ]),
    ];
    if (pay) {
      const row = el("div", "display:flex;align-items:center;gap:10px;padding:0 2px;");
      row.append(el("div", `font-size:15px;color:${MUTED};margin-right:auto`, "Pay with"));
      for (const method of ["Lightning", "On-chain"]) {
        const on = pay.method === method;
        const b = el("button", `height:40px;padding:0 16px;border-radius:20px;font:500 15px ${FONT};border:${on ? "0" : "1px solid " + LINE};background:${on ? INK : BG};color:${on ? "#fff" : INK};`, method);
        b.addEventListener("click", () => pay.onPick(method));
        row.append(b);
      }
      rows.push(row);
    }
    rows.push(card([
      el("div", `font-size:15px;color:${MUTED}`, "To"),
      lightning
        ? el("div", "font-size:17px;font-weight:500;line-height:1.5", "Lightning invoice")
        : el("div", `font:500 17px "Roboto Mono",monospace;word-break:break-all;line-height:1.5`, address),
    ]));
    return frame(rows, [button("Send", true), button("Cancel", false)]);
  }

  /** Sent. */
  function sent(amountSats) {
    const tick = el("div", `width:96px;height:96px;border-radius:50%;background:${INK};color:#fff;display:grid;place-items:center;font-size:52px;margin:60px auto 8px;`, "✓");
    return frame([
      tick,
      el("div", "text-align:center;font-size:28px;font-weight:500", "Sent"),
      el("div", `text-align:center;font-size:17px;color:${MUTED}`, sats(amountSats)),
    ], [button("Done", true)]);
  }

  root.OpenBmtWallet = { home, scanning, tapping, confirm, sent };
})(globalThis);
