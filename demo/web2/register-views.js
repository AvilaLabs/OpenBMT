// What the "With your register" view draws, apart from the counter. The register is a plain mock of the shop's own
// register, no brand, drawn only for the demo: it is not an OpenBMT app and not any vendor's. The owner's phone shows
// the part of the owner app's till screen that says what was read: when, and how many sales matched. Source of the
// port: apps/owner-android/src/android/PosScreen.kt.
"use strict";
(function (root) {
  const C = root.OpenBmtChrome || require("../ui/chrome.js");
  const OV = root.OpenBmtOwnerViews || require("../ui/owner-views.js");
  const W = C.WIDTH_DP, H = C.HEIGHT_DP;

  function el(tag, style, text) {
    const e = document.createElement(tag);
    if (style) e.style.cssText = style;
    if (text != null) e.textContent = text;
    return e;
  }

  const FONT = '"Ubuntu", system-ui, "Helvetica Neue", Arial, sans-serif';
  // A generic modern register: neutral greys, one slate accent, rounded tiles. No company's look.
  const INK = "#0f172a", MUTED = "#64748b", LINE = "#e2e8f0", GROUND = "#f1f5f9", TILE = "#ffffff", ACCENT = "#334155";

  /** The register's size on the stage: the counter tablet's, turned landscape. */
  const LANDSCAPE = { width: C.TABLET.height, height: C.TABLET.width, density: C.TABLET.density };

  /** The items of the shop and the order on the ticket, as the register shows them. */
  const ITEMS = [["Coffee", "4.00"], ["Bagel", "4.00"], ["Juice", "5.00"], ["Muffin", "3.50"], ["Tea", "3.00"], ["Cookie", "2.50"]];
  const LINES = [["Coffee", "4.00"], ["Bagel", "4.00"]];
  const CHARGE = "Charge 8.00 USD";

  /** A button of the register: [css] is its look; [onTap] is optional (Cash and Card do nothing: only Bitcoin is a target). */
  function tap(text, css, onTap) {
    const b = el("button", `border:0;font:inherit;cursor:pointer;${css}`, text);
    if (onTap) b.addEventListener("click", onTap);
    return b;
  }

  /** "Coffee 4.00": the name and the price, one at each end of the row; the text reads with a space between. */
  function row(name, price, css) {
    const r = el("div", `display:flex;justify-content:space-between;align-items:baseline;${css}`);
    r.append(el("span", null, name), document.createTextNode(" "), el("span", null, price));
    return r;
  }

  /** A check mark drawn in CSS. */
  function check(size) {
    return el("span", `display:inline-block;width:${size * 0.55}px;height:${size}px;border:solid ${ACCENT};border-width:0 ${size * 0.16}px ${size * 0.16}px 0;transform:rotate(40deg) translate(-8%,-12%);margin:0 ${size * 0.35}px 0 ${size * 0.15}px`);
  }

  /**
   * The shop's register with one order, on a tablet of [size] (landscape or portrait), as a plain mock of any register:
   * the sale screen with the items and the ticket, and the tender screen. [d]: {screen} is "sale" (the default), "tender"
   * or "paid" (the sale screen with its order closed as Bitcoin; the tender screen has closed). {taps} has charge(),
   * called when the Charge button is tapped, and bitcoin(), called when Bitcoin is tapped on the tender screen. Cash and
   * Card do nothing: only Bitcoin is a target of the demo.
   */
  function register(d, size = LANDSCAPE) {
    const w = size.width, h = size.height, wide = w > h;
    const dev = el("div", `width:${w}px;height:${h}px;position:relative;background:${GROUND};font-family:${FONT};color:${INK};display:flex;flex-direction:column;`);
    dev.className = "device";
    const page = el("div", `flex:1;min-height:0;display:flex;flex-direction:${wide ? "row" : "column"};padding:${C.STATUS_DP}px 0 ${C.NAV_DP}px;`);
    const screen = d.screen || (d.paid ? "paid" : "sale");
    const taps = d.taps || {};
    if (screen === "tender") {
      const box = el("div", "flex:1;display:flex;flex-direction:column;align-items:center;justify-content:center;gap:36px;padding:0 48px;");
      box.append(el("div", "font-size:46px;font-weight:500;letter-spacing:-0.01em;", "How is it paid?"));
      const kinds = el("div", `display:flex;gap:24px;width:100%;max-width:${wide ? 780 : 480}px;${wide ? "" : "flex-direction:column;"}`);
      const kind = (text, accent, onTap) => tap(text, `flex:1;height:${wide ? 150 : 110}px;border-radius:24px;font-size:34px;font-weight:500;`
        + (accent ? `background:${ACCENT};color:#fff;` : `background:${TILE};color:${INK};box-shadow:inset 0 0 0 2px ${LINE};`), onTap);
      kinds.append(kind("Cash", false), kind("Card", false), kind("Bitcoin", true, taps.bitcoin));
      box.append(kinds, el("div", `font-size:24px;color:${MUTED};text-align:center;`, "Bitcoin is a payment type the shop added."));
      page.append(box);
    } else {
      // Left two-thirds: the items; right third: the ticket.
      const items = el("div", `flex:2;min-width:0;min-height:0;display:grid;grid-template-columns:repeat(${wide ? 3 : 2},1fr);grid-auto-rows:1fr;gap:16px;padding:24px;`);
      for (const [name, price] of ITEMS) {
        const t = tap("", `background:${TILE};border-radius:22px;box-shadow:inset 0 0 0 2px ${LINE};display:flex;flex-direction:column;align-items:flex-start;justify-content:space-between;padding:20px 22px;text-align:left;color:${INK};`);
        t.append(el("span", "font-size:30px;font-weight:500;", name), document.createTextNode(" "), el("span", `font-size:26px;color:${MUTED};`, price));
        items.append(t);
      }
      const ticket = el("div", `flex:1;min-width:0;background:${TILE};${wide ? `border-left:2px solid ${LINE};` : `border-top:2px solid ${LINE};`}padding:24px 26px;display:flex;flex-direction:column;gap:14px;`);
      ticket.append(el("div", `font-size:22px;color:${MUTED};`, "Order 1042"));
      for (const [name, price] of LINES) ticket.append(row(name, price, "font-size:28px;"));
      ticket.append(el("div", `height:2px;background:${LINE};margin:6px 0;`), row("Total", "8.00 USD", "font-size:30px;font-weight:500;"));
      ticket.append(el("div", "flex:1"));
      if (screen === "paid") {
        const paid = el("div", `display:flex;align-items:center;justify-content:center;height:92px;border-radius:22px;background:${GROUND};font-size:28px;font-weight:500;`);
        paid.append(check(30), el("span", null, "Paid with Bitcoin"));
        ticket.append(paid);
      } else {
        ticket.append(tap(CHARGE, `height:92px;border-radius:22px;background:${ACCENT};color:#fff;font-size:30px;font-weight:500;`, taps.charge));
      }
      page.append(items, ticket);
    }
    dev.append(page, C.statusBar(true, w), C.navBar(true, w));
    return dev;
  }

  /**
   * PosScreen.kt:25 showTill(message): the owner's till screen, the part that says what was read: PosLines.lastRead and
   * PosLines.counts, the second in bold while an alert is open. [d] has the two lines and the number of open alerts.
   */
  function tillScreen(ui, d) {
    return OV.screen(ui, {
      title: "Till",
      // PosScreen.kt:93 lastRead, and PosScreen.kt:94 counts: 15 sp each.
      body: [ui.text(d.lastRead, 15), ui.text(d.counts, 15, d.alerts > 0)],
      back: () => {},
    });
  }

  const api = { register, tillScreen, LANDSCAPE, CHARGE };
  root.OpenBmtRegisterViews = api;
  if (typeof module !== "undefined" && module.exports) module.exports = api;
})(globalThis);
