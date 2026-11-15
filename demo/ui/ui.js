// The drawing layer of the Android apps, ported to the browser, one function for each Kotlin function.
// Source: apps/android-common/src/android/Ui.kt (and Icons.kt). Every function says which Kotlin line it mirrors;
// demo/ui/ui.test.js checks that those lines still hold the function. Names, parameters and the dp and sp values are
// the Kotlin ones. 1 dp is 1 CSS px at the device's width and 1 sp is 1 px. Colours come from palette.json, which
// the JVM test writes from Palette.kt.
//
// A view is a DOM element. The few android.view classes that Ui.kt builds on (LinearLayout, FrameLayout,
// ScrollView, TextView, GradientDrawable, LayoutParams, Gravity) are the first section: they are what the
// browser needs to lay views out the way Android does, and carry no look of their own.
"use strict";
(function (root) {
  const UI_KT = "apps/android-common/src/android/Ui.kt";

  // ---- android.view stand-ins (no Kotlin line: these are the platform's) --------------------------------------

  const MATCH_PARENT = -1;
  const WRAP_CONTENT = -2;

  /** android.view.Gravity: the platform's own bit values, so that "TOP or START" is written TOP | START. */
  const Gravity = {
    CENTER_HORIZONTAL: 0x01, CENTER_VERTICAL: 0x10, CENTER: 0x11,
    START: 0x800003, END: 0x800005, LEFT: 0x03, RIGHT: 0x05, TOP: 0x30, BOTTOM: 0x50,
  };

  /** The horizontal part of a gravity: "start", "center" or "end". */
  function hPart(g) {
    const h = g & 0x07;
    if (h === 1) return "center";
    if (h === 5) return "end";
    return "start";
  }
  /** The vertical part of a gravity: "top", "center" or "bottom". */
  function vPart(g) {
    const v = g & 0x70;
    if (v === 0x10) return "center";
    if (v === 0x50) return "bottom";
    return "top";
  }
  const FLEX = { start: "flex-start", center: "center", end: "flex-end", top: "flex-start", bottom: "flex-end" };

  /** LinearLayout.LayoutParams / FrameLayout.LayoutParams: width, height (MATCH_PARENT, WRAP_CONTENT or px), weight, margins, gravity. */
  class LayoutParams {
    constructor(width = MATCH_PARENT, height = WRAP_CONTENT, weight = 0) {
      this.width = width;
      this.height = height;
      this.weight = weight;
      this.leftMargin = 0;
      this.topMargin = 0;
      this.rightMargin = 0;
      this.bottomMargin = 0;
      this.gravity = null;
    }

    setMargins(l, t, r, b) {
      this.leftMargin = l;
      this.topMargin = t;
      this.rightMargin = r;
      this.bottomMargin = b;
    }
  }

  const LinearLayoutOrientation = { VERTICAL: "v", HORIZONTAL: "h" };

  /** A DOM element made as a view. */
  function viewEl(tag, cls) {
    const e = document.createElement(tag);
    e.className = "v" + (cls ? " " + cls : "");
    return e;
  }

  /** android.widget.LinearLayout. */
  function LinearLayout(orientation = "h") {
    const e = viewEl("div", "ll");
    e.dataset.layout = orientation;
    e.dataset.gravity = String(Gravity.START | Gravity.TOP);
    return e;
  }

  /** android.widget.FrameLayout. */
  function FrameLayout() {
    const e = viewEl("div", "fl");
    e.dataset.layout = "f";
    return e;
  }

  /** android.view.View: an empty view. */
  function View() {
    return viewEl("div", "");
  }

  /** android.widget.ScrollView: the child is added with [addView]; with [fillViewport] it is at least as tall as the view. */
  function ScrollView() {
    const e = viewEl("div", "sv");
    e.dataset.layout = "scroll";
    return e;
  }

  /** The gravity of the children of a LinearLayout. */
  function setGravity(e, g) {
    e.dataset.gravity = String(g);
    const layout = e.dataset.layout;
    if (layout === "v") {
      e.style.alignItems = FLEX[hPart(g)];
      e.style.justifyContent = FLEX[vPart(g)];
    } else if (layout === "h") {
      e.style.alignItems = FLEX[vPart(g)];
      e.style.justifyContent = FLEX[hPart(g)];
    }
  }

  /** TextView.gravity: how the text sits in the view's own box. */
  function setTextGravity(e, g) {
    const h = hPart(g);
    e.style.textAlign = h === "center" ? "center" : h === "end" ? "right" : "left";
    const v = vPart(g);
    if (v !== "top") {
      e.style.display = "flex";
      e.style.flexDirection = "column";
      e.style.justifyContent = FLEX[v];
      if (h === "center") e.style.alignItems = "center";
      if (h === "end") e.style.alignItems = "flex-end";
    }
  }

  /**
   * View.setPadding. A TextView in Roboto also has the platform's font padding (includeFontPadding, on by default): the
   * font's top and bottom from its bounding box, 0.16 em above and 0.027 em below the ascent and descent that a
   * browser lays a line out with. It is added here to the padding of every text, as the platform adds it to the text's height.
   */
  function setPadding(e, l, t, r, b) {
    const fp = Number(e.dataset.fp || 0);
    e.dataset.pad = `${l},${t},${r},${b}`;
    e.style.padding = `${t + FONT_PAD_TOP * fp}px ${r}px ${b + FONT_PAD_BOTTOM * fp}px ${l}px`;
  }
  const FONT_PAD_TOP = 0.16;
  const FONT_PAD_BOTTOM = 0.027;

  function setMinHeight(e, px) {
    e.style.minHeight = px + "px";
  }

  /** View.visibility: GONE takes no room; VISIBLE shows. */
  function setVisible(e, visible) {
    e.classList.toggle("gone", !visible);
  }

  /** ViewGroup.addView: puts [child] in [parent] and sizes it from [params] the way Android's layouts do. */
  function addView(parent, child, params) {
    const layout = parent.dataset.layout;
    let p = params;
    if (!p) {
      p = layout === "h" ? new LayoutParams(WRAP_CONTENT, WRAP_CONTENT) : layout === "f" ? new LayoutParams(MATCH_PARENT, MATCH_PARENT) : new LayoutParams(MATCH_PARENT, WRAP_CONTENT);
    }
    const s = child.style;
    s.margin = `${p.topMargin}px ${p.rightMargin}px ${p.bottomMargin}px ${p.leftMargin}px`;
    if (layout === "v") {
      if (p.gravity != null) s.alignSelf = FLEX[hPart(p.gravity)];
      if (p.width === MATCH_PARENT) s.alignSelf = "stretch";
      else if (p.width > 0) { s.width = p.width + "px"; s.flexShrink = "0"; }
      if (p.weight > 0) { s.flex = `${p.weight} 1 auto`; s.minHeight = s.minHeight || "0"; }
      else if (p.height > 0) { s.height = p.height + "px"; s.flexShrink = "0"; }
      else if (p.height === MATCH_PARENT) s.flex = "1 1 0";
      else s.flex = "0 0 auto";
    } else if (layout === "h") {
      if (p.gravity != null) s.alignSelf = FLEX[vPart(p.gravity)];
      if (p.height === MATCH_PARENT) s.alignSelf = "stretch";
      else if (p.height > 0) { s.height = p.height + "px"; }
      if (p.weight > 0) { s.flex = `${p.weight} 1 0px`; s.minWidth = "0"; }
      else if (p.width > 0) { s.width = p.width + "px"; s.flex = "0 0 auto"; }
      else if (p.width === MATCH_PARENT) { s.flex = "1 1 0px"; s.minWidth = "0"; }
      else { s.flex = "0 1 auto"; s.minWidth = "0"; }
    } else if (layout === "f") {
      const g = p.gravity == null ? Gravity.TOP | Gravity.START : p.gravity;
      s.justifySelf = p.width === MATCH_PARENT ? "stretch" : FLEX[hPart(g)];
      s.alignSelf = p.height === MATCH_PARENT ? "stretch" : FLEX[vPart(g)];
      if (p.width > 0) s.width = p.width + "px";
      if (p.height > 0) s.height = p.height + "px";
    } else if (layout === "scroll") {
      s.width = "100%";
    }
    parent.appendChild(child);
    return child;
  }

  /** Color.argb to CSS: an opaque colour is already #rrggbb; #aarrggbb is turned into the rgba() the browser reads. */
  function css(c) {
    if (typeof c === "number") {
      const a = ((c >>> 24) & 0xff) / 255;
      const r = (c >>> 16) & 0xff;
      const g = (c >>> 8) & 0xff;
      const b = c & 0xff;
      return `rgba(${r},${g},${b},${+a.toFixed(3)})`;
    }
    if (typeof c === "string" && /^#[0-9a-f]{8}$/i.test(c)) {
      const a = parseInt(c.slice(1, 3), 16) / 255;
      return `rgba(${parseInt(c.slice(3, 5), 16)},${parseInt(c.slice(5, 7), 16)},${parseInt(c.slice(7, 9), 16)},${+a.toFixed(3)})`;
    }
    return c;
  }

  /** android.graphics.drawable.GradientDrawable: a fill, a stroke inside the edge, round corners or an oval. */
  class GradientDrawable {
    constructor() {
      this.shape = "RECTANGLE";
      this.color = null;
      this.cornerRadius = 0;
      this.strokeWidth = 0;
      this.strokeColor = null;
    }

    setColor(c) { this.color = c; }

    setStroke(w, c) {
      this.strokeWidth = w;
      this.strokeColor = c;
    }

    /** Sets the view's background to this drawable. */
    paint(e) {
      e.style.backgroundColor = this.color == null ? "transparent" : css(this.color);
      e.style.borderRadius = this.shape === "OVAL" ? "50%" : this.cornerRadius + "px";
      if (this.strokeWidth > 0) {
        e.style.border = `${this.strokeWidth}px solid ${css(this.strokeColor)}`;
      } else {
        e.style.border = "";
      }
    }
  }
  GradientDrawable.OVAL = "OVAL";

  /** android.graphics.drawable.RippleDrawable(colour, content, mask): [content] may be null; the corner of the mask is [radiusDp]. */
  class RippleDrawable {
    constructor(colour, content, radiusDp) {
      this.colour = colour;
      this.content = content;
      this.radiusDp = radiusDp;
    }
  }

  /** View.background = drawable. [ripple] is the pressed colour of a RippleDrawable, or null. */
  function setBackground(e, drawable, ripple = null) {
    if (drawable instanceof RippleDrawable) {
      if (drawable.content) return setBackground(e, drawable.content, drawable.colour);
      return setRippleOnly(e, drawable.colour, drawable.radiusDp);
    }
    if (drawable instanceof GradientDrawable) drawable.paint(e);
    if (ripple != null) {
      e.style.setProperty("--ripple", css(ripple));
      e.classList.add("ripple");
    }
  }

  /** The colour of the pressed state over an unfilled view (a ripple with no background). */
  function setRippleOnly(e, ripple, radiusDp) {
    e.style.setProperty("--ripple", css(ripple));
    e.classList.add("ripple-over");
    if (radiusDp) e.style.borderRadius = radiusDp + "px";
  }

  function setClickable(e, on, onClick) {
    if (on) {
      e.classList.add("clickable");
      e.setAttribute("role", "button");
      e.tabIndex = 0;
      if (onClick) e.addEventListener("click", onClick);
    }
  }

  // ---- the sizes, distances and colours (Palette.kt: Type, Space; the colours are palette.json's) -----------------

  /** Palette.kt:242 Type: the sizes of text, in sp. */
  const Type = {
    HEADING: 24, BODY: 17, SECONDARY: 14, AMOUNT: 44, STATE: 34, CODE: 15,
    BAR_TITLE: 18, BAR_SUB: 13, FIELD: 17, LABEL: 13, LABELLED: 14,
  };

  /** Palette.kt:265 Space: the distances, in dp. */
  const Space = {
    GUTTER: 16, BLOCK: 12, TOUCH: 48, BUTTON: 56, FIELD: 56, RADIUS: 12,
    PAGE_GUTTER: 20, PAGE_EDGE: 24, PAGE_BLOCK: 18, BAR: 64, PRIMARY: 56, SECONDARY: 52, QUIET: 48, BUTTON_RADIUS: 14, FIELD_NEW: 52,
  };

  /** WordsModel.kt:4 Colour: the four colours of a words. */
  const Colour = { Blue: "Blue", Green: "Green", Red: "Red", Neutral: "Neutral" };

  /** Ui.kt:165 ButtonKind. */
  const ButtonKind = { Primary: "Primary", Secondary: "Secondary", Danger: "Danger", Good: "Good", Quiet: "Quiet" };

  /** Ui.kt:1325 LineStyle. */
  const LineStyle = { Plain: "Plain", Strong: "Strong", Small: "Small" };

  /** Icons.kt:12 Icon. */
  const Icon = { Back: "Back", Gear: "Gear", Tick: "Tick", Exclaim: "Exclaim", Info: "Info", Dash: "Dash", Person: "Person", Arrow: "Arrow", InfoCircle: "InfoCircle", Chevron: "Chevron", Padlock: "Padlock" };

  /** Palette.kt:51 Palette: the colours, read from palette.json. */
  class PaletteOf {
    constructor(json) {
      this.json = json;
      this.WHITE = json.white;
      this.BRAND = json.brand;
      this.ON_BRAND = json.onBrand;
      this.paid = json.outcomes.paid;
      this.waiting = json.outcomes.waiting;
      this.stopped = json.outcomes.stop;
      this.ended = json.outcomes.ended;
      this.light = new Scheme(json.light);
      this.dark = new Scheme(json.dark);
    }

    /** Palette.kt:105 of(night). */
    of(night) { return night ? this.dark : this.light; }
  }

  /** Palette.kt:20 Scheme. */
  class Scheme {
    constructor(j) {
      Object.assign(this, j);
    }

    /** Palette.kt:40 of(colour): blue is info, green good, red stop, neutral wait. */
    of(colour) {
      return { Blue: this.info, Green: this.good, Red: this.stop, Neutral: this.wait }[colour];
    }
  }

  /** WordsModel.kt:4 Colour.band(night). */
  function colourBand(palette, colour, night) {
    return palette.of(night).of(colour).band;
  }

  /** Convert a Kotlin Font spec to CSS. */
  function fontOf({ bold = false, mono = false, medium = false, serif = false }) {
    let family = '"Roboto", sans-serif';
    let weight = 400;
    if (medium) weight = 500;
    if (serif) { family = '"Noto Serif", serif'; weight = medium ? 500 : 400; }
    if (bold) weight = 700;
    if (mono) { family = '"Roboto Mono", monospace'; weight = bold ? 700 : 400; }
    return { family, weight };
  }

  // ---- Ui (Ui.kt:168) ---------------------------------------------------------------------------------------

  /**
   * Ui.kt:168 class Ui(context). The context is { palette, night, density, insets, window }: the palette object, the
   * phone's night mode, the density (2.625 for the emulator), the system bars' insets [left, top, right, bottom] in
   * dp and the window whose bar icons are set (see [edgeToEdge]).
   */
  class Ui {
    constructor(context) {
      this.context = context;
      this.density = context.density || 2.625;
      this.palette = context.palette;
      this.insets = context.insets || [0, 0, 0, 0];
      this.window = context.window || { statusLight: false, navLight: false };
    }

    /** Ui.kt:172 night. */
    get night() { return !!this.context.night; }

    /** Ui.kt:175 pal. */
    get pal() { return this.palette.of(this.night); }

    /** Ui.kt:177 px(dp): whole device pixels as the platform makes them, written in CSS px. */
    px(dp) { return Math.trunc(dp * this.density) / this.density; }

    /** Ui.kt:179 pxf(dp): at least one device pixel. */
    pxf(dp) { return Math.max(1, Math.trunc(dp * this.density)) / this.density; }

    /** Ui.kt:181 margins(top, bottom). */
    margins(top = 0, bottom = 0) {
      const p = new LayoutParams(MATCH_PARENT, WRAP_CONTENT);
      p.setMargins(0, this.px(top), 0, this.px(bottom));
      return p;
    }

    /** Ui.kt:185 text(...): a TextView; [colour] is a palette colour or null for the text colour. */
    text(text, size = Type.BODY, bold = false, mono = false, colour = null, medium = false, serif = false) {
      if (typeof bold === "object" && bold !== null) return this.text(text, size, bold.bold, bold.mono, bold.colour, bold.medium, bold.serif);
      const e = viewEl("div", "tv");
      e.textContent = text;
      e.style.fontSize = size + "px";
      e.style.color = css(colour == null ? this.pal.text : colour);
      const f = fontOf({ bold, mono, medium, serif });
      e.style.fontFamily = f.family;
      e.style.fontWeight = String(f.weight);
      if (!mono && !serif) e.dataset.fp = String(size);
      setPadding(e, 0, 0, 0, 0);
      return e;
    }

    /** Ui.kt:205 quoted(label, quotedText): text from elsewhere, a marking line then the text in quotation marks. */
    quoted(label, quotedText) {
      const v = LinearLayout("v");
      const bg = new GradientDrawable();
      bg.setColor(this.pal.surface);
      bg.cornerRadius = this.px(8);
      setBackground(v, bg);
      setPadding(v, this.px(Space.BLOCK), this.px(8), this.px(Space.BLOCK), this.px(8));
      addView(v, this.text(label, Type.SECONDARY, false, false, this.pal.secondary));
      addView(v, this.text(quotedText, Type.CODE, false, true));
      return v;
    }

    /** Ui.kt:216 words(model). */
    words(model) { return WordsView(this, model); }

    /** Ui.kt:219 wordsBelowBand(model). */
    wordsBelowBand(model) { return WordsView(this, model, false); }

    /** Ui.kt:222 button(label, kind, onClick) and Ui.kt:231 button(label, onClick). */
    button(label, kind, onClick) {
      if (typeof kind === "function" || kind === undefined) return this.secondary(label, kind);
      switch (kind) {
        case ButtonKind.Primary: return this.primary(label, Colour.Blue, onClick);
        case ButtonKind.Secondary: return this.secondary(label, onClick);
        case ButtonKind.Danger: return this.danger(label, onClick);
        case ButtonKind.Good: return this.good(label, onClick);
        default: return this.quiet(label, onClick);
      }
    }

    /** Ui.kt:237 primary(label, colour, onClick): the brand fill with on-brand bold text, 56 dp high. */
    primary(label, colour = Colour.Blue, onClick) {
      if (typeof colour === "function") { onClick = colour; colour = Colour.Blue; }
      return this.styled(label, ButtonKind.Primary, this.pal.of(colour), onClick);
    }

    /** Ui.kt:241 secondary(label, onClick): no fill, a 1 dp outline, 52 dp high. */
    secondary(label, onClick) { return this.styled(label, ButtonKind.Secondary, this.pal.info, onClick); }

    /** Ui.kt:244 quiet(label, onClick): no fill, no outline, the secondary text colour, 48 dp high. */
    quiet(label, onClick) { return this.styled(label, ButtonKind.Quiet, this.pal.info, onClick); }

    /** Ui.kt:247 danger(label, onClick): outlined in the stop colour. */
    danger(label, onClick) { return this.styled(label, ButtonKind.Danger, this.pal.stop, onClick); }

    /** Ui.kt:250 good(label, onClick): the good band's fill with white bold text, 56 dp high. */
    good(label, onClick) { return this.styled(label, ButtonKind.Good, this.pal.good, onClick); }

    /** Ui.kt:252 styled(label, kind, tone, onClick), with Ui.kt:157 UiButton's faded off state. */
    styled(label, kind, tone, onClick) {
      const e = viewEl("div", "ui-button tv");
      e.textContent = label;
      e.style.fontSize = Type.BODY + "px";
      e.dataset.fp = String(Type.BODY);
      const filled = kind === ButtonKind.Primary || kind === ButtonKind.Good;
      e.style.fontWeight = filled ? "700" : "400";
      const height = this.px(filled ? Space.PRIMARY : kind === ButtonKind.Quiet ? Space.QUIET : Space.SECONDARY);
      e.style.minHeight = height + "px";
      e.style.minWidth = "0";
      setPadding(e, this.px(Space.GUTTER), 0, this.px(Space.GUTTER), 0);
      const ink = {
        Primary: this.palette.ON_BRAND, Good: this.palette.WHITE, Secondary: this.pal.text, Quiet: this.pal.secondary, Danger: tone.ink,
      }[kind];
      e.style.color = css(ink);
      const fill = new GradientDrawable();
      fill.cornerRadius = this.px(Space.BUTTON_RADIUS);
      if (kind === ButtonKind.Primary) fill.setColor(this.palette.BRAND);
      else if (kind === ButtonKind.Good) fill.setColor(tone.band);
      else if (kind === ButtonKind.Secondary) { fill.setColor("transparent"); fill.setStroke(this.px(1), this.pal.fieldLine); }
      else if (kind === ButtonKind.Quiet) fill.setColor("transparent");
      else { fill.setColor("transparent"); fill.setStroke(this.pxf(1.5), tone.ink); }
      // The pressed state: a ripple, black over a filled button, the text's own colour over an outlined one.
      const ripple = filled ? "rgba(0,0,0,0.251)" : withAlpha(ink, 0x33);
      setBackground(e, fill, ripple);
      setClickable(e, true, onClick ? () => onClick() : null);
      return e;
    }

    /**
     * Ui.kt:310 field(hint, lines, mono): a field for typed text. The keyboard flags of the Kotlin code have no
     * meaning here; the look is [style]'s.
     */
    field(hint, lines = 1, mono = false) {
      const e = lines > 1 ? document.createElement("textarea") : document.createElement("input");
      if (lines > 1) e.rows = lines;
      else e.type = "text";
      e.className = "v edit";
      e.placeholder = hint;
      e.autocomplete = "off";
      e.spellcheck = false;
      e.dataset.minLines = String(lines);
      return this.style(e, mono);
    }

    /** Ui.kt:328 style(field, mono): the system sans (or monospace), the hint lighter, a surface fill with a 1 dp outline, 12 dp corners, 52 dp high at least. */
    style(field, mono = false) {
      const f = fontOf({ mono });
      field.style.fontFamily = f.family;
      field.style.fontSize = (mono ? Type.CODE : Type.FIELD) + "px";
      field.style.color = css(this.pal.text);
      field.style.setProperty("--hint", css(this.pal.hint));
      const bg = new GradientDrawable();
      bg.setColor(this.pal.surface);
      bg.setStroke(this.px(1), this.pal.fieldLine);
      bg.cornerRadius = this.px(Space.RADIUS);
      setBackground(field, bg);
      setPadding(field, this.px(Space.GUTTER), this.px(8), this.px(Space.GUTTER), this.px(8));
      field.style.minHeight = this.px(Space.FIELD_NEW) + "px";
      const multi = Number(field.dataset.minLines || 1) > 1;
      field.style.alignItems = multi ? "flex-start" : "center";
      field.style.textAlign = "left";
      return field;
    }

    /** Ui.kt:344 choice(options, selected): a radio list, each at least 48 dp high. */
    choice(options, selected = 0) {
      const group = LinearLayout("v");
      group.dataset.radio = "1";
      options.forEach((label, i) => {
        const r = viewEl("div", "radio tv");
        r.style.fontSize = Type.BODY + "px";
        r.style.color = css(this.pal.text);
        r.style.minHeight = this.px(Space.TOUCH) + "px";
        r.dataset.index = String(i);
        const ring = document.createElement("span");
        const on = i === selected;
        ring.className = "ring" + (on ? " on" : "");
        ring.style.color = css(on ? this.pal.info.ink : this.pal.secondary);
        ring.style.borderColor = css(on ? this.pal.info.ink : this.pal.secondary);
        r.appendChild(ring);
        const t = document.createElement("span");
        t.textContent = label;
        r.appendChild(t);
        addView(group, r);
        r.addEventListener("click", () => {
          for (const o of group.children) {
            const rg = o.querySelector(".ring");
            const sel = o === r;
            rg.classList.toggle("on", sel);
            rg.style.color = rg.style.borderColor = css(sel ? this.pal.info.ink : this.pal.secondary);
          }
        });
      });
      return group;
    }

    /** Ui.kt:367 chosen(group): the index of the chosen option, or -1. */
    chosen(group) {
      const kids = Array.from(group.children);
      return kids.findIndex((k) => (k.querySelector(".ring") || k).classList.contains("on") || k.dataset.checked === "1");
    }

    /**
     * Ui.kt:380 page(views) and Ui.kt:558 page(bar, body, actions, tone, scroll, ground, across): the old page of
     * views, or the page of the new look.
     */
    page(a, ...rest) {
      if (Array.isArray(a)) return this.pageViews(a);
      return this.pageNew(a, ...rest);
    }

    /** Ui.kt:380 page(views): a scrolling page of views in a column, 16 dp gutters, 12 dp between blocks. */
    pageViews(views) {
      const lead = views.length && views[0].isBleed ? views[0] : null;
      if (lead) lead.setTopInset(0);
      const column = LinearLayout("v");
      views.forEach((v, i) => {
        const params = new LayoutParams(MATCH_PARENT, WRAP_CONTENT);
        const side = i === 0 && lead ? 0 : this.px(Space.GUTTER);
        params.setMargins(side, 0, side, this.px(Space.BLOCK));
        addView(column, v, params);
      });
      const topBase = lead ? 0 : this.px(24);
      const bottomBase = this.px(24);
      const appearance = this.edgeToEdge(lead != null);
      const bars = this.barInsets();
      setPadding(column, bars[0], lead ? 0 : topBase + bars[1], bars[2], bottomBase + bars[3]);
      if (lead) lead.setTopInset(bars[1]);
      const sv = ScrollView();
      sv.style.backgroundColor = css(this.pal.background);
      addView(sv, column);
      column.style.minHeight = "100%";
      appearance();
      // A page taller than the screen fades at its lower edge, so it shows there is more below.
      sv.dataset.fade = String(this.px(32));
      return sv;
    }

    // ---- the new look: the bar, the page and the footer ------------------------------------------

    /** Ui.kt:437 rounded(fill, radiusDp, strokeDp, stroke). */
    rounded(fill, radiusDp, strokeDp = 0, stroke = 0) {
      const g = new GradientDrawable();
      g.setColor(fill);
      g.cornerRadius = radiusDp;
      if (strokeDp > 0) g.setStroke(this.pxf(strokeDp), stroke);
      return g;
    }

    /** Ui.kt:443 rippleOver(bg, radiusDp): the pressed colour is 0x14000000 in light and 0x22FFFFFF in dark. */
    rippleOver(bg, radiusDp) {
      const tap = this.night ? "rgba(255,255,255,0.133)" : "rgba(0,0,0,0.078)";
      return new RippleDrawable(tap, bg, radiusDp);
    }

    /** Ui.kt:452 lp(width, height). */
    lp(width = MATCH_PARENT, height = WRAP_CONTENT) { return new LayoutParams(width, height); }

    /** Ui.kt:455 divider(): a 1 dp line in the divider colour. */
    divider() {
      const v = View();
      v.style.backgroundColor = css(this.pal.divider);
      v.style.height = this.pxf(1) + "px";
      v.style.alignSelf = "stretch";
      v.style.flex = "0 0 auto";
      return v;
    }

    /** Ui.kt:460 icon(kind, colour, dp, stroke): a vector icon. */
    icon(kind, colour, dp, stroke = 2) {
      const v = IconView(kind, colour, stroke);
      v.style.width = this.px(dp) + "px";
      v.style.height = this.px(dp) + "px";
      v.style.flex = "none";
      return v;
    }

    /** Ui.kt:467 iconButton(kind, colour, description, onClick): a 48 dp touch target with a vector icon in it. */
    iconButton(kind, colour, description, onClick) {
      const v = viewEl("div", "");
      v.setAttribute("aria-label", description);
      v.style.width = this.px(Space.TOUCH) + "px";
      v.style.height = this.px(Space.TOUCH) + "px";
      v.style.flex = "none";
      setPadding(v, this.px(12), this.px(12), this.px(12), this.px(12));
      const svg = IconView(kind, colour, 2);
      svg.style.width = "100%";
      svg.style.height = "100%";
      v.appendChild(svg);
      setBackground(v, this.rippleOver(null, 24));
      setClickable(v, true, onClick ? () => onClick() : null);
      return v;
    }

    /**
     * Ui.kt:486 bar(title, sub, back, logo, gear, onBlack, ground): the bar of a page, 64 dp plus the status bar's
     * height, in the surface colour with a 1 dp divider under it. [logoUrl] is the 32 dp brand square.
     */
    bar(title, sub = null, back = null, logo = false, gear = null, onBlack = false, ground = null) {
      if (typeof title === "object" && title !== null) {
        const o = title;
        return this.bar(o.title, o.sub, o.back, o.logo, o.gear, o.onBlack, o.ground);
      }
      const fill = onBlack ? "#000000" : ground != null ? ground : this.pal.surface;
      const ink = onBlack ? this.palette.WHITE : this.pal.text;
      const subInk = onBlack ? "#d3d7de" : this.pal.secondary;
      const bar = BarView(this, onBlack);
      bar.style.backgroundColor = css(fill);
      const row = LinearLayout("h");
      setGravity(row, Gravity.CENTER_VERTICAL);
      setPadding(row, back ? this.px(8) : this.px(Space.PAGE_GUTTER), 0, gear ? this.px(8) : this.px(Space.PAGE_GUTTER), 0);
      if (back) addView(row, this.iconButton(Icon.Back, ink, "Back", back));
      if (logo) {
        const img = document.createElement("img");
        img.className = "v";
        img.src = this.context.logoUrl || "logo.png";
        img.alt = "";
        img.style.objectFit = "contain";
        img.style.borderRadius = "7px";
        img.style.display = "block";
        const p = new LayoutParams(this.px(32), this.px(32));
        p.rightMargin = this.px(12);
        addView(row, img, p);
      }
      const names = LinearLayout("v");
      const t = this.text(title, Type.BAR_TITLE, false, false, ink, true);
      t.classList.add("single");
      addView(names, t);
      if (sub != null) {
        const s = this.text(sub, Type.BAR_SUB, false, false, subInk);
        s.classList.add("single");
        addView(names, s);
      }
      addView(row, names, new LayoutParams(0, WRAP_CONTENT, 1));
      if (gear) addView(row, this.iconButton(Icon.Gear, ink, "Settings", gear));
      bar.row = row;
      addView(bar, row, this.lp(MATCH_PARENT, this.px(Space.BAR)));
      if (!onBlack) addView(bar, this.divider());
      return bar;
    }

    /**
     * Ui.kt:558 page(bar, body, actions, tone, scroll, ground, across): the bar fixed at the top, the body
     * scrolling under it (a 20 dp gutter, 24 dp at the top and bottom, 18 dp between blocks), the actions fixed at
     * the bottom over a 1 dp divider (14 dp above, 28 dp plus the navigation bar's height below, 10 dp between, 12
     * dp side by side when [across]). The insets are those of the context.
     */
    pageNew(bar, body, actions = [], tone = null, scroll = true, ground = null, across = false) {
      const barView = bar.isBar ? bar : null;
      const onBlack = !!(barView && barView.onBlack);
      if (tone && !onBlack) bar.style.backgroundColor = css(tone.tint);
      const root = LinearLayout("v");
      root.style.backgroundColor = css(onBlack ? "#000000" : ground != null ? ground : this.pal.background);
      addView(root, bar, this.lp());
      const column = LinearLayout("v");
      const firstWide = body.length > 0 && body[0].isFullWidth;
      body.forEach((v, i) => {
        const wide = !!v.isFullWidth;
        const side = wide ? 0 : this.px(Space.PAGE_GUTTER);
        const params = v.isFills ? new LayoutParams(MATCH_PARENT, 0, 1) : new LayoutParams(MATCH_PARENT, WRAP_CONTENT);
        params.setMargins(side, 0, side, i < body.length - 1 ? this.px(Space.PAGE_BLOCK) : 0);
        addView(column, v, params);
      });
      const top = firstWide || !scroll ? 0 : this.px(Space.PAGE_EDGE);
      const bottom = scroll ? this.px(Space.PAGE_EDGE) : 0;
      setPadding(column, 0, top, 0, bottom);
      let content;
      if (scroll) {
        content = ScrollView();
        addView(content, column);
        column.style.minHeight = "100%";
      } else {
        content = column;
      }
      addView(root, content, new LayoutParams(MATCH_PARENT, 0, 1));
      content.style.minHeight = "0";
      let footer = null;
      let buttons = null;
      if (actions.length > 0) {
        const f = LinearLayout("v");
        f.style.backgroundColor = css(ground != null ? ground : this.pal.surface);
        addView(f, this.divider());
        buttons = LinearLayout(across ? "h" : "v");
        actions.forEach((b, i) => {
          if (across) {
            const p = new LayoutParams(0, WRAP_CONTENT, 1);
            p.leftMargin = i === 0 ? 0 : this.px(12);
            addView(buttons, b, p);
          } else {
            const p = this.lp();
            p.topMargin = i === 0 ? 0 : this.px(10);
            addView(buttons, b, p);
          }
        });
        addView(f, buttons, this.lp());
        addView(root, f, this.lp());
        footer = f;
      }
      const appearance = this.edgeToEdge(onBlack);
      // The window's insets: the sides, the status bar over the bar, the navigation bar under the footer.
      const bars = this.barInsets();
      const nav = this.navInset();
      root.style.paddingLeft = bars[0] + "px";
      root.style.paddingRight = bars[2] + "px";
      if (barView) barView.setTopInset(bars[1]);
      if (footer) {
        setPadding(buttons, this.px(Space.PAGE_GUTTER), this.px(14), this.px(Space.PAGE_GUTTER), this.px(28) + nav);
        setPadding(column, 0, top, 0, bottom);
      } else {
        setPadding(column, 0, top, 0, bottom + nav);
      }
      appearance();
      return root;
    }

    /** Ui.kt:656 navInset(insets): the bottom of the system bars alone (the navigation bar). */
    navInset() { return this.insets[3]; }

    // ---- the four ways a screen's words are shown --------------------------------------------------

    /** Ui.kt:664 detailsFold(content): the "Details" toggle and, under it, [content], hidden until the toggle is pressed. */
    detailsFold(content, detailsTitle = "Details") {
      setVisible(content, false);
      const toggle = this.text(`${detailsTitle} (show)`, Type.SECONDARY, true, false, this.pal.info.ink);
      toggle.style.minHeight = this.px(Space.TOUCH) + "px";
      setTextGravity(toggle, Gravity.CENTER_VERTICAL);
      toggle.classList.add("clickable");
      toggle.addEventListener("click", () => {
        const open = content.classList.contains("gone");
        setVisible(content, open);
        toggle.textContent = `${detailsTitle} (${open ? "hide" : "show"})`;
      });
      return [toggle, content];
    }

    /** Ui.kt:677 detailsViews(model). */
    detailsViews(model) {
      const out = [];
      if (model.quotedText != null) out.push(this.quoted(model.quotedLabel || "", model.quotedText));
      if (model.details && model.details.length) {
        out.push(...this.detailsFold(this.text(model.details.join("\n"), Type.CODE - 2, false, true)));
      }
      return out;
    }

    /** Ui.kt:686 toneOf(model). */
    toneOf(model) { return this.pal.of(model.colour); }

    /** Ui.kt:688 whoLine(model, size, ink, withIcon). */
    whoLine(model, size, ink, withIcon) {
      const v = LinearLayout("h");
      setGravity(v, Gravity.CENTER_VERTICAL);
      if (withIcon) {
        const p = new LayoutParams(this.px(16), this.px(16));
        p.rightMargin = this.px(6);
        addView(v, this.icon(Icon.Person, ink, 16, 2.2), p);
      }
      addView(v, this.text(model.who, size, false, false, ink, true));
      return v;
    }

    /**
     * Ui.kt:701 resultPanel(model): for a screen that is one message: a full-width panel in the tone's tint with a
     * 76 dp circle in the tone's band holding a white icon, the heading at 27 sp bold, the what, and who does the
     * next step; the next line below in a surface card with an arrow; the quoted text and the details fold.
     */
    resultPanel(model) {
      const tone = this.toneOf(model);
      const panel = ResultPanel();
      const head = LinearLayout("v");
      setGravity(head, Gravity.CENTER_HORIZONTAL);
      head.style.backgroundColor = css(tone.tint);
      setPadding(head, this.px(Space.PAGE_GUTTER), this.px(28), this.px(Space.PAGE_GUTTER), this.px(28));
      const kind = { Green: Icon.Tick, Red: Icon.Exclaim, Blue: Icon.Info, Neutral: Icon.Dash }[model.colour];
      const circle = FrameLayout();
      const g = new GradientDrawable();
      g.shape = GradientDrawable.OVAL;
      g.setColor(tone.band);
      setBackground(circle, g);
      addView(circle, this.icon(kind, this.palette.WHITE, 40, 2.6), (() => { const p = new LayoutParams(this.px(40), this.px(40)); p.gravity = Gravity.CENTER; return p; })());
      addView(head, circle, new LayoutParams(this.px(76), this.px(76)));
      const headingInk = model.colour === Colour.Red ? tone.ink : this.pal.text;
      const h = this.text(model.heading, 27, true, false, headingInk);
      setTextGravity(h, Gravity.CENTER);
      addView(head, h, (() => { const p = this.lp(); p.topMargin = this.px(18); return p; })());
      const w = this.text(model.what, 16, false, false, this.pal.secondary);
      setTextGravity(w, Gravity.CENTER);
      addView(head, w, (() => { const p = this.lp(); p.topMargin = this.px(8); return p; })());
      const wl = new LayoutParams(WRAP_CONTENT, WRAP_CONTENT);
      wl.topMargin = this.px(12);
      wl.gravity = Gravity.CENTER_HORIZONTAL;
      addView(head, this.whoLine(model, 13, this.pal.secondary, true), wl);
      addView(panel, head, this.lp());
      const below = LinearLayout("v");
      setPadding(below, this.px(Space.PAGE_GUTTER), this.px(Space.PAGE_BLOCK), this.px(Space.PAGE_GUTTER), 0);
      const next = LinearLayout("h");
      setGravity(next, Gravity.CENTER_VERTICAL);
      setBackground(next, this.rounded(this.pal.surface, 14, 1, this.pal.divider));
      setPadding(next, this.px(16), this.px(14), this.px(16), this.px(14));
      const ap = new LayoutParams(this.px(20), this.px(20));
      ap.rightMargin = this.px(12);
      addView(next, this.icon(Icon.Arrow, this.pal.text, 20), ap);
      addView(next, this.text(model.next, 15, false, false, this.pal.text, true), new LayoutParams(0, WRAP_CONTENT, 1));
      addView(below, next, this.lp());
      for (const v of this.detailsViews(model)) {
        const p = this.lp();
        p.topMargin = this.px(Space.BLOCK);
        addView(below, v, p);
      }
      addView(panel, below, this.lp());
      return panel;
    }

    /** Ui.kt:765 titleBlock(model, barTitle): the heading at 26 sp bold when it differs from the bar's title, then the what at 15 sp in secondary. */
    titleBlock(model, barTitle) {
      const v = LinearLayout("v");
      if (model.heading !== barTitle) addView(v, this.text(model.heading, 26, true), this.lp());
      const p = this.lp();
      if (model.heading !== barTitle) p.topMargin = this.px(6);
      addView(v, this.text(model.what, 15, false, false, this.pal.secondary), p);
      return v;
    }

    /** Ui.kt:773 hint(model): the next step under the content: an info-circle icon, the next line at 14 sp in secondary and who under it at 13 sp medium. */
    hint(model) {
      const v = LinearLayout("h");
      const ip = new LayoutParams(this.px(20), this.px(20));
      ip.rightMargin = this.px(10);
      ip.topMargin = this.px(1);
      addView(v, this.icon(Icon.InfoCircle, this.pal.secondary, 20, 1.8), ip);
      const col = LinearLayout("v");
      addView(col, this.text(model.next, 14, false, false, this.pal.secondary), this.lp());
      const wp = this.lp();
      wp.topMargin = this.px(4);
      addView(col, this.text(model.who, 13, false, false, this.pal.secondary, true), wp);
      addView(v, col, new LayoutParams(0, WRAP_CONTENT, 1));
      return v;
    }

    /** Ui.kt:787 compactWords(model): the heading at 22 sp bold and the what at 15 sp in secondary, both centred. */
    compactWords(model) {
      const v = LinearLayout("v");
      const h = this.text(model.heading, 22, true);
      setTextGravity(h, Gravity.CENTER);
      addView(v, h, this.lp());
      const w = this.text(model.what, 15, false, false, this.pal.secondary);
      setTextGravity(w, Gravity.CENTER);
      const p = this.lp();
      p.topMargin = this.px(6);
      addView(v, w, p);
      return v;
    }

    /** Ui.kt:795 working(model): centred in the body, a 64 dp ring with an orange arc that turns once a second, the heading at 24 sp bold, the what at 16 sp and the next line at 15 sp medium. */
    working(model) {
      const v = WorkingView();
      setGravity(v, Gravity.CENTER);
      addView(v, SpinnerView(this, this.pal.fieldLine), new LayoutParams(this.px(64), this.px(64)));
      const h = this.text(model.heading, 24, true);
      setTextGravity(h, Gravity.CENTER);
      const hp = this.lp(); hp.topMargin = this.px(24);
      addView(v, h, hp);
      const w = this.text(model.what, 16, false, false, this.pal.secondary);
      setTextGravity(w, Gravity.CENTER);
      const wp = this.lp(); wp.topMargin = this.px(8);
      addView(v, w, wp);
      const n = this.text(model.next, 15, false, false, null, true);
      setTextGravity(n, Gravity.CENTER);
      const np = this.lp(); np.topMargin = this.px(12);
      addView(v, n, np);
      return v;
    }

    // ---- blocks ------------------------------------------------------------------------------------

    /** Ui.kt:810 label(text): a small heading over a group: 13 sp bold, upper case, letter-spaced 0.6, in secondary. */
    label(text) {
      const e = this.text(text, Type.LABEL, true, false, this.pal.secondary);
      e.style.textTransform = "uppercase";
      e.style.letterSpacing = (0.6 / Type.LABEL) + "em";
      return e;
    }

    /** Ui.kt:816 labelled(label, field): the label of a field at 14 sp medium in secondary, 6 dp above [field]. */
    labelled(label, field) {
      const v = LinearLayout("v");
      const p = this.lp();
      p.bottomMargin = this.px(6);
      addView(v, this.text(label, Type.LABELLED, false, false, this.pal.secondary, true), p);
      addView(v, field, this.lp());
      return v;
    }

    /** Ui.kt:827 pinPair(label1, label2): two PIN fields side by side, 12 dp apart, each with its label. */
    pinPair(label1, label2) {
      const pin = () => this.style(this.field(""));
      const a = pin();
      const b = pin();
      a.type = b.type = "password";
      a.inputMode = b.inputMode = "numeric";
      const row = LinearLayout("h");
      const pa = new LayoutParams(0, WRAP_CONTENT, 1); pa.rightMargin = this.px(6);
      const pb = new LayoutParams(0, WRAP_CONTENT, 1); pb.leftMargin = this.px(6);
      addView(row, this.labelled(label1, a), pa);
      addView(row, this.labelled(label2, b), pb);
      return { view: row, first: a, second: b };
    }

    /** Ui.kt:845 chips(options, selected): up to three options as pills (40 dp, corner 20); with more than three, the radio list of [choice]. */
    chips(options, selected = 0) {
      if (options.length > 3) return this.choice(options, selected);
      return this.pills(options, selected, 40);
    }

    /** Ui.kt:872 tabs(options, selected, describe, onPick): two or more tabs side by side (48 dp, corner 20). */
    tabs(options, selected, describe, onPick) {
      const group = this.pills(options, selected, 48);
      group.setAttribute("aria-label", describe(options[selected] || ""));
      let shown = selected;
      group.addEventListener("click", (ev) => {
        const pill = ev.target.closest("[data-pill]");
        if (!pill) return;
        const now = Number(pill.dataset.pill);
        if (now !== shown) {
          shown = now;
          this.restylePills(group, now);
          group.setAttribute("aria-label", describe(options[now]));
          if (onPick) onPick(now);
        }
      });
      return group;
    }

    /** The RadioGroup of PillButtons that [chips] and [tabs] share. */
    pills(options, selected, heightDp) {
      const group = LinearLayout("h");
      group.style.alignItems = "stretch";
      options.forEach((label, i) => {
        const pill = PillButton(this, heightDp);
        pill.textContent = label;
        pill.dataset.pill = String(i);
        const p = new LayoutParams(0, MATCH_PARENT, 1);
        p.leftMargin = i === 0 ? 0 : this.px(4);
        p.rightMargin = i === options.length - 1 ? 0 : this.px(4);
        addView(group, pill, p);
      });
      this.restylePills(group, selected);
      return group;
    }

    /** Ui.kt:1271 PillButton.restyle() for every pill of a group. */
    restylePills(group, selected) {
      const pal = this.pal;
      Array.from(group.children).forEach((pill, i) => {
        const on = i === selected;
        pill.style.color = css(on ? pal.background : pal.text);
        const g = new GradientDrawable();
        g.cornerRadius = this.px(20);
        if (on) g.setColor(pal.text);
        else { g.setColor("transparent"); g.setStroke(this.px(1), pal.fieldLine); }
        g.paint(pill);
        pill.dataset.checked = on ? "1" : "0";
      });
    }

    /** Ui.kt:901 card(radius): a surface card, clipped to its corners. */
    card(radius = 16) {
      const v = LinearLayout("v");
      setBackground(v, this.rounded(this.pal.surface, radius));
      v.style.overflow = "hidden";
      return v;
    }

    /** Ui.kt:908 dot(colour, dp). */
    dot(colour, dp) {
      const v = View();
      const g = new GradientDrawable();
      g.shape = GradientDrawable.OVAL;
      g.setColor(colour);
      setBackground(v, g);
      v.style.width = this.px(dp) + "px";
      v.style.height = this.px(dp) + "px";
      v.style.flex = "none";
      return v;
    }

    /** Ui.kt:918 rows(items): one surface card (corner 16) of rows of at least 56 dp, a 1 dp divider between: an optional tone dot, a title at 16 sp medium, an optional sub, an optional count badge and a chevron. */
    rows(items) {
      const card = this.card();
      items.forEach((item, i) => {
        if (i > 0) addView(card, this.divider());
        const row = LinearLayout("h");
        setGravity(row, Gravity.CENTER_VERTICAL);
        row.style.minHeight = this.px(56) + "px";
        setPadding(row, this.px(16), this.px(10), this.px(12), this.px(10));
        if (item.colour != null) {
          const p = new LayoutParams(this.px(10), this.px(10));
          p.rightMargin = this.px(12);
          addView(row, this.dot(this.pal.of(item.colour).band, 10), p);
        }
        const col = LinearLayout("v");
        addView(col, this.text(item.title, 16, false, false, item.danger ? this.pal.stop.ink : this.pal.text, true));
        if (item.sub != null) addView(col, this.text(item.sub, 13, false, false, this.pal.secondary));
        addView(row, col, new LayoutParams(0, WRAP_CONTENT, 1));
        if (item.count != null) {
          const b = this.text(String(item.count), 13, true, false, this.palette.WHITE);
          setTextGravity(b, Gravity.CENTER);
          b.style.minWidth = this.px(24) + "px";
          setPadding(b, this.px(8), this.px(2), this.px(8), this.px(2));
          setBackground(b, this.rounded(this.night ? this.pal.chip : "#111317", 12));
          const p = new LayoutParams(WRAP_CONTENT, WRAP_CONTENT);
          p.leftMargin = this.px(8);
          addView(row, b, p);
        }
        const cp = new LayoutParams(this.px(20), this.px(20));
        cp.leftMargin = this.px(8);
        addView(row, this.icon(Icon.Chevron, this.pal.hint, 20), cp);
        if (item.onClick) {
          setClickable(row, true, () => item.onClick());
          setBackground(row, this.rippleOver(null, 0));
        }
        addView(card, row, this.lp());
      });
      return card;
    }

    /** Ui.kt:958 facts(pairs): one surface card of lines: the key in secondary on the left and the value at 15 sp medium on the right, in tabular figures. */
    facts(pairs) {
      const card = this.card();
      pairs.forEach((pr, i) => {
        if (i > 0) addView(card, this.divider());
        const row = LinearLayout("h");
        setPadding(row, this.px(16), this.px(14), this.px(16), this.px(14));
        const kp = new LayoutParams(WRAP_CONTENT, WRAP_CONTENT);
        kp.rightMargin = this.px(12);
        addView(row, this.text(pr[0], 15, false, false, this.pal.secondary), kp);
        const val = this.text(pr[1], 15, false, false, null, true);
        setTextGravity(val, Gravity.END);
        val.style.fontFeatureSettings = '"tnum"';
        addView(row, val, new LayoutParams(0, WRAP_CONTENT, 1));
        addView(card, row, this.lp());
      });
      return card;
    }

    /** Ui.kt:980 status(rows): one surface card of lines: a tone dot, a title in bold and its words under it. */
    status(rows) {
      const card = this.card();
      rows.forEach((item, i) => {
        if (i > 0) addView(card, this.divider());
        const row = LinearLayout("h");
        setPadding(row, this.px(16), this.px(14), this.px(16), this.px(14));
        const dp = new LayoutParams(this.px(10), this.px(10));
        dp.rightMargin = this.px(12);
        dp.topMargin = this.px(6);
        addView(row, this.dot(this.pal.of(item.colour).band, 10), dp);
        const col = LinearLayout("v");
        addView(col, this.text(item.title, 16, true));
        const wp = this.lp();
        wp.topMargin = this.px(2);
        addView(col, this.text(item.words, 14, false, false, this.pal.secondary), wp);
        addView(row, col, new LayoutParams(0, WRAP_CONTENT, 1));
        addView(card, row, this.lp());
      });
      return card;
    }

    /** Ui.kt:1002 notice(model): a tinted panel inside the gutter, corner 14: the heading in the tone's ink and bold, then the what in the text colour. */
    notice(model) {
      const tone = this.toneOf(model);
      const v = LinearLayout("v");
      setBackground(v, this.rounded(tone.tint, 14));
      setPadding(v, this.px(16), this.px(14), this.px(16), this.px(14));
      addView(v, this.text(model.heading, 16, true, false, tone.ink), this.lp());
      const p = this.lp();
      p.topMargin = this.px(4);
      addView(v, this.text(model.what, 15), p);
      return v;
    }

    /** Ui.kt:1015 wordCards(words): the check words as a 2 by 2 grid of surface cards with a 2 dp outline in the text colour. */
    wordCards(words) {
      const grid = LinearLayout("v");
      for (let start = 0; start < words.length; start += 2) {
        const row = LinearLayout("h");
        for (let k = 0; k < 2; k++) {
          const at = start + k;
          if (at >= words.length) {
            addView(row, View(), new LayoutParams(0, 1, 1));
            continue;
          }
          const c = LinearLayout("v");
          setBackground(c, this.rounded(this.pal.surface, 14, 2, this.pal.text));
          setPadding(c, this.px(14), this.px(12), this.px(14), this.px(14));
          addView(c, this.text(String(at + 1), 13, true, false, this.pal.secondary));
          const wp = this.lp();
          wp.topMargin = this.px(2);
          addView(c, this.text(words[at], 24, true), wp);
          const p = new LayoutParams(0, WRAP_CONTENT, 1);
          p.leftMargin = k === 0 ? 0 : this.px(5);
          p.rightMargin = k === 0 ? this.px(5) : 0;
          p.bottomMargin = start + 2 < words.length ? this.px(10) : 0;
          addView(row, c, p);
        }
        addView(grid, row, this.lp());
      }
      return grid;
    }

    /** Ui.kt:1048 receipt(lines): a payment to approve: one surface card with a 5 dp brand strip at the top and a line per row, with dividers. */
    receipt(lines) {
      const card = this.card();
      const strip = View();
      strip.style.backgroundColor = css(this.palette.BRAND);
      addView(card, strip, this.lp(MATCH_PARENT, this.px(5)));
      lines.forEach((l, i) => {
        if (i > 0) addView(card, this.divider());
        const t = l.style === LineStyle.Strong ? this.text(l.text, 18, true)
          : l.style === LineStyle.Small ? this.text(l.text, 13, false, false, this.pal.secondary)
            : this.text(l.text, 16);
        setPadding(t, this.px(16), this.px(12), this.px(16), this.px(12));
        addView(card, t, this.lp());
      });
      return card;
    }

    /** Ui.kt:1065 tiles(items): a two-column grid of 92 dp surface cards, each with a tone dot at the top and the title at the bottom. */
    tiles(items) {
      const grid = LinearLayout("v");
      for (let start = 0; start < items.length; start += 2) {
        const row = LinearLayout("h");
        for (let k = 0; k < 2; k++) {
          const at = start + k;
          const params = new LayoutParams(0, this.px(92), 1);
          params.leftMargin = k === 0 ? 0 : this.px(6);
          params.rightMargin = k === 0 ? this.px(6) : 0;
          params.bottomMargin = start + 2 < items.length ? this.px(12) : 0;
          if (at >= items.length) {
            addView(row, View(), params);
            continue;
          }
          const item = items[at];
          const c = FrameLayout();
          setBackground(c, this.rippleOver(this.rounded(this.pal.surface, 16), 16));
          setPadding(c, this.px(16), this.px(14), this.px(16), this.px(14));
          if (item.onClick) setClickable(c, true, () => item.onClick());
          const dp = new LayoutParams(this.px(10), this.px(10));
          dp.gravity = Gravity.TOP | Gravity.START;
          addView(c, this.dot(this.pal.of(item.colour).band, 10), dp);
          const tp = new LayoutParams(WRAP_CONTENT, WRAP_CONTENT);
          tp.gravity = Gravity.BOTTOM | Gravity.START;
          addView(c, this.text(item.title, 16, false, false, null, true), tp);
          addView(row, c, params);
        }
        addView(grid, row, this.lp());
      }
      return grid;
    }

    /** Ui.kt:1099 lock(): a 72 dp surface square with a padlock, centred, above the words of a PIN screen. */
    lock() {
      const v = LinearLayout("h");
      setGravity(v, Gravity.CENTER_HORIZONTAL);
      const square = FrameLayout();
      setBackground(square, this.rounded(this.pal.surface, 20, 1, this.pal.divider));
      const ip = new LayoutParams(this.px(34), this.px(34));
      ip.gravity = Gravity.CENTER;
      addView(square, this.icon(Icon.Padlock, this.pal.text, 34, 1.8), ip);
      addView(v, square, new LayoutParams(this.px(72), this.px(72)));
      v.style.alignSelf = "stretch";
      return v;
    }

    /** Ui.kt:1114 keypad(onDigit, onDelete): six dots for the entered digits, then a 3 by 4 grid of 64 dp round keys. */
    keypad(onDigit, onDelete) { return Keypad(this, onDigit, onDelete); }

    /** Ui.kt:1117 codeCard(view): a white card, corner 24, 16 dp padding, with a soft shadow, round a QR view. */
    codeCard(view) {
      const v = LinearLayout("h");
      setGravity(v, Gravity.CENTER_HORIZONTAL);
      const c = FrameLayout();
      setBackground(c, this.rounded(this.palette.WHITE, 24));
      setPadding(c, this.px(16), this.px(16), this.px(16), this.px(16));
      // elevation 10 dp: the ambient and the key shadow of Material's 10 dp
      c.style.boxShadow = "0 10px 20px rgba(0,0,0,0.16), 0 3px 6px rgba(0,0,0,0.10)";
      const p = new LayoutParams(WRAP_CONTENT, WRAP_CONTENT);
      p.gravity = Gravity.CENTER;
      addView(c, view, p);
      addView(v, c, new LayoutParams(WRAP_CONTENT, WRAP_CONTENT));
      v.style.alignSelf = "stretch";
      return v;
    }

    /** Ui.kt:1136 cameraFrame(preview): the scanner's body. */
    cameraFrame(preview) { return CameraFrame(this, preview); }

    /** Ui.kt:1140 barInsets(insets): the left, top, right and bottom insets of the system bars, the cutout and the keyboard. */
    barInsets() { return this.insets; }

    /**
     * Ui.kt:1161 edgeToEdge(bandOnTop): the bars' icons are dark on the page's light background and light on a dark
     * one or on a band. Returns what sets the icons, as the Kotlin one does; here it sets them on the context's window.
     */
    edgeToEdge(bandOnTop) {
      const lightStatus = !bandOnTop && !this.night;
      const lightNav = !this.night;
      return () => {
        this.window.statusLight = lightStatus;
        this.window.navLight = lightNav;
      };
    }

    /** Ui.kt:1188 decorUnderBars(): the window lays its content out behind the bars; a device here always does. */
    decorUnderBars() {}
  }

  /** Icons are drawn white-on-dark: the colour of an ink that has an alpha, as an 8-digit string. */
  function withAlpha(colour, alpha) {
    const c = css(colour);
    const m = /^#([0-9a-f]{6})$/i.exec(c);
    if (!m) return c;
    const r = parseInt(m[1].slice(0, 2), 16), g = parseInt(m[1].slice(2, 4), 16), b = parseInt(m[1].slice(4, 6), 16);
    return `rgba(${r},${g},${b},${+(alpha / 255).toFixed(3)})`;
  }

  // ---- views of Ui.kt ---------------------------------------------------------------------------------------

  /** Ui.kt:59 Bleed and Ui.kt:1209 BarView: the bar takes the status bar's height as top padding. */
  function BarView(ui, onBlack) {
    const v = LinearLayout("v");
    v.isBar = true;
    v.isBleed = true;
    v.onBlack = onBlack;
    v.row = null;
    v.setTopInset = (px) => { v.style.paddingTop = px + "px"; };
    return v;
  }

  /** Ui.kt:1218 ResultPanel: edge to edge. */
  function ResultPanel() {
    const v = LinearLayout("v");
    v.isFullWidth = true;
    return v;
  }

  /** Ui.kt:1221 WorkingView: it fills the body and centres what it holds. */
  function WorkingView() {
    const v = LinearLayout("v");
    v.isFills = true;
    return v;
  }

  /** Ui.kt:1224 SpinnerView: a 64 dp ring with an arc that turns once a second. */
  function SpinnerView(ui, track) {
    const v = viewEl("div", "spinner");
    const w = 5;
    v.innerHTML = `<svg width="100%" height="100%" viewBox="0 0 64 64"><circle cx="32" cy="32" r="${32 - w / 2}" fill="none" stroke="${css(track)}" stroke-width="${w}"/>`
      + `<path d="${arcPath(32, 32, 32 - w / 2, -90, 10)}" fill="none" stroke="${css(ui.palette.BRAND)}" stroke-width="${w}" stroke-linecap="round"/></svg>`;
    return v;
  }

  /** An SVG arc path from [start] degrees for [sweep] degrees, clockwise (the canvas's drawArc). */
  function arcPath(cx, cy, r, start, sweep) {
    const a0 = (start * Math.PI) / 180;
    const a1 = ((start + sweep) * Math.PI) / 180;
    return `M ${cx + r * Math.cos(a0)} ${cy + r * Math.sin(a0)} A ${r} ${r} 0 ${sweep > 180 ? 1 : 0} 1 ${cx + r * Math.cos(a1)} ${cy + r * Math.sin(a1)}`;
  }

  /** Ui.kt:1271 PillButton: one option of chips or tabs. */
  function PillButton(ui, heightDp = 40) {
    const v = viewEl("div", "tv clickable");
    v.dataset.fp = "15";
    v.style.fontSize = "15px";
    v.style.fontWeight = "500";
    v.style.minHeight = ui.px(heightDp) + "px";
    setTextGravity(v, Gravity.CENTER);
    v.style.display = "flex";
    v.style.alignItems = "center";
    v.style.justifyContent = "center";
    v.style.textAlign = "center";
    setPadding(v, ui.px(12), ui.px(8), ui.px(12), ui.px(8));
    return v;
  }

  /** Ui.kt:1313 RowItem. */
  function RowItem(title, sub = null, colour = null, count = null, danger = false, onClick = null) {
    return { title, sub, colour, count, danger, onClick };
  }

  /** Ui.kt:1323 StatusItem. */
  function StatusItem(colour, title, words) { return { colour, title, words }; }

  /** Ui.kt:1328 ReceiptLine. */
  function ReceiptLine(text, style = LineStyle.Plain) { return { text, style }; }

  /** Ui.kt:1331 TileItem. */
  function TileItem(colour, title, onClick = null) { return { colour, title, onClick }; }

  /** Ui.kt:1334 Keypad: [entered] says how many of the six dots are filled. */
  function Keypad(ui, onDigit, onDelete) {
    const DOTS = 6;
    const v = LinearLayout("v");
    setGravity(v, Gravity.CENTER_HORIZONTAL);
    const dots = [];
    const dotRow = LinearLayout("h");
    setGravity(dotRow, Gravity.CENTER);
    for (let i = 0; i < DOTS; i++) {
      const d = View();
      dots.push(d);
      const p = new LayoutParams(ui.px(14), ui.px(14));
      p.setMargins(ui.px(7), 0, ui.px(7), 0);
      d.style.flex = "none";
      addView(dotRow, d, p);
    }
    const dp = new LayoutParams(WRAP_CONTENT, WRAP_CONTENT);
    dp.bottomMargin = ui.px(24);
    addView(v, dotRow, dp);
    v.entered = 0;
    const paintDots = () => {
      const pal = ui.pal;
      dots.forEach((d, i) => {
        const g = new GradientDrawable();
        g.shape = GradientDrawable.OVAL;
        if (i < v.entered) g.setColor(pal.text);
        else { g.setColor("transparent"); g.setStroke(ui.px(2), pal.fieldLine); }
        g.paint(d);
      });
    };
    v.setEntered = (n) => { v.entered = Math.max(0, Math.min(DOTS, n)); paintDots(); };
    paintDots();
    const keys = [1, 2, 3, 4, 5, 6, 7, 8, 9, -1, 0, -2];
    const key = (k) => {
      if (k === -1) return View();
      const pal = ui.pal;
      const t = ui.text(k === -2 ? "⌫" : String(k), 26, false, false, pal.text, true);
      setTextGravity(t, Gravity.CENTER);
      t.style.display = "flex";
      t.style.alignItems = "center";
      t.style.justifyContent = "center";
      const bg = new GradientDrawable();
      bg.shape = GradientDrawable.OVAL;
      bg.setColor(pal.surface);
      bg.setStroke(ui.px(1), pal.divider);
      setBackground(t, ui.rippleOver(bg, 32));
      setClickable(t, true, () => {
        if (k === -2) { v.setEntered(v.entered - 1); if (onDelete) onDelete(); } else { v.setEntered(v.entered + 1); if (onDigit) onDigit(k); }
      });
      return t;
    };
    for (let r = 0; r < 4; r++) {
      const row = LinearLayout("h");
      setGravity(row, Gravity.CENTER);
      for (let c = 0; c < 3; c++) {
        const p = new LayoutParams(ui.px(64), ui.px(64));
        p.setMargins(ui.px(10), ui.px(6), ui.px(10), ui.px(6));
        addView(row, key(keys[r * 3 + c]), p);
      }
      addView(v, row, new LayoutParams(WRAP_CONTENT, WRAP_CONTENT));
    }
    return v;
  }

  /**
   * Ui.kt:1425 CameraFrame(ui, preview): the preview on black, a 280 dp frame with the rest dimmed, an orange scan
   * line, the words at the bottom in white and a pill for the progress. Its [words] and [progress] are the
   * Kotlin functions of the same names (Ui.kt:1464, Ui.kt:1478).
   */
  function CameraFrame(ui, preview) {
    const v = FrameLayout();
    v.style.backgroundColor = "#000000";
    v.style.position = "relative";
    v.style.overflow = "hidden";
    v.isFills = true;
    if (preview) {
      const p = new LayoutParams(MATCH_PARENT, MATCH_PARENT);
      p.gravity = Gravity.CENTER;
      addView(v, preview, p);
    }
    const overlay = View();
    // Overlay.onDraw (Ui.kt:1491): 280 dp square 40 dp above the middle, corner 24, dimmed to 0x73, white 3 dp frame,
    // a 12 dp glow 0x33F7931A and a 3 dp orange line across the middle.
    overlay.style.position = "relative";
    overlay.innerHTML = `<div style="position:absolute;left:50%;top:calc(50% - 40px);width:280px;height:280px;transform:translate(-50%,-50%);border-radius:24px;box-shadow:0 0 0 2000px rgba(0,0,0,0.451);border:3px solid ${css(ui.palette.WHITE)}"></div>`
      + `<div style="position:absolute;left:calc(50% - 128px);top:calc(50% - 40px - 6px);width:256px;height:12px;border-radius:6px;background:rgba(247,147,26,0.2)"></div>`
      + `<div style="position:absolute;left:calc(50% - 128px);top:calc(50% - 40px - 1.5px);width:256px;height:3px;border-radius:1.5px;background:${css(ui.palette.BRAND)}"></div>`;
    addView(v, overlay, new LayoutParams(MATCH_PARENT, MATCH_PARENT));
    const words = LinearLayout("v");
    setGravity(words, Gravity.CENTER_HORIZONTAL);
    setPadding(words, ui.px(Space.PAGE_GUTTER), 0, ui.px(Space.PAGE_GUTTER), ui.px(Space.PAGE_EDGE));
    const pill = LinearLayout("h");
    setGravity(pill, Gravity.CENTER_VERTICAL);
    setPadding(pill, ui.px(14), ui.px(8), ui.px(16), ui.px(8));
    const pg = new GradientDrawable();
    pg.setColor("rgba(0,0,0,0.6)");
    pg.cornerRadius = ui.px(20);
    setBackground(pill, pg);
    const dp = new LayoutParams(ui.px(8), ui.px(8));
    dp.rightMargin = ui.px(8);
    addView(pill, ui.dot(ui.palette.BRAND, 8), dp);
    const progress = ui.text("", 14, false, false, ui.palette.WHITE);
    addView(pill, progress);
    setVisible(pill, false);
    const pp = new LayoutParams(WRAP_CONTENT, WRAP_CONTENT);
    pp.bottomMargin = ui.px(12);
    addView(words, pill, pp);
    const gp = new LayoutParams(MATCH_PARENT, WRAP_CONTENT);
    gp.gravity = Gravity.BOTTOM;
    addView(v, words, gp);
    const wordViews = [];
    /** Ui.kt:1464 words(model): the heading, the what and the next line in white; null clears them. */
    v.words = (model) => {
      for (const w of wordViews) w.remove();
      wordViews.length = 0;
      if (!model) return;
      const h = ui.text(model.heading, 20, true, false, ui.palette.WHITE);
      const w = ui.text(model.what, 15, false, false, ui.palette.WHITE);
      const n = ui.text(model.next, 15, false, false, ui.palette.WHITE, true);
      for (const t of [h, w, n]) {
        setTextGravity(t, Gravity.CENTER);
        const p = new LayoutParams(MATCH_PARENT, WRAP_CONTENT);
        p.topMargin = ui.px(4);
        addView(words, t, p);
        wordViews.push(t);
      }
    };
    /** Ui.kt:1478 progress(line): the progress line as a pill with an orange dot; null hides it. */
    v.progress = (line) => {
      setVisible(pill, line != null);
      progress.textContent = line == null ? "" : line;
    };
    return v;
  }

  /**
   * Ui.kt:71 WordsView(context, model, showBand): one words on screen: a coloured band with the heading, the two
   * sentences, who does the next step, quoted text, and a "Details" fold. [setTopInset] makes it the first view of a page.
   */
  function WordsView(ui, model, showBand = true) {
    const v = LinearLayout("v");
    v.isBleed = true;
    const band = LinearLayout("v");
    const body = LinearLayout("v");
    const shape = new GradientDrawable();
    shape.setColor(colourBand(ui.palette, model.colour, ui.night));
    shape.cornerRadius = ui.px(Space.RADIUS);
    setBackground(band, shape);
    setPadding(band, ui.px(Space.GUTTER), ui.px(Space.BLOCK), ui.px(Space.GUTTER), ui.px(Space.BLOCK));
    addView(band, ui.text(model.heading, Type.HEADING, true, false, ui.palette.WHITE));
    if (showBand) addView(v, band);
    const pad = (t, top) => { setPadding(t, 0, ui.px(top), 0, 0); return t; };
    addView(body, pad(ui.text(model.what, Type.BODY), Space.BLOCK));
    addView(body, pad(ui.text(model.next, Type.BODY, true), 8));
    addView(body, pad(ui.text(model.who, Type.SECONDARY, false, false, ui.pal.secondary), 4));
    if (model.quotedText != null) addView(body, ui.quoted(model.quotedLabel || "", model.quotedText), ui.margins(Space.BLOCK));
    if (model.details && model.details.length) {
      const fold = ui.text(model.details.join("\n"), Type.CODE - 2, false, true);
      setVisible(fold, false);
      const toggle = ui.text("Details (show)", Type.SECONDARY, true, false, ui.pal.info.ink);
      toggle.style.minHeight = ui.px(Space.TOUCH) + "px";
      setTextGravity(toggle, Gravity.CENTER_VERTICAL);
      toggle.classList.add("clickable");
      toggle.addEventListener("click", () => {
        const open = fold.classList.contains("gone");
        setVisible(fold, open);
        toggle.textContent = `Details (${open ? "hide" : "show"})`;
      });
      addView(body, toggle);
      addView(body, fold);
    }
    addView(v, body);
    v.setTopInset = (px) => {
      shape.cornerRadius = 0;
      shape.paint(band);
      setPadding(band, ui.px(Space.GUTTER), ui.px(Space.BLOCK) + px, ui.px(Space.GUTTER), ui.px(Space.BLOCK));
      setPadding(body, ui.px(Space.GUTTER), 0, ui.px(Space.GUTTER), 0);
    };
    return v;
  }

  /**
   * Ui.kt:122 SaleBand(context): the top of a counter's sale screen: a band in the colour of the state that fills a
   * third of the height, the state in large type and the amount at 44 sp under it. [show] is Ui.kt:142.
   */
  function SaleBand(ui, heightPx = 915) {
    const v = LinearLayout("v");
    v.isBleed = true;
    setGravity(v, Gravity.CENTER_VERTICAL);
    v.style.minHeight = Math.floor(heightPx / 3) + "px";
    const heading = ui.text("", Type.STATE, true, false, ui.palette.WHITE);
    const amount = ui.text("", Type.AMOUNT, true, false, ui.palette.WHITE);
    const line = ui.text("", Type.BODY, false, false, ui.palette.WHITE);
    addView(v, heading);
    addView(v, amount);
    addView(v, line);
    v.style.backgroundColor = css(colourBand(ui.palette, Colour.Neutral, ui.night));
    v.setTopInset = (px) => setPadding(v, ui.px(Space.GUTTER), px + ui.px(Space.BLOCK), ui.px(Space.GUTTER), ui.px(Space.BLOCK));
    v.setTopInset(0);
    /** Ui.kt:142 show(model, amountText, lineText). */
    v.show = (model, amountText, lineText) => {
      v.style.backgroundColor = css(colourBand(ui.palette, model.colour, ui.night));
      heading.textContent = model.heading;
      amount.textContent = amountText;
      line.textContent = lineText;
      setVisible(line, lineText !== "");
    };
    return v;
  }

  /** Ui.kt:38 isNight(c): true when the phone is in night mode. */
  function isNight(c) { return !!c.night; }

  /**
   * Ui.kt:47 applyNight(): the window's own colour set again from the palette. Returns the colour the window's
   * background takes, for the device to paint.
   */
  function applyNight(ui) { return ui.pal.background; }

  // ---- Icons.kt:15 IconDrawable ---------------------------------------------------------------------------------

  /** Icons.kt:15 IconDrawable(icon, colour, strokeWidth): the icon as an SVG on the 24 by 24 grid, a stroke with round ends. */
  function IconView(icon, colour, strokeWidth = 2) {
    const c = css(colour);
    const stroke = `fill="none" stroke="${c}" stroke-width="${strokeWidth}" stroke-linecap="round" stroke-linejoin="round"`;
    const fill = `fill="${c}" stroke="none"`;
    let body;
    switch (icon) {
      case Icon.Back: body = `<path ${stroke} d="M19 12L5 12M12 5L5 12L12 19"/>`; break;
      case Icon.Arrow: body = `<path ${stroke} d="M5 12L19 12M13 6L19 12L13 18"/>`; break;
      case Icon.Chevron: body = `<path ${stroke} d="M9 6L15 12L9 18"/>`; break;
      case Icon.Tick: body = `<path ${stroke} d="M5 12.5L10 17.5L19 7"/>`; break;
      case Icon.Dash: body = `<path ${stroke} d="M6 12L18 12"/>`; break;
      case Icon.Exclaim: body = `<path ${stroke} d="M12 5L12 13.5"/><circle ${fill} cx="12" cy="18.5" r="1.4"/>`; break;
      case Icon.Info: body = `<circle ${fill} cx="12" cy="6.5" r="1.4"/><path ${stroke} d="M12 11L12 19"/>`; break;
      case Icon.InfoCircle: body = `<circle ${stroke} cx="12" cy="12" r="9"/><circle ${fill} cx="12" cy="7.8" r="1.1"/><path ${stroke} d="M12 11L12 17"/>`; break;
      case Icon.Person: body = `<circle ${stroke} cx="12" cy="8" r="3.6"/><path ${stroke} d="M5.5 20C5.5 15.8 8.4 14 12 14C15.6 14 18.5 15.8 18.5 20"/>`; break;
      case Icon.Gear: {
        let spokes = "";
        for (let i = 0; i < 8; i++) spokes += `<path ${stroke} transform="rotate(${i * 45} 12 12)" d="M12 2.8L12 5"/>`;
        body = `<circle ${stroke} cx="12" cy="12" r="3"/><circle ${stroke} cx="12" cy="12" r="7"/>${spokes}`;
        break;
      }
      case Icon.Padlock: body = `<rect ${stroke} x="5" y="11" width="14" height="10" rx="2.5"/><path ${stroke} d="M8 11L8 8A4 5 0 0 1 16 8L16 11"/><circle ${fill} cx="12" cy="16" r="1.2"/>`; break;
      default: body = "";
    }
    const v = viewEl("div", "");
    v.innerHTML = `<svg viewBox="0 0 24 24" width="100%" height="100%" style="display:block;overflow:visible">${body}</svg>`;
    return v;
  }

  const api = {
    UI_KT, MATCH_PARENT, WRAP_CONTENT, Gravity, LayoutParams, LinearLayout, FrameLayout, View, ScrollView, GradientDrawable, RippleDrawable,
    setGravity, setTextGravity, setPadding, setMinHeight, setVisible, addView, setBackground, setRippleOnly, setClickable, css, viewEl,
    Type, Space, Colour, ButtonKind, LineStyle, Icon, PaletteOf, Scheme, colourBand, Ui, withAlpha,
    BarView, ResultPanel, WorkingView, SpinnerView, PillButton, RowItem, StatusItem, ReceiptLine, TileItem, Keypad, CameraFrame,
    WordsView, SaleBand, isNight, applyNight, IconView, arcPath,
  };
  root.OpenBmtUi = api;
  if (typeof module !== "undefined" && module.exports) module.exports = api;
})(typeof globalThis !== "undefined" ? globalThis : this);
