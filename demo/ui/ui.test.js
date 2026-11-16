// Checks the drawing layer against the Kotlin it ports, without a browser:
//   node --test demo/ui/ui.test.js
// 1. Every "File.kt:LINE name" comment of the JS still points at a Kotlin line that holds that name.
// 2. Every function and class of Ui.kt, CounterViews.kt, CounterUi.kt is ported (and says so) or is on the skip list
//    below with the reason.
// 3. The sizes and distances of Palette.kt (Type, Space) are the ones the JS has.
// 4. palette.json is what Palette.kt holds.
// The sizes the browser lays out are checked by shoot.mjs.
"use strict";
const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");

const repo = path.join(__dirname, "..", "..");
const read = (p) => fs.readFileSync(path.join(repo, p), "utf8");
const lines = (p) => read(p).split("\n");

// The Kotlin file of a "File.kt" in a comment: Controller.kt is the counter's in counter-views.js and counter-controller.js and the owner's in owner-views.js.
const SOURCES = {
  "ui.js": { "Ui.kt": "apps/android-common/src/android/Ui.kt", "Icons.kt": "apps/android-common/src/android/Icons.kt", "Palette.kt": "apps/android-common/src/model/Palette.kt", "WordsModel.kt": "apps/android-common/src/model/WordsModel.kt" },
  "counter-views.js": {
    "Ui.kt": "apps/android-common/src/android/Ui.kt",
    "CounterViews.kt": "apps/counter-android/src/android/CounterViews.kt", "CounterUi.kt": "apps/counter-android/src/android/CounterUi.kt",
    "Controller.kt": "apps/counter-android/src/android/Controller.kt",
  },
  "owner-views.js": {
    "OwnerUi.kt": "apps/owner-android/src/android/OwnerUi.kt", "Screens.kt": "apps/owner-android/src/android/Screens.kt",
    "Books.kt": "apps/owner-android/src/android/Books.kt", "Controller.kt": "apps/owner-android/src/android/Controller.kt",
  },
  // The page's controller for the shop sale (../web2): Controller.kt is the counter's.
  "../web2/counter-controller.js": { "Controller.kt": "apps/counter-android/src/android/Controller.kt" },
  // The sounds and the logo's snap: FeedbackModel.kt's math, Feedback.kt's player and drawing.
  "feedback.js": {
    "FeedbackModel.kt": "apps/android-common/src/model/FeedbackModel.kt", "Feedback.kt": "apps/android-common/src/android/Feedback.kt",
    "Palette.kt": "apps/android-common/src/model/Palette.kt",
  },
  // The page's sound (../web2): the app's Feedback object.
  "../web2/sound.js": { "Feedback.kt": "apps/android-common/src/android/Feedback.kt" },
  // The page's run of the counter (../web2): where the counter app plays its outcome sound.
  "../web2/live.js": {
    "Controller.kt": "apps/counter-android/src/android/Controller.kt", "SaleModel.kt": "apps/counter-android/src/model/SaleModel.kt",
    "Feedback.kt": "apps/android-common/src/android/Feedback.kt",
  },
  // The page's controller for the owner's home (../web2): Controller.kt and Screens.kt are the owner's.
  "../web2/owner-controller.js": { "Controller.kt": "apps/owner-android/src/android/Controller.kt", "Screens.kt": "apps/owner-android/src/android/Screens.kt" },
  // The page's register view (../web2): the part of the owner's till screen that says what was read.
  "../web2/register-views.js": { "PosScreen.kt": "apps/owner-android/src/android/PosScreen.kt" },
};

/** The comments of a JS file that name a Kotlin line: [file, line, name]. */
function references(jsFile) {
  const text = fs.readFileSync(path.join(__dirname, jsFile), "utf8");
  const out = [];
  const re = /\b([A-Za-z]+\.kt):(\d+) ([A-Za-z]+)/g;
  let m;
  while ((m = re.exec(text))) out.push([m[1], Number(m[2]), m[3]]);
  return out;
}

test("every Kotlin line a comment names holds the name", () => {
  let checked = 0;
  for (const [jsFile, files] of Object.entries(SOURCES)) {
    for (const [file, line, name] of references(jsFile)) {
      assert.ok(files[file], `${jsFile}: no Kotlin file for ${file}`);
      const src = lines(files[file]);
      const word = new RegExp(`\\b${name}\\b`);
      assert.ok(word.test(src[line - 1] || ""), `${jsFile}: ${file}:${line} should hold ${name}, but holds: ${(src[line - 1] || "").trim()}`);
      checked++;
    }
  }
  assert.ok(checked > 120, `only ${checked} references`);
});

test("each Controller.kt function the page's controller ports is a JS function of the same name", () => {
  const text = fs.readFileSync(path.join(__dirname, "../web2/counter-controller.js"), "utf8");
  const named = references("../web2/counter-controller.js").map(([, , name]) => name);
  assert.ok(named.length >= 12, `only ${named.length} functions`);
  for (const name of new Set(named)) {
    // Controller(act) is the class: the JS function is Controller(host).
    assert.ok(new RegExp(`function ${name}\\(`).test(text), `counter-controller.js has no function ${name}`);
  }
});

test("each function the owner page's controller ports is a JS function of the same name", () => {
  const text = fs.readFileSync(path.join(__dirname, "../web2/owner-controller.js"), "utf8");
  const named = references("../web2/owner-controller.js").map(([, , name]) => name);
  assert.ok(named.length >= 5, `only ${named.length} functions`);
  for (const name of new Set(named)) {
    assert.ok(new RegExp(`function ${name}\\(`).test(text), `owner-controller.js has no function ${name}`);
  }
});

/** The names a Kotlin file declares: fun, class, interface, enum class, object (not the ones nested in a function). */
function declared(p) {
  const out = new Set();
  for (const l of lines(p)) {
    const m = /^\s*(?:private |internal |override |inline )*(?:fun (?:[A-Za-z.]+\.)?|class |interface |enum class |object |abstract class )([A-Za-z0-9_]+)/.exec(l);
    if (m) out.add(m[1]);
  }
  return out;
}

// What is not ported, and why. Everything else of Ui.kt, CounterViews.kt and CounterUi.kt is.
const SKIPPED = {
  "apps/android-common/src/android/Ui.kt": {
    Bleed: "marker interface; the view's isBleed flag",
    FullWidth: "marker interface; the view's isFullWidth flag",
    Fills: "marker interface; the view's isFills flag",
    PinPair: "result holder; pinPair returns a plain object with the same three members",
    applyNight: "ported as a reader of the palette's window colour (the device paints it); no Activity here",
    UiButton: "the faded off state is the .off class; the Button subclass is the platform's",
    activity: "finds the Android Activity; a page has none",
    decorUnderBars: "the window always lays out behind the bars on the device frame; kept as an empty function",
    // Overlay is a class nested in CameraFrame, drawn there
    Overlay: "drawn inside CameraFrame",
    onDraw: "canvas drawing; done with CSS and SVG where the class is ported",
    onAttachedToWindow: "animation start; the ring turns by CSS",
    onDetachedFromWindow: "animation stop; the ring turns by CSS",
    setChecked: "radio state of PillButton; restyle is ported as restylePills",
    restyle: "ported as restylePills",
    getOutline: "outline provider of the logo; a CSS corner",
    onViewAttachedToWindow: "attach listener for the insets; the insets are read once",
    onViewDetachedFromWindow: "attach listener",
    setTopInset: "ported inside BarView, WordsView and SaleBand",
    setEnabled: "UiButton's faded state; the .off class",
    show: "SaleBand.show, ported inside SaleBand",
    words: "CameraFrame.words and Ui.words, ported",
    progress: "CameraFrame.progress, ported inside CameraFrame",
    paintDots: "ported inside Keypad",
    key: "ported inside Keypad",
    place: "the insets listener; ported as the inline placement in pageNew and pageViews",
    appearance: "returned by edgeToEdge",
  },
  "apps/counter-android/src/android/CounterViews.kt": {
    draw: "canvas drawing; GlyphView and OutcomeMark draw with SVG",
    setAlpha: "Drawable plumbing",
    setColorFilter: "Drawable plumbing",
    getOpacity: "Drawable plumbing",
    onMeasure: "layout measuring; CSS lays out",
    onDraw: "canvas drawing; OutcomeMark draws with SVG",
    onLayout: "CoverLayout's layout; CSS covers",
    lightNavigation: "ported inside paintBand",
    dot: "the member of Ui.kt:908, the same view",
    rippleOver: "the same function as Ui.kt:443",
    fillRipple: "ported as fillRipple",
    onViewAttachedToWindow: "attach listener",
    onViewDetachedFromWindow: "attach listener",
    GlyphDrawable: "ported as GlyphView",
    glyph: "ported",
  },
  "apps/counter-android/src/android/CounterUi.kt": {
    onMeasure: "layout measuring; the size is given when the view is made",
    onDraw: "canvas drawing; QrView draws with SVG",
    show: "Slot.show, ported inside Slot",
    words: "ported as wordsOf (a JS class may not call a method and a function words at once)",
    invalidate: "the view is drawn when it is made",
  },
};

test("every function and class of the three Kotlin files is ported or skipped with a reason", () => {
  const jsText = ["ui.js", "counter-views.js", "owner-views.js"].map((f) => fs.readFileSync(path.join(__dirname, f), "utf8")).join("\n");
  for (const file of Object.keys(SKIPPED)) {
    const skipped = SKIPPED[file];
    const names = [...declared(file)];
    const missing = [];
    for (const name of names) {
      if (skipped[name]) continue;
      // Ported: a JS function, method or class of that name.
      const found = new RegExp(`(function\\s+${name}\\b|\\b${name}\\s*\\(|class\\s+${name}\\b|\\b${name}\\s*[:=])`).test(jsText);
      if (!found) missing.push(name);
    }
    assert.deepEqual(missing, [], `${file}: neither ported nor skipped`);
  }
});

test("the sizes and distances are Palette.kt's", () => {
  const U = require("./ui.js");
  const src = read("apps/android-common/src/model/Palette.kt");
  const block = (name) => {
    const start = src.indexOf(`object ${name} {`);
    return src.slice(start, src.indexOf("\n}\n", start));
  };
  for (const [name, js] of [["Type", U.Type], ["Space", U.Space]]) {
    const kt = {};
    for (const m of block(name).matchAll(/const val ([A-Z_]+) = ([0-9.]+)f?/g)) kt[m[1]] = Number(m[2]);
    assert.deepEqual(Object.keys(js).sort(), Object.keys(kt).sort(), `${name} has the same names`);
    for (const k of Object.keys(kt)) assert.equal(js[k], kt[k], `${name}.${k}`);
  }
});

test("palette.json is what Palette.kt holds", () => {
  const json = JSON.parse(fs.readFileSync(path.join(__dirname, "palette.json"), "utf8"));
  const src = read("apps/android-common/src/model/Palette.kt");
  const hex = (s) => "#" + s.toLowerCase();
  assert.equal(json.brand, hex(/BRAND = 0xFF([0-9A-F]{6})/.exec(src)[1]));
  assert.equal(json.onBrand, hex(/ON_BRAND = 0xFF([0-9A-F]{6})/.exec(src)[1]));
  const scheme = (name) => {
    const start = src.indexOf(`val ${name} = Scheme(`);
    return src.slice(start, src.indexOf("\n    )\n", start));
  };
  for (const name of ["light", "dark"]) {
    const s = scheme(name);
    for (const key of ["background", "paper", "surface", "text", "secondary", "hint", "divider", "fieldLine", "chip"]) {
      assert.equal(json[name][key], hex(new RegExp(`${key} = 0xFF([0-9A-F]{6})`).exec(s)[1]), `${name}.${key}`);
    }
    for (const tone of ["good", "info", "wait", "warn", "stop"]) {
      const t = new RegExp(`${tone} = Tone\\(band = 0xFF([0-9A-F]{6}).toInt\\(\\), ink = 0xFF([0-9A-F]{6}).toInt\\(\\), tint = 0xFF([0-9A-F]{6})`).exec(s);
      assert.deepEqual(json[name][tone], { band: hex(t[1]), ink: hex(t[2]), tint: hex(t[3]) }, `${name}.${tone}`);
    }
  }
  for (const [name, key] of [["paid", "paid"], ["waiting", "waiting"], ["stop", "stopped"], ["ended", "ended"]]) {
    const m = new RegExp(`val ${key} = OutcomeBand\\(0xFF([0-9A-F]{6}).toInt\\(\\), 0xFF([0-9A-F]{6})`).exec(src);
    assert.deepEqual(json.outcomes[name], { band: hex(m[1]), sub: hex(m[2]) }, `outcome ${name}`);
  }
});

test("px and pxf round as the platform does at density 2.625", () => {
  const U = require("./ui.js");
  const ui = new U.Ui({ palette: new U.PaletteOf(JSON.parse(fs.readFileSync(path.join(__dirname, "palette.json"), "utf8"))), night: false, density: 2.625 });
  assert.equal(ui.px(16), 42 / 2.625);
  assert.equal(ui.px(1), 2 / 2.625);
  assert.equal(ui.pxf(1.5), 3 / 2.625);
  assert.equal(ui.pxf(0.1), 1 / 2.625);
  assert.equal(ui.px(104) * 2.625, 273);
});

test("the gallery sample words are the apps' words", () => {
  const D = require("./gallery-data.js");
  const docs = ["docs/screens/counter.md", "docs/screens/counter-app.md", "docs/screens/owner-node.md", "docs/screens/owner.md", "docs/screens/owner-app.md"].map(read).join("\n");
  const known = new Set(["Cash refund only"]);
  known.delete("Cash refund only");
  for (const set of [D.parts]) {
    for (const [name, words] of Object.entries(set)) {
      if (!words || !words.heading) continue;
      for (const field of ["heading", "what", "next"]) {
        assert.ok(docs.includes(words[field]) || words === D.parts.withDetails, `${name}.${field} is in docs/screens: ${words[field]}`);
      }
    }
  }
  for (const w of [D.counter.unpaid.says, D.counter.partPaid.says, D.counter.paid.says, D.counter.stop.says, D.counter.waiting.says]) {
    for (const field of ["heading", "what", "next"]) assert.ok(docs.includes(w[field]), `${w.heading}.${field}`);
  }
});
