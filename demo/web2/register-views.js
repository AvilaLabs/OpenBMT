// What the "With your register" view draws, apart from the counter. The first way's register is a plain mock of the
// shop's own register, no brand, drawn only for the demo: it is not an OpenBMT app and not any vendor's. The other two
// are drawn as Square Register and Clover Station look to a cashier, from the vendors' own product pictures, with no
// logo. The owner's phone shows
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

  /** The register of the first way ("Your register sends the amount") and its taps: the plain mock's own words. */
  function plain(d, size) {
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

  /** A line icon of 24 units, drawn in the current text colour. */
  function icon(paths, size = 24, fill = "none") {
    const s = document.createElement("span");
    s.style.cssText = `display:inline-flex;width:${size}px;height:${size}px;`;
    s.innerHTML = `<svg width="${size}" height="${size}" viewBox="0 0 24 24" fill="${fill}" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">${paths}</svg>`;
    return s;
  }
  const ICONS = {
    search: '<circle cx="11" cy="11" r="6.5"/><path d="M16 16l4.5 4.5"/>',
    checkout: '<rect x="4" y="3" width="16" height="18" rx="2.5"/><path d="M8 8h8M8 12h8M8 16h5"/>',
    inventory: '<path d="M3.5 8L12 3.5 20.5 8v8L12 20.5 3.5 16z"/><path d="M3.5 8L12 12.5 20.5 8M12 12.5v8"/>',
    transactions: '<path d="M6 3h12v18l-3-2-3 2-3-2-3 2z"/><path d="M9 8h6M9 12h6"/>',
    notifications: '<path d="M6 17V11a6 6 0 0112 0v6l1.5 2h-15z"/><path d="M10 21h4"/>',
    more: '<circle cx="5.5" cy="12" r="1.3"/><circle cx="12" cy="12" r="1.3"/><circle cx="18.5" cy="12" r="1.3"/>',
    menu: '<path d="M4 7h16M4 12h16M4 17h16"/>',
    lock: '<rect x="5" y="11" width="14" height="9" rx="2"/><path d="M8 11V8a4 4 0 018 0v3"/>',
    tick: '<path d="M5 12.5l4.5 4.5L19 7.5"/>',
  };

  // ---- Square: the Square Register / Square POS app, landscape, white. ----
  const SQ = { INK: "#000000", MUTED: "#737373", LINE: "#e5e5e5", TILE: "#f2f2f2", ROW: "#f2f2f2" };
  const SQ_PICTURE = ["#f3d9b1", "#c9dff0", "#f4c9c9", "#d6e6c4", "#e3d3ef", "#efe0bd"];
  const SQ_TABS = ["Favorites", "Drinks", "Bakery", "All items"];
  const SQ_TILES = ["Coffee", "Bagel", "Juice", "Muffin", "Tea", "Cookie"];
  const SQ_LINES = [["Coffee", "$4.00"], ["Bagel", "$4.00"]];
  const SQ_BAR = [["Checkout", "checkout"], ["Inventory", "inventory"], ["Transactions", "transactions"], ["Notifications", "notifications"], ["More", "more"]];
  const SQUARE_CHARGE = "Charge $8.00";

  function squareSale(wide, taps) {
    const body = el("div", `flex:1;min-height:0;display:flex;flex-direction:${wide ? "row" : "column"};`);
    // The items: the search field, the category tabs and the grid of tiles.
    const left = el("div", `flex:${wide ? 2 : 1};min-width:0;min-height:0;display:flex;flex-direction:column;padding:16px 16px 0;gap:12px;`);
    const search = el("div", `display:flex;align-items:center;gap:10px;height:44px;border-radius:10px;background:${SQ.TILE};padding:0 14px;color:${SQ.MUTED};font-size:17px;width:${wide ? "70%" : "100%"};box-sizing:border-box;`);
    search.append(icon(ICONS.search, 20), el("span", null, "Search"));
    const tabs = el("div", "display:flex;gap:24px;font-size:16px;");
    for (const t of SQ_TABS) {
      const on = t === "All items";
      tabs.append(el("span", `padding:6px 0;${on ? `color:${SQ.INK};font-weight:600;border-bottom:3px solid ${SQ.INK};` : `color:${SQ.MUTED};border-bottom:3px solid transparent;`}`, t));
    }
    const grid = el("div", `flex:1;min-height:0;display:grid;grid-template-columns:repeat(${wide ? 4 : 3},1fr);${wide ? "align-content:start;" : "grid-template-rows:repeat(2,minmax(0,1fr));"}gap:12px;overflow:hidden;padding-bottom:12px;`);
    SQ_TILES.forEach((name, i) => {
      const t = el("div", `${wide ? "aspect-ratio:1/1;" : ""}min-width:0;min-height:0;background:${SQ.TILE};border-radius:12px;position:relative;overflow:hidden;`);
      t.append(el("div", `position:absolute;left:12%;top:10%;width:46%;height:46%;border-radius:${i % 2 ? "14px" : "50%"};background:${SQ_PICTURE[i]};`),
        el("div", `position:absolute;left:12px;bottom:10px;font-size:14px;color:${SQ.INK};`, name));
      grid.append(t);
    });
    left.append(search, tabs, grid);
    // The cart.
    const cart = el("div", `${wide ? "flex:1;" : "flex:none;height:330px;box-sizing:border-box;"}min-width:0;${wide ? `border-left:1px solid ${SQ.LINE};` : `border-top:1px solid ${SQ.LINE};`}padding:16px 18px;display:flex;flex-direction:column;gap:10px;`);
    cart.append(el("div", `font-size:14px;color:${SQ.MUTED};`, "2 items"));
    SQ_LINES.forEach(([name, price], i) => {
      const line = el("div", "display:flex;align-items:center;gap:12px;font-size:17px;");
      line.append(el("div", `width:38px;height:38px;border-radius:8px;background:${SQ_PICTURE[i]};flex:none;`), el("span", "flex:1", name), document.createTextNode(" "), el("span", null, price));
      cart.append(line);
    });
    cart.append(el("div", `height:1px;background:${SQ.LINE};margin:4px 0;`), row("Tax", "$0.00", `font-size:16px;color:${SQ.MUTED};`), el("div", "flex:1"));
    cart.append(tap("Save cart", `height:50px;border-radius:12px;background:${SQ.TILE};color:${SQ.INK};font-size:17px;font-weight:600;`),
      tap(SQUARE_CHARGE, `height:54px;border-radius:27px;background:#000;color:#fff;font-size:18px;font-weight:600;`, taps.charge));
    body.append(left, cart);
    const bar = el("div", `display:flex;justify-content:space-around;align-items:center;height:56px;border-top:1px solid ${SQ.LINE};flex:none;`);
    for (const [label, key] of SQ_BAR) {
      const item = el("div", `display:flex;flex-direction:column;align-items:center;gap:2px;font-size:11px;color:${label === "Checkout" ? SQ.INK : SQ.MUTED};${label === "Checkout" ? "font-weight:600;" : ""}`);
      item.append(icon(ICONS[key], 22), el("span", null, label));
      bar.append(item);
    }
    return [body, bar];
  }

  function squarePayment(wide, taps) {
    const box = el("div", `flex:1;min-height:0;display:flex;flex-direction:column;padding:${wide ? "20px 40px 0" : "28px 32px 0"};gap:6px;`);
    box.append(el("div", `font-size:${wide ? 52 : 60}px;font-weight:700;letter-spacing:-0.02em;line-height:1.05;`, "$8.00"),
      el("div", `font-size:17px;color:${SQ.MUTED};margin-bottom:10px;`, "Select payment"));
    const bar = (text, onTap) => tap(text, `height:${wide ? 44 : 54}px;border-radius:12px;background:${SQ.ROW};color:${SQ.INK};font-size:17px;text-align:left;padding:0 18px;width:100%;`, onTap);
    const list = el("div", "display:flex;flex-direction:column;gap:8px;");
    for (const name of ["Cash", "Bitcoin", "Gift card", "Other payment types"]) list.append(bar(name));
    list.append(el("div", `font-size:14px;color:${SQ.MUTED};margin-top:8px;`, "Custom payment methods"), bar("OpenBMT", taps.bitcoin));
    box.append(list, el("div", `font-size:13px;color:${SQ.MUTED};margin-top:10px;`,
      "Bitcoin here is Square's own: it pays into Square's wallet. OpenBMT pays into the shop's own wallet."));
    return [box];
  }

  function squareDone() {
    const box = el("div", "flex:1;display:flex;flex-direction:column;align-items:center;justify-content:center;gap:10px;");
    const ring = el("div", "width:96px;height:96px;border-radius:50%;background:#000;color:#fff;display:flex;align-items:center;justify-content:center;margin-bottom:10px;");
    ring.append(icon(ICONS.tick, 56));
    box.append(ring, el("div", "font-size:48px;font-weight:700;letter-spacing:-0.02em;", "$8.00"), el("div", `font-size:20px;color:${SQ.MUTED};`, "All done"),
      tap("New sale", "height:54px;padding:0 48px;border-radius:27px;background:#000;color:#fff;font-size:18px;font-weight:600;margin-top:22px;"));
    return [box];
  }

  // ---- Clover: the Clover Station, landscape, light grey. ----
  const CL = { GROUND: "#e9ecef", WHITE: "#ffffff", INK: "#212121", MUTED: "#757575", GREEN: "#4caf50", BLUE: "#2196f3", SELECT: "#d6ebfb" };
  const CL_CATEGORIES = [["Drinks", "#1e88e5"], ["Bakery", "#ef6c00"], ["Breakfast", "#8e24aa"]];
  const CL_ITEMS = [["Coffee", "$4.00", "#1e88e5"], ["Latte", "$5.00", "#8e24aa"], ["Tea", "$3.00", "#43a047"], ["Juice", "$5.00", "#ef6c00"], ["Bagel", "$4.00", "#fbc02d"], ["Muffin", "$3.50", "#d81b60"]];
  const CL_LINES = [["Coffee", "$4.00"], ["Bagel", "$4.00"]];

  function cloverSale(wide, taps) {
    const top = el("div", `display:flex;align-items:center;gap:14px;height:44px;flex:none;background:${CL.WHITE};padding:0 14px;font-size:17px;box-shadow:0 1px 3px rgba(0,0,0,0.18);position:relative;`);
    top.append(icon(ICONS.menu, 24), el("span", "font-size:19px;font-weight:500;flex:1", "Register"),
      el("span", `color:${CL.MUTED};font-size:15px;`, "Scan barcode or search"), el("span", `font-size:15px;font-weight:600;color:${CL.BLUE};`, "ORDERS"));
    const body = el("div", `flex:1;min-height:0;display:flex;flex-direction:${wide ? "row" : "column"};`);
    // The order.
    const order = el("div", `${wide ? "flex:1.1;" : "order:3;flex:none;height:340px;"}min-width:0;background:${CL.WHITE};${wide ? "" : `border-top:1px solid #cfd8dc;`}display:flex;flex-direction:column;padding:12px 14px;gap:8px;font-size:17px;`);
    order.append(el("div", `font-size:18px;font-weight:600;padding-bottom:6px;border-bottom:1px solid #cfd8dc;`, "Dine in order"));
    for (const [name, price] of CL_LINES) order.append(row(name, price, "font-size:17px;"));
    order.append(el("div", "flex:1"), row("Subtotal", "$8.00", `font-size:15px;color:${CL.MUTED};`), row("Tax", "$0.00", `font-size:15px;color:${CL.MUTED};`),
      row("Total", "$8.00", "font-size:19px;font-weight:700;"));
    const fast = el("div", "display:flex;align-items:center;gap:6px;flex-wrap:wrap;font-size:13px;");
    fast.append(el("div", `color:${CL.MUTED};width:100%;`, "Fast pay with cash"));
    for (const amount of ["$5.00", "$10.00", "$20.00", "Custom"]) fast.append(tap(amount, `flex:1;height:32px;padding:0 4px;border-radius:4px;background:${CL.GROUND};color:${CL.INK};font-size:13px;`));
    const pay = el("div", "display:flex;gap:8px;");
    pay.append(tap("Save", `flex:1;height:48px;border-radius:4px;background:#fff;color:${CL.INK};box-shadow:inset 0 0 0 1px #b0bec5;font-size:17px;font-weight:600;`),
      tap("Pay", `flex:1;height:48px;border-radius:4px;background:${CL.GREEN};color:#fff;font-size:17px;font-weight:600;`, taps.charge));
    order.append(fast, pay);
    // The categories.
    const cats = el("div", `${wide ? "flex:0.8;" : "order:1;flex:none;"}min-width:0;display:flex;flex-direction:${wide ? "column" : "row"};gap:6px;padding:8px 6px;`);
    for (const [name, colour] of CL_CATEGORIES) {
      const on = name === "Drinks";
      cats.append(el("div", `${wide ? "" : "flex:1;"}background:${on ? CL.SELECT : CL.WHITE};border-left:6px solid ${colour};padding:14px 12px;font-size:16px;`, name));
    }
    // The items.
    const items = el("div", `${wide ? "" : "order:2;"}flex:${wide ? 2.2 : 1};min-width:0;min-height:0;display:flex;flex-direction:column;padding:8px 8px 0;gap:8px;`);
    const qty = el("div", "display:flex;gap:6px;align-items:center;font-size:14px;");
    qty.append(el("span", `color:${CL.MUTED};margin-right:4px;`, "QTY"));
    ["1", "2", "3", "4", "5", "CUSTOM"].forEach((q, i) => qty.append(el("span", `padding:8px ${q === "CUSTOM" ? 12 : 14}px;border-radius:4px;background:${i === 0 ? CL.BLUE : CL.WHITE};color:${i === 0 ? "#fff" : CL.INK};`, q)));
    const grid = el("div", `flex:1;min-height:0;display:grid;grid-template-columns:repeat(3,1fr);gap:8px;align-content:start;overflow:hidden;`);
    for (const [name, price, colour] of CL_ITEMS) {
      const card = el("div", `position:relative;background:${CL.WHITE};border-radius:4px;height:${wide ? 104 : 120}px;padding:56px 10px 0;box-sizing:border-box;font-size:16px;overflow:hidden;`);
      card.append(el("div", `position:absolute;left:0;top:0;border-style:solid;border-width:30px 30px 0 0;border-color:${colour} transparent transparent transparent;`),
        el("div", null, name), el("div", `color:${CL.MUTED};font-size:14px;`, price));
      grid.append(card);
    }
    items.append(qty, grid);
    body.append(order, cats, items);
    return [top, body];
  }

  function cloverPayment(wide, taps) {
    const box = el("div", `flex:1;min-height:0;display:flex;flex-direction:column;align-items:center;justify-content:center;gap:26px;padding:0 40px;`);
    box.append(el("div", "font-size:60px;font-weight:600;", "$8.00"));
    const grid = el("div", `display:grid;grid-template-columns:repeat(2,1fr);gap:14px;width:100%;max-width:${wide ? 640 : 480}px;`);
    const big = (text, onTap) => tap(text, `height:${wide ? 96 : 120}px;border-radius:4px;background:#fff;color:${CL.INK};box-shadow:0 1px 3px rgba(0,0,0,0.25);font-size:24px;font-weight:500;`, onTap);
    grid.append(big("Cash"), big("Credit/Debit"), big("Bitcoin", taps.bitcoin), big("Gift card"));
    box.append(grid);
    return [box];
  }

  function cloverPaid() {
    const box = el("div", "flex:1;display:flex;flex-direction:column;align-items:center;justify-content:center;gap:14px;");
    const ring = el("div", `width:92px;height:92px;border-radius:50%;background:${CL.GREEN};color:#fff;display:flex;align-items:center;justify-content:center;`);
    ring.append(icon(ICONS.tick, 56));
    const acts = el("div", "display:flex;gap:14px;margin-top:22px;");
    acts.append(tap("No receipt", `height:56px;padding:0 36px;border-radius:4px;background:#fff;color:${CL.INK};box-shadow:0 1px 3px rgba(0,0,0,0.25);font-size:18px;font-weight:500;`),
      tap("Print", `height:56px;padding:0 48px;border-radius:4px;background:${CL.GREEN};color:#fff;font-size:18px;font-weight:500;`));
    box.append(ring, el("div", "font-size:34px;font-weight:600;", "Paid"), el("div", `font-size:28px;color:${CL.MUTED};`, "$8.00"), acts);
    return [box];
  }

  /**
   * A register with one order on a tablet of [size] (landscape or portrait). [d]: {kind} is "plain" (the default: the
   * mock of the first way, no brand), "square" (Square Register) or "clover" (Clover Station); {screen} is "sale", "tender"
   * (the payment screen; Square's "Select payment") or "paid" (the order closed: Square's done screen, Clover's Paid). {taps}
   * has charge(), called when Charge (Square), Pay (Clover) or the plain register's Charge is tapped, and bitcoin(), called when
   * the payment type that is the demo's target is tapped: OpenBMT on Square, Bitcoin on Clover and the plain register. The
   * other payment types do nothing.
   */
  function register(d, size = LANDSCAPE) {
    const kind = d.kind || "plain";
    if (kind === "plain") return plain(d, size);
    const w = size.width, h = size.height, wide = w > h;
    const square = kind === "square";
    const screen = d.screen || (d.paid ? "paid" : "sale");
    const taps = d.taps || {};
    const ground = square ? "#ffffff" : CL.GROUND;
    const dev = el("div", `width:${w}px;height:${h}px;position:relative;background:${ground};font-family:${FONT};color:${square ? SQ.INK : CL.INK};display:flex;flex-direction:column;`);
    dev.className = "device";
    const page = el("div", `flex:1;min-height:0;display:flex;flex-direction:column;padding:${C.STATUS_DP}px 0 ${C.NAV_DP}px;`);
    let parts;
    if (square) parts = screen === "tender" ? squarePayment(wide, taps) : screen === "paid" ? squareDone() : squareSale(wide, taps);
    else parts = screen === "tender" ? cloverPayment(wide, taps) : screen === "paid" ? cloverPaid() : cloverSale(wide, taps);
    page.append(...parts);
    dev.append(page);
    if (!square) {
      // The black strip with the Android buttons and, at the right, the cashier and a lock.
      const strip = el("div", `position:absolute;left:0;right:0;bottom:0;height:${C.NAV_DP}px;background:#000;color:#fff;display:flex;align-items:center;justify-content:flex-end;gap:8px;padding-right:16px;font-size:14px;font-weight:600;letter-spacing:0.04em;`);
      strip.append(el("span", null, "ALEX"), icon(ICONS.lock, 18));
      dev.append(strip);
    }
    dev.append(C.statusBar(true, w), C.navBar(square, w));
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

  const api = { register, tillScreen, LANDSCAPE, CHARGE, SQUARE_CHARGE, CLOVER_PAY: "Pay" };
  root.OpenBmtRegisterViews = api;
  if (typeof module !== "undefined" && module.exports) module.exports = api;
})(globalThis);
