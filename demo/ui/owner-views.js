// The owner app's drawing, ported to the browser, one function for each Kotlin function.
// Sources: apps/owner-android/src/android/OwnerUi.kt (the pages and cards of the new look), Screens.kt (home and the
// Money page) and Books.kt (the sales by day). The owner app has no list of single sales and no page for one
// sale: its record of sales is the Books page, the totals by day. A words is a model object, as in counter-views.js.
"use strict";
(function (root) {
  const U = root.OpenBmtUi || require("./ui.js");
  const {
    MATCH_PARENT, WRAP_CONTENT, Gravity, LayoutParams, LinearLayout, FrameLayout, GradientDrawable,
    setGravity, setPadding, setVisible, addView, setBackground, setClickable, css, viewEl,
    Type, Colour, Icon, Ui, RowItem, IconView,
  } = U;
  const OU_KT = "apps/owner-android/src/android/OwnerUi.kt";
  const SC_KT = "apps/owner-android/src/android/Screens.kt";
  const BK_KT = "apps/owner-android/src/android/Books.kt";
  const proto = Ui.prototype;

  /** OwnerUi.kt:94 Slot(gapDp): a container whose views are [gapDp] apart. */
  function Slot(ui, gapDp = 12) {
    const v = LinearLayout("v");
    v.show = (...views) => {
      while (v.firstChild) v.removeChild(v.firstChild);
      const gap = Math.trunc(gapDp * ui.density) / ui.density;
      views.forEach((x, i) => {
        const p = new LayoutParams(MATCH_PARENT, WRAP_CONTENT);
        if (i > 0) p.topMargin = gap;
        addView(v, x, p);
      });
    };
    return v;
  }

  /** OwnerUi.kt:48 foldViews(model): the lines of a words that notice() and hint() do not show: the quoted text and the "Details" fold. */
  proto.foldViews = function (model) {
    const out = [];
    if (model.quotedText != null) out.push(this.quoted(model.quotedLabel || "", model.quotedText));
    if (model.details && model.details.length) {
      const fold = this.text(model.details.join("\n"), 13, false, true);
      setVisible(fold, false);
      const toggle = this.text("Details (show)", 14, true, false, this.pal.info.ink);
      toggle.style.minHeight = this.px(48) + "px";
      U.setTextGravity(toggle, Gravity.CENTER_VERTICAL);
      toggle.classList.add("clickable");
      toggle.addEventListener("click", () => {
        const open = fold.classList.contains("gone");
        setVisible(fold, open);
        toggle.textContent = `Details (${open ? "hide" : "show"})`;
      });
      out.push(toggle);
      out.push(fold);
    }
    return out;
  };

  /** OwnerUi.kt:68 inline(model): a words as a message inside a page: the notice, the next step and who under it, then the folds. */
  proto.inline = function (model) {
    const col = Slot(this, 10);
    col.show(...[this.notice(model), this.hint(model), ...this.foldViews(model)]);
    return col;
  };

  /** OwnerUi.kt:45 words(w): a words in the body of an owner page. */
  proto.ownerWords = function (w) {
    return this.inline(w);
  };

  /** OwnerUi.kt:143 stack(views, gap): views one under the other, [gap] dp apart. */
  function stack(ui, views, gap = 8) {
    const s = Slot(ui, gap);
    s.show(...views);
    return s;
  }

  /** OwnerUi.kt:146 line(text, muted, mono, bold): a line of text in the body: [muted] for a small line in secondary. */
  function line(ui, text, muted = false, mono = false, bold = false) {
    return ui.text(text, muted ? 13 : 15, bold, mono, muted ? ui.pal.secondary : null);
  }

  /** OwnerUi.kt:150 para(text): a paragraph of the body: 15 sp in secondary. */
  function para(ui, text) {
    return ui.text(text, 15, false, false, ui.pal.secondary);
  }

  /**
   * OwnerUi.kt:157 screen(title, body, actions, back, sub, words, tone, logo, lead, after): a page of the new look with a bar of
   * [title], the body scrolling and the actions fixed at the bottom. [words] are the screen's own words: its heading (when it
   * is not the bar's title) and what first, the next step and who last, and the folds after them.
   */
  function screen(ui, { title, body = [], actions = [], back = null, sub = null, words = null, tone = null, logo = false, lead = null, after = null }) {
    const all = [];
    if (lead) all.push(lead);
    if (words) all.push(ui.titleBlock(words, title));
    all.push(...body);
    if (words) {
      all.push(ui.hint(words));
      all.push(...ui.foldViews(words));
    }
    if (after) all.push(after);
    return ui.page(ui.bar(title, sub, back, logo), all, actions, tone);
  }

  // ---- OwnerUi.kt:337 OwnerIcon ---------------------------------------------------------------------------------

  /** OwnerUi.kt:337 OwnerIcon(kind, colour, strokeWidth): the icons of the home groups and of a fold, as paths on a 24 by 24 grid. */
  function OwnerIconView(kind, colour, strokeWidth = 2) {
    const c = css(colour);
    const stroke = `fill="none" stroke="${c}" stroke-width="${strokeWidth}" stroke-linecap="round" stroke-linejoin="round"`;
    let body;
    switch (kind) {
      case "Money": body = `<rect ${stroke} x="3" y="7" width="18" height="12"/><path ${stroke} d="M3 11L21 11M16 15L18 15"/>`; break;
      case "Devices": body = `<rect ${stroke} x="4" y="3" width="16" height="18" rx="2"/><path ${stroke} d="M8 7L16 7M8 11L16 11M8 15L13 15"/>`; break;
      case "Bell": body = `<path ${stroke} d="M6 8A6 6 0 0 1 18 8C18 15 21 17 21 17L3 17C3 17 6 15 6 8"/><path ${stroke} d="M10 21L14 21"/>`; break;
      case "Up": body = `<path ${stroke} d="M6 15L12 9L18 15"/>`; break;
      default: body = `<path ${stroke} d="M6 9L12 15L18 9"/>`;
    }
    const v = viewEl("div", "");
    v.innerHTML = `<svg viewBox="0 0 24 24" width="100%" height="100%" style="display:block;overflow:visible">${body}</svg>`;
    return v;
  }

  // ---- cards -----------------------------------------------------------------------------------------------

  /** OwnerUi.kt:398 card(radiusDp, fill, line): a card with a one-pixel outline, clipped to its corners. */
  proto.ownerCard = function (radiusDp = 16, fill = null, lineColour = null) {
    const v = LinearLayout("v");
    const g = new GradientDrawable();
    g.setColor(fill == null ? this.pal.surface : fill);
    g.cornerRadius = radiusDp;
    g.setStroke(this.pxf(1), lineColour == null ? this.pal.divider : lineColour);
    setBackground(v, g);
    v.style.overflow = "hidden";
    return v;
  };

  /** OwnerUi.kt:410 divider(colour): a line of one pixel in [colour]. */
  proto.ownerDivider = function (colour = null) {
    const v = U.View();
    v.style.backgroundColor = css(colour == null ? this.pal.divider : colour);
    v.style.height = this.pxf(1) + "px";
    v.style.alignSelf = "stretch";
    v.style.flex = "0 0 auto";
    return v;
  };

  /** OwnerUi.kt:415 tap(view, onClick): a view that ripples when pressed and calls [onClick]. */
  proto.tap = function (view, onClick) {
    const tapColour = this.night ? "rgba(255,255,255,0.133)" : "rgba(0,0,0,0.078)";
    setClickable(view, true, onClick ? () => onClick() : null);
    U.setRippleOnly(view, tapColour, 0);
  };

  /** OwnerUi.kt:423 icon(drawable, dp): an icon view of [dp] by [dp]. */
  proto.ownerIcon = function (iconView, dp) {
    iconView.style.width = this.px(dp) + "px";
    iconView.style.height = this.px(dp) + "px";
    iconView.style.flex = "none";
    return iconView;
  };

  /** OwnerUi.kt:433 statusPill(model): the pill of the network status: its dot and heading in the tone's ink on its tint. */
  proto.statusPill = function (model) {
    const tone = this.pal.of(model.colour);
    const pill = LinearLayout("h");
    setGravity(pill, Gravity.CENTER_VERTICAL);
    setPadding(pill, this.px(8), this.px(5), this.px(10), this.px(5));
    pill.style.backgroundColor = css(tone.tint);
    pill.style.borderRadius = "999px";
    pill.style.overflow = "hidden";
    const dp = new LayoutParams(this.px(8), this.px(8));
    dp.rightMargin = this.px(7);
    const g = new GradientDrawable();
    g.shape = GradientDrawable.OVAL;
    g.setColor(tone.ink);
    const dot = U.View();
    setBackground(dot, g);
    dot.style.flex = "none";
    addView(pill, dot, dp);
    addView(pill, this.text(model.heading, 13, false, false, tone.ink, true));
    return pill;
  };

  /** OwnerUi.kt:461 heroCard(status, balance, extra, from): the network status as a pill (with its what and next under it when it is not good), the balance, and [extra] under the amount. */
  proto.heroCard = function (status, balance, extra) {
    const card = this.ownerCard(20);
    setPadding(card, this.px(20), this.px(18), this.px(20), this.px(18));
    addView(card, this.statusPill(status), new LayoutParams(WRAP_CONTENT, WRAP_CONTENT));
    if (status.colour !== Colour.Green) {
      const wp = new LayoutParams(MATCH_PARENT, WRAP_CONTENT); wp.topMargin = this.px(10);
      addView(card, this.text(status.what, 15, false, false, this.pal.secondary), wp);
      const np = new LayoutParams(MATCH_PARENT, WRAP_CONTENT); np.topMargin = this.px(4);
      addView(card, this.text(status.next, 15, false, false, null, true), np);
    }
    const lp = new LayoutParams(MATCH_PARENT, WRAP_CONTENT); lp.topMargin = this.px(10);
    addView(card, this.text(balance.lead, 14, false, false, this.pal.secondary), lp);
    const amount = this.text(balance.amount, 38, true);
    amount.style.fontFeatureSettings = '"tnum"';
    amount.style.letterSpacing = "-0.015em";
    addView(card, amount);
    const checked = this.text(balance.checked, 13, false, false, this.pal.secondary);
    checked.style.fontFeatureSettings = '"tnum"';
    addView(card, checked);
    for (const v of extra) {
      const p = new LayoutParams(MATCH_PARENT, WRAP_CONTENT); p.topMargin = this.px(12);
      addView(card, v, p);
    }
    return card;
  };

  /** OwnerUi.kt:488 waitingCard(rows): the warm card of what waits: a heading, and a row for each of [rows] with its count in a black badge. Null when no row. */
  proto.waitingCard = function (rows, title) {
    if (!rows.length) return null;
    const warn = this.pal.warn;
    const card = this.ownerCard(16, warn.tint, warn.band);
    const l = this.label(title);
    l.style.color = css(warn.ink);
    setPadding(l, this.px(16), this.px(12), this.px(16), this.px(8));
    addView(card, l);
    for (const r of rows) {
      addView(card, this.ownerDivider(warn.band));
      const row = LinearLayout("h");
      setGravity(row, Gravity.CENTER_VERTICAL);
      row.style.minHeight = this.px(52) + "px";
      setPadding(row, this.px(16), this.px(6), this.px(12), this.px(6));
      addView(row, this.text(r.title, 16, false, false, null, true), new LayoutParams(0, WRAP_CONTENT, 1));
      const b = this.text(String(r.count), 14, true, false, this.palette.WHITE);
      U.setTextGravity(b, Gravity.CENTER);
      b.style.minWidth = this.px(26) + "px";
      setPadding(b, this.px(8), this.px(2), this.px(8), this.px(2));
      const g = new GradientDrawable();
      g.cornerRadius = this.px(13);
      g.setColor(this.night ? this.pal.chip : "#111317");
      setBackground(b, g);
      const bp = new LayoutParams(WRAP_CONTENT, WRAP_CONTENT); bp.leftMargin = this.px(8);
      addView(row, b, bp);
      const cp = new LayoutParams(this.px(18), this.px(18)); cp.leftMargin = this.px(8);
      addView(row, this.ownerIcon(IconView(Icon.Chevron, this.pal.secondary, 2), 18), cp);
      this.tap(row, r.onClick);
      addView(card, row);
    }
    return card;
  };

  /** OwnerUi.kt:527 GroupRow(icon, gear, title, sub, onClick). */
  function GroupRow(icon, gear, title, sub, onClick = null) { return { icon, gear, title, sub, onClick }; }

  /** OwnerUi.kt:530 groupCard(items): the group card of home: one surface card of rows, each with an icon tile. */
  proto.groupCard = function (items) {
    const card = this.ownerCard(16);
    items.forEach((item, i) => {
      if (i > 0) addView(card, this.ownerDivider());
      const row = LinearLayout("h");
      row.dataset.role = "group-row";
      setGravity(row, Gravity.CENTER_VERTICAL);
      row.style.minHeight = this.px(64) + "px";
      setPadding(row, this.px(14), this.px(10), this.px(12), this.px(10));
      const tile = FrameLayout();
      const g = new GradientDrawable();
      g.cornerRadius = this.px(12);
      g.setColor(this.pal.background);
      setBackground(tile, g);
      const glyph = item.gear ? IconView(Icon.Gear, this.pal.text, 2) : OwnerIconView(item.icon || "Money", this.pal.text);
      const gp = new LayoutParams(this.px(20), this.px(20));
      gp.gravity = Gravity.CENTER;
      addView(tile, this.ownerIcon(glyph, 20), gp);
      const tp = new LayoutParams(this.px(40), this.px(40));
      tp.rightMargin = this.px(14);
      addView(row, tile, tp);
      const col = LinearLayout("v");
      addView(col, this.text(item.title, 16, false, false, null, true));
      addView(col, this.text(item.sub, 13, false, false, this.pal.secondary));
      addView(row, col, new LayoutParams(0, WRAP_CONTENT, 1));
      const cp = new LayoutParams(this.px(18), this.px(18));
      cp.leftMargin = this.px(8);
      addView(row, this.ownerIcon(IconView(Icon.Chevron, this.pal.secondary, 2), 18), cp);
      this.tap(row, item.onClick);
      addView(card, row);
    });
    return card;
  };

  // ---- the screens -----------------------------------------------------------------------------------------

  /**
   * Screens.kt:52 showHome(): the owner's home: the bar with the logo, the content (the hero card with the status and the
   * balance, the waiting card when something waits, the four groups) and the footer with Spend and the other buttons.
   * [d] holds the words and values of the moment.
   */
  function showHome(ui, d) {
    const content = Slot(ui, 14);
    const footer = [ui.primary(d.words.spend, Colour.Blue, () => {})];
    for (const b of d.words.footer || []) footer.push(ui.button(b, () => {}));
    const page = screen(ui, { title: "Owner", body: [content], actions: footer, logo: true });
    const views = [];
    views.push(ui.heroCard(d.status, d.balance, (d.extra || []).map((m) => ui.ownerWords(m))));
    const rows = (d.waiting || []).map((r) => ({ title: r.title, count: r.count, onClick: () => {} }));
    const waiting = ui.waitingCard(rows, d.words.waitingTitle);
    if (waiting) views.push(waiting);
    views.push(ui.groupCard(d.groups.map((g) => GroupRow(g.icon, g.gear, g.title, g.sub, () => {}))));
    content.show(...views);
    return page;
  }

  /**
   * Controller.kt:638 showFirstStart(message): the owner app's first screen: the title block and the next step of the
   * words, with Make a new shop wallet and Use setup text in the footer. [d] has the words and the two labels.
   */
  function showFirstStart(ui, d) {
    return screen(ui, {
      title: "Owner", body: [], logo: true, words: d.words,
      actions: [ui.primary(d.make, Colour.Blue, () => {}), ui.button(d.paste, () => {})],
    });
  }

  /** Screens.kt:131 groupPage(title, rows): a page of the rows of one group of home. */
  function groupPage(ui, title, rows) {
    return screen(ui, { title, body: [ui.rows(rows)], back: () => {} });
  }

  /** Screens.kt:135 showMoney(): the Money page: the entries by title, and the Books row with its sub. [d] has the titles. */
  function showMoney(ui, d) {
    const rows = d.titles.map((t) => RowItem(t, null, null, null, false, () => {}));
    rows.push(RowItem(d.books.title, d.books.sub, null, null, false, () => {}));
    return groupPage(ui, d.title, rows);
  }

  /** Books.kt:13 showBooks(): the days of the sales and refunds the node holds, newest first, and the two files for a bookkeeper. */
  function showBooks(ui, d) {
    const views = [];
    if (d.empty) views.push(ui.ownerWords(d.empty));
    if (d.rows.length) {
      views.push(ui.rows(d.rows.map((r) => RowItem(r.date, r.totals))));
      views.push(line(ui, d.note, true));
    }
    const footer = [];
    if (d.accounting) footer.push(ui.primary(d.accounting, Colour.Blue, () => {}));
    if (d.ledger) footer.push(ui.button(d.ledger, () => {}));
    return screen(ui, { title: d.title, body: views, actions: footer, back: () => {} });
  }

  const api = {
    OU_KT, SC_KT, BK_KT, Slot, stack, line, para, screen, OwnerIconView, GroupRow, showHome, showFirstStart, groupPage, showMoney, showBooks,
  };
  root.OpenBmtOwnerViews = api;
  if (typeof module !== "undefined" && module.exports) module.exports = api;
})(typeof globalThis !== "undefined" ? globalThis : this);
