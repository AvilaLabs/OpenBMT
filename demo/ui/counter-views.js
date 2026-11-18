// The counter app's drawing, ported to the browser, one function for each Kotlin function.
// Sources: apps/counter-android/src/android/CounterViews.kt, CounterUi.kt, and the screens of Controller.kt that the
// shop's sale needs (home, new sale, the sale, sales, a sale's details, today). The functions are added to Ui, as
// the Kotlin ones are extension functions of Ui. A words is a model object: { colour, heading, what, next, who,
// quotedLabel, quotedText, details } (WordsModel); a SayWords adds its tone ("info", "good", "stop", "ended").
"use strict";
(function (root) {
  const U = root.OpenBmtUi || require("./ui.js");
  const {
    MATCH_PARENT, WRAP_CONTENT, Gravity, LayoutParams, LinearLayout, FrameLayout, View, GradientDrawable,
    setGravity, setTextGravity, setPadding, setVisible, addView, setBackground, setClickable, css, viewEl,
    Space, Type, Colour, Icon, Ui, RowItem, IconView,
  } = U;
  const CV_KT = "apps/counter-android/src/android/CounterViews.kt";
  const CU_KT = "apps/counter-android/src/android/CounterUi.kt";
  const CT_KT = "apps/counter-android/src/android/Controller.kt";
  const TONES = { info: "Blue", good: "Green", stop: "Red", ended: "Neutral" };
  const proto = Ui.prototype;

  /** CounterViews.kt:41 outcomeBand(colour): paid is green, waiting blue, stop red, ended neutral. */
  function outcomeBand(ui, colour) {
    return { Green: ui.palette.paid, Blue: ui.palette.waiting, Red: ui.palette.stopped, Neutral: ui.palette.ended }[colour];
  }

  /** CounterViews.kt:49 roundedFill(fill, radius, strokeDp, stroke): the surface fill, a corner of [radius] dp. */
  proto.roundedFill = function (fill, radius, strokeDp = 0, stroke = 0) {
    const g = new GradientDrawable();
    g.setColor(fill);
    g.cornerRadius = radius;
    if (strokeDp > 0) g.setStroke(this.pxf(strokeDp), stroke);
    return g;
  };

  // CounterViews.kt:56 rippleOver(bg, radius) is the function Ui.kt:443 rippleOver under the same name (Ui.rippleOver).

  /** CounterViews.kt:66 surfaceCard(): a surface card, corner 16, that holds a column of views. */
  proto.surfaceCard = function () {
    const v = LinearLayout("v");
    setBackground(v, this.roundedFill(this.pal.surface, 16));
    v.style.overflow = "hidden";
    return v;
  };

  /** CounterViews.kt:74 line(): a 1 dp line in the divider colour. */
  proto.line = function () {
    return this.divider();
  };

  /** CounterViews.kt:80 dot(colour, dp): a round dot of [dp] in [colour]. */
  // The member Ui.kt:908 dot is the same view; the extension is the one the counter calls.

  /** CounterViews.kt:90 whoChip(model, onDark): the pill that says who does the next step. */
  proto.whoChip = function (model, onDark = false) {
    const t = this.text(model.who, 13, false, false, onDark ? this.palette.WHITE : this.pal.secondary, true);
    setBackground(t, this.roundedFill(onDark ? 0x2EFFFFFF : this.pal.chip, 999));
    setPadding(t, this.px(10), this.px(5), this.px(10), this.px(5));
    return t;
  };

  /** CounterViews.kt:101 matchWide(): a row's width: the whole of its parent. */
  function matchWide() {
    return new LayoutParams(MATCH_PARENT, WRAP_CONTENT);
  }

  /** CounterViews.kt:104 Glyph. */
  const Glyph = { Plus: "Plus", List: "List", Calendar: "Calendar", Lock: "Lock" };

  /** CounterViews.kt:107 GlyphDrawable(glyph, colour, width): strokes on a 24 by 24 grid, as an SVG. */
  function GlyphView(glyph, colour, width = 2) {
    const c = css(colour);
    const stroke = `fill="none" stroke="${c}" stroke-width="${width}" stroke-linecap="round" stroke-linejoin="round"`;
    let body = "";
    if (glyph === Glyph.Plus) {
      body = `<path ${stroke} d="M12 5L12 19M5 12L19 12"/>`;
    } else if (glyph === Glyph.List) {
      for (const y of [6, 12, 18]) body += `<path ${stroke} d="M8 ${y}L20 ${y}"/><circle cx="4" cy="${y}" r="${width / 2}" fill="${c}"/>`;
    } else if (glyph === Glyph.Calendar) {
      body = `<rect ${stroke} x="4" y="5" width="16" height="15" rx="2"/><path ${stroke} d="M4 10L20 10M9 3L9 7M15 3L15 7"/>`;
    } else {
      body = `<rect ${stroke} x="5" y="11" width="14" height="10" rx="2.5"/><path ${stroke} d="M8 11L8 8A4 4 0 0 1 16 8L16 11"/>`;
    }
    const v = viewEl("div", "");
    v.innerHTML = `<svg viewBox="0 0 24 24" width="100%" height="100%" style="display:block;overflow:visible">${body}</svg>`;
    return v;
  }

  /** CounterViews.kt:167 glyph(glyph, colour, dp, width): a glyph as a view of [dp] by [dp]. */
  proto.glyph = function (glyph, colour, dp, width = 2) {
    const v = GlyphView(glyph, colour, width);
    v.style.width = this.px(dp) + "px";
    v.style.height = this.px(dp) + "px";
    v.style.flex = "none";
    return v;
  };

  /**
   * CounterViews.kt:177 OutcomeMark(colour, band): the mark of an outcome screen, 112 dp: a white disc with a tick
   * for good, a clock for waiting and a dash for ended, or a white octagon with a cross for stop, in the band's colour.
   */
  function OutcomeMark(ui, colour, band) {
    const w = css(ui.palette.WHITE);
    const b = css(band);
    const s = (width) => `fill="none" stroke="${b}" stroke-width="${width}" stroke-linecap="round" stroke-linejoin="round"`;
    let body;
    if (colour === Colour.Green) {
      body = `<circle cx="56" cy="56" r="56" fill="${w}"/><path ${s(10)} d="M32 57L48 73L80 39"/>`;
    } else if (colour === Colour.Blue) {
      body = `<circle cx="56" cy="56" r="56" fill="${w}"/><circle ${s(8)} cx="56" cy="56" r="30"/><path ${s(8)} d="M56 40L56 57L67 65"/>`;
    } else if (colour === Colour.Red) {
      body = `<path fill="${w}" d="M33 4L79 4L108 33L108 79L79 108L33 108L4 79L4 33Z"/><path ${s(10)} d="M40 40L72 72M72 40L40 72"/>`;
    } else {
      body = `<circle cx="56" cy="56" r="56" fill="${w}"/><path ${s(10)} d="M34 56L78 56"/>`;
    }
    const v = viewEl("div", "");
    v.innerHTML = `<svg viewBox="0 0 112 112" width="100%" height="100%" style="display:block;overflow:visible">${body}</svg>`;
    v.style.width = ui.px(112) + "px";
    v.style.height = ui.px(112) + "px";
    v.style.flex = "none";
    return v;
  }

  /**
   * CounterViews.kt:236 WaitRing: the ring of a wait that knows its end, 72 dp, in white on the band: it empties over the
   * whole wait with the seconds left in it. set(secs, total, label) puts the number in and sweeps the ring to the next
   * second over one second; the same second again changes nothing. With reduced motion the CSS keeps the ring still
   * and full and only the number changes.
   */
  function WaitRing(ui, ink = ui.palette.WHITE, size = 72) {
    const w = css(ink);
    const r = 31;
    const whole = 2 * Math.PI * r;
    const v = viewEl("div", "wait-ring");
    v.innerHTML = `<svg viewBox="0 0 72 72" width="100%" height="100%" style="display:block;overflow:visible">` +
      `<circle cx="36" cy="36" r="${r}" fill="none" stroke="${w}" stroke-opacity="0.25" stroke-width="5"/>` +
      `<circle class="sweep" cx="36" cy="36" r="${r}" fill="none" stroke="${w}" stroke-width="5" stroke-linecap="round" ` +
      `stroke-dasharray="${whole}" stroke-dashoffset="0" transform="rotate(-90 36 36)"/>` +
      `<text x="36" y="36" text-anchor="middle" dominant-baseline="central" font-size="22" font-weight="700" fill="${w}"></text></svg>`;
    v.style.width = ui.px(size) + "px";
    v.style.height = ui.px(size) + "px";
    v.style.flex = "none";
    const sweep = v.querySelector(".sweep");
    const label = v.querySelector("text");
    let shown = -1;
    v.set = (secs, total, text) => {
      if (secs === shown) return;
      shown = secs;
      label.textContent = text;
      if (total <= 0) return;
      sweep.style.transition = "none";
      sweep.style.strokeDashoffset = String(whole * (1 - secs / total));
      sweep.getBoundingClientRect();
      sweep.style.transition = "stroke-dashoffset 1s linear";
      sweep.style.strokeDashoffset = String(whole * (1 - (secs - 1) / total));
    };
    return v;
  }

  /**
   * CounterViews.kt:310 PulseMark: the small mark beside the time of a wait, 10 dp. pulsing(on) makes it pulse, which it
   * does only while the check is healthy; with reduced motion the CSS keeps it still.
   */
  function PulseMark(ui) {
    const v = viewEl("div", "pulse-mark");
    v.style.width = ui.px(10) + "px";
    v.style.height = ui.px(10) + "px";
    v.style.borderRadius = "50%";
    v.style.background = css(ui.palette.WHITE);
    v.style.flex = "none";
    v.pulsing = (on) => v.classList.toggle("pulsing", !!on);
    return v;
  }

  /** CounterViews.kt:351 Centered: a column that takes the height that is left in the body of a page and centres what it holds. */
  function Centered() {
    const v = LinearLayout("v");
    setGravity(v, Gravity.CENTER);
    v.isFills = true;
    return v;
  }

  /** CounterViews.kt:359 stateChip(model): the state of a sale as the chip of its bar: a dot and the heading in the tone's ink, on the tone's tint. */
  proto.stateChip = function (model) {
    const tone = this.pal.of(model.colour);
    const v = LinearLayout("h");
    setGravity(v, Gravity.CENTER_VERTICAL);
    setBackground(v, this.roundedFill(tone.tint, 999));
    setPadding(v, this.px(12), this.px(6), this.px(12), this.px(6));
    const dp = new LayoutParams(this.px(8), this.px(8));
    dp.rightMargin = this.px(8);
    addView(v, this.dot(tone.ink, 8), dp);
    const t = this.text(model.heading, 14, false, false, tone.ink, true);
    t.classList.add("single");
    addView(v, t);
    return v;
  };

  /** CounterViews.kt:375 addToBar(bar, chip, ui): puts [chip] at the right end of a bar made by bar(). */
  function addToBar(bar, chip, ui) {
    const row = bar.row;
    if (!row) return;
    const p = new LayoutParams(WRAP_CONTENT, WRAP_CONTENT);
    p.leftMargin = ui.px(12);
    addView(row, chip, p);
  }

  /** CounterViews.kt:381 barButton(label, onClick): a small outlined button for a bar, 36 dp high. */
  proto.barButton = function (label, onClick) {
    const b = this.secondary(label, onClick);
    b.style.minHeight = this.px(36) + "px";
    b.style.fontSize = "14px";
    setPadding(b, this.px(14), 0, this.px(14), 0);
    b.style.fontWeight = "700";
    return b;
  };

  /** CounterViews.kt:390 onBand(label, filled, band, onClick): a button on an outcome band. */
  proto.onBand = function (label, filled, band, onClick) {
    const b = filled ? this.primary(label, Colour.Blue, onClick) : this.secondary(label, onClick);
    const radius = 14;
    const fill = filled ? this.roundedFill(this.palette.WHITE, radius) : this.roundedFill("transparent", radius, 1.5, 0xB3FFFFFF);
    b.style.color = css(filled ? band : this.palette.WHITE);
    b.style.border = "";
    setBackground(b, this.rippleOver(fill, radius));
    return b;
  };

  /**
   * CounterViews.kt:408 paintBand(page, bar, band, window): colours a page made by page() with a bar of
   * onBlack = true as the whole of an outcome screen: the band behind the status bar, the bar and the footer, no
   * divider, the title in the band's second-line colour; the navigation bar's icons are set light.
   */
  proto.paintBand = function (page, bar, band) {
    page.style.backgroundColor = css(band.band);
    bar.style.backgroundColor = css(band.band);
    if (page.children.length > 2) {
      const footer = page.children[2];
      footer.style.backgroundColor = "transparent";
      footer.children[0].classList.add("gone");
    }
    const names = bar.row && bar.row.children[0];
    const title = names && names.children[0];
    if (title) title.style.color = css(band.sub);
    // lightNavigation (CounterViews.kt:313): the navigation bar's icons light, for a screen dark to the bottom edge.
    this.window.navLight = false;
  };

  /** CounterViews.kt:444 HomeLine(key, words, extra, retry). */
  function HomeLine(key, words, extra = null, retry = null) { return { key, words, extra, retry }; }

  /** CounterViews.kt:450 healthCard(words): one surface card with the heading and a tone dot, the what under it, and, when the tone is not good, who does the next step and the next line. */
  proto.healthCard = function (words) {
    const model = words;
    const tone = this.pal.of(model.colour);
    const card = this.surfaceCard();
    const row = LinearLayout("h");
    setPadding(row, this.px(16), this.px(16), this.px(16), this.px(16));
    const dp = new LayoutParams(this.px(10), this.px(10));
    dp.rightMargin = this.px(12);
    dp.topMargin = this.px(6);
    addView(row, this.dot(tone.band, 10), dp);
    const col = LinearLayout("v");
    addView(col, this.text(model.heading, 15, true));
    const wp = matchWide(); wp.topMargin = this.px(4);
    addView(col, this.text(model.what, 14, false, false, this.pal.secondary), wp);
    if (words.tone !== "good") {
      const np = matchWide(); np.topMargin = this.px(8);
      addView(col, this.text(model.next, 14, false, false, null, true), np);
      const hp = matchWide(); hp.topMargin = this.px(4);
      addView(col, this.text(model.who, 13, false, false, this.pal.secondary, true), hp);
    }
    addView(row, col, new LayoutParams(0, WRAP_CONTENT, 1));
    addView(card, row, matchWide());
    return card;
  };

  /** CounterViews.kt:475 starterCard(heading, lines, seen): the card of one payment from the starter wallet that nobody at the counter has seen. [seenLabel] is its button's word. */
  proto.starterCard = function (heading, lines, seen, seenLabel) {
    const card = this.surfaceCard();
    const col = LinearLayout("v");
    setPadding(col, this.px(16), this.px(16), this.px(16), this.px(16));
    addView(col, this.text(heading, 15, true));
    for (const l of lines) {
      const lp = matchWide(); lp.topMargin = this.px(10);
      addView(col, this.text(l.label, 13, false, false, this.pal.secondary, true), lp);
      if (l.value) {
        const vp = matchWide(); vp.topMargin = this.px(2);
        const t = this.text(l.value, 20, true);
        t.style.fontFeatureSettings = '"tnum"';
        addView(col, t, vp);
      }
    }
    const bp = matchWide(); bp.topMargin = this.px(14);
    addView(col, this.primary(seenLabel, Colour.Blue, seen), bp);
    addView(card, col, matchWide());
    return card;
  };

  /**
   * CounterViews.kt:496 homeLines(lines, open): the other lines of home in one surface card: each shows its heading
   * with a tone dot. A line that is good shows only its heading, and a tap shows its what and its next; any other
   * line shows its words at once, with its "Try again" button under it. [open] holds the keys of the lines a tap has opened.
   */
  proto.homeLines = function (lines, open) {
    const card = this.surfaceCard();
    lines.forEach((l, i) => {
      if (i > 0) addView(card, this.line());
      const model = l.words;
      const tone = this.pal.of(model.colour);
      const good = l.words.tone === "good";
      const box = LinearLayout("v");
      const row = LinearLayout("h");
      setPadding(row, this.px(16), this.px(14), this.px(16), this.px(14));
      const dp = new LayoutParams(this.px(10), this.px(10));
      dp.rightMargin = this.px(12);
      dp.topMargin = this.px(6);
      addView(row, this.dot(tone.band, 10), dp);
      const col = LinearLayout("v");
      addView(col, this.text(model.heading, 15, true));
      const detail = LinearLayout("v");
      const wp = matchWide(); wp.topMargin = this.px(4);
      addView(detail, this.text(model.what, 14, false, false, this.pal.secondary), wp);
      const np = matchWide(); np.topMargin = this.px(8);
      addView(detail, this.text(model.next, 14, false, false, null, true), np);
      if (!good) {
        const hp = matchWide(); hp.topMargin = this.px(4);
        addView(detail, this.text(model.who, 13, false, false, this.pal.secondary, true), hp);
      }
      if (l.extra) {
        const ep = matchWide(); ep.topMargin = this.px(8);
        addView(detail, this.text(l.extra, 13, false, true, this.pal.secondary), ep);
      }
      setVisible(detail, !good || open.has(l.key));
      addView(col, detail, matchWide());
      addView(row, col, new LayoutParams(0, WRAP_CONTENT, 1));
      if (good) {
        setClickable(row, true, () => {
          const show = detail.classList.contains("gone");
          if (show) open.add(l.key); else open.delete(l.key);
          setVisible(detail, show);
        });
        setBackground(row, this.rippleOver(null, 0));
      }
      addView(box, row, matchWide());
      if (l.retry && !good) {
        const p = matchWide();
        p.setMargins(this.px(16), 0, this.px(16), this.px(14));
        addView(box, this.secondary("Try again", l.retry), p);
      }
      addView(card, box, matchWide());
    });
    return card;
  };

  /** CounterViews.kt:547 tile(label, glyph, brand, onClick): a tile of home: [label] at the bottom and [glyph] at the top. [brand] makes it the brand's fill. */
  proto.tile = function (label, glyph, brand, onClick) {
    const ink = brand ? this.palette.ON_BRAND : this.pal.text;
    const fill = brand ? this.roundedFill(this.palette.BRAND, 18) : this.roundedFill(this.pal.surface, 18, 1, this.pal.divider);
    const v = FrameLayout();
    setBackground(v, brand ? this.fillRipple(fill, 18) : this.rippleOver(fill, 18));
    setPadding(v, this.px(16), this.px(16), this.px(16), this.px(16));
    setClickable(v, true, onClick ? () => onClick() : null);
    v.setAttribute("aria-label", label);
    const gp = new LayoutParams(this.px(brand ? 28 : 24), this.px(brand ? 28 : 24));
    gp.gravity = Gravity.TOP | Gravity.START;
    addView(v, this.glyph(glyph, ink, brand ? 28 : 24, brand ? 2.2 : 2), gp);
    const tp = new LayoutParams(WRAP_CONTENT, WRAP_CONTENT);
    tp.gravity = Gravity.BOTTOM | Gravity.START;
    addView(v, this.text(label, brand ? 20 : 18, brand, false, ink, !brand), tp);
    return v;
  };

  /** CounterViews.kt:567 fillRipple(fill, radius): a ripple of 0x40000000 over a filled drawable. */
  proto.fillRipple = function (fill, radius) {
    return new U.RippleDrawable("rgba(0,0,0,0.251)", fill, radius);
  };

  /** CounterViews.kt:579 amountPad(press): the keys of the amount of a new sale: 1 to 9, then C, 0 and the delete key, as 64 dp round surface keys. [press] is given the key's face. */
  proto.amountPad = function (press) {
    const faces = ["1", "2", "3", "4", "5", "6", "7", "8", "9", "C", "0", "⌫"];
    const grid = LinearLayout("v");
    setGravity(grid, Gravity.CENTER_HORIZONTAL);
    for (let r = 0; r < 4; r++) {
      const row = LinearLayout("h");
      setGravity(row, Gravity.CENTER);
      for (let c = 0; c < 3; c++) {
        const face = faces[r * 3 + c];
        const key = this.text(face, 26, false, false, this.pal.text, true);
        key.style.display = "flex";
        key.style.alignItems = "center";
        key.style.justifyContent = "center";
        const bg = new GradientDrawable();
        bg.shape = GradientDrawable.OVAL;
        bg.setColor(this.pal.surface);
        bg.setStroke(this.pxf(1), this.pal.divider);
        setBackground(key, this.rippleOver(bg, 32));
        setClickable(key, true, () => press(face));
        const p = new LayoutParams(this.px(64), this.px(64));
        p.setMargins(this.px(10), this.px(5), this.px(10), this.px(5));
        key.style.flex = "none";
        addView(row, key, p);
      }
      addView(grid, row, new LayoutParams(WRAP_CONTENT, WRAP_CONTENT));
    }
    return grid;
  };

  /** CounterViews.kt:621 needRow(colour, title, sub, link, count, amount, onClick): a row of Today that needs a look: a tone dot, the title, and what sits under it, with the number of a total on the right. */
  proto.needRow = function (colour, title, sub, link, count, amount, onClick) {
    const row = LinearLayout("h");
    setGravity(row, Gravity.CENTER_VERTICAL);
    row.style.minHeight = this.px(56) + "px";
    setPadding(row, this.px(16), this.px(10), this.px(12), this.px(10));
    const dp = new LayoutParams(this.px(10), this.px(10));
    dp.rightMargin = this.px(12);
    addView(row, this.dot(colour, 10), dp);
    const col = LinearLayout("v");
    addView(col, this.text(title, 16, false, false, null, true));
    if (sub != null) addView(col, this.text(sub, 13, false, false, this.pal.secondary));
    if (link != null) addView(col, link);
    addView(row, col, new LayoutParams(0, WRAP_CONTENT, 1));
    const right = LinearLayout("v");
    setGravity(right, Gravity.END);
    const c = this.text(count, 16, true);
    c.style.fontFeatureSettings = '"tnum"';
    setTextGravity(c, Gravity.END);
    addView(right, c);
    if (amount != null) {
      const a = this.text(amount, 13, false, false, this.pal.secondary);
      a.style.fontFeatureSettings = '"tnum"';
      setTextGravity(a, Gravity.END);
      addView(right, a);
    }
    const rp = new LayoutParams(WRAP_CONTENT, WRAP_CONTENT);
    rp.leftMargin = this.px(8);
    addView(row, right, rp);
    if (onClick) {
      setClickable(row, true, () => onClick());
      setBackground(row, this.rippleOver(null, 0));
    }
    return row;
  };

  /** CounterViews.kt:667 FillsFrame: a column of the body of a page that takes the height that is left, for the scanner. */
  function FillsFrame() {
    const v = FrameLayout();
    v.isFills = true;
    return v;
  }

  /** CounterViews.kt:673 CoverLayout(aspectW, aspectH): the camera's surface, scaled to cover the whole of the layout and centred; what runs over the edges is cut. */
  function CoverLayout(aspectW, aspectH) {
    const v = FrameLayout();
    v.style.overflow = "hidden";
    v.style.position = "relative";
    v.cover = (child) => {
      child.style.position = "absolute";
      child.style.left = "50%";
      child.style.top = "50%";
      child.style.transform = "translate(-50%, -50%)";
      child.style.aspectRatio = `${aspectW} / ${aspectH}`;
      child.style.minWidth = "100%";
      child.style.minHeight = "100%";
      v.appendChild(child);
    };
    return v;
  }

  // ---- CounterUi.kt ------------------------------------------------------------------------------------------

  /** CounterUi.kt:18 words(w): one words on screen. */
  proto.wordsOf = function (w) {
    return this.words(w);
  };

  /** CounterUi.kt:25 pinField(context, hint): a field for a PIN: digits only, shown as dots. */
  function pinField(ui, hint) {
    const f = ui.field(hint);
    f.type = "password";
    f.inputMode = "numeric";
    return f;
  }

  /** CounterUi.kt:34 amountField(context, hint): a field for a number with an optional point. */
  function amountField(ui, hint) {
    const f = ui.field(hint);
    f.inputMode = "decimal";
    return f;
  }

  /** CounterUi.kt:46 Slot: a container whose content is replaced as a screen updates; with nothing in it, it takes no room. */
  function Slot() {
    const v = LinearLayout("v");
    setVisible(v, false);
    v.show = (...views) => {
      while (v.firstChild) v.removeChild(v.firstChild);
      for (const x of views) addView(v, x, x.layoutParams);
      setVisible(v, views.length > 0);
    };
    return v;
  }

  /**
   * CounterUi.kt:65 QrView(payment, maxHeightPx): the payment code drawn from its modules: black squares on white, a
   * quiet zone of 4 squares on every side, whole device pixels for each square (QrLayout.box), as large as fits.
   * [modules] is an array of rows of booleans.
   */
  function QrView(ui, modules, side) {
    const v = viewEl("div", "qr");
    const n = modules.length;
    const dev = ui.density;
    const wdev = Math.trunc(side * dev);
    const quiet = 4;
    const cell = Math.trunc(wdev / (n + 2 * quiet));
    const left = Math.trunc((wdev - cell * (n + 2 * quiet)) / 2);
    let d = "";
    for (let y = 0; y < n; y++) {
      for (let x = 0; x < n; x++) {
        if (modules[y][x]) d += `M${left + (quiet + x) * cell} ${left + (quiet + y) * cell}h${cell}v${cell}h${-cell}z`;
      }
    }
    v.style.width = side + "px";
    v.style.height = side + "px";
    v.style.background = "#ffffff";
    v.setAttribute("aria-label", "Payment code for the customer to scan");
    v.innerHTML = `<svg viewBox="0 0 ${wdev} ${wdev}" width="100%" height="100%" shape-rendering="crispEdges" style="display:block"><path fill="#000000" d="${d}"/></svg>`;
    return v;
  }

  // ---- the screens of Controller.kt ----------------------------------------------------------------------------

  /**
   * The taps of a screen: [d].taps names what each control does ({newSale, open, done, ...}); a control that is not
   * named does nothing, as in the gallery. The Android controller sets each of these in the same place, as the
   * lambda of the control.
   */
  function taps(d) {
    const given = d.taps || {};
    return (name) => given[name] || (() => {});
  }

  /** Controller.kt:760 left(view): [view] at the left edge of the body, as wide as it is. */
  function left(view) {
    const v = LinearLayout("h");
    addView(v, view, new LayoutParams(WRAP_CONTENT, WRAP_CONTENT));
    return v;
  }

  /**
   * Controller.kt:1069 showHome(): the counter's home: the bar with the gear (and a Lock button when a cashier is at
   * the till), the health card, the card of the starter wallet, the other lines, the note, the
   * three tiles, the open requests and the counter's line. [d] holds the words and values of the moment.
   */
  function showHome(ui, d) {
    const tap = taps(d);
    const health = Slot();
    const lines = Slot();
    const noteSlot = Slot();
    const requestsSlot = Slot();
    const starterSlot = Slot();
    const bar = ui.bar("Counter", d.cashier ? `Cashier: ${d.cashier}` : null, null, true, tap("settings"));
    if (d.cashier) {
      // Controller.kt:1086: the Lock button goes in before the gear, 4 dp from it.
      const row = bar.row;
      const p = new LayoutParams(WRAP_CONTENT, WRAP_CONTENT);
      p.rightMargin = ui.px(4);
      const lock = ui.barButton(d.words.lock, tap("lock"));
      addView(row, lock, p);
      row.insertBefore(lock, row.children[row.children.length - 2]);
    }
    const tiles = LinearLayout("v");
    addView(tiles, ui.tile(d.words.newSale, Glyph.Plus, true, tap("newSale")), new LayoutParams(MATCH_PARENT, ui.px(104)));
    const pair = LinearLayout("h");
    const p1 = new LayoutParams(0, ui.px(104), 1); p1.rightMargin = ui.px(6);
    addView(pair, ui.tile(d.words.sales, Glyph.List, false, tap("sales")), p1);
    const p2 = new LayoutParams(0, ui.px(104), 1); p2.leftMargin = ui.px(6);
    addView(pair, ui.tile(d.words.today, Glyph.Calendar, false, tap("today")), p2);
    const pp = new LayoutParams(MATCH_PARENT, WRAP_CONTENT); pp.topMargin = ui.px(12);
    addView(tiles, pair, pp);
    if (d.words.bills) {
      const bp = new LayoutParams(MATCH_PARENT, ui.px(104)); bp.topMargin = ui.px(12);
      addView(tiles, ui.tile(d.words.bills, Glyph.List, false, tap("bills")), bp);
    }
    // counterLine() (Controller.kt:1067): the text of the line, 17 sp, 16 dp above.
    const counterLine = ui.text(d.counterLine || "", Type.BODY);
    setPadding(counterLine, 0, ui.px(16), 0, 0);
    const page = ui.page(bar, [health, starterSlot, lines, noteSlot, tiles, requestsSlot, counterLine], [], null);
    health.show(ui.healthCard(d.health));
    const open = new Set(d.open || []);
    if (d.lines && d.lines.length) lines.show(ui.homeLines(d.lines.map((l) => HomeLine(l.key, l.words, l.extra, l.retry)), open));
    if (d.starterCards && d.starterCards.length) {
      starterSlot.show(...d.starterCards.map((c) => {
        const card = ui.starterCard(c.heading, c.lines, () => {}, d.words.starterSeen);
        card.layoutParams = ui.margins(0, 12);
        return card;
      }));
    }
    if (d.requests && d.requests.length) {
      const label = ui.label(d.words.openRequests);
      setPadding(label, 0, ui.px(20), 0, ui.px(8));
      requestsSlot.show(label, ui.rows(d.requests.map((r) => RowItem(r.title, r.sub, null, null, false, () => {}))));
    }
    return page;
  }

  /**
   * Controller.kt:1692 showNewSale(): the amount of the sale in 60 sp, the keys, and the buttons Open the sale and
   * Request to send. [d] has the entry's text, the price line and the words.
   */
  function showNewSale(ui, d) {
    const tap = taps(d);
    const display = ui.text(d.entry, 60, true);
    display.style.fontFeatureSettings = '"tnum"';
    setTextGravity(display, Gravity.CENTER);
    const msg = Slot();
    const centre = Centered();
    const amountLabel = ui.text(d.words.amountLabel, 14, false, false, ui.pal.secondary, true);
    setTextGravity(amountLabel, Gravity.CENTER);
    addView(centre, amountLabel, ui.margins());
    addView(centre, display, ui.margins(6));
    if (d.price) {
      const t = ui.text(d.price, 14, false, false, ui.pal.secondary);
      setTextGravity(t, Gravity.CENTER);
      addView(centre, t, ui.margins(6));
    }
    addView(centre, msg, ui.margins(12));
    const pad = ui.amountPad((face) => {
      // The entry's own rules are the model's (AmountEntry); the page only draws what it is given.
      if (d.press) d.press(face, display);
    });
    const open = ui.primary(d.words.open, Colour.Blue, tap("open"));
    const buttons = [open];
    if (d.words.request) buttons.push(ui.secondary(d.words.request, tap("request")));
    return ui.page(ui.bar("New sale", null, tap("back")), [centre, pad], buttons);
  }

  /**
   * Controller.kt:1940 showSale(...): the sale's screen. While the sale shows its code (not paid yet, part paid) the
   * page has the state chip in the bar, the amount, the code in its card, the words, the extra notices and the
   * buttons; every other state is the whole screen in the colour of the outcome. [d] has the state's words, the
   * amounts, the code's modules and the flags.
   */
  function showSale(ui, d) {
    const tap = taps(d);
    const says = d.says;
    const code = d.code;
    const bar = ui.bar("Sale " + d.number, null, null, false, null, !code);
    let page;
    if (code) {
      addToBar(bar, ui.stateChip(says), ui);
      const body = [];
      if (d.flash) body.push(ui.notice(d.flash));
      const amount = LinearLayout("v");
      // Controller.kt:1940 showSale, CR-285: a part paid sale leads with what is missing, under its heading.
      if (d.stillHeading) {
        const still = ui.text(d.stillHeading, 16, false, false, ui.pal.secondary, true);
        setTextGravity(still, Gravity.CENTER);
        addView(amount, still, ui.margins());
      }
      const big = ui.text(d.amountBig, 48, true);
      big.style.fontFeatureSettings = '"tnum"';
      setTextGravity(big, Gravity.CENTER);
      addView(amount, big, ui.margins(d.stillHeading ? 2 : 0));
      const small = ui.text(d.amountSmall, 16, false, false, ui.pal.secondary);
      small.style.fontFeatureSettings = '"tnum"';
      setTextGravity(small, Gravity.CENTER);
      addView(amount, small, ui.margins(2));
      const got = ui.text(d.receivedLine, 14, false, false, ui.pal.secondary);
      got.style.fontFeatureSettings = '"tnum"';
      setTextGravity(got, Gravity.CENTER);
      addView(amount, got, ui.margins(2));
      body.push(amount);
      if (d.modules) {
        const side = Math.min(ui.px(276), ui.context.width - ui.px(2 * 20 + 2 * 16), Math.trunc(ui.context.height * 0.45));
        const qr = FrameLayout();
        addView(qr, QrView(ui, d.modules, side), new LayoutParams(side, side));
        body.push(ui.codeCard(qr));
      }
      // Controller.kt:2030 lightningFirst: CR-283, over the instant limit the code is the invoice alone; the line says why and the button shows the on-chain code.
      if (d.lightningFirst) {
        const line = ui.text(d.words.lightningLine, 13, false, false, ui.pal.secondary);
        setTextGravity(line, Gravity.CENTER);
        body.push(line);
        body.push(ui.button(d.words.lightningButton, tap("onChain")));
      }
      const words = LinearLayout("v");
      setGravity(words, Gravity.CENTER_HORIZONTAL);
      const what = ui.text(says.what, 16, false, false, ui.pal.secondary);
      setTextGravity(what, Gravity.CENTER);
      addView(words, what, ui.margins());
      const next = ui.text(says.next, 17, false, false, null, true);
      setTextGravity(next, Gravity.CENTER);
      addView(words, next, ui.margins(8));
      const chip = left(ui.whoChip(says));
      setGravity(chip, Gravity.CENTER_HORIZONTAL);
      addView(words, chip, ui.margins(10));
      body.push(words);
      (d.extra || []).forEach((w) => body.push(ui.notice(w)));
      if (d.request) {
        body.push(ui.button(d.words.shareLink, tap("shareLink")));
        body.push(ui.button(d.words.shareCode, tap("shareCode")));
      }
      body.push(ui.button(d.words.copy, tap("copy")));
      body.push(ui.button(d.words.details, tap("details")));
      const actions = [];
      if (d.buttons.requote) actions.push(ui.primary(d.words.requote, Colour.Blue, tap("requote")));
      if (d.buttons.cancel) actions.push(ui.danger(d.words.cancel, tap("cancel")));
      if (d.buttons.override) actions.push(ui.button(d.words.override, tap("override")));
      actions.push(ui.button(d.words.done, tap("done")));
      page = ui.page(bar, body, actions);
    } else {
      const band = outcomeBand(ui, says.colour);
      const centre = Centered();
      addView(centre, OutcomeMark(ui, says.colour, band.band), new LayoutParams(ui.px(112), ui.px(112)));
      const heading = ui.text(says.heading, d.outcomeHeadingSp, true, false, ui.palette.WHITE);
      setTextGravity(heading, Gravity.CENTER);
      addView(centre, heading, ui.margins(16));
      const line = ui.text(d.outcomeLine, 20, false, false, band.sub, true);
      line.style.fontFeatureSettings = '"tnum"';
      setTextGravity(line, Gravity.CENTER);
      addView(centre, line, ui.margins(8));
      // Controller.kt:1940 showSale, CR-286: a short check shows a ring that empties; a wait for a block says how long ago the
      // payment was seen. The mark beside it pulses only while the check is healthy. The numbers move in place (d.live.update).
      if (d.ringSecs != null || d.seen != null) {
        const row = LinearLayout("h");
        setGravity(row, Gravity.CENTER);
        let ring = null;
        let seen = null;
        if (d.ringSecs != null) {
          ring = WaitRing(ui);
          addView(row, ring, new LayoutParams(ui.px(72), ui.px(72)));
        }
        if (d.seen != null) {
          seen = ui.text(d.seen, 17, false, false, ui.palette.WHITE, true);
          seen.style.fontFeatureSettings = '"tnum"';
          setTextGravity(seen, Gravity.CENTER);
          addView(row, seen, new LayoutParams(WRAP_CONTENT, WRAP_CONTENT));
        }
        const mark = PulseMark(ui);
        const mp = new LayoutParams(ui.px(10), ui.px(10));
        mp.leftMargin = ui.px(10);
        addView(row, mark, mp);
        const update = (n) => {
          if (ring && n.ringSecs != null) ring.set(n.ringSecs, n.ringTotal, n.ringLabel);
          if (seen && n.seen != null) seen.textContent = n.seen;
          mark.pulsing(n.pulse);
        };
        update(d);
        if (d.live) d.live.update = update;
        addView(centre, row, ui.margins(16));
      }
      // CR-254: the sale's number and its bill, large enough to read across the counter.
      for (const pl of d.paidLines || []) {
        const lbl = ui.text(pl.label, 14, false, false, band.sub, true);
        setTextGravity(lbl, Gravity.CENTER);
        addView(centre, lbl, ui.margins(14));
        const val = ui.text(pl.value, 34, true, false, ui.palette.WHITE);
        val.style.fontFeatureSettings = '"tnum"';
        val.classList.add("select");
        setTextGravity(val, Gravity.CENTER);
        addView(centre, val, ui.margins(2));
      }
      const what = ui.text(says.what, 17, false, false, ui.palette.WHITE);
      setTextGravity(what, Gravity.CENTER);
      addView(centre, what, ui.margins(28));
      const next = ui.text(says.next, 17, true, false, ui.palette.WHITE);
      setTextGravity(next, Gravity.CENTER);
      addView(centre, next, ui.margins(10));
      const more = [...(d.extra || [])];
      if (d.flash) more.unshift(d.flash);
      for (const w of more) {
        const card = LinearLayout("v");
        setBackground(card, ui.roundedFill(0x29000000, 16));
        setPadding(card, ui.px(16), ui.px(14), ui.px(16), ui.px(14));
        addView(card, ui.text(w.heading, 15, true, false, ui.palette.WHITE));
        addView(card, ui.text(w.what, 15, false, false, ui.palette.WHITE), ui.margins(4));
        addView(centre, card, ui.margins(16));
      }
      const actions = [];
      actions.push(ui.onBand(d.words.done, d.isPaid && says.colour === Colour.Green, band.band, tap("done")));
      if (d.buttons.override) actions.push(ui.onBand(d.words.override, false, band.band, tap("override")));
      actions.push(ui.onBand(d.words.details, false, band.band, tap("details")));
      page = ui.page(bar, [centre], actions);
      ui.paintBand(page, bar, band);
    }
    return page;
  }

  /**
   * Controller.kt:606 drawNotices() and MainActivity's place(): the notices of CR-284 stand one under the other at the
   * top of whatever screen shows, and the screen is placed under them. [d] has the notices (index, green, heading, next,
   * tap) and the line "and N more in Sales" or null. With none, [page] is the whole screen.
   */
  function withNotices(ui, page, d) {
    if (!d || (!d.notices.length && !d.more)) return page;
    const gap = ui.px(8);
    const column = LinearLayout("v");
    setPadding(column, gap, ui.barInsets()[1] + gap, gap, 0);
    column.style.backgroundColor = css(ui.pal.background);
    column.className += " notices";
    for (const n of d.notices) {
      const tone = ui.pal.of(n.green ? Colour.Green : Colour.Red);
      const v = LinearLayout("v");
      setBackground(v, ui.rounded(tone.tint, 14));
      setPadding(v, ui.px(16), ui.px(10), ui.px(16), ui.px(10));
      addView(v, ui.text(n.heading, 16, true, false, tone.ink));
      addView(v, ui.text(n.next, 14), ui.margins(2));
      setClickable(v, true, n.tap);
      const p = new LayoutParams(MATCH_PARENT, WRAP_CONTENT);
      p.bottomMargin = gap;
      addView(column, v, p);
    }
    if (d.more) {
      const more = ui.text(d.more, 14, true);
      setPadding(more, ui.px(16), ui.px(8), ui.px(16), ui.px(8));
      addView(column, more);
    }
    const shell = LinearLayout("v");
    addView(shell, column, new LayoutParams(MATCH_PARENT, WRAP_CONTENT));
    addView(shell, page, new LayoutParams(MATCH_PARENT, 0, 1));
    shell.style.minHeight = "0";
    page.style.minHeight = "0";
    return shell;
  }

  /** Controller.kt:2333 showSales(): the label, and one card of rows, one for each sale, newest first. */
  function showSales(ui, d) {
    const views = [ui.label(d.words.heading)];
    if (!d.rows.length) views.push(ui.text(d.words.none, 16));
    else views.push(ui.rows(d.rows.map((r) => RowItem(r.title, r.sub, r.colour, null, false, () => {}))));
    return ui.page(ui.bar("Sales", null, () => {}), views, []);
  }

  /** Controller.kt:2362 showSaleDetails(index, notice): a sale's facts, its words, the note, the refunds, and the buttons. */
  function showSaleDetails(ui, d) {
    const views = [];
    if (d.notice) views.push(ui.notice(d.notice));
    views.push(ui.label("Sale " + d.number));
    views.push(ui.facts(d.facts));
    views.push(ui.notice(d.says));
    for (const w of d.flags || []) views.push(ui.notice(w));
    if (d.note) views.push(ui.facts([[d.words.note, d.note]]));
    views.push(ui.label(d.words.refunds));
    if (!d.refunds.length) views.push(ui.text(d.words.noRefund, 16));
    for (const r of d.refunds) {
      views.push(ui.text(r.line, 14));
      views.push(ui.notice(r.says));
    }
    const actions = d.actions.map((a) => ui.button(a, () => {}));
    return ui.page(ui.bar("Details and refunds", null, () => {}), views, actions);
  }

  /** Controller.kt:2769 heroTile(line): a small tile inside the hero card: the label, the amount and what is under it. */
  function heroTile(ui, line) {
    const v = LinearLayout("v");
    setBackground(v, ui.roundedFill(ui.pal.background, 14));
    setPadding(v, ui.px(12), ui.px(12), ui.px(12), ui.px(12));
    if (line) {
      addView(v, ui.text(line.label, 13, false, false, ui.pal.secondary));
      const a = ui.text(line.amount, 17, true);
      a.style.fontFeatureSettings = '"tnum"';
      addView(v, a, ui.margins(2));
      if (line.detail) {
        const t = ui.text(line.detail, 13, false, false, ui.pal.secondary);
        t.style.fontFeatureSettings = '"tnum"';
        addView(v, t, ui.margins(2));
      }
    }
    return v;
  }

  /**
   * Controller.kt:2781 showToday(people): the day's totals: the hero card (what was paid, and how), the rows that need a look
   * and the quiet lines in one card, and the lines per person behind the manager's PIN (or the PIN's entry).
   */
  function showToday(ui, d) {
    const views = [];
    const paid = d.paid;
    const hero = ui.surfaceCard();
    setPadding(hero, ui.px(20), ui.px(20), ui.px(20), ui.px(20));
    if (paid) {
      addView(hero, ui.text(paid.label, 14, false, false, ui.pal.secondary, true));
      const a = ui.text(paid.amount, 44, true);
      a.style.fontFeatureSettings = '"tnum"';
      addView(hero, a, ui.margins(4));
      if (paid.detail) {
        const t = ui.text(paid.detail, 15, false, false, ui.pal.secondary);
        t.style.fontFeatureSettings = '"tnum"';
        addView(hero, t, ui.margins(4));
      }
    }
    const tiles = LinearLayout("h");
    const c1 = new LayoutParams(0, WRAP_CONTENT, 1); c1.rightMargin = ui.px(5);
    addView(tiles, heroTile(ui, d.chain), c1);
    const c2 = new LayoutParams(0, WRAP_CONTENT, 1); c2.leftMargin = ui.px(5);
    addView(tiles, heroTile(ui, d.lightning), c2);
    addView(hero, tiles, ui.margins(14));
    views.push(hero);
    const card = ui.surfaceCard();
    const red = ui.pal.stop.band;
    const blue = ui.pal.info.band;
    const info = ui.pal.info.ink;
    const more = (words, row) => {
      const box = LinearLayout("v");
      addView(box, row, matchWide());
      const detail = LinearLayout("v");
      setVisible(detail, false);
      setPadding(detail, ui.px(38), 0, ui.px(16), ui.px(12));
      addView(detail, ui.text(words.what, 14, false, false, ui.pal.secondary));
      addView(detail, ui.text(words.next, 14, false, false, null, true), ui.margins(6));
      addView(box, detail, matchWide());
      setClickable(row, true, () => setVisible(detail, detail.classList.contains("gone")));
      return box;
    };
    let first = true;
    const add = (v) => {
      if (!first) addView(card, ui.line());
      first = false;
      addView(card, v, matchWide());
    };
    if (d.stopped) add(more(d.stopped.words, ui.needRow(red, d.stopped.words.heading, null, null, d.stopped.count, null, () => {})));
    if (d.waiting) {
      const link = LinearLayout("h");
      setGravity(link, Gravity.CENTER_VERTICAL);
      const ip = new LayoutParams(ui.px(14), ui.px(14));
      ip.rightMargin = ui.px(5);
      addView(link, ui.icon(Icon.InfoCircle, info, 14, 2.2), ip);
      addView(link, ui.text(d.waiting.words.heading, 13, false, false, info, true));
      add(more(d.waiting.words, ui.needRow(blue, d.waiting.label, d.waiting.sub, link, d.waiting.count, d.waiting.money, () => {})));
    }
    if (d.refunds) add(ui.needRow(blue, d.refunds.label, null, null, d.refunds.count, d.refunds.rest, null));
    const quiet = LinearLayout("v");
    setPadding(quiet, ui.px(16), ui.px(12), ui.px(16), ui.px(12));
    d.quiet.forEach((l, i) => {
      const row = LinearLayout("h");
      addView(row, ui.text(l.label, 14, false, false, ui.pal.secondary), new LayoutParams(0, WRAP_CONTENT, 1));
      const val = ui.text(l.value, 14, false, false, ui.pal.secondary);
      val.style.fontFeatureSettings = '"tnum"';
      setTextGravity(val, Gravity.END);
      addView(row, val);
      addView(quiet, row, ui.margins(i === 0 ? 0 : 8));
    });
    add(quiet);
    views.push(card);
    if (d.people) {
      const per = ui.surfaceCard();
      const l = ui.label(d.words.perPerson);
      setPadding(l, ui.px(16), ui.px(12), ui.px(16), ui.px(4));
      addView(per, l, matchWide());
      for (const p of d.people) {
        const row = LinearLayout("h");
        setGravity(row, Gravity.CENTER_VERTICAL);
        setPadding(row, ui.px(16), ui.px(12), ui.px(16), ui.px(12));
        const initial = ui.text(p.name.slice(0, 1).toUpperCase(), 15, true);
        setTextGravity(initial, Gravity.CENTER);
        initial.style.display = "flex";
        initial.style.alignItems = "center";
        initial.style.justifyContent = "center";
        setBackground(initial, ui.roundedFill(ui.pal.background, 18));
        const ip = new LayoutParams(ui.px(36), ui.px(36));
        ip.rightMargin = ui.px(12);
        addView(row, initial, ip);
        const col = LinearLayout("v");
        addView(col, ui.text(`${p.name} (${p.role})`, 15, false, false, null, true));
        addView(col, ui.text(p.line, 13, false, false, ui.pal.secondary), ui.margins(2));
        if (p.tips) addView(col, ui.text(p.tips, 13, false, false, ui.pal.secondary), ui.margins(2));
        addView(row, col, new LayoutParams(0, WRAP_CONTENT, 1));
        addView(per, ui.line());
        addView(per, row, matchWide());
      }
      views.push(per);
    } else {
      const model = d.pinWords;
      const locked = ui.surfaceCard();
      const head = LinearLayout("h");
      setGravity(head, Gravity.CENTER_VERTICAL);
      setPadding(head, ui.px(16), ui.px(14), ui.px(12), ui.px(14));
      setClickable(head, true, null);
      setBackground(head, ui.rippleOver(null, 0));
      const badge = FrameLayout();
      setBackground(badge, ui.roundedFill(ui.pal.background, 12));
      const gp = new LayoutParams(ui.px(20), ui.px(20));
      gp.gravity = Gravity.CENTER;
      addView(badge, ui.glyph(Glyph.Lock, ui.pal.text, 20), gp);
      const bp = new LayoutParams(ui.px(40), ui.px(40));
      bp.rightMargin = ui.px(14);
      addView(head, badge, bp);
      const col = LinearLayout("v");
      addView(col, ui.text(model.heading, 16, false, false, null, true));
      addView(col, ui.text(model.what, 13, false, false, ui.pal.secondary), ui.margins(2));
      addView(head, col, new LayoutParams(0, WRAP_CONTENT, 1));
      addView(locked, head, matchWide());
      const form = LinearLayout("v");
      setVisible(form, false);
      setPadding(form, ui.px(16), 0, ui.px(16), ui.px(16));
      addView(form, ui.text(model.next, 14, false, false, null, true));
      addView(form, ui.labelled(d.words.managerPin, pinField(ui, "")), ui.margins(12));
      addView(form, Slot(), ui.margins(8));
      addView(form, ui.primary(d.words.showLines, Colour.Blue, () => {}), ui.margins(12));
      addView(locked, form, matchWide());
      head.addEventListener("click", () => setVisible(form, form.classList.contains("gone")));
      views.push(locked);
    }
    views.push(ui.text(d.foot, 13, false, false, ui.pal.secondary));
    return ui.page(ui.bar("Today", null, () => {}), views, []);
  }

  const api = {
    CV_KT, CU_KT, CT_KT, TONES, outcomeBand, matchWide, Glyph, GlyphView, OutcomeMark, WaitRing, PulseMark, Centered, addToBar, HomeLine, FillsFrame, CoverLayout,
    pinField, amountField, Slot, QrView, left, showHome, showNewSale, showSale, withNotices, showSales, showSaleDetails, heroTile, showToday,
  };
  root.OpenBmtCounterViews = api;
  if (typeof module !== "undefined" && module.exports) module.exports = api;
})(typeof globalThis !== "undefined" ? globalThis : this);
