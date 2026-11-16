// The customer's wallet: a plain bitcoin wallet on the customer's phone, no brand, drawn only for the demo.
// It is not an OpenBMT app; any wallet that scans a payment code works the same way at the counter.
// The phone's own home screen, then five wallet screens: home, scanning, tapping, the check before sending, and sent.
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

  /** Home: the balance, and Scan to pay. */
  function home(balance) {
    return frame([
      card([el("div", `font-size:15px;color:${MUTED}`, "Balance"), el("div", "font-size:36px;font-weight:500", sats(balance))]),
      el("div", `font-size:15px;color:${MUTED};padding:4px 2px`, "To pay a shop, scan the code on its screen or hold your phone to the counter."),
    ], [button("Scan", true)]);
  }

  const SVG = "http://www.w3.org/2000/svg";

  /** A glyph of [paths] (an SVG path each) in white strokes on a 24-unit square, [size] px wide. */
  function glyph(paths, size) {
    const svg = document.createElementNS(SVG, "svg");
    svg.setAttribute("viewBox", "0 0 24 24");
    svg.setAttribute("width", String(size));
    svg.setAttribute("height", String(size));
    svg.setAttribute("fill", "none");
    svg.setAttribute("stroke", "#fff");
    svg.setAttribute("stroke-width", "1.9");
    svg.setAttribute("stroke-linecap", "round");
    svg.setAttribute("stroke-linejoin", "round");
    for (const d of paths) {
      const p = document.createElementNS(SVG, "path");
      p.setAttribute("d", d);
      svg.append(p);
    }
    return svg;
  }

  const circle = (cx, cy, r) => `M${cx - r} ${cy}a${r} ${r} 0 1 0 ${2 * r} 0a${r} ${r} 0 1 0 ${-2 * r} 0`;

  /** The apps of the home screen: made-up looks, no real app's icon. */
  const APPS = [
    ["Photos", "#b5654e", [circle(8.5, 9, 2), "M3 19l6-6 4 4 3-3 5 5", "M4 4h16v16H4z"]],
    ["Maps", "#5a8f63", ["M12 21s-6-5.5-6-10a6 6 0 0 1 12 0c0 4.5-6 10-6 10z", circle(12, 11, 2)]],
    ["Clock", "#5b6f8f", [circle(12, 12, 9), "M12 7v5l3 2"]],
    ["Calendar", "#8a6fb0", ["M4 6h16v14H4z", "M4 11h16M8 3v4M16 3v4"]],
    ["Weather", "#4c8ab0", ["M7 18a4 4 0 0 1 0-8 5 5 0 0 1 9.5-1A4.5 4.5 0 0 1 17 18z"]],
    ["Notes", "#b9923a", ["M5 3h14v18H5z", "M9 8h6M9 12h6M9 16h3"]],
    ["Calculator", "#6b6f78", ["M5 3h14v18H5z", "M8 7h8", "M9 12h.01M12 12h.01M15 12h.01M9 16h.01M12 16h.01M15 16h.01"]],
    ["Bitcoin Wallet", "#0f766e", ["M4 8h16v11H4z", "M4 8V6a2 2 0 0 1 2-2h11v4", "M16 13.5h.01"]],
  ];
  const DOCK = [
    ["#5a8f63", ["M6 3h4l2 5-2.5 1.5a11 11 0 0 0 5 5L16 12l5 2v4a2 2 0 0 1-2 2A16 16 0 0 1 4 5a2 2 0 0 1 2-2z"]],
    ["#4c8ab0", ["M4 5h16v11H9l-5 4z"]],
    ["#6b6f78", ["M4 8h3l2-3h6l2 3h3v11H4z", circle(12, 13, 3.5)]],
    ["#b5654e", [circle(12, 12, 9), "M3 12h18M12 3c3 3 3 15 0 18M12 3c-3 3-3 15 0 18"]],
  ];

  function tile(color, paths) {
    const t = el("div", `width:56px;height:56px;border-radius:18px;background:${color};display:grid;place-items:center;box-shadow:0 2px 6px rgba(30,45,60,0.18);`);
    t.append(glyph(paths, 28));
    return t;
  }

  /**
   * The phone's own home screen, before any app is open: a wallpaper, the clock and date, eight apps and a dock of
   * four. Only the Bitcoin Wallet icon is a button; the other icons do nothing.
   */
  function homeScreen() {
    const dev = el("div", `width:${W}px;height:${H}px;position:relative;background:linear-gradient(165deg,#bcd3ea 0%,#d6dcef 52%,#e6d9e8 100%);font-family:${FONT};color:#1d2b38;display:flex;flex-direction:column;`);
    dev.className = "device";
    const page = el("div", `flex:1;display:flex;flex-direction:column;padding:${C.STATUS_DP}px 0 ${C.NAV_DP}px;`);
    const clock = el("div", "text-align:center;margin-top:56px;");
    clock.append(el("div", "font-size:76px;font-weight:300;line-height:1;letter-spacing:-1px", "12:30"),
      el("div", "font-size:18px;margin-top:10px;opacity:0.8", "Saturday, May 16"));
    const grid = el("div", "display:grid;grid-template-columns:repeat(4,1fr);row-gap:24px;padding:0 14px;");
    for (const [label, color, paths] of APPS) {
      const target = label === "Bitcoin Wallet";
      const app = el(target ? "button" : "div", "display:flex;flex-direction:column;align-items:center;gap:6px;background:none;border:0;padding:4px 0;border-radius:18px;font:inherit;color:inherit;");
      app.append(tile(color, paths), el("div", "font-size:12px;text-align:center;line-height:1.15;max-width:84px", label));
      grid.append(app);
    }
    const dock = el("div", "display:flex;justify-content:space-around;margin:26px 14px 14px;padding:12px 0;border-radius:34px;background:rgba(255,255,255,0.45);");
    for (const [color, paths] of DOCK) dock.append(tile(color, paths));
    page.append(clock, el("div", "flex:1"), grid, dock);
    dev.append(page, C.statusBar(true), C.navBar(true));
    return dev;
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
    const tip = el("div", "position:absolute;left:0;right:0;bottom:28px;text-align:center;color:#fff;font-size:16px;", "Reading the payment…");
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

  root.OpenBmtWallet = { homeScreen, home, scanning, tapping, confirm, sent };
})(globalThis);
