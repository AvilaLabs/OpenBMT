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

  const FONT = '"Roboto", system-ui, sans-serif';
  const INK = "#16181d", MUTED = "#5f6470", LINE = "#e3e5ea", BG = "#ffffff", SOFT = "#f2f3f6";

  function button(text, primary, onTap) {
    const b = el("button", `height:56px;border-radius:28px;font:500 17px ${FONT};border:${primary ? "0" : "1px solid " + LINE};background:${primary ? INK : BG};color:${primary ? "#fff" : INK};`, text);
    if (onTap) b.addEventListener("click", onTap);
    return b;
  }

  /**
   * The shop's register with one order. [d]: {paid} is true once the order is closed with the Bitcoin payment type;
   * {taps} has bitcoin(), called when Bitcoin is tapped on an order that is not closed. Cash and Card do nothing:
   * only Bitcoin is a target of the demo.
   */
  function register(d) {
    const dev = el("div", `width:${W}px;height:${H}px;position:relative;background:${BG};font-family:${FONT};color:${INK};display:flex;flex-direction:column;`);
    dev.className = "device";
    const page = el("div", `flex:1;display:flex;flex-direction:column;padding:${C.STATUS_DP}px 0 ${C.NAV_DP}px;`);
    const head = el("div", "height:64px;display:flex;align-items:center;padding:0 20px;font-size:22px;font-weight:500;", "Register");
    const body = el("div", "flex:1;display:flex;flex-direction:column;padding:8px 20px;gap:16px;");
    const order = el("div", `background:${SOFT};border-radius:20px;padding:20px;display:flex;flex-direction:column;gap:6px;`);
    order.append(el("div", `font-size:15px;color:${MUTED}`, "Order 1042"), el("div", "font-size:34px;font-weight:500", "Total 8.00 USD"));
    body.append(order);
    const foot = el("div", "display:flex;flex-direction:column;gap:12px;padding:16px 20px 20px;");
    if (d.paid) {
      foot.append(el("div", "text-align:center;font-size:24px;font-weight:500;padding:12px 0", "Paid with Bitcoin"));
    } else {
      foot.append(button("Cash", false), button("Card", false), button("Bitcoin", true, d.taps && d.taps.bitcoin));
    }
    page.append(head, body, foot);
    dev.append(page, C.statusBar(true), C.navBar(true));
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

  const api = { register, tillScreen };
  root.OpenBmtRegisterViews = api;
  if (typeof module !== "undefined" && module.exports) module.exports = api;
})(globalThis);
